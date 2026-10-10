//! User-local player shortcuts. Hook reads are atomic and never perform file IO.
#[cfg(windows)]
#[path = "viewer_shortcuts/windows.rs"]
mod imp;
#[cfg(not(windows))]
#[path = "viewer_shortcuts/linux.rs"]
mod imp;
pub(crate) use imp::*;
