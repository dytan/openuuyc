//! One software playback decoder across OpenUUYC processes on this machine.
use anyhow::{Context, Result};
use std::fs::File;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

static OCCUPIED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub(crate) struct SoftwarePlaybackBusy;
impl std::fmt::Display for SoftwarePlaybackBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("已有一个软解窗口正在播放，请先关闭它或切换到硬解。整个客户端最多允许一个软解播放窗口。")
    }
}
impl std::error::Error for SoftwarePlaybackBusy {}

pub(crate) struct SoftwareSlot {
    _lock: File,
    _thread_bound: PhantomData<*const ()>,
}

impl SoftwareSlot {
    pub(crate) fn acquire() -> Result<Rc<Self>> {
        if OCCUPIED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(SoftwarePlaybackBusy.into());
        }
        let result = Self::acquire_global();
        if result.is_err() {
            OCCUPIED.store(false, Ordering::Release);
        }
        result
    }

    fn acquire_global() -> Result<Rc<Self>> {
        let dir = crate::paths::app_data_dir().context("软解互斥目录不可用")?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("software-video-playback.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .context("创建软解互斥锁失败")?;
        file.try_lock().map_err(|_| SoftwarePlaybackBusy)?;
        Ok(Rc::new(Self {
            _lock: file,
            _thread_bound: PhantomData,
        }))
    }
}

impl Drop for SoftwareSlot {
    fn drop(&mut self) {
        OCCUPIED.store(false, Ordering::Release);
    }
}
