//! Linux host screen capture: wlroots screencopy on Wayland, X11 GetImage on
//! Xorg sessions. Frames are CPU BGRA buffers already scaled to the size the
//! encoder will use, mirroring what the Windows GPU preprocessing returns.
use anyhow::{Context, Result, bail};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) use crate::media::capture::{Screen, SourceGone};
use super::host::{scale, wayland};

pub(crate) fn session_locked() -> Option<bool> {
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Wayland,
    X11,
}

pub(crate) fn kind() -> Option<Kind> {
    use std::sync::OnceLock;
    static KIND: OnceLock<Option<Kind>> = OnceLock::new();
    *KIND.get_or_init(|| {
        if wayland::screencopy_available() {
            Some(Kind::Wayland)
        } else if std::env::var_os("WAYLAND_DISPLAY").is_none()
            && std::env::var_os("DISPLAY").is_some()
        {
            Some(Kind::X11)
        } else {
            None
        }
    })
}

/// Why this session cannot be captured, for diagnostics and UI.
pub(crate) fn unavailable_reason() -> Option<&'static str> {
    match kind() {
        Some(_) => None,
        None if std::env::var_os("WAYLAND_DISPLAY").is_some() => Some(
            "当前 Wayland 合成器不支持 wlr-screencopy（Hyprland/Sway 等支持；GNOME/KDE 暂不支持被控）",
        ),
        None => Some("未检测到图形会话（缺少 WAYLAND_DISPLAY / DISPLAY）"),
    }
}

pub(crate) fn screens() -> Result<Vec<Screen>> {
    match kind() {
        Some(Kind::Wayland) => {
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
        }
        Some(Kind::X11) => {
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
        None => Ok(Vec::new()),
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
    X11(X11),
}

pub(crate) struct Desktop {
    pub screen: Screen,
    pub generation: u64,
    pub available: bool,
    pub cursor: Option<super::cursor_shape::Snapshot>,
    backend: Backend,
    scratch: Vec<u8>,
    last_hash: u64,
}

impl Desktop {
    pub fn open_selected(selected: &Screen) -> Result<Self> {
        let screen = refresh(selected)?;
        let backend = match kind() {
            Some(Kind::Wayland) => {
                Backend::Wayland(Box::new(wayland::Screencopy::open(&screen.device_name)?))
            }
            Some(Kind::X11) => Backend::X11(X11::open(&screen)?),
            None => bail!(unavailable_reason().unwrap_or("无法采集屏幕")),
        };
        tracing::info!(output = %screen.device_name, width = screen.width, height = screen.height,
            backend = match &backend { Backend::Wayland(_) => "wlr-screencopy", Backend::X11(_) => "X11" },
            "Linux host capture opened");
        Ok(Self {
            screen,
            generation: 0,
            available: true,
            cursor: None,
            backend,
            scratch: Vec::new(),
            last_hash: 0,
        })
    }
    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            Backend::Wayland(_) => "wlr-screencopy",
            Backend::X11(_) => "X11",
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
        Ok(self.next_timed(timeout, quality, cursor, hdr, maximum)?.map(|(f, _, _)| f))
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
        let mut grab_dur = Duration::ZERO;
        let mut scale_dur = Duration::ZERO;
        let size = match &mut self.backend {
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
                scale::convert(&data, w, h, w as usize * 4, false, false, size.0, size.1, &mut out);
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
    /// CPU-normalise durations. Only implemented for the Wayland backend.
    pub fn next_full(
        &mut self,
        timeout: u32,
    ) -> Result<Option<(Vec<u8>, u32, u32, Duration, Duration)>> {
        let timeout = Duration::from_millis(u64::from(timeout.max(1)));
        match &mut self.backend {
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
