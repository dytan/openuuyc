//! Single control-center instance per user session (Linux flock).
use anyhow::{Context, Result};
use std::fs::File;

pub struct Instance(File);

pub fn acquire() -> Result<Option<Instance>> {
    let path = crate::paths::session_lock_path("control-center");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("创建程序实例保护失败：{}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Instance(file))),
        Err(std::fs::TryLockError::WouldBlock) => {
            tracing::warn!("OpenUUYC 已在运行，请勿重复启动。");
            Ok(None)
        }
        Err(std::fs::TryLockError::Error(error)) => {
            Err(anyhow::Error::new(error).context("锁定程序实例保护失败"))
        }
    }
}

// Windows HWND registration — no-op on Linux.
pub(crate) fn register_window(_handle: isize) -> Result<()> {
    Ok(())
}
