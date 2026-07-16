#![allow(
    unsafe_code,
    reason = "GPUI 0.2.2 has no public Windows hide/show API; this module is the audited Win32 boundary"
)]

use std::ffi::c_void;

use gpui::Window;
use raw_window_handle::{HandleError, RawWindowHandle};
use thiserror::Error;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOW, ShowWindow};

#[derive(Debug, Error)]
pub enum WindowVisibilityError {
    #[error("无法读取 GPUI 窗口句柄: {0:?}")]
    Handle(HandleError),
    #[error("当前窗口不是 Win32 窗口")]
    UnsupportedHandle,
}

pub fn hide_window(window: &Window) -> Result<(), WindowVisibilityError> {
    set_window_visibility(window, false)
}

pub fn show_window(window: &Window) -> Result<(), WindowVisibilityError> {
    set_window_visibility(window, true)
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
