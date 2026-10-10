//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use super::{Action, Context, model::Model};

use anyhow::{Result, ensure};

use serde::{Deserialize, Serialize};

use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

/// Inert: the host annotation overlay is drawn by the Windows host agent.
#[derive(Default)]
pub(super) struct Backend;

impl Backend {
    pub fn interval(&self) -> Duration {
        Duration::from_secs(1)
    }
    pub fn exchange(
        &mut self,
        context: Context,
        command: Option<Action>,
        cancel: &CancellationToken,
    ) -> Result<i32> {
        crate::platform::linux::unsupported()
    }
}

pub(crate) fn run(name: &str, parent: u32) -> Result<()> {
    crate::platform::linux::unsupported()
}
