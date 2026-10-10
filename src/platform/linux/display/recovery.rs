//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

use anyhow::{Context, Result, ensure};

pub(crate) struct Handle(std::convert::Infallible);

pub(crate) struct Serial(Handle);

impl Serial {
    pub fn acquire() -> Result<Self> {
        crate::platform::linux::unsupported()
    }
}

pub(crate) fn birth(handle: Handle) -> Result<u64> {
    crate::platform::linux::unsupported()
}

pub(crate) fn current_birth() -> Result<u64> {
    crate::platform::linux::unsupported()
}

pub(crate) fn owner_alive(pid: u32, created: u64) -> Result<bool> {
    crate::platform::linux::unsupported()
}

pub(crate) fn parent(pid: u32, created: u64) -> Result<Handle> {
    crate::platform::linux::unsupported()
}

pub(crate) fn exited(handle: &Handle, timeout: u32) -> Result<bool> {
    crate::platform::linux::unsupported()
}

pub(crate) fn start_guard(token: &str) -> Result<()> {
    crate::platform::linux::unsupported()
}

pub(crate) fn ready(token: &str) -> Result<()> {
    crate::platform::linux::unsupported()
}
