//! Linux host screen capture.
//!
//! Preference, unless `OPENUUYC_CAPTURE` says otherwise:
//! - Wayland with `wlr-screencopy` (Hyprland, Sway, and other wlroots
//!   compositors): that protocol, cursor left out when the separate cursor
//!   channel is up. The ScreenCast portal is not opened.
//! - Wayland without `wlr-screencopy` (GNOME, KDE): XDG ScreenCast portal and
//!   a PipeWire stream (shared memory, including KDE's unmapped MemFd).
//! - X11: `GetImage` on the root window. The portal is not a fallback here.
//!
//! Frames are CPU BGRA buffers already scaled to the size the encoder will
//! use, mirroring what the Windows GPU preprocessing returns.
use anyhow::{Context, Result, bail, ensure};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::host::{scale, wayland};
pub(crate) use crate::media::capture::{Screen, SourceGone};

pub(crate) fn session_locked() -> Option<bool> {
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `zwlr_screencopy_manager_v1`.
    Wayland,
    /// XDG ScreenCast portal + PipeWire.
    Portal,
    X11,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Wayland => "wlr-screencopy",
            Self::Portal => "portal-screencast",
            Self::X11 => "X11",
        }
    }
}

pub(crate) fn wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Capture methods to try, best first.
///
/// `OPENUUYC_CAPTURE` is a comma-separated list of `wlr`, `portal`, and
/// `x11` (for example `portal` or `wlr,portal`). Without it, a Wayland
/// session uses wlr-screencopy when the compositor offers it and the portal
/// otherwise. An X11 session uses X11 only. A `wlr` entry is dropped when
/// the protocol is missing, so an explicit `wlr,portal` still reaches the
/// portal on GNOME/KDE.
pub(crate) fn preferred_backends(
    spec: Option<&str>,
    wayland: bool,
    wlr_available: bool,
    x11_display: bool,
) -> Result<Vec<Kind>> {
    let requested = if let Some(spec) = spec {
        let spec = spec.trim();
        ensure!(!spec.is_empty(), "OPENUUYC_CAPTURE 为空");
        let mut kinds = Vec::new();
        for name in spec.split(',') {
            kinds.push(match name.trim() {
                "wlr" => Kind::Wayland,
                "portal" => Kind::Portal,
                "x11" => Kind::X11,
                other => bail!("OPENUUYC_CAPTURE 中有未知的采集方式：{other}"),
            });
        }
        kinds
    } else if wayland {
        if wlr_available {
            vec![Kind::Wayland]
        } else {
            vec![Kind::Portal]
        }
    } else if x11_display {
        vec![Kind::X11]
    } else {
        Vec::new()
    };
    Ok(requested
        .into_iter()
        .filter(|kind| *kind != Kind::Wayland || wlr_available)
        .collect())
}

fn backends() -> Result<Vec<Kind>> {
    use std::sync::OnceLock;
    static ORDER: OnceLock<Result<Vec<Kind>, String>> = OnceLock::new();
    let order = ORDER.get_or_init(|| {
        let spec = std::env::var("OPENUUYC_CAPTURE").ok();
        preferred_backends(
            spec.as_deref(),
            wayland_session(),
            wayland::screencopy_available(),
            std::env::var_os("DISPLAY").is_some(),
        )
        .map_err(|error| format!("{error:#}"))
    });
    match order {
        Ok(kinds) => Ok(kinds.clone()),
        Err(error) => bail!("{error}"),
    }
}

/// The backend that will be tried first, for diagnostics.
pub(crate) fn kind() -> Option<Kind> {
    backends().ok().and_then(|kinds| kinds.into_iter().next())
}

/// Comma-separated backend labels, for the host self-test.
pub(crate) fn describe_order() -> String {
    match backends() {
        Ok(kinds) if kinds.is_empty() => "（无）".to_owned(),
        Ok(kinds) => kinds
            .iter()
            .map(|kind| kind.label())
            .collect::<Vec<_>>()
            .join(", "),
        Err(error) => format!("无法解析 OPENUUYC_CAPTURE：{error:#}"),
    }
}

/// The portal would open first and has no restore token yet, so `Start`
/// shows the desktop's share dialog.
pub(crate) fn portal_needs_consent() -> bool {
    kind() == Some(Kind::Portal) && !super::host::portal::remembered()
}

/// Why this session cannot be captured, for diagnostics and UI.
pub(crate) fn unavailable_reason() -> Option<&'static str> {
    match backends() {
        Err(_) => Some("OPENUUYC_CAPTURE 无法解析"),
        Ok(kinds) if !kinds.is_empty() => None,
        Ok(_) if wayland_session() => Some(
            "当前 Wayland 合成器没有可用的采集方式（未提供 wlr-screencopy，且 OPENUUYC_CAPTURE 没有选择 portal）",
        ),
        Ok(_) => Some("未检测到图形会话（缺少 WAYLAND_DISPLAY / DISPLAY）"),
    }
}

pub(crate) fn screens() -> Result<Vec<Screen>> {
    // Portal on an X11 session (explicit `OPENUUYC_CAPTURE`) pairs streams
    // with the X11 layout. Every Wayland session, including the portal
    // fallback, uses the compositor's outputs.
    let x11_layout = match kind() {
        None => return Ok(Vec::new()),
        Some(Kind::X11) => true,
        Some(Kind::Portal) => !wayland_session(),
        Some(Kind::Wayland) => false,
    };
    if !x11_layout {
        let outputs = wayland::outputs()?;
        let mut result = Vec::new();
        for (index, o) in outputs.iter().enumerate() {
            let scale = o.scale.max(1);
            let identity = format!("wayland/{}", o.name);
            let label = if o.description.is_empty() {
                format!("{} {}", o.make, o.model).trim().to_owned()
            } else {
                o.description.clone()
            };
            result.push(Screen {
                id: super::display::source_id(&identity)?,
                device_name: o.name.clone(),
                display_name: label,
                width: o.width as u32,
                height: o.height as u32,
                left: o.logical.0 * scale,
                top: o.logical.1 * scale,
                primary: (o.logical.0, o.logical.1) == (0, 0) || index == 0 && outputs.len() == 1,
                fps: ((o.refresh_mhz + 500) / 1000).clamp(1, 240) as u32,
                dpi_scale: Some((scale * 100) as u32),
                hdr: false,
                adapter: 0,
                render_adapter: None,
                identity: Some(identity),
            });
        }
        if !result.iter().any(|s| s.primary)
            && let Some(first) = result.first_mut()
        {
            first.primary = true;
        }
        Ok(result)
    } else {
        let displays = display_info::DisplayInfo::all().context("枚举 X11 显示器失败")?;
        let mut result = Vec::new();
        for d in displays.iter().filter(|d| d.width > 0 && d.height > 0) {
            let identity = format!("x11/{}", d.name);
            result.push(Screen {
                id: super::display::source_id(&identity)?,
                device_name: d.name.clone(),
                display_name: d.friendly_name.clone(),
                width: d.width,
                height: d.height,
                left: d.x,
                top: d.y,
                primary: d.is_primary,
                fps: if d.frequency.is_finite() && d.frequency > 1.0 {
                    d.frequency.round() as u32
                } else {
                    60
                },
                dpi_scale: Some((d.scale_factor * 100.0).round().max(100.0) as u32),
                hdr: false,
                adapter: 0,
                render_adapter: None,
                identity: Some(identity),
            });
        }
        Ok(result)
    }
}

pub(crate) fn refresh(selected: &Screen) -> Result<Screen> {
    let mut found = screens()?
        .into_iter()
        .find(|s| s.identity == selected.identity || s.device_name == selected.device_name)
        .ok_or(SourceGone)?;
    found.id = selected.id;
    Ok(found)
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct EncodingAdapter {
    #[serde(default)]
    pub id: Option<crate::media::selection::GpuId>,
    pub luid: u64,
    pub vendor: u32,
    pub name: String,
}
/// Linux host encoding is CPU-only for now.
pub(crate) fn encoding_adapters() -> Result<Vec<EncodingAdapter>> {
    Ok(Vec::new())
}

/// Captured frame: tight BGRA rows (`width * 4` bytes each).
#[derive(Clone)]
pub(crate) struct Frame {
    pub width: u32,
    pub height: u32,
    pub texture: Arc<Vec<u8>>,
    pub captured: Instant,
    pub is_new: bool,
    pub hdr_metadata: Option<crate::media::video_color::HdrMetadata>,
}

enum Backend {
    Wayland(Box<wayland::Screencopy>),
    Portal(super::host::portal::Grab),
    X11(X11),
}

impl Backend {
    fn label(&self) -> &'static str {
        match self {
            Self::Wayland(_) => Kind::Wayland.label(),
            Self::Portal(_) => Kind::Portal.label(),
            Self::X11(_) => Kind::X11.label(),
        }
    }
}

fn open_backend(kind: Kind, screen: &Screen) -> Result<Backend> {
    match kind {
        Kind::Wayland => Ok(Backend::Wayland(Box::new(wayland::Screencopy::open(
            &screen.device_name,
        )?))),
        Kind::X11 => Ok(Backend::X11(X11::open(screen)?)),
        Kind::Portal => {
            // Embedded pointer, unless Hyprland is already publishing the
            // cursor on its own channel. The portal fixes this mode for the
            // life of the session.
            let embed_cursor = !super::host::pointer::available();
            Ok(Backend::Portal(super::host::portal::Grab::open(
                screen,
                embed_cursor,
            )?))
        }
    }
}

pub(crate) struct Desktop {
    pub screen: Screen,
    pub generation: u64,
    pub available: bool,
    pub cursor: Option<super::cursor_shape::Snapshot>,
    backend: Backend,
    scratch: Vec<u8>,
    last_hash: u64,
    /// ScreenSaver.Inhibit for this capture. Dropped with the desktop.
    _inhibit: Option<super::power_request::Request>,
}

impl Desktop {
    pub fn open_selected(selected: &Screen) -> Result<Self> {
        // A remote session has no local input, so the panel may be DPMS-off and
        // wlr-screencopy would deliver no frames. Wake it before capturing.
        // No-op unless this is Hyprland. Also runs when the portal was
        // explicitly selected there, so that path does not stall on a blank panel.
        if wayland_session() {
            super::host::pointer::wake_display();
        }
        let screen = refresh(selected)?;
        let kinds = backends()?;
        if kinds.is_empty() {
            bail!(unavailable_reason().unwrap_or("无法采集屏幕"));
        }
        let mut errors = Vec::new();
        for kind in kinds {
            match open_backend(kind, &screen) {
                Ok(backend) => {
                    tracing::info!(
                        output = %screen.device_name,
                        width = screen.width,
                        height = screen.height,
                        backend = backend.label(),
                        "Linux host capture opened"
                    );
                    let inhibit = match super::power_request::Request::capture() {
                        Ok(request) => Some(request),
                        Err(error) => {
                            tracing::debug!(
                                error = %format!("{error:#}"),
                                "host capture idle inhibition unavailable"
                            );
                            None
                        }
                    };
                    return Ok(Self {
                        screen,
                        generation: 0,
                        available: true,
                        cursor: None,
                        backend,
                        scratch: Vec::new(),
                        last_hash: 0,
                        _inhibit: inhibit,
                    });
                }
                Err(error) => {
                    tracing::info!(
                        backend = kind.label(),
                        error = %format!("{error:#}"),
                        "host capture backend unavailable"
                    );
                    errors.push(format!("{}: {error:#}", kind.label()));
                }
            }
        }
        bail!("无法采集屏幕：{}", errors.join("；"))
    }
    pub fn backend_name(&self) -> &'static str {
        self.backend.label()
    }
    /// Why a portal stream has not produced a picture yet.
    pub fn capture_stage(&self) -> Option<String> {
        match &self.backend {
            Backend::Portal(grab) => Some(grab.stage()),
            Backend::Wayland(_) | Backend::X11(_) => None,
        }
    }
    pub fn hdr_available(&self) -> bool {
        false
    }
    /// Capture, then convert/scale to the encoder size for `quality`/`maximum`.
    /// On Hyprland the cursor is published separately (see `host::pointer`) and
    /// excluded from the frame; otherwise it is composited in as a fallback.
    pub fn next(
        &mut self,
        timeout: u32,
        quality: i32,
        cursor: bool,
        hdr: bool,
        maximum: (u32, u32),
    ) -> Result<Option<Frame>> {
        Ok(self
            .next_timed(timeout, quality, cursor, hdr, maximum)?
            .map(|(f, _, _)| f))
    }

    /// Like [`Self::next`] but also returns the grab and scale/convert
    /// durations, for the `host-selftest --bench` profiler.
    pub fn next_timed(
        &mut self,
        timeout: u32,
        quality: i32,
        _cursor: bool,
        _hdr: bool,
        maximum: (u32, u32),
    ) -> Result<Option<(Frame, Duration, Duration)>> {
        let timeout = Duration::from_millis(u64::from(timeout.max(1)));
        let captured = Instant::now();
        let mut out = std::mem::take(&mut self.scratch);
        let grab_dur;
        let scale_dur;
        let size = match &mut self.backend {
            Backend::Portal(copy) => {
                let grab_started = Instant::now();
                let grabbed = copy.next(timeout)?;
                grab_dur = grab_started.elapsed();
                let Some(raw) = grabbed else {
                    self.scratch = out;
                    return Ok(None);
                };
                let size = target(raw.width, raw.height, quality, maximum);
                if (raw.width, raw.height) != (self.screen.width, self.screen.height) {
                    self.screen.width = raw.width;
                    self.screen.height = raw.height;
                    self.generation = self.generation.wrapping_add(1);
                }
                let scale_started = Instant::now();
                scale::convert(
                    &raw.pixels,
                    raw.width,
                    raw.height,
                    raw.width as usize * 4,
                    false,
                    false,
                    size.0,
                    size.1,
                    &mut out,
                );
                scale_dur = scale_started.elapsed();
                size
            }
            Backend::Wayland(copy) => {
                let grab_started = Instant::now();
                // Match the Windows host: when the separate cursor channel
                // is available do not composite the pointer into the frame,
                // so cursor-only moves cause no screen damage and cost
                // nothing on the video path. Fall back to compositing when
                // we cannot publish the cursor separately.
                let overlay_cursor = !super::host::pointer::available();
                let grabbed = copy.grab(overlay_cursor, timeout)?;
                grab_dur = grab_started.elapsed();
                let Some(raw) = grabbed else {
                    self.scratch = out;
                    return Ok(None);
                };
                let size = target(raw.width, raw.height, quality, maximum);
                if (raw.width, raw.height) != (self.screen.width, self.screen.height) {
                    self.screen.width = raw.width;
                    self.screen.height = raw.height;
                    self.generation = self.generation.wrapping_add(1);
                }
                let scale_started = Instant::now();
                scale::convert(
                    raw.data,
                    raw.width,
                    raw.height,
                    raw.stride as usize,
                    raw.rgb,
                    raw.y_invert,
                    size.0,
                    size.1,
                    &mut out,
                );
                scale_dur = scale_started.elapsed();
                size
            }
            Backend::X11(x) => {
                let grab_started = Instant::now();
                let (data, w, h) = x.grab()?;
                grab_dur = grab_started.elapsed();
                let size = target(w, h, quality, maximum);
                let scale_started = Instant::now();
                scale::convert(
                    &data,
                    w,
                    h,
                    w as usize * 4,
                    false,
                    false,
                    size.0,
                    size.1,
                    &mut out,
                );
                scale_dur = scale_started.elapsed();
                size
            }
        };
        let hash = sample_hash(&out);
        let is_new = hash != self.last_hash;
        self.last_hash = hash;
        let frame = Frame {
            width: size.0,
            height: size.1,
            texture: Arc::new(out),
            captured,
            is_new,
            hdr_metadata: None,
        };
        Ok(Some((frame, grab_dur, scale_dur)))
    }

    /// Grab one frame at native resolution, normalised to packed BGRA (B,G,R,X),
    /// for the GPU VPP path. Returns the pixels, dimensions, and the grab and
    /// CPU-normalise durations. Wayland screencopy and the portal both supply
    /// a CPU frame; X11 does not.
    pub fn next_full(
        &mut self,
        timeout: u32,
    ) -> Result<Option<(Vec<u8>, u32, u32, Duration, Duration)>> {
        let timeout = Duration::from_millis(u64::from(timeout.max(1)));
        match &mut self.backend {
            Backend::Portal(copy) => {
                let grab_started = Instant::now();
                let grabbed = copy.next(timeout)?;
                let grab_dur = grab_started.elapsed();
                let Some(raw) = grabbed else { return Ok(None) };
                Ok(Some((
                    raw.pixels,
                    raw.width,
                    raw.height,
                    grab_dur,
                    Duration::ZERO,
                )))
            }
            Backend::Wayland(copy) => {
                let overlay_cursor = !super::host::pointer::available();
                let grab_started = Instant::now();
                let grabbed = copy.grab(overlay_cursor, timeout)?;
                let grab_dur = grab_started.elapsed();
                let Some(raw) = grabbed else { return Ok(None) };
                let mut out = Vec::new();
                let norm_started = Instant::now();
                scale::convert(
                    raw.data,
                    raw.width,
                    raw.height,
                    raw.stride as usize,
                    raw.rgb,
                    raw.y_invert,
                    raw.width,
                    raw.height,
                    &mut out,
                );
                let norm_dur = norm_started.elapsed();
                Ok(Some((out, raw.width, raw.height, grab_dur, norm_dur)))
            }
            Backend::X11(_) => Ok(None),
        }
    }
}

fn target(width: u32, height: u32, quality: i32, maximum: (u32, u32)) -> (u32, u32) {
    let size = crate::media::geometry::output_size(width, height, quality);
    crate::media::geometry::fit_size(size.0, size.1, maximum)
}

/// Cheap change detector over a sparse sample of the frame.
fn sample_hash(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let step = (data.len() / 4096).max(1) | 1;
    let mut i = 0;
    while i < data.len() {
        hash = (hash ^ u64::from(data[i])).wrapping_mul(0x100_0000_01b3);
        i += step;
    }
    hash
}

struct X11 {
    connection: x11rb::rust_connection::RustConnection,
    root: u32,
    rect: (i16, i16, u16, u16),
}
impl X11 {
    fn open(screen: &Screen) -> Result<Self> {
        use x11rb::connection::Connection as _;
        let (connection, index) = x11rb::connect(None).context("无法连接 X11 显示服务")?;
        let root = connection.setup().roots[index].root;
        Ok(Self {
            connection,
            root,
            rect: (
                screen.left as i16,
                screen.top as i16,
                screen.width as u16,
                screen.height as u16,
            ),
        })
    }
    fn grab(&mut self) -> Result<(Vec<u8>, u32, u32)> {
        use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat};
        let (x, y, w, h) = self.rect;
        let reply = self
            .connection
            .get_image(ImageFormat::Z_PIXMAP, self.root, x, y, w, h, !0)?
            .reply()
            .context("X11 截屏失败")?;
        anyhow::ensure!(
            reply.data.len() >= usize::from(w) * usize::from(h) * 4,
            "X11 截屏像素格式不受支持（需要 24/32 位色深）"
        );
        Ok((reply.data, u32::from(w), u32::from(h)))
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, preferred_backends};

    #[test]
    fn wayland_with_wlr_stays_on_wlr() {
        assert_eq!(
            preferred_backends(None, true, true, true).unwrap(),
            vec![Kind::Wayland]
        );
    }

    #[test]
    fn wayland_without_wlr_uses_the_portal() {
        assert_eq!(
            preferred_backends(None, true, false, true).unwrap(),
            vec![Kind::Portal]
        );
    }

    #[test]
    fn x11_does_not_fall_through_to_the_portal() {
        assert_eq!(
            preferred_backends(None, false, false, true).unwrap(),
            vec![Kind::X11]
        );
    }

    #[test]
    fn no_session_has_no_backend() {
        assert!(
            preferred_backends(None, false, false, false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_explicit_portal_overrides_a_working_wlr() {
        assert_eq!(
            preferred_backends(Some("portal"), true, true, true).unwrap(),
            vec![Kind::Portal]
        );
    }

    #[test]
    fn an_explicit_list_skips_wlr_when_it_is_missing() {
        assert_eq!(
            preferred_backends(Some("wlr,portal"), true, false, true).unwrap(),
            vec![Kind::Portal]
        );
    }

    #[test]
    fn forcing_wlr_does_not_silently_select_the_portal() {
        assert!(
            preferred_backends(Some("wlr"), true, false, false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn unknown_and_empty_overrides_fail() {
        assert!(preferred_backends(Some("nvfbc"), true, true, true).is_err());
        assert!(preferred_backends(Some(""), true, true, true).is_err());
        assert!(preferred_backends(Some("wlr,nope"), true, true, true).is_err());
    }
}
