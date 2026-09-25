#[cfg(windows)]
#[path = "viewer_shortcuts_windows.rs"]
mod imp;
#[cfg(target_os = "linux")]
#[path = "viewer_shortcuts_linux.rs"]
mod imp;
pub(crate) use imp::*;

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU64, Ordering};

/// The player publishes its focused window id here; X11 and Wayland do not let
/// a process ask which window the compositor considers focused.
#[cfg(target_os = "linux")]
static FOCUSED: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "linux")]
pub(crate) fn set_focused_window(owner: u64) {
    FOCUSED.store(owner, Ordering::Release);
}

#[cfg(target_os = "linux")]
pub(crate) fn focused_window() -> u64 {
    FOCUSED.load(Ordering::Acquire)
}
