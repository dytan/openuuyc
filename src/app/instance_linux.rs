//! Single control-center instance per user session (Linux flock).
use anyhow::{Context, Result};
use std::fs::File;

pub struct Instance(File);

pub fn acquire() -> Result<Option<Instance>> {
    let dir = crate::paths::app_data_dir().context("无法确定实例保护目录")?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("control-center.lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .context("创建程序实例保护失败")?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Instance(file))),
        Err(_) => {
            tracing::warn!("OpenUUYC 已在运行，请勿重复启动。");
            Ok(None)
        }
    }
}

// Windows HWND registration — no-op on Linux.
pub(crate) fn register_window(_handle: isize) -> Result<()> {
    Ok(())
}
