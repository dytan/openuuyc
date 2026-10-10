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
    pub const ALL: [Self; 4] = [Self::Keep, Self::Microphone, Self::Speakers, Self::Both];
    pub fn label(self) -> &'static str {
        match self {
            Self::Keep => "全部保持原有",
            Self::Microphone => "仅调整麦克风",
            Self::Speakers => "仅调整扬声器",
            Self::Both => "都调整",
        }
    }
    pub fn microphone(self) -> bool {
        matches!(self, Self::Microphone | Self::Both)
    }
    pub fn speakers(self) -> bool {
        matches!(self, Self::Speakers | Self::Both)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Device {
    pub id: String,
    pub name: String,
}

impl Device {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.id.is_empty()
                && self.id.len() <= 4096
                && !self.id.contains('\0')
                && self.name.len() <= 4096
                && !self.name.contains('\0'),
            "播放设备标识无效"
        );
        Ok(())
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
