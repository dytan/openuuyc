//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use super::{
    broker::Remote,
    engine::{Engine, Geometry},
    wire::Event,
};

use anyhow::Result;

pub(super) struct Native {
    _never: std::convert::Infallible,
}

pub(super) enum Backend {
    Local(Native),
    Service(Remote),
}

impl Backend {
    pub fn new(
        policy: super::wire::Policy,
        require_service: bool,
        permitted: impl Fn() -> bool,
    ) -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub fn service(&self) -> bool {
        false
    }
    pub fn healthy(&self) -> bool {
        false
    }
    pub fn backend(&self) -> &str {
        ""
    }
    pub fn apply(
        &mut self,
        events: Vec<Event>,
        geometry: Geometry,
        permitted: impl Fn() -> bool,
    ) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn release(&mut self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn synchronize(&mut self, geometry: Geometry) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn configure(&mut self, configuration: super::config::Configuration) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn mouse_policy(&self) -> super::config::MousePolicy {
        unreachable!("Windows host only")
    }
    pub fn tick(&mut self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
}
