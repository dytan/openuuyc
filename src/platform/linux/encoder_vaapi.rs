//! VA-API H.264 hardware encoding on Linux (Intel iGPU via the iHD driver),
//! through the vendored cros-libva bindings.
//!
//! The capture side still hands us tight BGRA, which we convert to NV12 on the
//! CPU (BT.601 limited range, matching the SDR color space we tag the stream
//! with) and upload into a VA surface. The driver encodes a single low-latency
//! slice per frame (no B-frames, 1 reference, IDR on request or GOP boundary).
//!
//! cros-libva here exposes neither packed-header buffers nor VPP, so we emit the
//! SPS/PPS ourselves in Annex-B and prepend them to each IDR -- unless the
//! driver already inserted them into the coded buffer, which we detect and skip.
use anyhow::{Context as _, Result, bail, ensure};
use std::rc::Rc;

use cros_libva::{
    BufferType, Config, Context, Display, EncCodedBuffer, EncMiscParameter,
    EncMiscParameterFrameRate, EncMiscParameterHRD, EncMiscParameterRateControl, EncPictureParameter,
    EncPictureParameterBufferH264, EncSequenceParameter, EncSequenceParameterBufferH264,
    EncSliceParameter, EncSliceParameterBufferH264, H264EncPicFields, H264EncSeqFields, Image,
    MappedCodedBuffer, Picture, PictureH264, RcFlags, Surface, UsageHint, VAConfigAttrib,
    VAConfigAttribType, VAEntrypoint, VAImageFormat, VAProfile, VASurfaceID,
};

const VA_RT_FORMAT_YUV420: u32 = cros_libva::VA_RT_FORMAT_YUV420;
const FOURCC_NV12: u32 = cros_libva::VA_FOURCC_NV12;
const SHORT_TERM_REF: u32 = cros_libva::VA_PICTURE_H264_SHORT_TERM_REFERENCE;

/// One encoded access unit.
pub(super) struct Packet {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

#[derive(Clone, Copy)]
enum Rc_ {
    Cbr,
    Vbr,
    Cqp,
}

static VPP_NV12_DUMPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) struct Encoder {
    display: Rc<Display>,
    context: Rc<Context>,
    _config: Config,
    image_fmt: VAImageFormat,
    bgra_fmt: Option<VAImageFormat>,
    vpp: Option<cros_libva::VppContext>,
    src_surface: Option<Surface<()>>,
    src_dims: (u32, u32),
    // Reconstructed/reference surfaces (CurrPic / RefPicList). Separate from
    // the input surface so a VPP write into the input is not clobbered when the
    // driver uses CurrPic as the reconstruction target.
    surfaces: [Option<Surface<()>>; 2],
    /// Raw NV12 input for vaBeginPicture (CPU upload or VPP output).
    input_surface: Option<Surface<()>>,
    coded: EncCodedBuffer,
    width: u32,
    height: u32,
    aligned_w: u32,
    aligned_h: u32,
    mb_w: u16,
    mb_h: u16,
    level_idc: u8,
    profile_idc: u8,
    cabac: bool,
    rc: Rc_,
    bitrate: u32,
    observed_fps: f32,
    last_encode_at: Option<std::time::Instant>,
    applied_bitrate: u32,
    applied_fps: u32,
    fps: u32,
    gop: u32,
    // Encode state.
    slot: usize,      // which surface holds the frame being encoded
    frame_count: u32, // frames since start
    frame_num: u16,   // H.264 frame_num of the last reference
    idr_id: u16,
    poc_lsb: u16,
    have_ref: bool,
    ref_surface_id: VASurfaceID,
    ref_frame_idx: u32,
    ref_poc: i32,
    sps: Vec<u8>,
    pps: Vec<u8>,
    nv12_pad: Vec<u8>, // scratch for the bottom padding row
    driver_inserts_headers: Option<bool>,
}

const MAX_FRAME_NUM: u32 = 32; // log2_max_frame_num_minus4 = 1 -> 2^5
const MAX_POC_LSB: u32 = 64; // log2_max_pic_order_cnt_lsb_minus4 = 2 -> 2^6

impl Encoder {
    pub(super) fn new(size: (u32, u32), bitrate: u32, fps: u32) -> Result<Self> {
        ensure!(
            size.0 >= 32 && size.1 >= 32 && size.0 % 2 == 0 && size.1 % 2 == 0,
            "VA-API 编码尺寸非法: {}x{}",
            size.0,
            size.1
        );
        let display = open_display()?;
        let (profile, entrypoint) = pick_profile_entrypoint(&display)
            .context("当前 VA-API 驱动不支持 H.264 编码")?;

        // Required + rate-control attributes.
        let mut query = vec![
            VAConfigAttrib {
                type_: VAConfigAttribType::VAConfigAttribRTFormat,
                value: 0,
            },
            VAConfigAttrib {
                type_: VAConfigAttribType::VAConfigAttribRateControl,
                value: 0,
            },
        ];
        display
            .get_config_attributes(profile, entrypoint, &mut query)
            .map_err(|e| anyhow::anyhow!("查询 VA-API 编码属性失败: {e}"))?;
        ensure!(
            query[0].value & VA_RT_FORMAT_YUV420 != 0,
            "VA-API 编码不支持 YUV420"
        );
        let rc = if query[1].value & cros_libva::VA_RC_CBR != 0 {
            Rc_::Cbr
        } else if query[1].value & cros_libva::VA_RC_VBR != 0 {
            Rc_::Vbr
        } else {
            Rc_::Cqp
        };

        let config_attrs = vec![
            VAConfigAttrib {
                type_: VAConfigAttribType::VAConfigAttribRTFormat,
                value: VA_RT_FORMAT_YUV420,
            },
            VAConfigAttrib {
                type_: VAConfigAttribType::VAConfigAttribRateControl,
                value: match rc {
                    Rc_::Cbr => cros_libva::VA_RC_CBR,
                    Rc_::Vbr => cros_libva::VA_RC_VBR,
                    Rc_::Cqp => cros_libva::VA_RC_CQP,
                },
            },
        ];
        let config = display
            .create_config(config_attrs, profile, entrypoint)
            .map_err(|e| anyhow::anyhow!("创建 VA-API 编码配置失败: {e}"))?;

        let mb_w = size.0.div_ceil(16) as u16;
        let mb_h = size.1.div_ceil(16) as u16;
        let aligned_w = u32::from(mb_w) * 16;
        let aligned_h = u32::from(mb_h) * 16;

        // Recon/ref pool: ENCODER only, registered with the encode context.
        let surfaces = display
            .create_surfaces::<()>(
                VA_RT_FORMAT_YUV420,
                Some(FOURCC_NV12),
                aligned_w,
                aligned_h,
                Some(UsageHint::USAGE_HINT_ENCODER),
                vec![(), ()],
            )
            .map_err(|e| anyhow::anyhow!("创建 VA-API 编码表面失败: {e}"))?;

        // Input surface: ENCODER|VPP_WRITE so VPP can write NV12 into it and the
        // encoder can read it via vaBeginPicture. Kept OUT of the context's
        // render-target list (recon surfaces only) — matching ffmpeg's split.
        let mut input_surfaces = display
            .create_surfaces::<()>(
                VA_RT_FORMAT_YUV420,
                Some(FOURCC_NV12),
                aligned_w,
                aligned_h,
                Some(UsageHint::USAGE_HINT_ENCODER | UsageHint::USAGE_HINT_VPP_WRITE),
                vec![()],
            )
            .map_err(|e| anyhow::anyhow!("创建 VA-API 输入表面失败: {e}"))?;

        let context = display
            .create_context::<()>(&config, aligned_w, aligned_h, Some(&surfaces), true)
            .map_err(|e| anyhow::anyhow!("创建 VA-API 编码上下文失败: {e}"))?;

        let coded = context
            .create_enc_coded((aligned_w * aligned_h) as usize + 65_536)
            .map_err(|e| anyhow::anyhow!("创建 VA-API 码流缓冲失败: {e}"))?;

        let image_fmt = display
            .query_image_formats()
            .map_err(|e| anyhow::anyhow!("查询 VA-API 图像格式失败: {e}"))?
            .into_iter()
            .find(|f| f.fourcc == FOURCC_NV12)
            .context("VA-API 驱动未提供 NV12 图像格式")?;

        // Optional GPU scale+CSC path. Prefer BGRA (matches capture byte
        // order); fall back to BGRX. Failure just disables the VPP path.
        let bgra_fmt = display.query_image_formats().ok().and_then(|fmts| {
            let mut bgra = None;
            let mut bgrx = None;
            for f in fmts {
                if f.fourcc == cros_libva::VA_FOURCC_BGRA {
                    bgra = Some(f);
                } else if f.fourcc == cros_libva::VA_FOURCC_BGRX {
                    bgrx = Some(f);
                }
            }
            bgra.or(bgrx)
        });
        let vpp = match cros_libva::VppContext::new(Rc::clone(&display), aligned_w, aligned_h) {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!(error = %e, "VA-API VPP 不可用，回退 CPU 转换");
                None
            }
        };

        let level_idc = 51; // 5.1: ample headroom up to 4K.
        // Intel's low-power VDEnc entropy support is CABAC; ConstrainedBaseline
        // forbids CABAC, so Main/High must declare CABAC in both the PPS and the
        // picture parameters or the decoder reads the slice as CAVLC and fails.
        let (profile_idc, cabac) = match profile {
            x if x == VAProfile::VAProfileH264Main => (77u8, true),
            x if x == VAProfile::VAProfileH264High => (100u8, true),
            _ => (66u8, false),
        };
        let mut surfaces_iter = surfaces.into_iter();
        let s0 = surfaces_iter.next().context("缺少编码表面")?;
        let s1 = surfaces_iter.next().context("缺少编码表面")?;

        let mut enc = Self {
            display,
            context,
            _config: config,
            image_fmt,
            surfaces: [Some(s0), Some(s1)],
            input_surface: input_surfaces.pop(),
            coded,
            width: size.0,
            height: size.1,
            aligned_w,
            aligned_h,
            mb_w,
            mb_h,
            level_idc,
            profile_idc,
            cabac,
            rc,
            bitrate: bitrate.max(300_000),
            observed_fps: fps.clamp(1, 240) as f32,
            last_encode_at: None,
            applied_bitrate: 0,
            applied_fps: 0,
            fps: fps.clamp(1, 240),
            gop: 3600,
            slot: 0,
            frame_count: 0,
            frame_num: 0,
            idr_id: 0,
            poc_lsb: 0,
            have_ref: false,
            ref_surface_id: cros_libva::VA_INVALID_SURFACE,
            ref_frame_idx: 0,
            ref_poc: 0,
            sps: Vec::new(),
            pps: Vec::new(),
            nv12_pad: Vec::new(),
            driver_inserts_headers: None,
            bgra_fmt,
            vpp,
            src_surface: None,
            src_dims: (0, 0),
        };
        enc.sps = enc.build_sps();
        enc.pps = build_pps(enc.cabac);
        tracing::info!(
            width = size.0,
            height = size.1,
            profile = profile_name(profile),
            low_power = entrypoint == VAEntrypoint::VAEntrypointEncSliceLP,
            rc = match enc.rc { Rc_::Cbr => "CBR", Rc_::Vbr => "VBR", Rc_::Cqp => "CQP" },
            "VA-API H.264 encoder ready"
        );
        Ok(enc)
    }

    pub(super) fn maximum_size(&self) -> (u32, u32) {
        (4096, 4096)
    }
    pub(super) fn driver_inserts_headers(&self) -> Option<bool> {
        self.driver_inserts_headers
    }

    pub(super) fn set_rate(&mut self, bitrate: u32, fps: u32) {
        self.bitrate = bitrate.max(300_000);
        self.fps = fps.clamp(1, 240);
    }

    /// Encode one BGRA frame. `force` requests an IDR.
    pub(super) fn encode(&mut self, bgra: &[u8], _timestamp: i64, force: bool) -> Result<Vec<Packet>> {
        let pitch = self.width as usize * 4;
        ensure!(
            bgra.len() >= pitch * self.height as usize,
            "采集帧尺寸与 VA-API 编码器不一致"
        );

        // Upload into the dedicated input surface; recon lives in surfaces[slot].
        let input = self
            .input_surface
            .take()
            .context("VA-API 输入表面缺失")?;
        self.upload_nv12(&input, bgra, pitch)?;
        let recon_id = self.surfaces[self.slot]
            .as_ref()
            .context("VA-API 重建表面缺失")?
            .id();
        self.finish_encode(input, recon_id, force)
    }

    /// VA-API VPP path: upload full-res BGRA into an RGB surface, then let the
    /// GPU colour-convert + scale it into the NV12 encode surface. Only a CPU
    /// memcpy of the source frame happens on the CPU; CSC and scaling run on the
    /// iGPU. `src_pitch` is the source row stride in bytes. Caller falls back to
    /// the CPU path on Err.
    /// GPU-encode a frame already present in VA surface `src_id` (e.g. a dmabuf
    /// the compositor copied into). VPP scales+CSCs it into the split input
    /// surface, then encodes. Zero CPU copies of pixel data.
    pub(super) fn encode_surface(
        &mut self,
        src_id: VASurfaceID,
        src_w: u32,
        src_h: u32,
        force: bool,
    ) -> Result<Vec<Packet>> {
        ensure!(self.vpp.is_some(), "VA-API VPP 未初始化");
        ensure!(
            src_w >= 2 && src_h >= 2 && src_w % 2 == 0 && src_h % 2 == 0,
            "VPP 源尺寸非法: {src_w}x{src_h}"
        );
        self.vpp_to_input_and_encode(src_id, src_w, src_h, force)
    }

    /// Shared tail: VPP blit `src_id` -> input surface, sync, then encode with
    /// the split input/recon layout. Respects OPENUUYC_VPP_BRIDGE.
    fn vpp_to_input_and_encode(
        &mut self,
        src_id: VASurfaceID,
        src_w: u32,
        src_h: u32,
        force: bool,
    ) -> Result<Vec<Packet>> {
        let input = self
            .input_surface
            .take()
            .context("VA-API 输入表面缺失")?;
        let input_id = input.id();
        self.vpp
            .as_ref()
            .unwrap()
            .blit(src_id, (src_w, src_h), input_id, (self.width, self.height))
            .map_err(|e| anyhow::anyhow!("VA-API VPP 处理失败: {e}"))?;
        input
            .sync()
            .map_err(|e| anyhow::anyhow!("同步 VPP 输入表面失败: {e}"))?;
        let recon_id = self.surfaces[self.slot]
            .as_ref()
            .context("VA-API 重建表面缺失")?
            .id();
        if std::env::var_os("OPENUUYC_VPP_BRIDGE").is_some() {
            let recon = self.surfaces[self.slot]
                .take()
                .context("VA-API 重建表面缺失")?;
            let img = Image::create_from(
                &input,
                self.image_fmt,
                (self.aligned_w, self.aligned_h),
                (self.width, self.height),
            )
            .map_err(|e| anyhow::anyhow!("VPP GetImage 失败: {e}"))?;
            let va = *img.image();
            let buf = img.as_ref();
            self.upload_nv12_planes(
                &recon,
                &buf[va.offsets[0] as usize..],
                va.pitches[0] as usize,
                &buf[va.offsets[1] as usize..],
                va.pitches[1] as usize,
            )?;
            drop(img);
            let rid = recon.id();
            recon
                .sync()
                .map_err(|e| anyhow::anyhow!("同步 VPP→编码表面失败: {e}"))?;
            self.input_surface = Some(input);
            return self.finish_encode_same(recon, rid, force);
        }
        self.finish_encode(input, recon_id, force)
    }

    pub(super) fn encode_scaled(
        &mut self,
        bgra: &[u8],
        src_w: u32,
        src_h: u32,
        src_pitch: usize,
        _timestamp: i64,
        force: bool,
    ) -> Result<Vec<Packet>> {
        ensure!(self.vpp.is_some(), "VA-API VPP 未初始化");
        ensure!(
            src_w >= 2 && src_h >= 2 && src_w % 2 == 0 && src_h % 2 == 0,
            "VPP 源尺寸非法: {src_w}x{src_h}"
        );
        ensure!(
            src_pitch >= src_w as usize * 4 && bgra.len() >= src_pitch * src_h as usize,
            "VPP 源帧尺寸不足"
        );
        self.ensure_src_surface(src_w, src_h)?;
        self.upload_bgra(src_w, src_h, src_pitch, bgra)?;
        let src_id = self.src_surface.as_ref().context("VPP 源表面缺失")?.id();
        self.vpp_to_input_and_encode(src_id, src_w, src_h, force)
    }
    pub(super) fn display(&self) -> Rc<Display> {
        Rc::clone(&self.display)
    }

    pub(super) fn vpp_available(&self) -> bool {
        // GPU scale+CSC path: BGRA upload -> VPP (CSC+scale) -> NV12 encode.
        self.vpp.is_some() && self.bgra_fmt.is_some()
    }

    fn ensure_src_surface(&mut self, src_w: u32, src_h: u32) -> Result<()> {
        if self.src_dims == (src_w, src_h) && self.src_surface.is_some() {
            return Ok(());
        }
        ensure!(src_w % 2 == 0 && src_h % 2 == 0, "VPP 源尺寸必须为偶数");
        let fourcc = self
            .bgra_fmt
            .as_ref()
            .map(|f| f.fourcc)
            .unwrap_or(cros_libva::VA_FOURCC_BGRA);
        let surfaces = self
            .display
            .create_surfaces::<()>(
                cros_libva::VA_RT_FORMAT_RGB32,
                Some(fourcc),
                src_w,
                src_h,
                Some(UsageHint::USAGE_HINT_VPP_READ),
                vec![()],
            )
            .map_err(|e| anyhow::anyhow!("创建 VPP RGB 源表面失败: {e}"))?;
        self.src_surface = surfaces.into_iter().next();
        self.src_dims = (src_w, src_h);
        Ok(())
    }

    fn upload_bgra(&mut self, src_w: u32, src_h: u32, src_pitch: usize, bgra: &[u8]) -> Result<()> {
        let fmt = self.bgra_fmt.context("VPP 缺少 BGRA 图像格式")?;
        let surface = self.src_surface.as_ref().context("VPP 源表面缺失")?;
        // Prefer a derived view (direct write into surface memory). Fall back to
        // create_from + vaPutImage when derive is unavailable for RGB surfaces.
        let mut image = match Image::derive_from(surface, (src_w, src_h)) {
            Ok(img) if img.image().format.fourcc == fmt.fourcc => img,
            Ok(_) | Err(_) => Image::create_from(surface, fmt, (src_w, src_h), (src_w, src_h))
                .map_err(|e| anyhow::anyhow!("创建 VPP RGB 上传图像失败: {e}"))?,
        };
        let va = *image.image();
        let off = va.offsets[0] as usize;
        let dpitch = va.pitches[0] as usize;
        let row = (src_w as usize) * 4;
        ensure!(dpitch >= row, "VPP 上传目标 pitch 过小");
        let dst = image.as_mut();
        for y in 0..src_h as usize {
            let d = off + y * dpitch;
            let srow = y * src_pitch;
            dst[d..d + row].copy_from_slice(&bgra[srow..srow + row]);
        }
        drop(image); // derived: unmap; non-derived: vaPutImage on drop.
        surface
            .sync()
            .map_err(|e| anyhow::anyhow!("同步 VPP RGB 源表面失败: {e}"))?;
        Ok(())
    }

    fn finish_encode(
        &mut self,
        input: Surface<()>,
        recon_id: VASurfaceID,
        force: bool,
    ) -> Result<Vec<Packet>> {
        self.finish_encode_ex(input, recon_id, force, false)
    }

    fn finish_encode_same(
        &mut self,
        surface: Surface<()>,
        surface_id: VASurfaceID,
        force: bool,
    ) -> Result<Vec<Packet>> {
        self.finish_encode_ex(surface, surface_id, force, true)
    }

    fn finish_encode_ex(
        &mut self,
        input: Surface<()>,
        recon_id: VASurfaceID,
        force: bool,
        return_to_slot: bool,
    ) -> Result<Vec<Packet>> {
        let is_idr = force || !self.have_ref || self.frame_count % self.gop == 0;

        // Track the delivered frame rate so CBR budgets bits/frame against
        // reality instead of the nominal fps; otherwise, when we deliver ~14 of
        // a requested 60 fps, the driver only emits ~14/60 of the target rate
        // and the link sits half-empty (visible quality drops).
        let now = std::time::Instant::now();
        if let Some(prev) = self.last_encode_at {
            let dt = now.duration_since(prev).as_secs_f32();
            if dt > 0.003 && dt < 2.0 {
                self.observed_fps = 0.8 * self.observed_fps + 0.2 * (1.0 / dt);
            }
        }
        self.last_encode_at = Some(now);
        let eff_fps = self
            .observed_fps
            .round()
            .clamp(5.0, self.fps.clamp(1, 240) as f32) as u32;
        // Only reset the bitrate-control accumulator when something actually
        // changed; resetting every frame would wreck rate control.
        let rate_changed = self.bitrate.abs_diff(self.applied_bitrate) > self.applied_bitrate / 20;
        let reset_rc = is_idr || rate_changed || self.applied_fps != eff_fps;
        self.applied_bitrate = self.bitrate;
        self.applied_fps = eff_fps;
        if reset_rc {
            tracing::debug!(
                bitrate = self.bitrate,
                eff_fps,
                observed_fps = self.observed_fps,
                is_idr,
                "VA-API 码率控制更新"
            );
        }

        // Picture order + reference bookkeeping.
        let (slice_type, frame_num, poc) = if is_idr {
            self.frame_num = 0;
            self.poc_lsb = 0;
            (2u8, 0u16, 0i32) // I slice
        } else {
            let fnum = self.frame_num; // reference frame_num of the predecessor
            let poc = i32::from(self.poc_lsb);
            (0u8, fnum, poc) // P slice; frame_num here is prev ref's
        };

        let buffers =
            self.build_frame_buffers(recon_id, is_idr, slice_type, frame_num, poc, reset_rc, eff_fps)?;

        // Render.
        // BeginPicture(input); CurrPic(recon) — ffmpeg's split. Sharing one
        // surface for both left VPP-fed frames mid-gray on iHD.
        let mut picture = Picture::new(self.frame_count as u64, Rc::clone(&self.context), input);
        for buf in buffers {
            picture.add_buffer(buf);
        }
        let picture = picture
            .begin()
            .map_err(|e| anyhow::anyhow!("VA-API begin 失败: {e}"))?
            .render()
            .map_err(|e| anyhow::anyhow!("VA-API render 失败: {e}"))?
            .end()
            .map_err(|e| anyhow::anyhow!("VA-API end 失败: {e}"))?;
        let picture = picture.sync().map_err(|(e, _)| anyhow::anyhow!("VA-API sync 失败: {e}"))?;

        // Read the coded bitstream.
        let mut bitstream = Vec::new();
        {
            let mapped = MappedCodedBuffer::new(&self.coded)
                .map_err(|e| anyhow::anyhow!("映射 VA-API 码流失败: {e}"))?;
            for seg in mapped.segments() {
                bitstream.extend_from_slice(seg.buf);
            }
        }
        ensure!(!bitstream.is_empty(), "VA-API 输出空码流");

        let returned = picture
            .take_surface()
            .map_err(|_| anyhow::anyhow!("无法回收 VA-API 表面"))?;
        if return_to_slot {
            self.surfaces[self.slot] = Some(returned);
        } else {
            self.input_surface = Some(returned);
        }

        // Detect once whether the driver already emits SPS/PPS.
        if self.driver_inserts_headers.is_none() && is_idr {
            self.driver_inserts_headers = Some(contains_nal(&bitstream, 7));
        }

        let mut data = Vec::with_capacity(bitstream.len() + 64);
        if is_idr && self.driver_inserts_headers == Some(false) {
            data.extend_from_slice(&self.sps);
            data.extend_from_slice(&self.pps);
        }
        data.extend_from_slice(&bitstream);

        // Advance reference/frame state for the next frame.
        self.ref_surface_id = recon_id;
        self.ref_frame_idx = u32::from(frame_num);
        self.ref_poc = poc;
        self.have_ref = true;
        // The just-encoded reference gets the next frame_num / poc.
        self.frame_num = ((u32::from(self.frame_num) + if is_idr { 0 } else { 1 }) % MAX_FRAME_NUM) as u16;
        if is_idr {
            self.frame_num = 0;
            self.idr_id = self.idr_id.wrapping_add(1) & 0xffff;
        } else {
            self.frame_num = ((u32::from(self.ref_frame_idx) + 1) % MAX_FRAME_NUM) as u16;
        }
        self.poc_lsb = ((u32::from(self.poc_lsb) + 2) % MAX_POC_LSB) as u16;
        self.frame_count = self.frame_count.wrapping_add(1);
        self.slot ^= 1;

        Ok(vec![Packet {
            data,
            keyframe: is_idr,
        }])
    }

    fn build_frame_buffers(
        &self,
        curr_surface_id: VASurfaceID,
        is_idr: bool,
        slice_type: u8,
        frame_num: u16,
        poc: i32,
        reset_rc: bool,
        eff_fps: u32,
    ) -> Result<Vec<cros_libva::Buffer>> {
        let mut buffers = Vec::new();

        // Sequence parameters (sent with every IDR).
        if is_idr {
            let seq_fields = H264EncSeqFields::new(1, 1, 0, 0, 1, 1, 0, 2, 0);
            let crop = if self.aligned_h != self.height || self.aligned_w != self.width {
                Some(cros_libva::H264EncFrameCropOffsets::new(
                    0,
                    (self.aligned_w - self.width) / 2,
                    0,
                    (self.aligned_h - self.height) / 2,
                ))
            } else {
                None
            };
            let seq = EncSequenceParameterBufferH264::new(
                0,
                self.level_idc,
                self.gop,
                self.gop,
                1,
                self.bitrate,
                1,
                self.mb_w,
                self.mb_h,
                &seq_fields,
                0,
                0,
                0,
                0,
                0,
                [0; 256],
                crop,
                None,
                0,
                0,
                0,
                1,
                u32::from(self.fps.max(1)) * 2,
            );
            buffers.push(
                self.context
                    .create_buffer(BufferType::EncSequenceParameter(EncSequenceParameter::H264(
                        seq,
                    )))
                    .map_err(|e| anyhow::anyhow!("创建 seq 缓冲失败: {e}"))?,
            );

        }

        // Rate-control misc buffers are sent EVERY frame (not just on IDR) so
        // mid-session bitrate/fps updates take effect immediately.
        for mb in self.rate_control_buffers(reset_rc, eff_fps) {
            buffers.push(
                self.context
                    .create_buffer(BufferType::EncMiscParameter(mb))
                    .map_err(|e| anyhow::anyhow!("创建码率控制缓冲失败: {e}"))?,
            );
        }

        // Picture parameters.
        let curr_pic = PictureH264::new(curr_surface_id, u32::from(frame_num), SHORT_TERM_REF, poc, poc);
        let mut ref_frames = empty_refs();
        if !is_idr && self.have_ref {
            ref_frames[0] = PictureH264::new(
                self.ref_surface_id,
                self.ref_frame_idx,
                SHORT_TERM_REF,
                self.ref_poc,
                self.ref_poc,
            );
        }
        let pic_fields = H264EncPicFields::new(
            u32::from(is_idr),
            1,
            u32::from(self.cabac), // entropy_coding_mode_flag
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        );
        let pic = EncPictureParameterBufferH264::new(
            curr_pic,
            ref_frames,
            self.coded.id(),
            0,
            0,
            0,
            frame_num,
            26,
            0,
            0,
            0,
            0,
            &pic_fields,
        );
        buffers.push(
            self.context
                .create_buffer(BufferType::EncPictureParameter(EncPictureParameter::H264(pic)))
                .map_err(|e| anyhow::anyhow!("创建 pic 缓冲失败: {e}"))?,
        );

        // Slice parameters: one slice covering the whole frame.
        let mut list0 = empty_ref_list();
        if !is_idr && self.have_ref {
            list0[0] = PictureH264::new(
                self.ref_surface_id,
                self.ref_frame_idx,
                SHORT_TERM_REF,
                self.ref_poc,
                self.ref_poc,
            );
        }
        let list1 = empty_ref_list();
        let num_mbs = u32::from(self.mb_w) * u32::from(self.mb_h);
        let slice = EncSliceParameterBufferH264::new(
            0,
            num_mbs,
            cros_libva::VA_INVALID_ID,
            slice_type,
            0,
            self.idr_id,
            poc as u16,
            0,
            [0, 0],
            0,
            u8::from(!is_idr),
            0,
            0,
            list0,
            list1,
            0,
            0,
            0,
            [0; 32],
            [0; 32],
            0,
            [[0; 2]; 32],
            [[0; 2]; 32],
            0,
            [0; 32],
            [0; 32],
            0,
            [[0; 2]; 32],
            [[0; 2]; 32],
            0,
            0,
            0,
            0,
            0,
        );
        buffers.push(
            self.context
                .create_buffer(BufferType::EncSliceParameter(EncSliceParameter::H264(slice)))
                .map_err(|e| anyhow::anyhow!("创建 slice 缓冲失败: {e}"))?,
        );

        Ok(buffers)
    }

    fn rate_control_buffers(&self, reset: bool, eff_fps: u32) -> Vec<EncMiscParameter> {
        let mut out = Vec::new();
        if matches!(self.rc, Rc_::Cbr | Rc_::Vbr) {
            let target_percentage = if matches!(self.rc, Rc_::Cbr) { 100 } else { 95 };
            // ~0.5s VBV: smooths complex frames without adding noticeable latency.
            let window_ms = 500u32;
            // reset only when the rate actually changed, so BRC state survives.
            let rc_flags = RcFlags::new(u32::from(reset), 0, 0, 0, 0, 0, 0, 0, 0);
            let rc = EncMiscParameterRateControl::new(
                self.bitrate,
                target_percentage,
                window_ms,
                0,
                1,
                0,
                rc_flags,
                0,
                48, // max_qp: cap worst-case ugliness
                0,
                0,
            );
            out.push(EncMiscParameter::RateControl(rc));

            let buffer_size = self.bitrate; // ~1s coded buffer
            let initial = buffer_size / 2;
            out.push(EncMiscParameter::HRD(EncMiscParameterHRD::new(initial, buffer_size)));
        }
        out.push(EncMiscParameter::FrameRate(EncMiscParameterFrameRate::new(
            eff_fps.max(1),
            0,
        )));
        out
    }

    /// Upload already-converted NV12 planes into `surface` using the same
    /// create_from(aligned,aligned) PutImage path as the working CPU encoder.
    fn upload_nv12_planes(
        &mut self,
        surface: &Surface<()>,
        y_plane: &[u8],
        y_pitch: usize,
        c_plane: &[u8],
        c_pitch: usize,
    ) -> Result<()> {
        let mut image = Image::create_from(
            surface,
            self.image_fmt,
            (self.aligned_w, self.aligned_h),
            (self.aligned_w, self.aligned_h),
        )
        .map_err(|e| anyhow::anyhow!("创建 NV12 平面上传图像失败: {e}"))?;
        let va = *image.image();
        let off_y = va.offsets[0] as usize;
        let off_c = va.offsets[1] as usize;
        let pit_y = va.pitches[0] as usize;
        let pit_c = va.pitches[1] as usize;
        let (w, h) = (self.width as usize, self.height as usize);
        let (aw, ah) = (self.aligned_w as usize, self.aligned_h as usize);
        let dst = image.as_mut();
        for y in 0..h {
            let src = y * y_pitch;
            let d = off_y + y * pit_y;
            dst[d..d + w].copy_from_slice(&y_plane[src..src + w]);
        }
        for y in 0..h / 2 {
            let src = y * c_pitch;
            let d = off_c + y * pit_c;
            dst[d..d + w].copy_from_slice(&c_plane[src..src + w]);
        }
        if ah > h {
            let last = off_y + (h - 1) * pit_y;
            self.nv12_pad.clear();
            self.nv12_pad.extend_from_slice(&dst[last..last + w]);
            let pad = self.nv12_pad.clone();
            for y in h..ah {
                let line = off_y + y * pit_y;
                dst[line..line + w].copy_from_slice(&pad);
            }
            let last_c = off_c + (h / 2 - 1) * pit_c;
            let padc: Vec<u8> = dst[last_c..last_c + w].to_vec();
            for by in (h / 2)..(ah / 2) {
                let line = off_c + by * pit_c;
                dst[line..line + w].copy_from_slice(&padc);
            }
        }
        let _ = aw;
        drop(image);
        Ok(())
    }

        fn upload_nv12(&mut self, surface: &Surface<()>, bgra: &[u8], pitch: usize) -> Result<()> {
        let mut image = Image::create_from(
            surface,
            self.image_fmt,
            (self.aligned_w, self.aligned_h),
            (self.aligned_w, self.aligned_h),
        )
        .map_err(|e| anyhow::anyhow!("创建 NV12 上传图像失败: {e}"))?;
        let va = *image.image();
        let off_y = va.offsets[0] as usize;
        let off_c = va.offsets[1] as usize;
        let pit_y = va.pitches[0] as usize;
        let pit_c = va.pitches[1] as usize;
        let (w, h) = (self.width as usize, self.height as usize);
        let (aw, ah) = (self.aligned_w as usize, self.aligned_h as usize);
        let dst = image.as_mut();

        // One pass over 2x2 BGRA blocks: 4 luma samples + one averaged chroma.
        for by in 0..(h / 2) {
            let y0 = by * 2;
            let y1 = y0 + 1;
            let row0 = &bgra[y0 * pitch..y0 * pitch + w * 4];
            let row1 = &bgra[y1 * pitch..y1 * pitch + w * 4];
            let yline0 = off_y + y0 * pit_y;
            let yline1 = off_y + y1 * pit_y;
            let cline = off_c + by * pit_c;
            for bx in 0..(w / 2) {
                let x0 = bx * 2;
                let p00 = &row0[x0 * 4..x0 * 4 + 8]; // two pixels
                let p10 = &row1[x0 * 4..x0 * 4 + 8];
                let (b00, g00, r00) = (p00[0] as i32, p00[1] as i32, p00[2] as i32);
                let (b01, g01, r01) = (p00[4] as i32, p00[5] as i32, p00[6] as i32);
                let (b10, g10, r10) = (p10[0] as i32, p10[1] as i32, p10[2] as i32);
                let (b11, g11, r11) = (p10[4] as i32, p10[5] as i32, p10[6] as i32);
                dst[yline0 + x0] = luma(r00, g00, b00);
                dst[yline0 + x0 + 1] = luma(r01, g01, b01);
                dst[yline1 + x0] = luma(r10, g10, b10);
                dst[yline1 + x0 + 1] = luma(r11, g11, b11);
                let r = (r00 + r01 + r10 + r11) / 4;
                let g = (g00 + g01 + g10 + g11) / 4;
                let b = (b00 + b01 + b10 + b11) / 4;
                dst[cline + bx * 2] = chroma_u(r, g, b);
                dst[cline + bx * 2 + 1] = chroma_v(r, g, b);
            }
        }
        // Pad the alignment rows by replicating the last encoded row.
        if ah > h {
            let last = off_y + (h - 1) * pit_y;
            self.nv12_pad.clear();
            self.nv12_pad.extend_from_slice(&dst[last..last + w]);
            let pad = self.nv12_pad.clone();
            for y in h..ah {
                let line = off_y + y * pit_y;
                dst[line..line + w].copy_from_slice(&pad);
            }
            let last_c = off_c + (h / 2 - 1) * pit_c;
            let padc: Vec<u8> = dst[last_c..last_c + w].to_vec();
            for by in (h / 2)..(ah / 2) {
                let line = off_c + by * pit_c;
                dst[line..line + w].copy_from_slice(&padc);
            }
        }
        let _ = aw;
        drop(image); // vaPutImage uploads on drop.
        Ok(())
    }

    /// SPS for ConstrainedBaseline, matching the sequence parameters above.
    fn build_sps(&self) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.u(8, 0x67); // nal: ref_idc=3, type=7
        w.u(8, u32::from(self.profile_idc)); // profile_idc (Main/High/ConstrainedBaseline)
        // ConstrainedBaseline sets constraint_set1; Main/High clear all flags.
        w.u(8, if self.profile_idc == 66 { 0x40 } else { 0x00 });
        w.u(8, u32::from(self.level_idc));
        w.ue(0); // seq_parameter_set_id
        w.ue(1); // log2_max_frame_num_minus4
        w.ue(0); // pic_order_cnt_type
        w.ue(2); // log2_max_pic_order_cnt_lsb_minus4
        w.ue(1); // max_num_ref_frames
        w.u(1, 0); // gaps_in_frame_num_value_allowed_flag
        w.ue(u32::from(self.mb_w) - 1); // pic_width_in_mbs_minus1
        w.ue(u32::from(self.mb_h) - 1); // pic_height_in_map_units_minus1
        w.u(1, 1); // frame_mbs_only_flag
        w.u(1, 1); // direct_8x8_inference_flag
        let crop = self.aligned_w != self.width || self.aligned_h != self.height;
        w.u(1, u32::from(crop)); // frame_cropping_flag
        if crop {
            w.ue(0); // left
            w.ue((self.aligned_w - self.width) / 2); // right (CropUnitX=2)
            w.ue(0); // top
            w.ue((self.aligned_h - self.height) / 2); // bottom (CropUnitY=2)
        }
        w.u(1, 0); // vui_parameters_present_flag
        w.rbsp_trailing();
        annexb(&w.into_bytes())
    }
}

fn build_pps(cabac: bool) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.u(8, 0x68); // nal: ref_idc=3, type=8
    w.ue(0); // pic_parameter_set_id
    w.ue(0); // seq_parameter_set_id
    w.u(1, u32::from(cabac)); // entropy_coding_mode_flag (1=CABAC)
    w.u(1, 0); // bottom_field_pic_order_in_frame_present_flag
    w.ue(0); // num_slice_groups_minus1
    w.ue(0); // num_ref_idx_l0_default_active_minus1
    w.ue(0); // num_ref_idx_l1_default_active_minus1
    w.u(1, 0); // weighted_pred_flag
    w.u(2, 0); // weighted_bipred_idc
    w.se(0); // pic_init_qp_minus26
    w.se(0); // pic_init_qs_minus26
    w.se(0); // chroma_qp_index_offset
    w.u(1, 0); // deblocking_filter_control_present_flag
    w.u(1, 0); // constrained_intra_pred_flag
    w.u(1, 0); // redundant_pic_cnt_present_flag
    w.rbsp_trailing();
    annexb(&w.into_bytes())
}

#[inline]
fn luma(r: i32, g: i32, b: i32) -> u8 {
    (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).clamp(0, 255) as u8
}
#[inline]
fn chroma_u(r: i32, g: i32, b: i32) -> u8 {
    (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8
}
#[inline]
fn chroma_v(r: i32, g: i32, b: i32) -> u8 {
    (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8
}

fn empty_refs() -> [PictureH264; 16] {
    std::array::from_fn(|_| {
        PictureH264::new(cros_libva::VA_INVALID_ID, 0, cros_libva::VA_INVALID_SURFACE, 0, 0)
    })
}
fn empty_ref_list() -> [PictureH264; 32] {
    std::array::from_fn(|_| {
        PictureH264::new(cros_libva::VA_INVALID_ID, 0, cros_libva::VA_INVALID_SURFACE, 0, 0)
    })
}

/// Whether the Annex-B stream contains a NAL of `nal_type`.
fn contains_nal(data: &[u8], nal_type: u8) -> bool {
    let mut i = 0;
    while i + 4 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            let (sc, hdr) = if data[i + 2] == 1 {
                (3, i + 3)
            } else if data[i + 2] == 0 && data[i + 3] == 1 {
                (4, i + 4)
            } else {
                (0, 0)
            };
            if sc != 0 {
                if hdr < data.len() && data[hdr] & 0x1f == nal_type {
                    return true;
                }
                i = hdr;
                continue;
            }
        }
        i += 1;
    }
    false
}

/// Prepend a 4-byte start code and apply emulation-prevention to an RBSP NAL.
fn annexb(rbsp: &[u8]) -> Vec<u8> {
    let mut out = vec![0, 0, 0, 1];
    let mut zeros = 0;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(b);
        if b == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
    }
    out
}

struct BitWriter {
    bytes: Vec<u8>,
    cur: u8,
    nbits: u8,
}
impl BitWriter {
    fn new() -> Self {
        Self { bytes: Vec::new(), cur: 0, nbits: 0 }
    }
    fn bit(&mut self, b: u32) {
        self.cur = (self.cur << 1) | (b & 1) as u8;
        self.nbits += 1;
        if self.nbits == 8 {
            self.bytes.push(self.cur);
            self.cur = 0;
            self.nbits = 0;
        }
    }
    fn u(&mut self, n: u32, v: u32) {
        for i in (0..n).rev() {
            self.bit((v >> i) & 1);
        }
    }
    fn ue(&mut self, v: u32) {
        let code = v + 1;
        let bits = 32 - code.leading_zeros();
        for _ in 0..(bits - 1) {
            self.bit(0);
        }
        self.u(bits, code);
    }
    fn se(&mut self, v: i32) {
        let mapped = if v <= 0 {
            (-2 * v) as u32
        } else {
            (2 * v - 1) as u32
        };
        self.ue(mapped);
    }
    fn rbsp_trailing(&mut self) {
        self.bit(1);
        while self.nbits != 0 {
            self.bit(0);
        }
    }
    fn into_bytes(mut self) -> Vec<u8> {
        if self.nbits != 0 {
            self.cur <<= 8 - self.nbits;
            self.bytes.push(self.cur);
            self.nbits = 0;
        }
        self.bytes
    }
}

pub(crate) fn open_display() -> Result<Rc<Display>> {
    for index in 128..136u32 {
        let path = format!("/dev/dri/renderD{index}");
        if let Ok(display) = Display::open_drm_display(&path)
            && has_h264_encode(&display)
        {
            return Ok(display);
        }
    }
    if let Some(display) = Display::open()
        && has_h264_encode(&display)
    {
        return Ok(display);
    }
    bail!("未找到支持 H.264 编码的 VA-API 渲染节点")
}

fn has_h264_encode(display: &Rc<Display>) -> bool {
    pick_profile_entrypoint(display).is_some()
}

fn pick_profile_entrypoint(display: &Rc<Display>) -> Option<(VAProfile::Type, VAEntrypoint::Type)> {
    // Prefer CABAC-capable profiles: Intel low-power encode emits CABAC, which
    // ConstrainedBaseline cannot represent. ConstrainedBaseline stays last as a
    // CAVLC fallback for drivers that only offer it.
    let profiles = [
        VAProfile::VAProfileH264Main,
        VAProfile::VAProfileH264High,
        VAProfile::VAProfileH264ConstrainedBaseline,
    ];
    let supported = display.query_config_profiles().ok()?;
    // Prefer low-power (EncSliceLP) for latency, else regular EncSlice.
    for entrypoint in [
        VAEntrypoint::VAEntrypointEncSliceLP,
        VAEntrypoint::VAEntrypointEncSlice,
    ] {
        for profile in profiles {
            if supported.contains(&profile)
                && display
                    .query_config_entrypoints(profile)
                    .is_ok_and(|eps| eps.contains(&entrypoint))
            {
                return Some((profile, entrypoint));
            }
        }
    }
    None
}

fn profile_name(p: VAProfile::Type) -> &'static str {
    match p {
        x if x == VAProfile::VAProfileH264ConstrainedBaseline => "ConstrainedBaseline",
        x if x == VAProfile::VAProfileH264Main => "Main",
        x if x == VAProfile::VAProfileH264High => "High",
        _ => "H264",
    }
}

/// Whether VA-API H.264 encode is available on this machine at all.
pub(super) fn available() -> bool {
    open_display().is_ok()
}
