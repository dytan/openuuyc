#[cfg(windows)]
#[path = "windows_surface_windows.rs"]
mod imp;
#[cfg(target_os = "linux")]
#[path = "windows_surface_linux.rs"]
mod imp;
pub(crate) use imp::*;
