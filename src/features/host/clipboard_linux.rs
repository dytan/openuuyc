//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

pub(crate) mod agent {
    pub(crate) fn desktop_available() -> bool {
        false
    }
}

use crate::features::clipboard::Clipboard;

use crate::protocol::peer_platform::PeerPlatform;

use anyhow::{Result, ensure};

use serde::{Deserialize, Serialize};

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use std::time::Duration;

use tokio::sync::mpsc;

use tokio_util::sync::CancellationToken;

use webrtc::data_channel::RTCDataChannel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub files: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Status {
    pub active: bool,
    pub files: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub dragging: bool,
}

#[derive(Clone, Default)]
pub(crate) struct Receiver;

impl Receiver {
    pub fn official_drop(&self, catalog: Option<crate::account::feature_ability::FeatureCatalog>) {}
    pub fn native(&self, enabled: bool) {}
    pub fn bind_file(&self, channel: &Arc<RTCDataChannel>) {}
    pub fn close_file(&self, channel: &Arc<RTCDataChannel>) {}
    pub fn receive_file(&self, channel: &Arc<RTCDataChannel>, bytes: &[u8]) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn bind(&self, channel: u16) -> u64 {
        0
    }
    pub fn receive(&self, generation: u64, bytes: &[u8]) -> Result<bool> {
        crate::platform::linux::unsupported()
    }
}

pub(super) async fn run(
    receiver: Receiver,
    mut routes: tokio::sync::watch::Receiver<super::peer::ReportRoutes>,
    lease: super::Lease,
    connected: Arc<AtomicBool>,
    cancel: CancellationToken,
    platform: PeerPlatform,
    input: super::input::Receiver,
    screens: impl Fn() -> (Vec<crate::media::capture::Screen>, bool) + Send + Sync + 'static,
) {}

pub(super) fn snapshot(_clipboard: &Clipboard) -> Status {
    Status::default()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            files: true,
        }
    }
}
