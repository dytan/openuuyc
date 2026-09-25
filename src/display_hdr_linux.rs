//! Linux HDR/display capability probe stub.
use crate::capability::DisplayCapability;

pub(crate) fn capabilities(fallback_fps: u32) -> Vec<DisplayCapability> {
    // TODO(linux): query HDR via Wayland color-management / DRM when available.
    vec![DisplayCapability {
        id: 0,
        fps: fallback_fps,
        kind: 0,
        hdr: -1,
    }]
}

pub(crate) fn monitor_is_hdr(_monitor: isize) -> bool {
    false
}
