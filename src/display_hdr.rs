#[cfg(windows)]
#[path = "display_hdr_windows.rs"]
mod imp;
#[cfg(target_os = "linux")]
#[path = "display_hdr_linux.rs"]
mod imp;

pub(crate) use imp::*;
