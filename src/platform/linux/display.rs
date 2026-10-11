//! Local display enumeration (X11/Wayland via `display-info`).
// Virtual-display topology/driver/recovery are Windows host features.
#[path = "display/recovery.rs"]
pub(crate) mod recovery;
#[path = "display/topology.rs"]
pub(crate) mod topology;
#[path = "display/virtual_driver.rs"]
pub(crate) mod virtual_driver;
use crate::media::LocalDisplayInfo;
use anyhow::{Context, Result, bail};
use display_info::DisplayInfo;

pub fn detect_local_display() -> Result<LocalDisplayInfo> {
    let displays = DisplayInfo::all().context("failed to enumerate local displays")?;
    let display = displays
        .iter()
        .find(|display| display.is_primary)
        .or_else(|| {
            displays
                .iter()
                .max_by_key(|display| u64::from(display.width) * u64::from(display.height))
        })
        .context("no local display was detected")?;
    if display.width == 0 || display.height == 0 {
        bail!("local display reported an invalid resolution");
    }
    // Maximum refresh of active displays with a 30 Hz floor; the primary
    // display still supplies geometry.
    let refresh_hz = displays
        .iter()
        .filter(|display| display.frequency.is_finite() && display.frequency > 0.0)
        .map(|display| display.frequency.round() as u32)
        .max()
        .unwrap_or(30)
        .max(30);
    Ok(LocalDisplayInfo {
        width: display.width,
        height: display.height,
        refresh_hz,
    })
}

/// Current active display dimensions offered by the UU controller.
pub(crate) fn local_display_dimensions() -> Vec<(u32, u32)> {
    let mut modes = DisplayInfo::all()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d.width != 0 && d.height != 0)
        .map(|d| (d.width, d.height))
        .collect::<Vec<_>>();
    modes.sort_unstable();
    modes.dedup();
    if modes.is_empty() {
        modes.push((1920, 1080));
    }
    modes
}

/// Stable per-process protocol screen ids, keyed by output identity.
pub(crate) fn source_id(identity: &str) -> Result<i32> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static IDS: OnceLock<Mutex<HashMap<String, i32>>> = OnceLock::new();
    let mut ids = IDS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(id) = ids.get(identity) {
        return Ok(*id);
    }
    let id = i32::try_from(ids.len()).context("显示器标识已耗尽")?;
    ids.insert(identity.to_owned(), id);
    Ok(id)
}
