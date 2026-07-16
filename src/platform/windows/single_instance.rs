#![allow(
    unsafe_code,
    reason = "this module is the audited Win32 process-coordination boundary"
)]

use std::sync::Arc;
use std::thread::{self, JoinHandle};

use async_channel::{Receiver, Sender};
use thiserror::Error;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_FAILED, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForMultipleObjects,
};
use windows::core::{PCWSTR, w};

const MUTEX_NAME: PCWSTR = w!("Local\\com.tyx3211.sshtunnelpanel.native");
const ACTIVATION_EVENT_NAME: PCWSTR = w!("Local\\com.tyx3211.sshtunnelpanel.native.activate");

#[derive(Debug, Error)]
pub enum SingleInstanceError {
    #[error("无法创建 Windows 单实例互斥体: {0}")]
    CreateMutex(#[source] windows::core::Error),
    #[error("无法创建 Windows 应用激活事件: {0}")]
    CreateActivationEvent(#[source] windows::core::Error),
    #[error("无法创建 Windows 监听器停止事件: {0}")]
    CreateShutdownEvent(#[source] windows::core::Error),
    #[error("无法通知已有应用实例: {0}")]
    RequestActivation(#[source] windows::core::Error),
    #[error("无法启动应用激活监听线程: {0}")]
    SpawnListener(#[source] std::io::Error),
    #[error("应用激活监听线程已经启动")]
    ListenerAlreadyStarted,
}

#[derive(Debug, Error)]
pub enum ActivationListenerError {
    #[error("等待 Windows 应用激活事件失败: {0}")]
    Wait(#[source] windows::core::Error),
    #[error("Windows 应用激活监听器返回了意外状态: {0}")]
    UnexpectedWaitStatus(u32),
}

#[derive(Debug)]
pub enum ActivationSignal {
    Requested,
    ListenerFailed(ActivationListenerError),
}

pub enum SingleInstance {
    Primary(PrimaryInstance),
    Secondary(SecondaryInstance),
}

pub struct PrimaryInstance {
    _mutex: OwnedHandle,
    events: Arc<InstanceEvents>,
    listener: Option<JoinHandle<()>>,
}

pub struct SecondaryInstance {
    _mutex: OwnedHandle,
    activation_event: OwnedHandle,
}

struct InstanceEvents {
    activation: OwnedHandle,
    shutdown: OwnedHandle,
}

struct OwnedHandle(HANDLE);

// SAFETY: kernel object handles are process-wide values and Windows explicitly supports waiting
// on and signalling these event handles from threads other than the creating thread.
unsafe impl Send for OwnedHandle {}
// SAFETY: the wrapped handles are only used by thread-safe kernel synchronization functions.
unsafe impl Sync for OwnedHandle {}

impl SingleInstance {
    pub fn acquire() -> Result<Self, SingleInstanceError> {
        Self::acquire_named(MUTEX_NAME, ACTIVATION_EVENT_NAME)
    }

    fn acquire_named(
        mutex_name: PCWSTR,
        activation_event_name: PCWSTR,
    ) -> Result<Self, SingleInstanceError> {
        // SAFETY: default security is used, initial ownership is not requested, and the name is a
        // valid, nul-terminated UTF-16 string scoped to the current login session.
        let mutex = unsafe { CreateMutexW(None, false, mutex_name) }
            .map(OwnedHandle)
            .map_err(SingleInstanceError::CreateMutex)?;
        // SAFETY: GetLastError is read immediately after the successful CreateMutexW call.
        let primary = unsafe { GetLastError() } != ERROR_ALREADY_EXISTS;

        // SAFETY: this creates or opens a named auto-reset event with default security. An
        // auto-reset event intentionally coalesces repeated launch requests into one activation.
        let activation_event = unsafe { CreateEventW(None, false, false, activation_event_name) }
            .map(OwnedHandle)
            .map_err(SingleInstanceError::CreateActivationEvent)?;

        if primary {
            // SAFETY: a null name creates a private auto-reset event used only to stop the listener.
            let shutdown = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
                .map(OwnedHandle)
                .map_err(SingleInstanceError::CreateShutdownEvent)?;
            Ok(Self::Primary(PrimaryInstance {
                _mutex: mutex,
                events: Arc::new(InstanceEvents {
                    activation: activation_event,
                    shutdown,
                }),
                listener: None,
            }))
        } else {
            Ok(Self::Secondary(SecondaryInstance {
                _mutex: mutex,
                activation_event,
            }))
        }
    }
}

impl PrimaryInstance {
    pub fn start_activation_listener(
        &mut self,
    ) -> Result<Receiver<ActivationSignal>, SingleInstanceError> {
        if self.listener.is_some() {
            return Err(SingleInstanceError::ListenerAlreadyStarted);
        }
        let (sender, receiver) = async_channel::bounded(1);
        let events = Arc::clone(&self.events);
        let listener = thread::Builder::new()
            .name("app-activation-listener".to_owned())
            .spawn(move || listen_for_activation(&events, &sender))
            .map_err(SingleInstanceError::SpawnListener)?;
        self.listener = Some(listener);
        Ok(receiver)
    }
}

impl Drop for PrimaryInstance {
    fn drop(&mut self) {
        // SAFETY: shutdown is a live event owned by `events`; signalling is thread-safe.
        unsafe {
            let _ = SetEvent(self.events.shutdown.0);
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

impl SecondaryInstance {
    pub fn request_activation(&self) -> Result<(), SingleInstanceError> {
        // SAFETY: activation_event is a live event handle and SetEvent is thread-safe.
        unsafe { SetEvent(self.activation_event.0) }.map_err(SingleInstanceError::RequestActivation)
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this object owns exactly one valid Win32 handle.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn listen_for_activation(events: &InstanceEvents, sender: &Sender<ActivationSignal>) {
    let handles = [events.activation.0, events.shutdown.0];
    loop {
        // SAFETY: both handles remain alive through the shared `events` reference for the entire
        // wait. Waiting on kernel event handles from this dedicated thread is supported by Win32.
        let status = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
        if status == WAIT_OBJECT_0 {
            let _ = sender.try_send(ActivationSignal::Requested);
        } else if status.0 == WAIT_OBJECT_0.0 + 1 {
            break;
        } else {
            let error = if status == WAIT_FAILED {
                ActivationListenerError::Wait(windows::core::Error::from_win32())
            } else {
                ActivationListenerError::UnexpectedWaitStatus(status.0)
            };
            let _ = sender.try_send(ActivationSignal::ListenerFailed(error));
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    const TEST_MUTEX_NAME: PCWSTR =
        w!("Local\\com.tyx3211.sshtunnelpanel.native.single-instance.test");
    const TEST_EVENT_NAME: PCWSTR =
        w!("Local\\com.tyx3211.sshtunnelpanel.native.single-instance.test.activate");

    #[test]
    fn notifies_the_primary_instance_and_releases_owned_objects() {
        let first = SingleInstance::acquire_named(TEST_MUTEX_NAME, TEST_EVENT_NAME)
            .expect("first instance objects must be created");
        let SingleInstance::Primary(mut primary) = first else {
            panic!("first test instance must be primary");
        };
        let receiver = primary
            .start_activation_listener()
            .expect("activation listener must start");

        let second = SingleInstance::acquire_named(TEST_MUTEX_NAME, TEST_EVENT_NAME)
            .expect("second instance objects must be opened");
        let SingleInstance::Secondary(secondary) = second else {
            panic!("second test instance must be secondary");
        };
        secondary
            .request_activation()
            .expect("secondary instance must signal activation");

        let deadline = Instant::now() + Duration::from_secs(2);
        let signal = loop {
            if let Ok(signal) = receiver.try_recv() {
                break signal;
            }
            assert!(Instant::now() < deadline, "activation signal timed out");
            thread::sleep(Duration::from_millis(10));
        };
        assert!(matches!(signal, ActivationSignal::Requested));

        drop(secondary);
        drop(primary);
        let replacement = SingleInstance::acquire_named(TEST_MUTEX_NAME, TEST_EVENT_NAME)
            .expect("released instance objects must be reusable");
        assert!(matches!(replacement, SingleInstance::Primary(_)));
    }
}
