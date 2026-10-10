//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use anyhow::Result;

use super::*;

use crate::platform::display::{
    topology::VirtualProvider,
    virtual_driver::{FALLBACK_ID, Output},
};

use serde::{Deserialize, Serialize};

use std::time::Instant;

pub(super) fn target(output: Output, targets: &[Target]) -> Option<&Target> {
        None
    }

pub(crate) fn pin() -> Result<Option<Driver>> {
    crate::platform::linux::unsupported()
}

pub(super) fn screen(driver: &Driver) -> Result<Option<capture::Screen>> {
    crate::platform::linux::unsupported()
}

pub(super) fn ensure(
    driver: &Driver,
    width: u32,
    height: u32,
    ignore: &[String],
    verify_frame: bool,
) -> Result<Option<capture::Screen>> {
    crate::platform::linux::unsupported()
}

pub(super) fn retire(driver: &Driver, replacement_identity: Option<&str>) -> Result<bool> {
    crate::platform::linux::unsupported()
}

pub(crate) struct Maintainer {
    _never: std::convert::Infallible,
}

impl Maintainer {
    pub(crate) fn tick(&mut self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
}

pub(crate) fn start_background() {}
