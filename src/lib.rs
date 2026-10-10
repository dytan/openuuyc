//! OpenUUYC: UU protocol interoperability and Windows application composition.

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("OpenUUYC currently supports Windows and Linux only");

pub const APP_NAME: &str = "OpenUUYC";
pub(crate) const VIEWER_TITLE_PREFIX: &str = "OpenUUYC — ";

pub mod account;
pub mod application;
pub mod diagnostics;
pub mod features;
pub mod media;
pub mod protocol;
pub mod session;
pub mod transport;

pub mod plugins;
mod ui;

mod mac_keycodes;
mod paths;
mod platform;
