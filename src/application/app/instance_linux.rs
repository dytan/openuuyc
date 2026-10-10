//! Single control-center instance per user session (Linux advisory locks).
//!
//! Windows activates the running window through named mutexes and window
//! messages. Linux has no portable equivalent across X11 and Wayland, so a
//! second launch simply exits after logging that one is already running.
use anyhow::{Context, Result};
use std::fs::File;

pub struct Instance(#[allow(dead_code, reason = "Holds the advisory lock.")] File);

static UPDATE_EXIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) fn take_update_exit() -> bool {
    UPDATE_EXIT.swap(false, std::sync::atomic::Ordering::AcqRel)
}

fn lock(stem: &str) -> Result<Option<Instance>> {
    let path = crate::paths::session_lock_path(stem);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("创建程序实例保护失败：{}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Instance(file))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => {
            Err(anyhow::Error::new(error).context("锁定程序实例保护失败"))
        }
    }
}

pub fn acquire() -> Result<Option<Instance>> {
    let instance = lock("control-center")?;
    if instance.is_none() {
        tracing::warn!("OpenUUYC 已在运行，请勿重复启动。");
    }
    Ok(instance)
}
pub fn acquire_background() -> Result<Option<Instance>> {
    lock("control-center")
}

/// Window registration feeds Windows activation messages; nothing to do here.
pub(crate) fn register_window(_window: &winit::window::Window) -> Result<()> {
    Ok(())
}

pub(crate) fn reserve_after_exit() -> Result<Option<Instance>> {
    lock("control-center")
}
pub(crate) fn reserve_installer() -> Result<Instance> {
    lock("installer")?.context("另一个维护程序正在运行")
}
pub(crate) fn reserve_update() -> Result<Instance> {
    lock("updating")?.context("另一个更新正在进行")
}

pub(crate) struct Running(std::convert::Infallible);
impl Running {
    pub fn close(self, _updating: bool) -> Result<()> {
        match self.0 {}
    }
}
pub(crate) fn running_installed() -> Result<Option<Running>> {
    Ok(None)
}

/// Desktop notification actions run in-process on Linux (notify-send has no
/// activation URI handoff), so there is no running instance to deliver to.
pub(crate) fn deliver_notification(_uri: &str) -> Result<()> {
    Ok(())
}
