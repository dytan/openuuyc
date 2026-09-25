#[cfg(windows)]
#[path = "formats_windows.rs"]
mod imp;
#[cfg(target_os = "linux")]
#[path = "formats_linux.rs"]
mod imp;

pub(super) use imp::{Format, file_format, name, register, supported};
