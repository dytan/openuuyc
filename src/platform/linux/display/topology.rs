//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use anyhow::{Context, Result, bail, ensure};

use serde::{Deserialize, Serialize};

use std::mem::size_of;

/// CDS_TEST rejected the requested mode before any change was applied. Initial
/// preferences may retain the current mode; explicit runtime requests still fail.
#[derive(Debug, thiserror::Error)]
#[error("显示驱动拒绝分辨率：{0}")]
pub(crate) struct ModeTestRejected(pub i32);

pub(crate) const DPI_VALUES: [u32; 12] =
    [100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Mode {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dpi {
    pub current: u32,
    pub recommended: u32,
    pub supported: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VirtualProvider {
    Uu,
    OpenUuyc,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub identity: String,
    pub name: String,
    pub source: String,
    pub adapter: u64,
    pub adapter_path: String,
    pub target: u32,
    pub source_adapter: u64,
    pub source_id: u32,
    pub active: bool,
    pub available: bool,
    /// Both providers use the official virtual-screen wire type. Native
    /// operations and session cleanup still verify the actual driver/owner.
    pub virtual_provider: Option<VirtualProvider>,
    pub modes: Vec<Mode>,
    pub dpi: Option<Dpi>,
}

pub(crate) struct Topology {
    _never: std::convert::Infallible,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SavedTarget {
    pub identity: String,
    pub source_group: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub rotation: i32,
    pub refresh_numerator: u32,
    pub refresh_denominator: u32,
    pub scan_line_ordering: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SavedTopology {
    pub targets: Vec<SavedTarget>,
}


impl Topology {
    pub(crate) fn snapshot(&self) -> Result<SavedTopology> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn query(active_only: bool) -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn targets(&self) -> Result<Vec<Target>> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn metadata(&self) -> Result<Vec<Target>> {
        crate::platform::linux::unsupported()
    }
}

impl SavedTopology {
    pub(crate) fn apply(&self) -> Result<Vec<String>> {
        crate::platform::linux::unsupported()
    }
}

pub(crate) fn modes(source: &str) -> Result<Vec<Mode>> {
    crate::platform::linux::unsupported()
}

impl Target {
    pub(crate) fn revalidate(&self) -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn set_dpi(&self, requested: u32, active: impl Fn() -> bool) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn set_resolution(
        &self,
        width: u32,
        height: u32,
        active: impl Fn() -> bool,
    ) -> Result<()> {
        crate::platform::linux::unsupported()
    }
}
