//! User-local player shortcuts. Hook reads are atomic and never perform file IO.
#[cfg(windows)]
#[path = "viewer_shortcuts/windows.rs"]
mod imp;
#[cfg(not(windows))]
#[path = "viewer_shortcuts/linux.rs"]
mod imp;
pub(crate) use imp::*;

#[cfg(not(windows))]
use std::sync::atomic::{AtomicU64, Ordering};

/// The player publishes its focused window id here; X11 and Wayland do not let
/// a process ask which window the compositor considers focused.
#[cfg(not(windows))]
static FOCUSED: AtomicU64 = AtomicU64::new(0);

#[cfg(not(windows))]
pub(crate) fn set_focused_window(owner: u64) {
    FOCUSED.store(owner, Ordering::Release);
}

#[cfg(not(windows))]
pub(crate) fn focused_window() -> u64 {
    FOCUSED.load(Ordering::Acquire)
}
