//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use anyhow::Result;

use super::*;



pub(crate) fn watch(token: &str) -> Result<()> {
    crate::platform::linux::unsupported()
}

pub(crate) fn recover_abandoned() -> Result<()> {
    crate::platform::linux::unsupported()
}
