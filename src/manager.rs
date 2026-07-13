use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use async_channel::{Receiver, Sender};
use chrono::{SecondsFormat, Utc};
use thiserror::Error;

use crate::model::{
    ForwardConfig, ForwardDraft, ForwardId, ForwardMode, ForwardRuntime, ForwardStatus,
    ForwardValidationError, ForwardView, PersistedState, SshHost,
};
use crate::ports::{MAX_PORT_ATTEMPTS, find_nearest_available_port};
use crate::ssh_config::{HostDiscoveryError, read_ssh_config_hosts};
use crate::store::{JsonStore, StoreError};
use crate::tunnel::{ManagedProcess, ProcessEvent, TunnelError, spawn_tunnel};

const MAX_LOG_LINES: usize = 300;
const MAX_PENDING_LOG_EVENTS: usize = 1_024;

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    HostDiscovery(#[from] HostDiscoveryError),
    #[error(transparent)]
    Validation(#[from] ForwardValidationError),
    #[error(transparent)]
    Tunnel(#[from] TunnelError),
    #[error("找不到转发")]
    ForwardNotFound,
    #[error("请先停止转发再修改")]
    ForwardRunning,
    #[error("从 {start_port} 开始的 {MAX_PORT_ATTEMPTS} 个候选端口中没有找到可用本地端口")]
    NoAvailableLocalPort { start_port: u16 },
    #[error("无法检查本地监听地址 {host}: {source}")]
    PortProbe {
        host: String,
        #[source]
        source: std::io::Error,
    },
}

pub struct TunnelManager {
    store: JsonStore,
    persisted: PersistedState,
    discovered_hosts: Vec<SshHost>,
    runtimes: HashMap<ForwardId, ForwardRuntime>,
    processes: HashMap<ForwardId, ManagedProcess>,
    event_sender: Sender<ProcessEvent>,
    event_receiver: Receiver<ProcessEvent>,
    log_sender: Sender<ProcessEvent>,
    log_receiver: Receiver<ProcessEvent>,
    wake_sender: Option<Sender<()>>,
    shutting_down: bool,
}

impl TunnelManager {
    /// Loads persisted definitions and discovers hosts from OpenSSH config.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence cannot be read or SSH config parsing fails.
    pub fn initialize(store: JsonStore) -> Result<Self, ManagerError> {
        Self::initialize_inner(store, None)
    }

    pub(crate) fn initialize_notifying(
        store: JsonStore,
        wake_sender: Sender<()>,
    ) -> Result<Self, ManagerError> {
        Self::initialize_inner(store, Some(wake_sender))
    }

    fn initialize_inner(
        store: JsonStore,
        wake_sender: Option<Sender<()>>,
    ) -> Result<Self, ManagerError> {
        let persisted = store.read()?;
        let discovered_hosts = read_ssh_config_hosts()?;
        let (event_sender, event_receiver) = async_channel::unbounded();
        let (log_sender, log_receiver) = async_channel::bounded(MAX_PENDING_LOG_EVENTS);

        Ok(Self {
            store,
            persisted,
            discovered_hosts,
            runtimes: HashMap::new(),
            processes: HashMap::new(),
            event_sender,
            event_receiver,
            log_sender,
            log_receiver,
            wake_sender,
            shutting_down: false,
        })
    }

    #[must_use]
    pub fn hosts(&self) -> Vec<SshHost> {
        let mut by_alias = BTreeMap::new();
        for host in self
            .discovered_hosts
            .iter()
            .chain(self.persisted.manual_hosts.iter())
        {
            by_alias.insert(host.alias.clone(), host.clone());
        }
        by_alias.into_values().collect()
    }

    #[must_use]
    pub fn forwards(&self) -> Vec<ForwardView> {
        self.persisted
            .forwards
            .iter()
            .map(|config| ForwardView {
                config: config.clone(),
                runtime: self.runtimes.get(&config.id).cloned().unwrap_or_default(),
            })
            .collect()
    }

    /// Refreshes aliases from the user's OpenSSH config.
    ///
    /// # Errors
    ///
    /// Returns an error when the SSH config cannot be parsed.
    pub fn refresh_hosts(&mut self) -> Result<(), ManagerError> {
        self.discovered_hosts = read_ssh_config_hosts()?;
        Ok(())
    }

    /// Creates or updates a persisted forward definition.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a missing edit target, an active target, or a failed
    /// atomic persistence write.
    pub fn save_forward(&mut self, draft: ForwardDraft) -> Result<ForwardId, ManagerError> {
        let draft = draft.normalize()?;
        let now = now_iso();

        let id = if let Some(id) = draft.id.clone() {
            if self.processes.contains_key(&id) {
                return Err(ManagerError::ForwardRunning);
            }
            let current = self
                .persisted
                .forwards
                .iter_mut()
                .find(|forward| forward.id == id)
                .ok_or(ManagerError::ForwardNotFound)?;
            *current = ForwardConfig {
                id: id.clone(),
                name: draft.name,
                host_alias: draft.host_alias,
                mode: draft.mode,
                bind_address: draft.bind_address,
                listen_port: draft.listen_port,
                target_host: draft.target_host,
                target_port: draft.target_port,
                created_at: current.created_at.clone(),
                updated_at: now,
            };
            id
        } else {
            let id = ForwardId::new();
            self.persisted.forwards.push(ForwardConfig {
                id: id.clone(),
                name: draft.name,
                host_alias: draft.host_alias,
                mode: draft.mode,
                bind_address: draft.bind_address,
                listen_port: draft.listen_port,
                target_host: draft.target_host,
                target_port: draft.target_port,
                created_at: now.clone(),
                updated_at: now,
            });
            id
        };

        self.store.write(&self.persisted)?;
        Ok(id)
    }

    /// Stops and permanently removes a forward definition.
    ///
    /// # Errors
    ///
    /// Returns an error when the updated state cannot be persisted.
    pub fn delete_forward(&mut self, id: &ForwardId) -> Result<(), ManagerError> {
        if let Some(mut process) = self.processes.remove(id) {
            process.request_stop();
            process.join();
        }
        self.drain_events();
        self.persisted.forwards.retain(|forward| &forward.id != id);
        self.runtimes.remove(id);
        self.store.write(&self.persisted)?;
        Ok(())
    }

    /// Starts one saved forward unless it is already active or shutdown has begun.
    ///
    /// # Errors
    ///
    /// Returns an error when the definition is missing, no local port can be selected, or the SSH
    /// process and its Windows Job Object cannot be created.
    pub fn start_forward(&mut self, id: &ForwardId) -> Result<(), ManagerError> {
        if self.shutting_down || self.processes.contains_key(id) {
            return Ok(());
        }

        let forward = self
            .persisted
            .forwards
            .iter()
            .find(|forward| &forward.id == id)
            .cloned()
            .ok_or(ManagerError::ForwardNotFound)?;
        let actual_port = self.resolve_listen_port(&forward)?;
        let runtime = self.runtime_mut(id);
        runtime.status = ForwardStatus::Starting;
        runtime.pid = None;
        runtime.error = None;
        runtime.actual_listen_port = Some(actual_port);
        runtime.last_started_at = Some(now_iso());
        push_log(runtime, "正在启动 ssh 进程");
        if actual_port != forward.listen_port {
            push_log(
                runtime,
                format!(
                    "本地端口 {} 被占用，改用 {actual_port}",
                    forward.listen_port
                ),
            );
        }

        match spawn_tunnel(
            &forward,
            actual_port,
            self.event_sender.clone(),
            self.log_sender.clone(),
            self.wake_sender.clone(),
        ) {
            Ok(process) => {
                let pid = process.pid;
                self.runtime_mut(id).pid = Some(pid);
                push_log(
                    self.runtime_mut(id),
                    format!("已创建 ssh 进程 PID {pid}，等待转发就绪"),
                );
                self.processes.insert(id.clone(), process);
                Ok(())
            }
            Err(error) => {
                let runtime = self.runtime_mut(id);
                runtime.status = ForwardStatus::Failed;
                runtime.error = Some(error.to_string());
                push_log(runtime, format!("启动失败: {error}"));
                Err(error.into())
            }
        }
    }

    pub fn stop_forward(&mut self, id: &ForwardId) {
        if let Some(pid) = self.processes.get(id).map(|process| process.pid) {
            push_log(self.runtime_mut(id), format!("正在停止 PID {pid}"));
            if let Some(process) = self.processes.get(id) {
                process.request_stop();
            }
        }
        let runtime = self.runtime_mut(id);
        runtime.status = ForwardStatus::Stopped;
        runtime.error = None;
        runtime.actual_listen_port = None;
    }

    pub fn start_all(&mut self) -> Vec<(ForwardId, ManagerError)> {
        self.start_matching(None)
    }

    pub fn start_for_host(&mut self, host_alias: &str) -> Vec<(ForwardId, ManagerError)> {
        self.start_matching(Some(host_alias))
    }

    fn start_matching(&mut self, host_alias: Option<&str>) -> Vec<(ForwardId, ManagerError)> {
        let ids = self.forward_ids_for_host(host_alias);
        ids.into_iter()
            .filter_map(|id| self.start_forward(&id).err().map(|error| (id, error)))
            .collect()
    }

    pub fn stop_all(&mut self) {
        self.stop_matching(None);
    }

    pub fn stop_for_host(&mut self, host_alias: &str) {
        self.stop_matching(Some(host_alias));
    }

    fn stop_matching(&mut self, host_alias: Option<&str>) {
        let ids: Vec<_> = self
            .forward_ids_for_host(host_alias)
            .into_iter()
            .filter(|id| self.processes.contains_key(id))
            .collect();
        for id in ids {
            self.stop_forward(&id);
        }
    }

    fn forward_ids_for_host(&self, host_alias: Option<&str>) -> Vec<ForwardId> {
        self.persisted
            .forwards
            .iter()
            .filter(|forward| host_alias.is_none_or(|alias| forward.host_alias.as_str() == alias))
            .map(|forward| forward.id.clone())
            .collect()
    }

    pub fn clear_logs(&mut self, id: &ForwardId) {
        Arc::make_mut(&mut self.runtime_mut(id).logs).clear();
    }

    pub fn drain_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.log_receiver.try_recv() {
            changed = true;
            self.apply_event(event);
        }
        while let Ok(event) = self.event_receiver.try_recv() {
            changed = true;
            self.apply_event(event);
        }
        changed
    }

    pub fn shutdown(&mut self) {
        self.shutting_down = true;
        self.stop_all();
        for (_, mut process) in self.processes.drain() {
            process.join();
        }
        self.drain_events();
    }

    fn apply_event(&mut self, event: ProcessEvent) {
        match event {
            ProcessEvent::Log { id, line } => push_log(self.runtime_mut(&id), line),
            ProcessEvent::Ready { id } => {
                let runtime = self.runtime_mut(&id);
                if runtime.status == ForwardStatus::Starting {
                    runtime.status = ForwardStatus::Running;
                    runtime.error = None;
                    push_log(runtime, "SSH 转发已就绪");
                }
            }
            ProcessEvent::Exited {
                id,
                code,
                requested,
                startup_error,
            } => {
                self.processes.remove(&id);
                let shutting_down = self.shutting_down;
                let runtime = self.runtime_mut(&id);
                runtime.pid = None;
                runtime.actual_listen_port = None;
                if requested || shutting_down || runtime.status == ForwardStatus::Stopped {
                    runtime.status = ForwardStatus::Stopped;
                    runtime.error = None;
                    push_log(
                        runtime,
                        format!("已停止，退出码 {}", format_exit_code(code)),
                    );
                } else {
                    runtime.status = ForwardStatus::Failed;
                    let message = startup_error.unwrap_or_else(|| {
                        format!("ssh 已退出，退出码 {}", format_exit_code(code))
                    });
                    runtime.error = Some(message.clone());
                    push_log(runtime, message);
                }
            }
        }
    }

    fn resolve_listen_port(&self, forward: &ForwardConfig) -> Result<u16, ManagerError> {
        if forward.mode == ForwardMode::R {
            return Ok(forward.listen_port);
        }
        let reserved = self.reserved_local_ports(forward);
        find_nearest_available_port(forward.listen_port, &forward.bind_address, &reserved)
            .map_err(|source| ManagerError::PortProbe {
                host: forward.bind_address.clone(),
                source,
            })?
            .ok_or(ManagerError::NoAvailableLocalPort {
                start_port: forward.listen_port,
            })
    }

    fn reserved_local_ports(&self, current: &ForwardConfig) -> HashSet<u16> {
        self.persisted
            .forwards
            .iter()
            .filter(|forward| {
                forward.id != current.id
                    && forward.mode == ForwardMode::L
                    && bind_addresses_conflict(&forward.bind_address, &current.bind_address)
            })
            .filter_map(|forward| {
                self.runtimes.get(&forward.id).and_then(|runtime| {
                    matches!(
                        runtime.status,
                        ForwardStatus::Starting | ForwardStatus::Running
                    )
                    .then_some(runtime.actual_listen_port)
                    .flatten()
                })
            })
            .collect()
    }

    fn runtime_mut(&mut self, id: &ForwardId) -> &mut ForwardRuntime {
        self.runtimes.entry(id.clone()).or_default()
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn push_log(runtime: &mut ForwardRuntime, message: impl AsRef<str>) {
    let logs = Arc::make_mut(&mut runtime.logs);
    logs.push(Arc::from(format!(
        "[{}] {}",
        chrono::Local::now().format("%H:%M:%S"),
        message.as_ref()
    )));
    let overflow = logs.len().saturating_sub(MAX_LOG_LINES);
    if overflow != 0 {
        logs.drain(..overflow);
    }
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn bind_addresses_conflict(left: &str, right: &str) -> bool {
    left == right || matches!(left, "0.0.0.0" | "::") || matches!(right, "0.0.0.0" | "::")
}

fn format_exit_code(code: Option<i32>) -> String {
    code.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn manager() -> (TempDir, TunnelManager) {
        let directory = tempfile::tempdir().expect("test directory must be created");
        let store = JsonStore::prepare(
            directory.path().join("state.json"),
            directory.path().join("legacy.json"),
        )
        .expect("test store must initialize");
        let manager = TunnelManager::initialize(store).expect("manager must initialize");
        (directory, manager)
    }

    fn draft() -> ForwardDraft {
        ForwardDraft {
            id: None,
            name: "Web".into(),
            host_alias: "dev".into(),
            mode: ForwardMode::L,
            bind_address: "127.0.0.1".into(),
            listen_port: 30_000,
            target_host: "127.0.0.1".into(),
            target_port: 3000,
        }
    }

    #[test]
    fn saves_and_updates_forward_without_changing_creation_time() {
        let (_directory, mut manager) = manager();
        let id = manager
            .save_forward(draft())
            .expect("forward must be saved");
        let created_at = manager.forwards()[0].config.created_at.clone();
        let mut update = draft();
        update.id = Some(id.clone());
        update.name = "Updated".into();

        manager
            .save_forward(update)
            .expect("forward must be updated");

        let forward = &manager.forwards()[0].config;
        assert_eq!(forward.id, id);
        assert_eq!(forward.name, "Updated");
        assert_eq!(forward.created_at, created_at);
    }

    #[test]
    fn manual_host_overrides_discovered_host_with_same_alias() {
        let (_directory, mut manager) = manager();
        manager.discovered_hosts = vec![SshHost {
            id: "ssh-config:dev".into(),
            alias: "dev".into(),
            host_name: Some("old".into()),
            user: None,
            port: None,
            identity_file: None,
            proxy_jump: None,
            source: crate::model::HostSource::SshConfig,
        }];
        manager.persisted.manual_hosts = vec![SshHost {
            id: "manual:dev".into(),
            alias: "dev".into(),
            host_name: Some("new".into()),
            user: None,
            port: None,
            identity_file: None,
            proxy_jump: None,
            source: crate::model::HostSource::Manual,
        }];

        let hosts = manager.hosts();

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host_name.as_deref(), Some("new"));
    }

    #[test]
    fn selects_only_the_requested_host_for_bulk_actions() {
        let (_directory, mut manager) = manager();
        let dev_id = manager
            .save_forward(draft())
            .expect("dev forward must be saved");
        let mut production = draft();
        production.name = "Production".into();
        production.host_alias = "production".into();
        production.listen_port = 30_001;
        let production_id = manager
            .save_forward(production)
            .expect("production forward must be saved");

        assert_eq!(
            manager.forward_ids_for_host(Some("dev")),
            vec![dev_id.clone()]
        );
        assert_eq!(
            manager.forward_ids_for_host(Some("production")),
            vec![production_id.clone()]
        );
        assert!(manager.forward_ids_for_host(Some("missing")).is_empty());
        assert_eq!(
            manager.forward_ids_for_host(None),
            vec![dev_id, production_id]
        );
    }
}
