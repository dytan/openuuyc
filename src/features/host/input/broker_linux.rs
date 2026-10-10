//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use super::{
    engine::{Engine, Geometry},
    wire::Event,
};

use anyhow::{Result, ensure};

use serde::{Deserialize, Serialize};

use std::time::{Duration, Instant};

pub(crate) struct Remote {
    _never: std::convert::Infallible,
}

impl Remote {
    pub fn connect(
        policy: super::wire::Policy,
        permitted: impl Fn() -> bool,
    ) -> Result<Option<Self>> {
        crate::platform::linux::unsupported()
    }
    pub fn backend(&self) -> &str {
        ""
    }
    pub fn healthy(&self) -> bool {
        false
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

pub(crate) fn agent(name: &str, parent: u32) -> Result<()> {
    crate::platform::linux::unsupported()
}

pub(crate) fn serve(pipe: &str, permitted: impl Fn() -> bool) -> Result<()> {
        crate::platform::linux::unsupported()
    }

pub(super) fn secure_attention(permitted: impl Fn() -> bool) -> Result<()> {
        crate::platform::linux::unsupported()
    }
