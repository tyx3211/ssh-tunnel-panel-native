use std::io::{BufRead as _, BufReader};
use std::os::windows::io::AsRawHandle as _;
use std::os::windows::process::CommandExt as _;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use async_channel::Sender;
use thiserror::Error;
use win32job::{ExtendedLimitInfo, Job};

use crate::model::{ForwardConfig, ForwardId, ForwardMode};
use crate::ports::{is_port_available_on, resolve_bind_ips};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Error)]
pub enum TunnelError {
    #[error("启动 ssh.exe 失败")]
    Spawn(#[source] std::io::Error),
    #[error("创建 Windows Job Object 失败")]
    CreateJob(#[source] win32job::JobError),
    #[error("将 ssh.exe 加入 Windows Job Object 失败")]
    AssignJob(#[source] win32job::JobError),
    #[error("ssh.exe 未提供标准错误管道")]
    MissingStderr,
    #[error("无法解析本地监听地址 {host}")]
    ResolveBindAddress {
        host: String,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug)]
pub enum ProcessEvent {
    Log {
        id: ForwardId,
        line: String,
    },
    Ready {
        id: ForwardId,
    },
    Exited {
        id: ForwardId,
        code: Option<i32>,
        requested: bool,
        startup_error: Option<String>,
    },
}

pub struct ManagedProcess {
    pub pid: u32,
    message_sender: mpsc::SyncSender<SupervisorMessage>,
    join: Option<JoinHandle<()>>,
}

impl ManagedProcess {
    pub fn request_stop(&self) {
        let _ = self.message_sender.send(SupervisorMessage::Stop);
    }

    pub fn join(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Starts an SSH child supervised by a kill-on-close Windows Job Object.
///
/// # Errors
///
/// Returns an error when the process, pipes, Job Object, or supervisor thread cannot be created.
pub fn spawn_tunnel(
    forward: &ForwardConfig,
    actual_listen_port: u16,
    event_sender: Sender<ProcessEvent>,
    log_sender: Sender<ProcessEvent>,
    wake_sender: Option<Sender<()>>,
) -> Result<ManagedProcess, TunnelError> {
    let mut limit_info = ExtendedLimitInfo::new();
    limit_info.limit_kill_on_job_close();
    let job = Job::create_with_limit_info(&limit_info).map_err(TunnelError::CreateJob)?;

    let args = build_ssh_args(forward, actual_listen_port);
    let mut child = Command::new("ssh")
        .args(&args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(TunnelError::Spawn)?;

    if let Err(source) = job.assign_process(child.as_raw_handle() as isize) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(TunnelError::AssignJob(source));
    }

    let stderr = child.stderr.take().ok_or(TunnelError::MissingStderr)?;
    let pid = child.id();
    let id = forward.id.clone();
    let bind_address = forward.bind_address.clone();
    let mode = forward.mode;
    let bind_ips = if mode == ForwardMode::L {
        resolve_bind_ips(&bind_address).map_err(|source| TunnelError::ResolveBindAddress {
            host: bind_address.clone(),
            source,
        })?
    } else {
        Vec::new()
    };
    let (message_sender, message_receiver) = mpsc::sync_channel(128);
    let output_reader =
        spawn_output_reader(stderr, message_sender.clone()).map_err(TunnelError::Spawn)?;
    let join = thread::Builder::new()
        .name(format!("ssh-tunnel-{pid}"))
        .stack_size(256 * 1024)
        .spawn(move || {
            supervise_process(
                child,
                job,
                id,
                mode,
                bind_ips,
                actual_listen_port,
                message_receiver,
                event_sender,
                log_sender,
                wake_sender,
                output_reader,
            );
        })
        .map_err(TunnelError::Spawn)?;

    Ok(ManagedProcess {
        pid,
        message_sender,
        join: Some(join),
    })
}

#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "the supervisor thread must own its channel endpoints and bind address"
)]
fn supervise_process(
    mut child: Child,
    job: Job,
    id: ForwardId,
    mode: ForwardMode,
    bind_ips: Vec<std::net::IpAddr>,
    actual_listen_port: u16,
    message_receiver: mpsc::Receiver<SupervisorMessage>,
    event_sender: Sender<ProcessEvent>,
    log_sender: Sender<ProcessEvent>,
    wake_sender: Option<Sender<()>>,
    output_reader: JoinHandle<()>,
) {
    let started_at = Instant::now();
    let startup_deadline = started_at + STARTUP_TIMEOUT;
    let mut next_local_probe = started_at + Duration::from_millis(150);
    let mut ready = false;
    let mut requested = false;
    let mut startup_error = None;
    let mut job = Some(job);

    loop {
        let message = if ready || requested || startup_error.is_some() {
            message_receiver.recv().ok()
        } else {
            let now = Instant::now();
            let deadline = if mode == ForwardMode::L {
                next_local_probe.min(startup_deadline)
            } else {
                startup_deadline
            };
            match message_receiver.recv_timeout(deadline.saturating_duration_since(now)) {
                Ok(message) => Some(message),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => Some(SupervisorMessage::OutputClosed),
            }
        };

        match message {
            Some(SupervisorMessage::Stop) => {
                requested = true;
                drop(job.take());
            }
            Some(SupervisorMessage::Output(line)) => {
                if !ready && output_indicates_ready(&line, mode) {
                    ready = true;
                    send_event(
                        &event_sender,
                        wake_sender.as_ref(),
                        ProcessEvent::Ready { id: id.clone() },
                    );
                }
                send_log(
                    &log_sender,
                    wake_sender.as_ref(),
                    ProcessEvent::Log {
                        id: id.clone(),
                        line,
                    },
                );
            }
            Some(SupervisorMessage::OutputClosed) => {
                let _ = output_reader.join();
                let (code, wait_error) = match child.wait() {
                    Ok(status) => (status.code(), None),
                    Err(error) => (None, Some(format!("等待 ssh 退出失败: {error}"))),
                };
                let startup_error = startup_error.or(wait_error);
                send_event(
                    &event_sender,
                    wake_sender.as_ref(),
                    ProcessEvent::Exited {
                        id,
                        code,
                        requested,
                        startup_error,
                    },
                );
                return;
            }
            None => {}
        }

        let now = Instant::now();
        if !ready
            && !requested
            && startup_error.is_none()
            && mode == ForwardMode::L
            && now >= next_local_probe
        {
            if is_port_available_on(actual_listen_port, &bind_ips) {
                next_local_probe = now + POLL_INTERVAL;
            } else {
                ready = true;
                send_event(
                    &event_sender,
                    wake_sender.as_ref(),
                    ProcessEvent::Ready { id: id.clone() },
                );
            }
        }

        if !ready && !requested && startup_error.is_none() && now >= startup_deadline {
            startup_error = Some(format!(
                "SSH 转发在 {} 秒内未就绪，已终止",
                STARTUP_TIMEOUT.as_secs()
            ));
            drop(job.take());
        }
    }
}

#[derive(Debug)]
enum SupervisorMessage {
    Stop,
    Output(String),
    OutputClosed,
}

fn spawn_output_reader(
    reader: ChildStderr,
    sender: mpsc::SyncSender<SupervisorMessage>,
) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("ssh-output-reader".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut bytes = Vec::new();
            loop {
                bytes.clear();
                match reader.read_until(b'\n', &mut bytes) {
                    Ok(0) | Err(_) => {
                        let _ = sender.send(SupervisorMessage::OutputClosed);
                        return;
                    }
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&bytes).trim().to_owned();
                        if !line.is_empty() && sender.send(SupervisorMessage::Output(line)).is_err()
                        {
                            return;
                        }
                    }
                }
            }
        })
}

fn send_event(
    event_sender: &Sender<ProcessEvent>,
    wake_sender: Option<&Sender<()>>,
    event: ProcessEvent,
) {
    if event_sender.send_blocking(event).is_ok()
        && let Some(wake_sender) = wake_sender
    {
        let _ = wake_sender.try_send(());
    }
}

fn send_log(
    log_sender: &Sender<ProcessEvent>,
    wake_sender: Option<&Sender<()>>,
    event: ProcessEvent,
) {
    if log_sender.try_send(event).is_ok()
        && let Some(wake_sender) = wake_sender
    {
        let _ = wake_sender.try_send(());
    }
}

pub fn build_ssh_args(forward: &ForwardConfig, actual_listen_port: u16) -> Vec<String> {
    let spec = format!(
        "{}:{actual_listen_port}:{}:{}",
        format_forward_host(&forward.bind_address),
        format_forward_host(&forward.target_host),
        forward.target_port
    );
    let mode = match forward.mode {
        ForwardMode::L => "-L",
        ForwardMode::R => "-R",
    };

    [
        "-N",
        "-T",
        "-v",
        "-o",
        "BatchMode=yes",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=30",
        "-o",
        "ServerAliveCountMax=3",
        mode,
        &spec,
        &forward.host_alias,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn format_forward_host(host: &str) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

fn output_indicates_ready(line: &str, mode: ForwardMode) -> bool {
    let lower = line.to_ascii_lowercase();
    match mode {
        ForwardMode::L => lower.contains("local forwarding listening on"),
        ForwardMode::R => {
            lower.contains("remote forward success for")
                || lower.contains("entering interactive session")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_forward(mode: ForwardMode) -> ForwardConfig {
        ForwardConfig {
            id: ForwardId("forward-1".into()),
            name: "Web".into(),
            host_alias: "dev".into(),
            mode,
            bind_address: "::1".into(),
            listen_port: 3000,
            target_host: "2001:db8::1".into(),
            target_port: 8080,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn builds_safe_local_forward_arguments() {
        let args = build_ssh_args(&sample_forward(ForwardMode::L), 3001);

        assert!(args.windows(2).any(|pair| pair == ["-o", "BatchMode=yes"]));
        assert!(args.contains(&"ExitOnForwardFailure=yes".to_owned()));
        assert!(args.contains(&"[::1]:3001:[2001:db8::1]:8080".to_owned()));
        assert_eq!(args.last().map(String::as_str), Some("dev"));
    }

    #[test]
    fn recognizes_mode_specific_readiness() {
        assert!(output_indicates_ready(
            "debug1: Local forwarding listening on 127.0.0.1 port 3000.",
            ForwardMode::L
        ));
        assert!(output_indicates_ready(
            "debug1: remote forward success for: listen 127.0.0.1:3000",
            ForwardMode::R
        ));
        assert!(!output_indicates_ready(
            "debug1: Connecting to example.com",
            ForwardMode::R
        ));
    }
}
