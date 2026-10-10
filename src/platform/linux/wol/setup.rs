//! Host NIC Wake-on-LAN configuration is a Windows host feature; Linux reports it unsupported.

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use anyhow::{Context, Result, ensure};

use serde::{Deserialize, Serialize};

use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AdapterKey {
    pub guid: String,
    pub mac: String,
}

impl AdapterKey {
    pub fn validate(&self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Property {
    pub key: String,
    pub value: String,
    pub writable: bool,
}

impl Property {
    pub fn title(&self) -> &str {
        &self.key
    }
    pub fn enabled(&self) -> Option<bool> {
        None
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Adapter {
    pub key: AdapterKey,
    pub name: String,
    pub description: String,
    pub index: u32,
    pub connected: bool,
    pub items: Vec<Property>,
    pub errors: Vec<String>,
}

pub(crate) fn inspect(
    target: Option<&AdapterKey>,
    apply: bool,
    cancel: &CancellationToken,
) -> Result<Vec<Adapter>> {
    crate::platform::linux::unsupported()
}

pub(crate) fn configure_elevated(guid: String, mac: String) -> Result<()> {
    crate::platform::linux::unsupported()
}

pub(crate) fn configure(key: &AdapterKey, cancel: &CancellationToken) -> Result<Vec<Adapter>> {
    crate::platform::linux::unsupported()
}
