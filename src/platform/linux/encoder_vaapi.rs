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

pub(super) struct Encoder {
    display: Rc<Display>,
    context: Rc<Context>,
    _config: Config,
    image_fmt: VAImageFormat,
    // Two surfaces ping-ponged between "current" and "reference".
    surfaces: [Option<Surface<()>>; 2],
    coded: EncCodedBuffer,
    width: u32,
    height: u32,
    aligned_w: u32,
    aligned_h: u32,
    mb_w: u16,
    mb_h: u16,
    level_idc: u8,
    rc: Rc_,
    bitrate: u32,
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

        let level_idc = 51; // 5.1: ample headroom up to 4K.
        let mut surfaces_iter = surfaces.into_iter();
        let s0 = surfaces_iter.next().context("缺少编码表面")?;
        let s1 = surfaces_iter.next().context("缺少编码表面")?;

        let mut enc = Self {
            display,
            context,
            _config: config,
            image_fmt,
            surfaces: [Some(s0), Some(s1)],
            coded,
            width: size.0,
            height: size.1,
            aligned_w,
            aligned_h,
            mb_w,
            mb_h,
            level_idc,
            rc,
            bitrate: bitrate.max(300_000),
            fps: fps.clamp(1, 240),
            gop: 300,
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
        };
        enc.sps = enc.build_sps();
        enc.pps = build_pps();
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

        let is_idr = force || !self.have_ref || self.frame_count % self.gop == 0;

        // Take the current surface slot and upload NV12 into it.
        let surface = self.surfaces[self.slot]
            .take()
            .context("VA-API 编码表面缺失")?;
        let curr_surface_id = surface.id();
        self.upload_nv12(&surface, bgra, pitch)?;

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

        let buffers = self.build_frame_buffers(curr_surface_id, is_idr, slice_type, frame_num, poc)?;

        // Render.
        let mut picture = Picture::new(self.frame_count as u64, Rc::clone(&self.context), surface);
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

        // Return the surface to its slot; it is now the reference.
        let surface = picture
            .take_surface()
            .map_err(|_| anyhow::anyhow!("无法回收 VA-API 编码表面"))?;
        self.surfaces[self.slot] = Some(surface);

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
        self.ref_surface_id = curr_surface_id;
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

            // Rate control misc buffers.
            for mb in self.rate_control_buffers() {
                buffers.push(
                    self.context
                        .create_buffer(BufferType::EncMiscParameter(mb))
                        .map_err(|e| anyhow::anyhow!("创建码率控制缓冲失败: {e}"))?,
                );
            }
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
            0,
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

    fn rate_control_buffers(&self) -> Vec<EncMiscParameter> {
        let mut out = Vec::new();
        if matches!(self.rc, Rc_::Cbr | Rc_::Vbr) {
            let target_percentage = if matches!(self.rc, Rc_::Cbr) { 100 } else { 95 };
            // Small VBV (~0.5s) for low latency.
            let window_ms = 500u32;
            let rc_flags = RcFlags::new(1, 0, 0, 0, 0, 0, 0, 0, 0);
            let rc = EncMiscParameterRateControl::new(
                self.bitrate,
                target_percentage,
                window_ms,
                0,
                1,
                0,
                rc_flags,
                0,
                51,
                0,
                0,
            );
            out.push(EncMiscParameter::RateControl(rc));

            let buffer_size = self.bitrate; // ~1s coded buffer
            let initial = buffer_size / 2;
            out.push(EncMiscParameter::HRD(EncMiscParameterHRD::new(initial, buffer_size)));
        }
        out.push(EncMiscParameter::FrameRate(EncMiscParameterFrameRate::new(
            self.fps.max(1),
            0,
        )));
        out
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
        w.u(8, 66); // profile_idc = ConstrainedBaseline (baseline)
        w.u(8, 0x40); // constraint_set1_flag=1, rest 0
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

fn build_pps() -> Vec<u8> {
    let mut w = BitWriter::new();
    w.u(8, 0x68); // nal: ref_idc=3, type=8
    w.ue(0); // pic_parameter_set_id
    w.ue(0); // seq_parameter_set_id
    w.u(1, 0); // entropy_coding_mode_flag (CAVLC)
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

fn open_display() -> Result<Rc<Display>> {
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
    let profiles = [
        VAProfile::VAProfileH264ConstrainedBaseline,
        VAProfile::VAProfileH264Main,
        VAProfile::VAProfileH264High,
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
