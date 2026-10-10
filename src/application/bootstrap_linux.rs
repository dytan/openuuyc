//! The Windows launcher/runtime cache has no Linux counterpart: packages
//! install the binary in place, so resources sit next to the executable.
use anyhow::Result;
use std::path::PathBuf;

pub fn initialize() -> Result<()> {
    Ok(())
}
pub(crate) fn resource_origin() -> Option<PathBuf> {
    None
}
pub(crate) fn cached() -> bool {
    false
}
pub fn ready() {}
pub fn handoff() {}
