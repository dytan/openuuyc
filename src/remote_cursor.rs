//! Remote pointer state, independent of video decoding and local input.
//! GameViewerServer 589240: Message(15).SystemStateChange(2).CursorShape.
//! pos_x/y are image hotspots; coordinate_*_scale are sampled at shape changes,
//! not continuously transmitted. Watching uses capture-side cursor composition.

use std::sync::{Arc, Mutex};

use anyhow::{Result, ensure};
use prost::Message;

const MAX_PNG_BYTES: usize = 4 * 1024 * 1024;
const MAX_CURSOR_SIZE: u32 = 2048;

#[derive(Clone, Debug, PartialEq)]
pub struct CursorImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub hotspot: [u32; 2],
    pub system_type: i32,
}

#[derive(Clone, Debug)]
pub struct RemoteCursor {
    pub image: Arc<CursorImage>,
    /// Position sampled with this shape, not a live pointer position.
    pub sampled_position: Option<[f64; 2]>,
    pub screen_id: i32,
}

#[derive(Clone, Default)]
pub(crate) struct RemoteCursorState(Arc<Mutex<CursorState>>);

#[derive(Default)]
struct CursorState {
    cursor: Option<RemoteCursor>,
    hidden: bool,
    /// Host SystemStateChange(1).SecureDesktop — Winlogon / lock / UAC.
    secure_desktop: bool,
}

impl RemoteCursorState {
    pub fn snapshot(&self) -> Option<RemoteCursor> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .cursor
            .clone()
    }

    pub fn hidden(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).hidden
    }

    pub fn secure_desktop(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .secure_desktop
    }

    pub fn clear(&self) {
        {
            let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
            state.secure_desktop = false;
        }
        self.publish(None, false);
    }

    fn publish(&self, mut cursor: Option<RemoteCursor>, hidden: bool) {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if let (Some(previous), Some(next)) = (state.cursor.as_ref(), &mut cursor)
            && previous.image == next.image
        {
            next.image = previous.image.clone();
        }
        state.cursor = cursor;
        state.hidden = hidden;
    }

    pub fn receive(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() <= MAX_PNG_BYTES + 1024,
            "cursor state message too large"
        );
        let change = SystemStateChange::decode(bytes)?;
        match change.state {
            Some(SystemState::SecureDesktop(payload)) => {
                let info = decode_secure_desktop_info(&payload);
                let active = info.active;
                let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
                let changed = state.secure_desktop != active;
                state.secure_desktop = active;
                drop(state);
                tracing::info!(
                    active,
                    changed,
                    enabled = info.enabled,
                    session = info.session,
                    station = info.station,
                    flags = info.flags,
                    state_code = info.state_code,
                    payload_bytes = payload.len(),
                    payload_hex = %hex_prefix(&payload, 32),
                    "host SystemState SecureDesktop"
                );
                Ok(())
            }
            Some(SystemState::Permission(payload)) => {
                let perm = decode_permission_info(&payload);
                tracing::info!(
                    payload_bytes = payload.len(),
                    payload_hex = %hex_prefix(&payload, 32),
                    kind = perm.kind,
                    value = perm.value,
                    "host SystemState Permission"
                );
                Ok(())
            }
            Some(SystemState::PrivateScreen(payload)) => {
                tracing::info!(
                    payload_bytes = payload.len(),
                    payload_hex = %hex_prefix(&payload, 32),
                    "host SystemState PrivateScreen (ignored)"
                );
                Ok(())
            }
            Some(SystemState::Cursor(shape)) => {
                tracing::trace!(
                    screen = shape.screen_id,
                    kind = shape.cursor_type,
                    width = shape.width,
                    height = shape.height,
                    bytes = shape.byte_value.len(),
                    x = shape.coordinate_x_scale,
                    y = shape.coordinate_y_scale,
                    "remote cursor state"
                );
                let hidden = shape.cursor_type == -1;
                let parsed = parse_shape(shape);
                match parsed {
                    Ok(cursor) => self.publish(cursor, hidden),
                    Err(error) => {
                        self.clear();
                        return Err(error);
                    }
                }
                Ok(())
            }
            Some(other) => {
                tracing::debug!(
                    variant = system_state_label(&other),
                    "host SystemStateChange ignored"
                );
                Ok(())
            }
            None => Ok(()),
        }
    }
}

fn system_state_label(state: &SystemState) -> &'static str {
    match state {
        SystemState::SecureDesktop(_) => "SecureDesktop",
        SystemState::Cursor(_) => "Cursor",
        SystemState::Permission(_) => "Permission",
        SystemState::FileTransfer(_) => "FileTransfer",
        SystemState::PrivateScreen(_) => "PrivateScreen",
        SystemState::ClientUiReady(_) => "ClientUiReady",
    }
}

fn hex_prefix(bytes: &[u8], max: usize) -> String {
    bytes
        .iter()
        .take(max)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

/// Nested SecureDesktop body observed from a Windows host lock screen:
/// `enabled=1` (field 1), session/station often -1 (fields 3/4), flags/state 1 (5/6).
/// Empty payload is treated as active (oneof present ⇒ entered secure desktop).
fn decode_secure_desktop_info(payload: &[u8]) -> SecureDesktopDecoded {
    if payload.is_empty() {
        return SecureDesktopDecoded {
            active: true,
            enabled: true,
            session: 0,
            station: 0,
            flags: 0,
            state_code: 0,
        };
    }
    if let Ok(info) = SecureDesktopInfo::decode(payload) {
        // Field 1 (`enabled`) is the authoritative on/off bit from the host.
        let active = info.enabled || info.active;
        return SecureDesktopDecoded {
            active,
            enabled: active,
            session: info.session,
            station: info.station,
            flags: info.flags,
            state_code: info.state_code,
        };
    }
    SecureDesktopDecoded {
        active: true,
        enabled: true,
        session: 0,
        station: 0,
        flags: 0,
        state_code: 0,
    }
}

fn decode_permission_info(payload: &[u8]) -> PermissionDecoded {
    if let Ok(info) = PermissionInfo::decode(payload) {
        return PermissionDecoded {
            kind: info.kind,
            value: info.value,
        };
    }
    PermissionDecoded { kind: 0, value: 0 }
}

fn parse_shape(shape: CursorShape) -> Result<Option<RemoteCursor>> {
    // The official sender publishes type -1 with no image when hidden.
    if shape.cursor_type == -1 {
        return Ok(None);
    }
    let position = [shape.coordinate_x_scale, shape.coordinate_y_scale];
    let sampled_position = (shape.screen_id >= 0
        && position
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
    .then_some(position);
    ensure!(
        shape.byte_value.len() <= MAX_PNG_BYTES,
        "cursor PNG too large"
    );
    ensure!(
        shape.width >= 0 && shape.height >= 0,
        "negative cursor dimensions"
    );
    let (width, height) = (shape.width as u32, shape.height as u32);
    ensure!(
        width <= MAX_CURSOR_SIZE && height <= MAX_CURSOR_SIZE,
        "cursor dimensions too large"
    );
    if !shape.byte_value.is_empty() {
        ensure!(width > 0 && height > 0, "empty cursor dimensions");
        ensure!(
            shape.pos_x >= 0
                && shape.pos_y >= 0
                && shape.pos_x < shape.width
                && shape.pos_y < shape.height,
            "invalid cursor hotspot"
        );
    }
    Ok(Some(RemoteCursor {
        image: Arc::new(CursorImage {
            png: shape.byte_value,
            width,
            height,
            hotspot: [shape.pos_x.max(0) as u32, shape.pos_y.max(0) as u32],
            system_type: shape.cursor_type,
        }),
        sampled_position,
        screen_id: shape.screen_id,
    }))
}

#[derive(Clone, Copy, Debug, Default)]
struct SecureDesktopDecoded {
    active: bool,
    enabled: bool,
    session: i64,
    station: i64,
    flags: i32,
    state_code: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct PermissionDecoded {
    kind: i32,
    value: i32,
}

#[derive(Clone, PartialEq, Message)]
struct SecureDesktopInfo {
    #[prost(bool, tag = "1")]
    enabled: bool,
    #[prost(bool, tag = "2")]
    active: bool,
    /// Observed as int64 -1 on lock (session id unknown / Winlogon).
    #[prost(int64, tag = "3")]
    session: i64,
    #[prost(int64, tag = "4")]
    station: i64,
    #[prost(int32, tag = "5")]
    flags: i32,
    #[prost(int32, tag = "6")]
    state_code: i32,
}

#[derive(Clone, PartialEq, Message)]
struct PermissionInfo {
    #[prost(int32, tag = "1")]
    kind: i32,
    #[prost(int32, tag = "2")]
    value: i32,
}

#[derive(Clone, PartialEq, Message)]
struct SystemStateChange {
    #[prost(oneof = "SystemState", tags = "1, 2, 3, 4, 5, 6")]
    state: Option<SystemState>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
enum SystemState {
    #[prost(bytes, tag = "1")]
    SecureDesktop(Vec<u8>),
    #[prost(message, tag = "2")]
    Cursor(CursorShape),
    #[prost(bytes, tag = "3")]
    Permission(Vec<u8>),
    #[prost(bytes, tag = "4")]
    FileTransfer(Vec<u8>),
    #[prost(bytes, tag = "5")]
    PrivateScreen(Vec<u8>),
    // Preserve oneof ordering for this notification without enabling
    // the official remote-upgrade query that consumes this state.
    #[prost(bytes, tag = "6")]
    ClientUiReady(Vec<u8>),
}

#[derive(Clone, PartialEq, Message)]
struct CursorShape {
    #[prost(int32, tag = "1")]
    pos_x: i32,
    #[prost(int32, tag = "2")]
    pos_y: i32,
    #[prost(int32, tag = "3")]
    width: i32,
    #[prost(int32, tag = "4")]
    height: i32,
    #[prost(bytes, tag = "5")]
    byte_value: Vec<u8>,
    #[prost(int32, tag = "6")]
    cursor_type: i32,
    #[prost(double, tag = "7")]
    coordinate_x_scale: f64,
    #[prost(double, tag = "8")]
    coordinate_y_scale: f64,
    #[prost(int32, tag = "9")]
    screen_id: i32,
}
