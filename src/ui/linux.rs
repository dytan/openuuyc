//! Linux egui/winit shell (milestone stub).
//!
//! TODO(linux): full event loop with egui-winit + glow (or wgpu) replacing D3D11.
use super::{AppFactory, WindowConfig};
use anyhow::{Result, bail};

pub(super) fn run(_config: WindowConfig, _factory: AppFactory) -> Result<()> {
    bail!(
        "TODO(linux): graphical UI shell not implemented yet — use CLI subcommands \
         (login, devices, native-status, rtc-selftest) for now"
    )
}
