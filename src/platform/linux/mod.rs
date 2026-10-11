//! Linux OS integration.
//!
//! Linux runs OpenUUYC as a controller (viewer). Upstream's host-side services
//! (resident service, capture, virtual drivers, WoL configuration, toast
//! notifications, ...) are Windows-only. Shared code reaches them through
//! `crate::platform::windows::*`; on Linux that path re-exports this module, and
//! every item below is either a real Linux implementation (device profile,
//! paths, input deadline timer, known folders) or a stub reporting that the
//! host-side capability is unavailable on Linux.
#![allow(dead_code, unused_variables, clippy::unused_self)]

pub(crate) mod capture;
pub(crate) mod components;
pub(crate) mod cursor_shape;
pub(crate) mod decoder;
pub(crate) mod device_profile;
pub(crate) mod display;
pub(crate) mod display_hdr;
pub(crate) mod drag_drop;
pub(crate) mod encoder;
pub(crate) mod file_locations;
pub(crate) mod host;
pub(crate) mod host_service;
pub(crate) mod input;
pub(crate) mod loopback;
pub(crate) mod notifications;
pub(crate) mod surface;
pub(crate) mod system_power;
pub(crate) mod transfer;
pub(crate) mod virtual_audio;
pub(crate) mod wol;

/// Error text for host-side features that only exist on Windows.
pub(crate) const UNSUPPORTED: &str = "Linux 版本暂不支持本机被控功能";

pub(crate) fn unsupported<T>() -> anyhow::Result<T> {
    anyhow::bail!(UNSUPPORTED)
}

/// `std::os::windows::process::CommandExt::creation_flags` stand-in: Linux has no
/// console-window flags, so shared spawn code can keep calling it.
pub(crate) trait CommandExt {
    fn creation_flags(&mut self, flags: u32) -> &mut Self;
}
impl CommandExt for std::process::Command {
    fn creation_flags(&mut self, _flags: u32) -> &mut Self {
        self
    }
}

/// Open a file or folder with the desktop's default handler.
pub(crate) fn open_path(path: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context;
    std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("启动 xdg-open 失败")?;
    Ok(())
}
