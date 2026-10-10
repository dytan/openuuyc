//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]


use anyhow::{Context, Result, ensure};

use std::{mem::size_of, ptr};

use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Output {
    pub adapter: u64,
    pub target: u32,
}

pub(crate) const FALLBACK_ID: Uuid = Uuid::from_u128(0x7d7e98aa_6992_46e0_a833_ca92a0ab8563);

pub(crate) struct Driver(std::convert::Infallible);

#[repr(C)]
#[derive(Default)]
pub(crate) struct Watchdog {
    pub timeout: u32,
    pub countdown: u32,
}

impl Driver {
    pub(crate) fn device_instance() -> Result<String> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn open() -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn add(&self, id: Uuid, width: u32, height: u32, hz: u32) -> Result<Output> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn persistent_supported(&self) -> bool {
        false
    }
    pub(crate) fn add_fallback(&self, width: u32, height: u32, hz: u32) -> Result<Output> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn fallback(&self) -> Result<Option<Output>> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn pin_fallback(&self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn owns_fallback_pin(&self) -> bool {
        false
    }
    pub(crate) fn fallback_state(&self) -> Result<(Option<Output>, u32)> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn remove(&self, id: Uuid) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn render_adapter(&self, adapter: u64) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn watchdog(&self) -> Result<Watchdog> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn ping(&self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
}
