#![allow(
    unsafe_code,
    reason = "this module is the audited Win32 named-mutex ownership boundary"
)]

use thiserror::Error;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::{PCWSTR, w};

#[derive(Debug, Error)]
pub enum SingleInstanceError {
    #[error("无法创建 Windows 单实例互斥体: {0}")]
    Create(#[source] windows::core::Error),
}

pub struct SingleInstance {
    handle: HANDLE,
    primary: bool,
}

impl SingleInstance {
    pub fn acquire() -> Result<Self, SingleInstanceError> {
        Self::acquire_named(w!("Local\\com.tyx3211.sshtunnelpanel.native"))
    }

    fn acquire_named(name: PCWSTR) -> Result<Self, SingleInstanceError> {
        // SAFETY: the mutex uses default security, does not request initial ownership, and the
        // name is a static, nul-terminated UTF-16 string scoped to the current login session.
        let handle =
            unsafe { CreateMutexW(None, false, name) }.map_err(SingleInstanceError::Create)?;
        // SAFETY: GetLastError is read immediately after the successful CreateMutexW call.
        let primary = unsafe { GetLastError() } != ERROR_ALREADY_EXISTS;
        Ok(Self { handle, primary })
    }

    pub fn is_primary(&self) -> bool {
        self.primary
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: this object owns exactly one valid mutex handle returned by CreateMutexW.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_second_holder_and_releases_the_mutex_on_drop() {
        let name = w!("Local\\com.tyx3211.sshtunnelpanel.native.test");
        let first = SingleInstance::acquire_named(name).expect("first mutex must be created");
        let second = SingleInstance::acquire_named(name).expect("second mutex must be opened");

        assert!(first.is_primary());
        assert!(!second.is_primary());

        drop(second);
        drop(first);
        let replacement =
            SingleInstance::acquire_named(name).expect("released mutex must be reusable");
        assert!(replacement.is_primary());
    }
}
