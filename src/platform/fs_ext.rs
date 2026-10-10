//! Filesystem extension traits used by shared code.
//!
//! Windows re-exports the std traits. Linux provides inert equivalents: Windows
//! file attributes (hidden, reparse point) have no meaning there, and the
//! callers already use `symlink_metadata` to reject links.
#[cfg(windows)]
pub(crate) use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

#[cfg(not(windows))]
pub(crate) trait MetadataExt {
    /// No Windows attribute bits are ever set on Linux.
    fn file_attributes(&self) -> u32;
    /// Modification time as a Windows FILETIME (100 ns ticks since 1601).
    fn last_write_time(&self) -> u64;
}

#[cfg(not(windows))]
impl MetadataExt for std::fs::Metadata {
    fn file_attributes(&self) -> u32 {
        if self.is_dir() { 0x10 } else { 0 }
    }
    fn last_write_time(&self) -> u64 {
        const EPOCH_DELTA: u64 = 11_644_473_600;
        self.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| (d.as_secs() + EPOCH_DELTA) * 10_000_000 + u64::from(d.subsec_nanos() / 100))
            .unwrap_or(0)
    }
}

#[cfg(not(windows))]
pub(crate) trait OpenOptionsExt {
    fn share_mode(&mut self, mode: u32) -> &mut Self;
    fn custom_flags(&mut self, flags: u32) -> &mut Self;
}

#[cfg(not(windows))]
impl OpenOptionsExt for std::fs::OpenOptions {
    fn share_mode(&mut self, _mode: u32) -> &mut Self {
        self
    }
    /// FILE_FLAG_OPEN_REPARSE_POINT (0x0020_0000) maps to O_NOFOLLOW; other
    /// Windows flags have no Linux counterpart.
    fn custom_flags(&mut self, flags: u32) -> &mut Self {
        if flags & 0x0020_0000 != 0 {
            std::os::unix::fs::OpenOptionsExt::custom_flags(self, libc::O_NOFOLLOW);
        }
        self
    }
}
