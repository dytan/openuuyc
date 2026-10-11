//! Linux host display session: physical outputs only. Virtual displays,
//! resolution/DPI changes and topology journaling are Windows features.

#![allow(dead_code, unused_variables, unused_imports, clippy::all)]

#[path = "displays_fallback.rs"]
pub(crate) mod fallback;


#[path = "displays_recovery.rs"]
pub(crate) mod recovery;


pub(crate) fn transfer_preferences(to_service: bool) -> anyhow::Result<()> {
    Ok(())
}

use super::{Lease, capture, lock};
use crate::platform::display::virtual_driver::Driver;

use crate::platform::display::topology::Target;

use anyhow::{Context, Result, ensure};

use std::sync::Arc;

pub(crate) struct Session {
    lease: Lease,
}

impl Session {
    pub(crate) fn new(lease: Lease, remote: &str) -> Result<Arc<Self>> {
        Ok(Arc::new(Self { lease }))
    }
    pub(crate) async fn prepare(
        &self,
        options: crate::features::stream_control::publisher::ConnectOptions,
    ) -> Result<capture::Screen> {
        ensure!(
            options.kind == 1 && matches!(options.connect_type, 1 | 2),
            "不支持的被控连接类型"
        );
        let screens = tokio::task::spawn_blocking(capture::screens).await??;
        if screens.is_empty() {
            anyhow::bail!(
                capture::unavailable_reason().unwrap_or("没有可采集的显示器")
            );
        }
        screens
            .iter()
            .find(|s| s.primary)
            .or_else(|| screens.first())
            .cloned()
            .context("没有可采集的显示器")
    }
    pub(crate) fn initial_screen(&self, screen: &capture::Screen) -> capture::Screen {
        screen.clone()
    }
    pub(crate) fn annotate(
        &self,
        screen: &mut capture::Screen,
        target: &mut Option<Target>,
    ) -> Result<(i32, i32)> {
        Ok((0, 1))
    }
    pub(crate) fn owns(&self, identity: &str) -> bool {
        false
    }
    pub(crate) async fn create(&self, resolutions: Vec<(u32, u32)>) -> Result<capture::Screen> {
        anyhow::bail!("Linux 被控暂不支持虚拟显示器")
    }
    pub(crate) async fn remove(&self, identity: String) -> Result<()> {
        anyhow::bail!("Linux 被控暂不支持虚拟显示器")
    }
    pub(crate) async fn ensure_fallback(
        &self,
        previous: capture::Screen,
    ) -> Result<Option<capture::Screen>> {
        Ok(None)
    }
    pub(crate) fn fallback_screen(&self) -> Result<Option<capture::Screen>> {
        Ok(None)
    }
    pub(crate) fn allows_fallback_retirement(&self) -> bool {
        false
    }
    pub(crate) async fn retire_fallback(&self, replacement: String) -> Result<bool> {
        Ok(false)
    }
    pub(crate) async fn enter_super(
        &self,
        width: u32,
        height: u32,
        dpi: u32,
        use_saved: bool,
    ) -> Result<capture::Screen> {
        anyhow::bail!("Linux 被控暂不支持超级分辨率")
    }
    pub(crate) async fn quit_super(&self) -> Result<()> {
        Ok(())
    }
    pub(crate) async fn resolution_choice(&self, identity: String, kind: i32) -> Result<()> {
        Ok(())
    }
    pub(crate) async fn set_resolution(
        &self,
        target: Target,
        width: u32,
        height: u32,
    ) -> Result<capture::Screen> {
        anyhow::bail!("Linux 被控暂不支持修改分辨率")
    }
    pub(crate) async fn set_dpi(&self, target: Target, dpi: u32) -> Result<()> {
        anyhow::bail!("Linux 被控暂不支持修改缩放")
    }
    pub(crate) async fn close(&self) -> Result<()> {
        Ok(())
    }
    pub(crate) async fn rollback_preparation(&self) -> Result<()> {
        Ok(())
    }
}
