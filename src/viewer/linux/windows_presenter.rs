//! Linux stub for the D3D11 viewer presenter.
use crate::viewer::{
    ConnectionProgress, NativeViewerSession, ViewerDisplayHandle, ViewerWindowEvent,
};
use anyhow::{Result, bail};
use std::sync::mpsc as std_mpsc;

pub struct ConnectingWindowsRunConfig {
    pub alias: String,
    pub progress: std_mpsc::Receiver<ConnectionProgress>,
    pub session: std_mpsc::Receiver<ViewerWindowEvent>,
    pub display_sender: tokio::sync::oneshot::Sender<ViewerDisplayHandle>,
}

pub fn run_connecting(_config: ConnectingWindowsRunConfig) -> Result<()> {
    bail!("TODO(linux): viewer window / video presenter not implemented")
}

pub fn run(_session: NativeViewerSession) -> Result<()> {
    bail!("TODO(linux): viewer window / video presenter not implemented")
}

/// Placeholder so `ui::window_manager` type-checks on Linux.
pub struct ConnectingWindowsRunner;
