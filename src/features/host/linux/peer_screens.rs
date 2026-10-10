//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]




use super::Published;
use crate::features::host::{
    Lease, VideoConfig, capture, format::Negotiated, lock, track::VideoTrack, transport::Transport,
};

use crate::platform::display::topology::{Target, Topology};

use anyhow::{Context, Result, ensure};

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
};

use tokio::sync::watch;

use tokio_util::sync::CancellationToken;

use webrtc::{
    peer_connection::RTCPeerConnection, rtp_transceiver::rtp_sender::RTCRtpSender,
    track::track_local::TrackLocal,
};

pub(crate) const TRACK_COUNT: usize = 5;

pub(super) struct Inventory {
    _never: std::convert::Infallible,
}

impl Inventory {
    pub fn read() -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub async fn collect(cancel: &CancellationToken) -> Result<Option<Self>> {
        crate::platform::linux::unsupported()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScreenInfo {
    pub screen: capture::Screen,
    pub initial: capture::Screen,
    pub target: Option<Target>,
    pub kind: i32,
    pub resolution_type: i32,
}

pub(super) struct Reports {
    pub media_ready: Arc<AtomicBool>,
    pub capabilities: Mutex<Option<Vec<crate::features::host::format::Capability>>>,
    pub catalog: Mutex<Vec<ScreenInfo>>,
    pub current: AtomicI32,
    pub sequence: Arc<std::sync::atomic::AtomicI64>,
    pub publications: Vec<Mutex<Option<watch::Receiver<Published>>>>,
    pub pointers: Vec<Arc<Mutex<Option<crate::platform::windows::cursor_shape::Snapshot>>>>,
    pub changed: watch::Sender<u64>,
}

impl Reports {
    pub fn notify(&self) {
        unreachable!("Windows host only")
    }
    pub fn media(&self) -> Vec<(usize, Published)> {
        unreachable!("Windows host only")
    }
}

pub(crate) struct Slot {
    _never: std::convert::Infallible,
}

pub(crate) struct Screens {
    _never: std::convert::Infallible,
}

pub(crate) struct Deferred {
    pub options: crate::features::stream_control::publisher::ConnectOptions,
    pub remote: crate::protocol::capability::DeviceCapability,
    pub encoding: crate::features::host::EncodingSettings,
}

impl Screens {
    pub(super) fn update_media_intent(&self) -> VideoConfig {
        unreachable!("Windows host only")
    }
    pub(crate) async fn resume_capture(&mut self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn prepare_video(&mut self) -> Result<Option<i32>> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn set_fps_limit(&mut self, limit: u32) {
        unreachable!("Windows host only")
    }
    pub(crate) fn set_media_default(&mut self, config: VideoConfig) {
        unreachable!("Windows host only")
    }
    pub(crate) fn handshake_source(&self) -> Option<capture::Screen> {
        unreachable!("Windows host only")
    }
    pub(crate) fn authorization(&self) -> Lease {
        unreachable!("Windows host only")
    }
    pub(super) async fn new(
        connection: Arc<RTCPeerConnection>,
        lease: Lease,
        cancel: CancellationToken,
        connected: Arc<AtomicBool>,
        initial: VideoConfig,
        negotiated: Arc<Negotiated>,
        network: Transport,
        selected: Option<capture::Screen>,
        displays: Arc<crate::features::host::displays::Session>,
        deferred: Option<Deferred>,
    ) -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn refresh(&mut self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(super) fn inventory_revision(&self) -> u64 {
        unreachable!("Windows host only")
    }
    pub(crate) fn info(&self, id: i32) -> Result<ScreenInfo> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn active_slot(&self, id: i32) -> Option<usize> {
        unreachable!("Windows host only")
    }
    pub(crate) fn available_slot(&self) -> Option<usize> {
        unreachable!("Windows host only")
    }
    pub(crate) async fn register(&mut self, indices: &[i32]) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn start(&mut self, id: i32) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn stop(&mut self, screen: i32) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(super) async fn maintain(&mut self, revision: u64, inventory: Inventory) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(super) async fn close(&mut self) {
        unreachable!("Windows host only")
    }
}
