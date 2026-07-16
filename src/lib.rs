#![deny(unsafe_code)]

#[cfg(feature = "ui")]
pub mod app;
#[cfg(feature = "ui")]
mod assets;
pub mod manager;
pub mod model;
#[cfg(feature = "ui")]
mod platform;
pub mod ports;
pub mod ssh_config;
pub mod store;
#[cfg(feature = "ui")]
mod tray;
pub mod tunnel;
#[cfg(feature = "ui")]
mod ui;
