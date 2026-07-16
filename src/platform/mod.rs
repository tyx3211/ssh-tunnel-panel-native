#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub(crate) use windows::{
    ActivationSignal, PrimaryInstance, SingleInstance, WindowVisibilityError, hide_window,
    show_window,
};

#[cfg(not(target_os = "windows"))]
compile_error!("the GPUI application currently requires a Windows platform backend");
