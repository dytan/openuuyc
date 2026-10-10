//! Linux stub: this host-side module is Windows-only (generated from upstream signatures).

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

#[path = "displays_fallback.rs"]
pub(crate) mod fallback;


#[path = "displays_recovery.rs"]
pub(crate) mod recovery;


pub(crate) fn transfer_preferences(to_service: bool) -> anyhow::Result<()> {
    crate::platform::linux::unsupported()
}

use super::{Lease, capture, lock};

use crate::platform::display::{
    self,
    topology::{SavedTarget, SavedTopology, Target, Topology},
    virtual_driver::Driver,
};

use anyhow::{Context, Result, ensure};

use sha2::{Digest, Sha256};

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};


pub(crate) struct Session {
    _never: std::convert::Infallible,
}

impl Session {
    pub(crate) fn new(lease: Lease, remote: &str) -> Result<Arc<Self>> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn annotate(
        &self,
        screen: &mut capture::Screen,
        target: &mut Option<Target>,
    ) -> Result<(i32, i32)> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn owns(&self, identity: &str) -> bool {
        false
    }
    pub(crate) async fn create(&self, resolutions: Vec<(u32, u32)>) -> Result<capture::Screen> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn remove(&self, identity: String) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn ensure_fallback(
        &self,
        previous: capture::Screen,
    ) -> Result<Option<capture::Screen>> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn fallback_screen(&self) -> Result<Option<capture::Screen>> {
        crate::platform::linux::unsupported()
    }
    pub(crate) fn allows_fallback_retirement(&self) -> bool {
        false
    }
    pub(crate) async fn retire_fallback(&self, replacement: String) -> Result<bool> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn enter_super(
        &self,
        width: u32,
        height: u32,
        dpi: u32,
        use_saved: bool,
    ) -> Result<capture::Screen> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn quit_super(&self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn resolution_choice(&self, identity: String, kind: i32) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn set_resolution(
        &self,
        target: Target,
        width: u32,
        height: u32,
    ) -> Result<capture::Screen> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn set_dpi(&self, target: Target, dpi: u32) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn close(&self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
    pub(crate) async fn rollback_preparation(&self) -> Result<()> {
        crate::platform::linux::unsupported()
    }
}
