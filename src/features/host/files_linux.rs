//! Linux host: remote file transfer into this machine is not wired up yet
//! (upstream runs it through a Windows user-session agent). File requests are
//! recognised and consumed so they never reach the input path.
use super::Lease;
use crate::features::file_transfer::host as executor;
use anyhow::Result;
use std::sync::{Arc, atomic::AtomicBool};
use tokio_util::sync::CancellationToken;
use webrtc::data_channel::RTCDataChannel;

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Status {
    pub active: bool,
    pub error: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct Receiver;
impl Receiver {
    pub fn capabilities(&self, _capabilities: executor::Capabilities) {}
    pub fn bind(&self, _channel: &Arc<RTCDataChannel>) {}
    pub fn close(&self, _channel: &Arc<RTCDataChannel>) {}
    fn consume(bytes: &[u8]) -> Result<bool> {
        Ok(executor::decode(bytes)?.is_some() || executor::settings_requested(bytes)?)
    }
    pub fn receive_control(&self, bytes: &[u8]) -> Result<bool> {
        Self::consume(bytes)
    }
    pub async fn receive(&self, _channel: &Arc<RTCDataChannel>, bytes: &[u8]) -> Result<bool> {
        Self::consume(bytes)
    }
}

pub(super) async fn run(
    _receiver: Receiver,
    _lease: Lease,
    _connected: Arc<AtomicBool>,
    cancel: CancellationToken,
    _scope: String,
    _sequence: Arc<std::sync::atomic::AtomicI64>,
) {
    cancel.cancelled().await;
}
