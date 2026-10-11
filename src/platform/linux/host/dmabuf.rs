//! Zero-copy screen capture via zwlr_screencopy_v1 + linux-dmabuf.
//!
//! Instead of copying the framebuffer into a wl_shm buffer (CPU, ~20 ms) and
//! then CPU-normalising it, we create a VA-API RGB surface, export it as a DRM
//! PRIME dmabuf, wrap that fd as a wl_buffer and let the compositor copy the
//! frame straight into VA-API surface memory. The encoder's VPP then scales +
//! colour-converts that surface into the NV12 encode input on the GPU, so the
//! frame never touches the CPU.
//!
//! Everything here is gated behind OPENUUYC_ZEROCOPY with automatic fallback to
//! the shm path, because dmabuf import depends on the compositor accepting a
//! VA-allocated buffer's modifier.
use super::wayland::{State, bind_outputs, connect, dispatch_until};
use anyhow::{Context, Result, bail, ensure};
use cros_libva::{Display, Surface, UsageHint, VA_RT_FORMAT_RGB32};
use std::os::fd::{AsFd, OwnedFd};
use std::rc::Rc;
use std::time::{Duration, Instant};
use wayland_client::{
    Connection, EventQueue, delegate_noop,
    protocol::{wl_buffer, wl_output},
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
};
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

delegate_noop!(State: ignore ZwpLinuxDmabufV1);
delegate_noop!(State: ignore ZwpLinuxBufferParamsV1);

/// A reusable VA surface backed by a dmabuf the compositor copies into.
struct Slot {
    va: Surface<()>,
    buffer: wl_buffer::WlBuffer,
    _fd: OwnedFd,
    fourcc: u32,
    width: u32,
    height: u32,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.buffer.destroy();
    }
}

pub(crate) struct DmabufCapture {
    display: Rc<Display>,
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    manager: ZwlrScreencopyManagerV1,
    dmabuf: ZwpLinuxDmabufV1,
    output: wl_output::WlOutput,
    slot: Option<Slot>,
}

impl DmabufCapture {
    /// Open a dmabuf capture for `output_name`, sharing the encoder's VA display.
    pub(crate) fn open(display: Rc<Display>, output_name: &str) -> Result<Self> {
        let (connection, globals, mut queue) = connect()?;
        let qh = queue.handle();
        let manager = globals
            .bind::<ZwlrScreencopyManagerV1, _, _>(&qh, 3..=3, ())
            .context("合成器的 wlr-screencopy 不支持 v3（dmabuf 需要）")?;
        let dmabuf = globals
            .bind::<ZwpLinuxDmabufV1, _, _>(&qh, 3..=4, ())
            .context("合成器缺少 zwp_linux_dmabuf_v1")?;
        let mut state = State::default();
        let _xdg = bind_outputs(&globals, &qh, &mut state);
        queue.roundtrip(&mut state)?;
        queue.roundtrip(&mut state)?;
        let index = state
            .outputs
            .iter()
            .position(|(_, o)| o.name == output_name)
            .with_context(|| format!("找不到显示输出 {output_name}"))?;
        let output = state.outputs[index].0.clone().context("显示输出不可用")?;
        Ok(Self {
            display,
            connection,
            queue,
            state,
            manager,
            dmabuf,
            output,
            slot: None,
        })
    }

    /// Capture one frame into a VA surface and return its id + dimensions.
    /// `None` on timeout. The surface stays owned by the capture (reused).
    pub(crate) fn grab(
        &mut self,
        cursor: bool,
        timeout: Duration,
    ) -> Result<Option<(cros_libva::VASurfaceID, u32, u32)>> {
        let qh = self.queue.handle();
        self.state.frame = Default::default();
        let frame = self
            .manager
            .capture_output(i32::from(cursor), &self.output, &qh, ());
        let deadline = Instant::now() + timeout.max(Duration::from_millis(50));
        let described = dispatch_until(&self.connection, &mut self.queue, &mut self.state, deadline, |s| {
            s.frame.failed || (s.frame.dmabuf.is_some() && s.frame.buffer_done)
        })?;
        if !described || self.state.frame.failed {
            frame.destroy();
            if self.state.frame.failed {
                bail!("合成器拒绝 dmabuf 采集");
            }
            return Ok(None);
        }
        let (fourcc, width, height) = self.state.frame.dmabuf.context("缺少 dmabuf 描述")?;
        ensure!(width >= 2 && height >= 2, "dmabuf 尺寸非法 {width}x{height}");
        self.ensure_slot(fourcc, width, height)
            .context("创建 dmabuf 目标失败")?;
        let slot = self.slot.as_ref().context("dmabuf 槽缺失")?;
        frame.copy(&slot.buffer);
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
        ensure!(!self.state.frame.failed, "dmabuf 采集复制失败");
        let slot = self.slot.as_ref().context("dmabuf 槽缺失")?;
        // Make the compositor's write visible to VA before VPP reads it.
        slot.va.sync().map_err(|e| anyhow::anyhow!("同步 dmabuf 表面失败: {e}"))?;
        Ok(Some((slot.va.id(), slot.width, slot.height)))
    }

    fn ensure_slot(&mut self, fourcc: u32, width: u32, height: u32) -> Result<()> {
        if let Some(slot) = &self.slot {
            if slot.fourcc == fourcc && slot.width == width && slot.height == height {
                return Ok(());
            }
        }
        self.slot = None;
        // Map the compositor's DRM fourcc to the VA fourcc with the same byte
        // order (DRM codes differ from VA codes for packed RGB).
        let va_fourcc = drm_to_va_fourcc(fourcc)
            .with_context(|| format!("不支持的 dmabuf fourcc 0x{fourcc:08x}"))?;
        // Create a VA RGB surface matching the compositor's requested fourcc.
        let surfaces = self
            .display
            .create_surfaces::<()>(
                VA_RT_FORMAT_RGB32,
                Some(va_fourcc),
                width,
                height,
                Some(UsageHint::USAGE_HINT_VPP_READ | UsageHint::USAGE_HINT_EXPORT),
                vec![()],
            )
            .map_err(|e| anyhow::anyhow!("创建 dmabuf VA 表面失败 (fourcc 0x{fourcc:08x}): {e}"))?;
        let va = surfaces.into_iter().next().context("无 VA 表面")?;
        let prime = va
            .export_prime()
            .map_err(|e| anyhow::anyhow!("导出 PRIME 失败: {e}"))?;
        ensure!(!prime.objects.is_empty() && !prime.layers.is_empty(), "PRIME 描述为空");
        let object = &prime.objects[0];
        let layer = &prime.layers[0];
        let modifier = object.drm_format_modifier;
        // Build a wl_buffer from the exported dmabuf.
        let qh = self.queue.handle();
        let params = self.dmabuf.create_params(&qh, ());
        params.add(
            object.fd.as_fd(),
            0,
            layer.offset[0],
            layer.pitch[0],
            (modifier >> 32) as u32,
            (modifier & 0xffff_ffff) as u32,
        );
        let buffer = params.create_immed(
            width as i32,
            height as i32,
            prime.layers[0].drm_format,
            wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags::empty(),
            &qh,
            (),
        );
        params.destroy();
        self.queue.roundtrip(&mut self.state).ok();
        // Keep the fd alive for the lifetime of the buffer: dup it out of the
        // PRIME descriptor (the descriptor is dropped with `prime`).
        let fd = object.fd.try_clone().context("dup dmabuf fd 失败")?;
        self.slot = Some(Slot {
            va,
            buffer,
            _fd: fd,
            fourcc,
            width,
            height,
        });
        Ok(())
    }

    /// Diagnostic: capture one frame and report the fourcc/modifier and whether
    /// the VA surface actually received non-uniform pixels.
    pub(crate) fn probe(&mut self) -> Result<String> {
        let got = self.grab(false, Duration::from_millis(300))?;
        let Some((id, w, h)) = got else {
            return Ok("dmabuf probe: 采集超时".into());
        };
        let slot = self.slot.as_ref().context("槽缺失")?;
        let mut msg = format!("dmabuf probe: ok surface={id} {w}x{h} fourcc=0x{:08x}", slot.fourcc);
        match cros_libva::Image::derive_from(&slot.va, (w, h)) {
            Ok(img) => {
                let va = *img.image();
                let buf = img.as_ref();
                let off = va.offsets[0] as usize;
                let pitch = va.pitches[0] as usize;
                let n = (w as usize * 4).min(buf.len().saturating_sub(off));
                let row = &buf[off..off + n];
                let distinct: std::collections::HashSet<_> = row.chunks(4).map(|c| c.to_vec()).collect();
                let mean = row.iter().map(|&b| b as u32).sum::<u32>() / row.len().max(1) as u32;
                msg.push_str(&format!(
                    " derive pitch={pitch} firstRowDistinct={} mean={mean}",
                    distinct.len()
                ));
            }
            Err(e) => msg.push_str(&format!(" derive 失败: {e}")),
        }
        Ok(msg)
    }
}


/// Open a VA display + the primary wlr output and run a one-frame probe.
pub(crate) fn probe_default(output_name: &str) -> Result<String> {
    let display = crate::platform::linux::encoder::vaapi::open_display()?;
    let mut cap = DmabufCapture::open(display, output_name)?;
    cap.probe()
}

/// Map a DRM packed-RGB fourcc (as offered by screencopy linux_dmabuf) to the
/// VA-API fourcc with the same in-memory byte order.
fn drm_to_va_fourcc(drm: u32) -> Option<u32> {
    // DRM little-endian names vs VA byte-order names:
    //   DRM XR24 (XRGB8888, bytes B,G,R,X) == VA_FOURCC_BGRX
    //   DRM AR24 (ARGB8888, bytes B,G,R,A) == VA_FOURCC_BGRA
    //   DRM XB24 (XBGR8888, bytes R,G,B,X) == VA_FOURCC_RGBX
    //   DRM AB24 (ABGR8888, bytes R,G,B,A) == VA_FOURCC_RGBA
    const DRM_XR24: u32 = 0x3432_5258;
    const DRM_AR24: u32 = 0x3432_5241;
    const DRM_XB24: u32 = 0x3432_4258;
    const DRM_AB24: u32 = 0x3432_4241;
    Some(match drm {
        DRM_XR24 => cros_libva::VA_FOURCC_BGRX,
        DRM_AR24 => cros_libva::VA_FOURCC_BGRA,
        DRM_XB24 => cros_libva::VA_FOURCC_RGBX,
        DRM_AB24 => cros_libva::VA_FOURCC_RGBA,
        _ => return None,
    })
}
