#[cfg(windows)]
#[path = "native_windows.rs"]
mod imp;
#[cfg(target_os = "linux")]
#[path = "native_linux.rs"]
mod imp;

pub(super) use imp::{Command, post, pump, safe_name, shutdown, start};
