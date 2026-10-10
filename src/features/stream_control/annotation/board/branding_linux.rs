//! Linux stub for GDI-based whiteboard branding strokes.
use super::*;
use anyhow::Result;

pub(super) fn build(
    _base: u32,
    _screen: i32,
    _metrics: Metrics,
    _background: [u8; 3],
) -> Result<Vec<Stroke>> {
    // TODO(linux): vectorize brand/logo strokes without GDI.
    Ok(Vec::new())
}
