//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]



use anyhow::{Context, Result, ensure};

use std::{sync::Arc, time::Duration};

use tokio_util::sync::CancellationToken;

use webrtc::data_channel::data_channel_state::RTCDataChannelState;

pub(crate) struct UpdateNotice {
    _never: std::convert::Infallible,
}

impl UpdateNotice {
    pub(super) fn new(
        routes: super::ReportTarget,
        cancel: CancellationToken,
        resume: Option<crate::features::host::update_resume::Context>,
        screens: Arc<tokio::sync::Mutex<super::screens::Screens>>,
    ) -> Arc<Self> {
        unreachable!("Windows host only")
    }
    pub(crate) async fn send(&self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
}
