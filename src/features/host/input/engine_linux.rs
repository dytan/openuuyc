//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use super::wire::{Event, KeyOrButton};

use anyhow::{Result, ensure};

use serde::{Deserialize, Serialize};

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Screen {
    pub id: i32,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl Screen {
    pub fn rect(&self) -> Result<(i32, i32, i32, i32)> {
        crate::platform::linux::unsupported()
    }
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Geometry {
    pub current: i32,
    pub screens: Vec<Screen>,
}

impl Geometry {
    pub fn validate(&self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn screen(&self, id: Option<i32>) -> Result<&Screen> {
        crate::platform::linux::unsupported()
    }
}

pub(crate) struct Engine {
    _never: std::convert::Infallible,
}

impl Engine {
    pub fn new(
        hardware: bool,
        policy: super::wire::Policy,
        origin: u32,
        privileged: bool,
    ) -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub fn backend(&self) -> &'static str {
        unreachable!("Windows host only")
    }
    pub fn take_sas(&mut self) -> bool {
        unreachable!("Windows host only")
    }
    pub fn configure(&mut self, configuration: super::config::Configuration) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn mouse_policy(&self) -> super::config::MousePolicy {
        unreachable!("Windows host only")
    }
    pub fn synchronize(&mut self, geometry: Geometry) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn apply(
        &mut self,
        event: Event,
        geometry: Geometry,
        permitted: impl Fn() -> bool,
    ) -> Result<()> {
        unreachable!("Windows host only")
    }
    pub fn tick(&mut self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub fn release(&mut self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
}
