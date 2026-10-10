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
pub(crate) use linux::{capture, decoder, display, display_hdr, encoder, surface, transfer};
