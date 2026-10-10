//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use anyhow::Result;

use super::{Lease, lock};

use crate::media::audio::{
    encoder::Config,
    sender::{Frame, RATE, Source, Transmitter},
};

use crossbeam_queue::ArrayQueue;

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::sync::Notify;

use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DefaultDevices {
    #[default]
    Keep,
    Microphone,
    Speakers,
    Both,
}

impl DefaultDevices {
    pub fn label(self) -> &'static str {
        unreachable!("Windows host only")
    }
    pub fn microphone(self) -> bool {
        unreachable!("Windows host only")
    }
    pub fn speakers(self) -> bool {
        unreachable!("Windows host only")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Device {
    pub id: String,
    pub name: String,
}

impl Device {
    pub fn validate(&self) -> anyhow::Result<()> {
        crate::platform::linux::unsupported()
    }
}

#[derive(Clone, Default)]
pub(crate) struct Inventory {
    pub devices: Vec<Device>,
    pub error: Option<String>,
    pub pending: bool,
    pub updated: Option<Instant>,
}

#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Status {
    pub configured: bool,
    pub capturing: bool,
    pub device: String,
    pub error: Option<String>,
    pub packets: u64,
    #[serde(default)]
    pub target_bitrate: u32,
}

#[derive(Clone)]
pub(crate) struct Audio(std::convert::Infallible);

impl Audio {
    pub fn new(
        lease: Lease,
        cancel: CancellationToken,
        connected: Arc<AtomicBool>,
        transport: super::transport::Transport,
    ) -> Self {
        unreachable!("Windows host only")
    }
    pub fn enable_quality_control(&self, enabled: bool, audio_only: bool) {
        unreachable!("Windows host only")
    }
    pub fn quality_hello(&self) -> Option<Vec<u8>> {
        unreachable!("Windows host only")
    }
    pub fn quality_request(&self, bytes: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        crate::platform::linux::unsupported()
    }
    pub fn configure(&self, encoding: Option<Config>) {
        unreachable!("Windows host only")
    }
    pub fn set_media_allowed(&self, allowed: bool) {
        unreachable!("Windows host only")
    }
    pub fn allowed(&self) -> bool {
        unreachable!("Windows host only")
    }
    pub fn transport_lost(&self) {
        unreachable!("Windows host only")
    }
    pub fn transmitter(&self) -> &Transmitter {
        unreachable!("Windows host only")
    }
    pub fn run(&self) {
        unreachable!("Windows host only")
    }
}
