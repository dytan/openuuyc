//! wlroots-family Wayland screen capture (zwlr_screencopy_manager_v1) and output
//!
//! Current path copies into a wl_shm buffer (CPU). GPU VPP scale+CSC is decode-
//! clean behind OPENUUYC_ZEROCOPY but still shm-bound (~20 ms grab). The next
//! speedup is zwlr_screencopy linux_dmabuf + gbm → VA DRM_PRIME_2 import into
//! the VPP source surface (cros-libva already exposes ExternalBufferDescriptor
//! / MemoryType::DrmPrime2). Keep that path behind the same env var with shm
//! fallback so the live stream never regresses.
//! inventory. Hyprland, Sway, river, labwc and Wayfire implement it; GNOME and
//! KDE need the ScreenCast portal instead, which is not wired up yet.
use anyhow::{Context, Result, bail, ensure};
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::time::{Duration, Instant};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, WEnum, delegate_noop,
    globals::{GlobalList, GlobalListContents, registry_queue_init},
    protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct Output {
    pub name: String,
    pub description: String,
    pub make: String,
    pub model: String,
    /// Current mode in physical pixels.
    pub width: i32,
    pub height: i32,
    pub refresh_mhz: i32,
    pub scale: i32,
    /// Compositor layout (logical) rectangle.
    pub logical: (i32, i32, i32, i32),
    pub transform: i32,
}

#[derive(Default)]
pub(super) struct State {
    pub outputs: Vec<(Option<wl_output::WlOutput>, Output)>,
    pub(super) frame: FrameState,
}

#[derive(Default)]
pub(super) struct FrameState {
    pub(super) buffer: Option<(u32, u32, u32, u32)>,
    /// linux_dmabuf offer: (fourcc, width, height).
    pub(super) dmabuf: Option<(u32, u32, u32)>,
    pub(super) buffer_done: bool,
    pub(super) y_invert: bool,
    pub(super) ready: bool,
    pub(super) failed: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_output::WlOutput, usize> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, output)) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            wl_output::Event::Geometry {
                make,
                model,
                transform,
                ..
            } => {
                output.make = make;
                output.model = model;
                output.transform = match transform {
                    WEnum::Value(t) => t as i32,
                    WEnum::Unknown(t) => t as i32,
                };
            }
            wl_output::Event::Mode {
                flags,
                width,
                height,
                refresh,
            } => {
                let current = match flags {
                    WEnum::Value(f) => f.contains(wl_output::Mode::Current),
                    WEnum::Unknown(f) => f & 1 != 0,
                };
                if current {
                    output.width = width;
                    output.height = height;
                    output.refresh_mhz = refresh;
                }
            }
            wl_output::Event::Scale { factor } => output.scale = factor,
            wl_output::Event::Name { name } => output.name = name,
            wl_output::Event::Description { description } => output.description = description,
            _ => {}
        }
    }
}
impl Dispatch<ZxdgOutputV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, output)) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.logical.0 = x;
                output.logical.1 = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.logical.2 = width;
                output.logical.3 = height;
            }
            zxdg_output_v1::Event::Name { name } if output.name.is_empty() => output.name = name,
            _ => {}
        }
    }
}
impl Dispatch<ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let frame = &mut state.frame;
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let format = match format {
                    WEnum::Value(f) => f as u32,
                    WEnum::Unknown(f) => f,
                };
                // Keep the first offered SHM format; dmabuf offers come separately.
                if frame.buffer.is_none() {
                    frame.buffer = Some((format, width, height, stride));
                }
            }
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                frame.y_invert = match flags {
                    WEnum::Value(f) => f.contains(zwlr_screencopy_frame_v1::Flags::YInvert),
                    WEnum::Unknown(f) => f & 1 != 0,
                };
            }
            zwlr_screencopy_frame_v1::Event::LinuxDmabuf {
                format,
                width,
                height,
            } => {
                if frame.dmabuf.is_none() {
                    frame.dmabuf = Some((format, width, height));
                }
            }
            zwlr_screencopy_frame_v1::Event::BufferDone => frame.buffer_done = true,
            zwlr_screencopy_frame_v1::Event::Ready { .. } => frame.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => frame.failed = true,
            _ => {}
        }
    }
}
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ZwlrScreencopyManagerV1);
delegate_noop!(State: ZxdgOutputManagerV1);

/// Dispatch until `done` or the deadline. Never blocks without a timeout.
pub(super) fn dispatch_until(
    connection: &Connection,
    queue: &mut EventQueue<State>,
    state: &mut State,
    deadline: Instant,
    done: impl Fn(&State) -> bool,
) -> Result<bool> {
    loop {
        queue.dispatch_pending(state)?;
        if done(state) {
            return Ok(true);
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        queue.flush()?;
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let fd = guard.connection_fd();
        let mut poll = libc::pollfd {
            fd: std::os::fd::AsRawFd::as_raw_fd(&fd),
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout = deadline.saturating_duration_since(now).as_millis().min(1000) as i32;
        let ready = unsafe { libc::poll(&mut poll, 1, timeout.max(1)) };
        if ready > 0 {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e).context("读取 Wayland 事件失败"),
            }
        } else {
            drop(guard);
        }
        let _ = connection;
    }
}

pub(super) fn connect() -> Result<(Connection, GlobalList, EventQueue<State>)> {
    let connection = Connection::connect_to_env().context("无法连接 Wayland 合成器")?;
    let (globals, queue) =
        registry_queue_init::<State>(&connection).context("读取 Wayland 全局对象失败")?;
    Ok((connection, globals, queue))
}

pub(super) fn bind_outputs(
    globals: &GlobalList,
    qh: &QueueHandle<State>,
    state: &mut State,
) -> Option<ZxdgOutputManagerV1> {
    let registry = globals.registry();
    let xdg = globals.bind::<ZxdgOutputManagerV1, _, _>(qh, 2..=3, ()).ok();
    for global in globals.contents().clone_list() {
        if global.interface == "wl_output" {
            let index = state.outputs.len();
            let output: wl_output::WlOutput =
                registry.bind(global.name, global.version.min(4), qh, index);
            if let Some(xdg) = &xdg {
                xdg.get_xdg_output(&output, qh, index);
            }
            state.outputs.push((Some(output), Output::default()));
        }
    }
    xdg
}

/// Whether the compositor exposes wlr-screencopy at all.
pub(crate) fn screencopy_available() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && connect().is_ok_and(|(_, globals, _)| {
            globals
                .contents()
                .with_list(|list| list.iter().any(|g| g.interface == "zwlr_screencopy_manager_v1"))
        })
}

pub(crate) fn outputs() -> Result<Vec<Output>> {
    let (connection, globals, mut queue) = connect()?;
    let qh = queue.handle();
    let mut state = State::default();
    let _xdg = bind_outputs(&globals, &qh, &mut state);
    let deadline = Instant::now() + Duration::from_secs(2);
    // Two roundtrips: wl_output events, then xdg-output events.
    for _ in 0..2 {
        queue.roundtrip(&mut state).context("Wayland 输出枚举失败")?;
        ensure!(Instant::now() < deadline, "Wayland 输出枚举超时");
    }
    Ok(state
        .outputs
        .into_iter()
        .map(|(_, o)| o)
        .filter(|o| o.width > 0 && o.height > 0)
        .collect())
}
impl Dispatch<wayland_client::protocol::wl_callback::WlCallback, ()> for State {
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_callback::WlCallback,
        _: wayland_client::protocol::wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// A raw BGRA-family frame borrowed from the shared-memory buffer.
pub(crate) struct Raw<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    /// True when the bytes are R,G,B,X (ABGR/XBGR8888) instead of B,G,R,X.
    pub rgb: bool,
    pub y_invert: bool,
    pub data: &'a [u8],
}

struct Shm {
    _fd: OwnedFd,
    map: memmap2::MmapMut,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    key: (u32, u32, u32, u32),
}
impl Drop for Shm {
    fn drop(&mut self) {
        self.buffer.destroy();
        self.pool.destroy();
    }
}

pub(crate) struct Screencopy {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    manager: ZwlrScreencopyManagerV1,
    shm: wl_shm::WlShm,
    output: wl_output::WlOutput,
    pub info: Output,
    buffer: Option<Shm>,
    _xdg: Option<ZxdgOutputManagerV1>,
}

const XRGB: u32 = 1; // wl_shm::Format::Xrgb8888
const ARGB: u32 = 0; // wl_shm::Format::Argb8888
const XBGR: u32 = 0x3432_4258; // fourcc XB24
const ABGR: u32 = 0x3432_4241; // fourcc AB24

impl Screencopy {
    pub(crate) fn open(name: &str) -> Result<Self> {
        let (connection, globals, mut queue) = connect()?;
        let qh = queue.handle();
        let manager = globals
            .bind::<ZwlrScreencopyManagerV1, _, _>(&qh, 1..=3, ())
            .context("合成器不支持 wlr-screencopy（GNOME/KDE 需要 ScreenCast 门户，暂未支持）")?;
        let shm = globals
            .bind::<wl_shm::WlShm, _, _>(&qh, 1..=1, ())
            .context("合成器缺少 wl_shm")?;
        let mut state = State::default();
        let xdg = bind_outputs(&globals, &qh, &mut state);
        queue.roundtrip(&mut state)?;
        queue.roundtrip(&mut state)?;
        let index = state
            .outputs
            .iter()
            .position(|(_, o)| o.name == name)
            .with_context(|| format!("找不到显示输出 {name}"))?;
        let output = state.outputs[index].0.clone().context("显示输出不可用")?;
        let info = state.outputs[index].1.clone();
        Ok(Self {
            connection,
            queue,
            state,
            manager,
            shm,
            output,
            info,
            buffer: None,
            _xdg: xdg,
        })
    }

    /// Capture one frame of the output. `None` on timeout.
    pub(crate) fn grab(&mut self, cursor: bool, timeout: Duration) -> Result<Option<Raw<'_>>> {
        let qh = self.queue.handle();
        self.state.frame = FrameState::default();
        let frame = self
            .manager
            .capture_output(i32::from(cursor), &self.output, &qh, ());
        let v3 = wayland_client::Proxy::version(&self.manager) >= 3;
        let deadline = Instant::now() + timeout.max(Duration::from_millis(50));
        let described = dispatch_until(
            &self.connection,
            &mut self.queue,
            &mut self.state,
            deadline,
            |s| s.frame.failed || (s.frame.buffer.is_some() && (!v3 || s.frame.buffer_done)),
        )?;
        if !described || self.state.frame.failed {
            frame.destroy();
            if self.state.frame.failed {
                bail!("合成器拒绝本次屏幕采集");
            }
            return Ok(None);
        }
        let key = self.state.frame.buffer.context("缺少采集缓冲描述")?;
        let (format, width, height, stride) = key;
        ensure!(
            matches!(format, XRGB | ARGB | XBGR | ABGR),
            "不支持的屏幕采集像素格式 0x{format:08x}"
        );
        if self.buffer.as_ref().is_none_or(|b| b.key != key) {
            self.buffer = None;
            let size = (stride as usize)
                .checked_mul(height as usize)
                .context("采集缓冲大小溢出")?;
            let fd = unsafe {
                let raw = libc::memfd_create(c"openuuyc-screencopy".as_ptr(), libc::MFD_CLOEXEC);
                ensure!(raw >= 0, "memfd_create 失败");
                OwnedFd::from_raw_fd(raw)
            };
            ensure!(
                unsafe { libc::ftruncate(std::os::fd::AsRawFd::as_raw_fd(&fd), size as i64) } == 0,
                "分配采集共享内存失败"
            );
            let map = unsafe { memmap2::MmapMut::map_mut(&fd)? };
            let pool = self.shm.create_pool(fd.as_fd(), size as i32, &qh, ());
            let buffer = pool.create_buffer(
                0,
                width as i32,
                height as i32,
                stride as i32,
                match format {
                    XRGB => wl_shm::Format::Xrgb8888,
                    ARGB => wl_shm::Format::Argb8888,
                    XBGR => wl_shm::Format::Xbgr8888,
                    _ => wl_shm::Format::Abgr8888,
                },
                &qh,
                (),
            );
            self.buffer = Some(Shm {
                _fd: fd,
                map,
                pool,
                buffer,
                key,
            });
        }
        let shm = self.buffer.as_ref().context("采集缓冲缺失")?;
        frame.copy(&shm.buffer);
        let copied = dispatch_until(
            &self.connection,
            &mut self.queue,
            &mut self.state,
            deadline + Duration::from_millis(500),
            |s| s.frame.ready || s.frame.failed,
        )?;
        frame.destroy();
        if !copied {
            return Ok(None);
        }
        ensure!(!self.state.frame.failed, "屏幕采集复制失败");
        let shm = self.buffer.as_ref().context("采集缓冲缺失")?;
        Ok(Some(Raw {
            width,
            height,
            stride,
            rgb: matches!(format, XBGR | ABGR),
            y_invert: self.state.frame.y_invert,
            data: &shm.map[..],
        }))
    }
}
