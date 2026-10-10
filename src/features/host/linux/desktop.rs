//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use super::{VideoConfig, capture, encoder, format};

use crate::features::stream_control::publisher;

use crate::protocol::capability::DeviceCapability;

use anyhow::{Context, Result, ensure};

use std::sync::Arc;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Capabilities {
    pub screen: capture::Screen,
    pub codecs: Vec<format::Capability>,
    pub adapters: Vec<capture::EncodingAdapter>,
}

#[derive(Default)]
pub(super) struct Cache(tokio::sync::Mutex<Option<Arc<Capabilities>>>);

impl Cache {
    pub fn snapshot(&self) -> Option<Arc<Capabilities>> {
        unreachable!("Windows host only")
    }
    pub async fn prepare(
        &self,
        screen: capture::Screen,
        active: impl Fn() -> bool + Send + 'static,
    ) -> Result<Arc<Capabilities>> {
        unreachable!("Windows host only")
    }
    pub async fn replace(&self, value: Option<Capabilities>) {
        unreachable!("Windows host only")
    }
    pub async fn invalidate(&self) {
        unreachable!("Windows host only")
    }
}

pub(crate) struct Prepared {
    pub screen: capture::Screen,
    pub negotiated: Arc<format::Negotiated>,
    pub config: VideoConfig,
}

impl Prepared {
    pub fn new(
        options: &publisher::ConnectOptions,
        screen: capture::Screen,
        capabilities: &[format::Capability],
        remote: &DeviceCapability,
        settings: super::EncodingSettings,
    ) -> Result<Self> {
        crate::platform::linux::unsupported()
    }
}
