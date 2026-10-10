//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use super::{VideoConfig, capture, lock};

use crate::transport::rtc::ConnectionCore;

use anyhow::Result;

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::sync::mpsc;

use tokio_util::sync::CancellationToken;

use webrtc::{
    api::media_engine::MediaEngine,
    data_channel::RTCDataChannel,
    ice_transport::{ice_candidate::RTCIceCandidateInit, ice_server::RTCIceServer},
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        policy::ice_transport_policy::RTCIceTransportPolicy,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::{
        RTCPFeedback,
        rtp_codec::{
            RTCRtpCodecCapability, RTCRtpCodecParameters, RTCRtpHeaderExtensionCapability,
            RTPCodecType,
        },
    },
    track::track_local::{TrackLocal, track_local_static_rtp::TrackLocalStaticRTP},
};

pub(crate) struct Peer {
    _never: std::convert::Infallible,
}

pub(super) struct ReportRoutes {
    _never: std::convert::Infallible,
}

#[path = "peer_upgrade.rs"]
mod upgrade;

pub(crate) use upgrade::UpdateNotice;

impl Peer {
    pub(crate) async fn new(
        screen: Option<capture::Screen>,
        owner: super::SessionLease,
        cancel: CancellationToken,
        displays: Arc<super::displays::Session>,
        ice_servers: Vec<RTCIceServer>,
        relay: bool,
        initial: VideoConfig,
        control_screens: bool,
        negotiated: Arc<super::format::Negotiated>,
        network: super::network::Policy,
        input_policy: super::input::wire::Policy,
        deferred: Option<screens::Deferred>,
        audio_control: bool,
        annotation_extension: bool,
        diagnostics_extension: bool,
        drag_extension: bool,
        controlling_features: Option<crate::account::feature_ability::FeatureCatalog>,
        clipboard_platform: crate::protocol::peer_platform::PeerPlatform,
        clipboard_level: i32,
        file_capabilities: crate::features::file_transfer::host::Capabilities,
        data_only: bool,
        update_resume: Option<super::update_resume::Context>,
    ) -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn local_candidate(
        &self,
        mut candidate: RTCIceCandidateInit,
    ) -> Option<RTCIceCandidateInit> {
        None
    }
    pub(crate) async fn answer(
        &self,
        sdp: String,
        restart: bool,
        network_type: u8,
    ) -> Result<String> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn add_candidate(&self, mut candidate: RTCIceCandidateInit) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn access_revoked(&self) -> bool {
        false
    }
    pub(crate) fn displays(&self) -> Vec<crate::protocol::capability::DisplayCapability> {
        Vec::new()
    }
    pub(crate) fn take_prepared_capabilities(&self) -> Option<Vec<super::format::Capability>> {
        None
    }
    pub(crate) fn ended(&self) -> bool {
        false
    }
    pub(crate) fn network_change(&self) -> Option<u8> {
        None
    }
    pub(crate) async fn close(mut self) {}
    pub(crate) fn load_input_configuration(
        &mut self,
        client: crate::session::host_client::HostClient,
    ) {}
}






#[path = "peer_screens.rs"]
pub(crate) mod screens;



type ReportTarget = tokio::sync::watch::Sender<ReportRoutes>;

#[derive(Clone)]
pub(super) struct Published {
    _never: std::convert::Infallible,
}
