//! Desktop GUI ownership and shared application controls.
use anyhow::Result;

pub(crate) mod chrome;
#[cfg(windows)]
use crate::platform::graphics as d3d11;

#[cfg(windows)]
mod tray;
pub(crate) mod window_manager;
#[cfg(windows)]
mod windows;

// Linux: winit event loop + wgpu presenter (Vulkan/GL) instead of D3D11.
#[cfg(not(windows))]
pub(crate) mod gfx;
#[cfg(not(windows))]
mod shell;
#[cfg(not(windows))]
mod wgpu_backend;
#[cfg(not(windows))]
pub(crate) mod wgpu_video;

mod app;
pub(crate) mod branding;
pub(crate) mod controls;
pub(crate) mod fonts;
pub(crate) mod theme;
use app::AppFactory;

use app::AppSession;
pub(crate) use app::{App, WindowConfig};

pub(crate) fn run(config: WindowConfig, factory: AppFactory) -> Result<()> {
    #[cfg(windows)]
    {
        windows::run(config, factory)
    }
    #[cfg(not(windows))]
    {
        shell::run(config, factory)
    }
}
