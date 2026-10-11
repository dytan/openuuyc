//! Linux host microphone: no virtual capture device yet, so remote microphone
//! audio is accepted on the wire and dropped.

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

/// Remote microphone playback needs a virtual capture device on the host;
/// Linux keeps the channel alive but reports it as unavailable.
#[derive(Clone)]
pub(crate) struct Receiver {
    status: tokio::sync::watch::Receiver<Status>,
}

pub(crate) struct Session {
    _status: tokio::sync::watch::Sender<Status>,
    receiver: Receiver,
}

impl Session {
    pub fn new(
        lease: Lease,
        cancel: CancellationToken,
        connected: Arc<AtomicBool>,
    ) -> Result<Self> {
        let (tx, rx) = tokio::sync::watch::channel(Status::default());
        Ok(Self {
            _status: tx,
            receiver: Receiver { status: rx },
        })
    }
    pub fn receiver(&self) -> Receiver {
        self.receiver.clone()
    }
    pub fn close(&mut self) {}
}

impl Receiver {
    pub async fn policy(&self, enabled: bool) -> Result<()> {
        if enabled {
            anyhow::bail!("Linux 被控暂不支持远端麦克风")
        }
        Ok(())
    }
    pub fn status(&self) -> tokio::sync::watch::Receiver<Status> {
        self.status.clone()
    }
    pub fn transport_lost(&self) {}
    pub fn select_source(&self) -> u64 {
        0
    }
    pub fn receive(&self, generation: u64, data: Bytes, sequence: u16, timestamp: u32) {}
}
