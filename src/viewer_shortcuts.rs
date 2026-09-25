#[cfg(windows)]
#[path = "viewer_shortcuts_windows.rs"]
mod imp;
#[cfg(target_os = "linux")]
#[path = "viewer_shortcuts_linux.rs"]
mod imp;
pub(crate) use imp::*;
