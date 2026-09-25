//! Cross-platform application data roots.
//! Windows keeps LOCALAPPDATA\OpenUUYC; Linux uses XDG (`$XDG_DATA_HOME` / `~/.local/share`).
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

/// Root directory for OpenUUYC local state (logs, settings, caches).
pub(crate) fn app_data_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .context("LOCALAPPDATA must be an absolute directory")?;
        return Ok(base.join("OpenUUYC"));
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            let path = PathBuf::from(xdg);
            if !path.is_absolute() {
                bail!("XDG_DATA_HOME must be absolute");
            }
            return Ok(path.join("openuuyc"));
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .context("HOME must be an absolute directory for OpenUUYC data")?;
        return Ok(home.join(".local/share/openuuyc"));
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        bail!("unsupported platform for app data directory")
    }
}

/// Lower-case `openuuyc` coordination directory used by auth locks (historical Windows path).
pub(crate) fn credential_coord_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .context("LOCALAPPDATA is required for session-store coordination")?;
        return Ok(base.join("openuuyc"));
    }
    #[cfg(target_os = "linux")]
    {
        Ok(app_data_dir()?.join("credentials"))
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        bail!("unsupported platform for credential coordination")
    }
}

/// Session-scoped advisory lock path (`$XDG_RUNTIME_DIR`, else `/tmp` + uid).
/// Prefer the runtime dir so locks die with the login session instead of lingering
/// under the durable XDG data directory.
#[cfg(target_os = "linux")]
pub(crate) fn session_lock_path(stem: &str) -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let uid = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("Uid:")?
                    .split_whitespace()
                    .next()
                    .map(str::to_owned)
            })
        })
        .unwrap_or_else(|| "user".to_owned());
    base.join(format!("openuuyc-{stem}-{uid}.lock"))
}
