//! GUI ownership and shared application controls.
use anyhow::Result;

#[cfg(windows)]
pub(crate) mod chrome;
#[cfg(windows)]
pub(crate) mod d3d11;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

pub(crate) mod window_manager;
mod app;
pub(crate) mod branding;
pub(crate) mod controls;
pub(crate) mod theme;
use app::AppFactory;

use app::AppSession;
pub(crate) use app::{App, WindowConfig};

pub(crate) fn run(config: WindowConfig, factory: AppFactory) -> Result<()> {
    #[cfg(windows)]
    {
        return windows::run(config, factory);
    }
    #[cfg(target_os = "linux")]
    {
        return linux::run(config, factory);
    }
}
