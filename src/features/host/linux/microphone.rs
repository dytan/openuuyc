//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]


use super::{Lease, lock};

use crate::protocol::microphone as errors;

use anyhow::{Context, Result};

use bytes::Bytes;

use crossbeam_queue::ArrayQueue;

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use tokio_util::sync::CancellationToken;

#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Status {
    // Local publication revision, never a new network or IPC protocol field.
    #[serde(skip)]
    pub policy_generation: u64,
    pub enabled: bool,
    pub active: bool,
    #[serde(default)]
    pub speaker_active: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub routing_error: Option<String>,
    pub underrun_frames: u64,
}

pub(crate) fn error_code(error: &anyhow::Error) -> i32 {
        0
    }

pub(crate) struct Receiver {
    _never: std::convert::Infallible,
}

pub(crate) struct Session {
    _never: std::convert::Infallible,
}

impl Session {
    pub fn new(
        lease: Lease,
        cancel: CancellationToken,
        connected: Arc<AtomicBool>,
    ) -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub fn receiver(&self) -> Receiver {
        unreachable!("Windows host only")
    }
    pub fn close(&mut self) {}
}

impl Receiver {
    pub async fn policy(&self, enabled: bool) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn status(&self) -> tokio::sync::watch::Receiver<Status> {
        unreachable!("Windows host only")
    }
    pub fn transport_lost(&self) {}
    pub fn select_source(&self) -> u64 {
        0
    }
    pub fn receive(&self, generation: u64, data: Bytes, sequence: u16, timestamp: u32) {}
}
