#[cfg(windows)]
#[path = "software_slot_windows.rs"]
mod imp;
#[cfg(target_os = "linux")]
#[path = "software_slot_linux.rs"]
mod imp;
pub(crate) use imp::*;
