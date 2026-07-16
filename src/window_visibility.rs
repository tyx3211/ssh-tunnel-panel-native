#![allow(
    unsafe_code,
    reason = "GPUI 0.2.2 has no public Windows hide/show API; this module is the audited Win32 boundary"
)]

use std::ffi::c_void;
use std::thread;
use std::time::{Duration, Instant};

use gpui::Window;
use raw_window_handle::{HandleError, RawWindowHandle};
use thiserror::Error;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, SW_HIDE, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
    ShowWindowAsync,
};
use windows::core::w;

const EXISTING_WINDOW_LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);
const EXISTING_WINDOW_RETRY_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Error)]
pub enum WindowVisibilityError {
    #[error("无法读取 GPUI 窗口句柄: {0:?}")]
    Handle(HandleError),
    #[error("当前窗口不是 Win32 窗口")]
    UnsupportedHandle,
    #[error("等待已有 GPUI 窗口超时")]
    ExistingWindowNotFound,
    #[error("Windows 拒绝将已有窗口切换到前台")]
    ForegroundActivationRejected,
}

pub fn hide_window(window: &Window) -> Result<(), WindowVisibilityError> {
    set_window_visibility(window, false)
}

pub fn show_window(window: &Window) -> Result<(), WindowVisibilityError> {
    set_window_visibility(window, true)
}

pub fn activate_existing_window() -> Result<(), WindowVisibilityError> {
    let deadline = Instant::now() + EXISTING_WINDOW_LOOKUP_TIMEOUT;
    let hwnd = loop {
        // SAFETY: both arguments are static, nul-terminated UTF-16 strings. The returned HWND is
        // only used for non-owning window-management calls in this function.
        if let Ok(hwnd) = unsafe { FindWindowW(w!("Zed::Window"), w!("SSH Tunnel Panel")) } {
            break hwnd;
        }
        if Instant::now() >= deadline {
            return Err(WindowVisibilityError::ExistingWindowNotFound);
        }
        thread::sleep(EXISTING_WINDOW_RETRY_INTERVAL);
    };

    // SAFETY: `hwnd` belongs to the first application instance and remains owned by GPUI.
    // Restore only minimized windows so a hidden maximized window keeps its maximized state.
    unsafe {
        let command = if IsIconic(hwnd).as_bool() {
            SW_RESTORE
        } else {
            SW_SHOW
        };
        let _ = ShowWindowAsync(hwnd, command);
        if !SetForegroundWindow(hwnd).as_bool() {
            return Err(WindowVisibilityError::ForegroundActivationRejected);
        }
    }
    Ok(())
}

fn set_window_visibility(window: &Window, visible: bool) -> Result<(), WindowVisibilityError> {
    let handle = raw_window_handle::HasWindowHandle::window_handle(window)
        .map_err(WindowVisibilityError::Handle)?
        .as_raw();
    let RawWindowHandle::Win32(handle) = handle else {
        return Err(WindowVisibilityError::UnsupportedHandle);
    };
    let hwnd = HWND(handle.hwnd.get() as *mut c_void);
    let command = if visible { SW_SHOW } else { SW_HIDE };

    // SAFETY: `hwnd` is borrowed from the live GPUI `Window`, this runs on GPUI's foreground
    // thread, and ShowWindow neither takes ownership of nor outlives the handle.
    unsafe {
        let _ = ShowWindow(hwnd, command);
    }
    Ok(())
}
