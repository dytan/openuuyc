//! Native OS/driver implementations. Product policy belongs to sessions/features.
//! Select the real backend here; no placeholder implementations for future OSes.
pub(crate) mod fs_ext;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(windows)]
pub(crate) use windows::{
    capture, decoder, display, display_hdr, encoder, graphics, surface, swapchain, transfer,
    video_shader,
};

// Linux is a controller (viewer) platform. Host-side services that only exist on
// Windows are exposed under the same module paths so shared code compiles
// unchanged; each Linux shim reports "unsupported" or a neutral default.
#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux as windows;
#[cfg(target_os = "linux")]
pub(crate) use linux::{
    capture, decoder, display, display_hdr, encoder, power_request, surface, terminal, transfer,
};
#[cfg(windows)]
pub(crate) use windows::terminal;
/// The pty implementation is shared by Linux and macOS. Only Linux is built here.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod posix;

/// Foreground command of a host terminal, when the platform can see it.
///
/// The name is the process in the foreground group, not the shell that started
/// it. A pipeline reports the last process, matching `ps` `tpgid`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Foreground {
    pub name: String,
    pub command: String,
}
