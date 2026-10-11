//! Linux host video encoding: VA-API H.264 hardware encode on Intel iGPUs when
//! available, with the in-tree Rust software H.264 encoder (CPU BGRA) as the
//! automatic fallback. `OPENUUYC_HOST_ENCODER=software|vaapi` forces a path.
use crate::features::host::format::{self, Backend, Capability, Format, Rate};
use crate::media::encoding::rate::Controller;
use anyhow::{Context, Result, ensure};
use openuuyc_codec::encoder::{Config, Encoder as Core};
use std::sync::{Arc, atomic::AtomicBool};

#[path = "encoder_vaapi.rs"]
mod vaapi;

pub(crate) use crate::media::encoding::{Encoded, FrameTiming};

pub(crate) struct Runtime;
impl Runtime {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct SwitchCandidate(pub String);

/// `OPENUUYC_HOST_ENCODER` preference.
#[derive(Clone, Copy, PartialEq)]
enum Pref {
    Auto,
    Software,
    Vaapi,
}
fn preference() -> Pref {
    match std::env::var("OPENUUYC_HOST_ENCODER")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "software" | "sw" | "cpu" => Pref::Software,
        "vaapi" | "va" | "hw" | "intel" => Pref::Vaapi,
        _ => Pref::Auto,
    }
}

/// Verify the encode paths against one real captured frame and report the
/// capabilities to advertise. The VA-API (Intel) capability is only offered
/// after a successful encode -> decode round-trip, so a broken GPU path never
/// reaches a live session; the software capability is always offered as the
/// runtime fallback.
pub(crate) fn probe(
    desktop: &mut super::capture::Desktop,
    is_active: impl Fn() -> bool,
) -> Result<Vec<Capability>> {
    let mut frame = None;
    for _ in 0..20 {
        ensure!(is_active(), "被控准备已取消");
        if let Some(f) = desktop.next(100, 1, false, false, (1280, 720))? {
            frame = Some(f);
            break;
        }
    }
    let frame = frame.context("所选桌面尚未提供可采集的画面")?;
    let rate = Rate {
        target: 2_000_000,
        peak: 2_000_000,
        fps: 30,
        quality: 1,
        quality_target: format::QualityTarget {
            bitrate: 2_000_000,
            fps: 30,
        },
    };

    let pref = preference();
    let mut capabilities = Vec::new();

    // VA-API first so it ranks above software when both validate.
    if pref != Pref::Software {
        match probe_vaapi(&frame) {
            Ok(cap) => {
                tracing::info!("VA-API H.264 编码已验证（编码→解码往返通过），将优先使用硬件编码");
                capabilities.push(cap);
            }
            Err(error) if pref == Pref::Vaapi => {
                tracing::warn!(%error, "OPENUUYC_HOST_ENCODER=vaapi 但 VA-API 编码验证失败，回退软件编码");
            }
            Err(error) => {
                tracing::info!(%error, "VA-API 编码不可用，使用软件编码");
            }
        }
    }

    // Software capability (skipped only when VA-API is forced AND available).
    if !(pref == Pref::Vaapi && !capabilities.is_empty()) {
        let mut encoder =
            Encoder::software_format(&(), (frame.width, frame.height), Format::AVC, rate)?;
        let output = encoder.encode(&frame.texture, 0, true)?;
        let parsed = output
            .iter()
            .find_map(|p| {
                crate::media::video_format::parse_stream_format(Format::AVC.codec.media(), &p.data)
            })
            .context("软件编码器未输出可验证的序列头")?;
        ensure!(
            (parsed.visible_width, parsed.visible_height) == (frame.width, frame.height),
            "软件编码器输出尺寸与输入不一致"
        );
        capabilities.push(Capability {
            adapter: 0,
            backend: Backend::Software,
            format: Format::AVC,
            maximum: encoder.maximum_size(),
        });
    }

    ensure!(!capabilities.is_empty(), "没有可用的被控编码后端");
    Ok(capabilities)
}

/// Encode an IDR + a P frame with VA-API and decode them back with both the
/// software and VA-API decoders, confirming the bitstream is conformant.
fn probe_vaapi(frame: &super::capture::Frame) -> Result<Capability> {
    let rate = Rate {
        target: 4_000_000,
        peak: 4_000_000,
        fps: 30,
        quality: 1,
        quality_target: format::QualityTarget {
            bitrate: 4_000_000,
            fps: 30,
        },
    };
    let mut encoder =
        Encoder::hardware_format(&(), (frame.width, frame.height), Format::AVC, rate, format::Color::Sdr)?;
    let idr = encoder.encode(&frame.texture, 0, true)?;
    let inter = encoder.encode(&frame.texture, 10_000, false)?;
    ensure!(!idr.is_empty() && !inter.is_empty(), "VA-API 未产出码流");

    let parsed = idr
        .iter()
        .find_map(|p| {
            crate::media::video_format::parse_stream_format(Format::AVC.codec.media(), &p.data)
        })
        .context("VA-API 输出缺少可解析的序列头")?;
    ensure!(
        (parsed.visible_width, parsed.visible_height) == (frame.width, frame.height),
        "VA-API 输出尺寸与输入不一致（SPS {}x{}）",
        parsed.visible_width,
        parsed.visible_height
    );

    // Software decode round-trip (independent, strict parser).
    let mut sw = openuuyc_codec::decoder::Decoder::new(openuuyc_codec::Codec::H264, &[])
        .context("创建软件解码器失败")?;
    let mut decoded = 0usize;
    for p in idr.iter().chain(inter.iter()) {
        decoded += sw.push(&p.data, 0).context("软件解码 VA-API 码流失败")?.len();
    }
    ensure!(decoded > 0, "软件解码器未还原任何帧");

    // VA-API decode round-trip (best effort; failure here is not fatal to the
    // software-validated stream but is logged).
    let au: Vec<Vec<u8>> = idr.iter().chain(inter.iter()).map(|p| p.data.clone()).collect();
    match super::decoder::vaapi_decode_roundtrip(frame.width, frame.height, &au) {
        Ok(n) => tracing::debug!(frames = n, "VA-API 解码往返通过"),
        Err(e) => tracing::warn!(error = %e, "VA-API 解码往返未通过（软件解码已通过）"),
    }

    Ok(Capability {
        adapter: 0,
        backend: Backend::Intel,
        format: Format::AVC,
        maximum: encoder.maximum_size(),
    })
}

enum Inner {
    Software(Core),
    Vaapi(Box<vaapi::Encoder>),
}

pub(crate) struct Encoder {
    inner: Inner,
    size: (u32, u32),
    format: Format,
    rate: Rate,
    frame_rate: Controller,
}
impl Encoder {
    pub(crate) fn hardware_format(
        _device: &(),
        size: (u32, u32),
        format: Format,
        rate: Rate,
        _color: format::Color,
    ) -> Result<Self> {
        ensure!(format == Format::AVC, "VA-API 被控仅支持 H.264 (AVC)");
        let encoder = vaapi::Encoder::new(size, rate.target, rate.fps)?;
        Ok(Self {
            inner: Inner::Vaapi(Box::new(encoder)),
            size,
            format,
            rate,
            frame_rate: Controller::new(rate),
        })
    }
    pub(crate) fn software_format(
        _device: &(),
        size: (u32, u32),
        format: Format,
        rate: Rate,
    ) -> Result<Self> {
        let core = Core::new(Config {
            width: size.0,
            height: size.1,
            format: format.software(),
            fps: rate.fps,
            bitrate: rate.target,
        })?;
        Ok(Self {
            inner: Inner::Software(core),
            size,
            format,
            rate,
            frame_rate: Controller::new(rate),
        })
    }
    pub(crate) fn implementation(&self) -> i32 {
        match &self.inner {
            Inner::Software(_) => Backend::Software.implementation(),
            Inner::Vaapi(_) => Backend::Intel.implementation(),
        }
    }
    pub(crate) fn maximum_size(&self) -> (u32, u32) {
        match &self.inner {
            Inner::Software(_) => openuuyc_codec::encoder::maximum_size(self.format.codec.media()),
            Inner::Vaapi(v) => v.maximum_size(),
        }
    }
    pub(crate) fn configure_rate(&mut self, rate: Rate) -> Result<bool> {
        let Some(update) = self.frame_rate.decide(rate) else {
            return Ok(false);
        };
        match &mut self.inner {
            Inner::Software(core) => core.configure(update.rate.fps, update.rate.target)?,
            Inner::Vaapi(v) => v.set_rate(update.rate.target, update.rate.fps),
        }
        let key = self.rate.quality != rate.quality;
        self.rate = rate;
        self.frame_rate.commit(update);
        Ok(key)
    }
    pub(crate) fn encode(&mut self, bgra: &[u8], timestamp: i64, keyframe: bool) -> Result<Vec<Encoded>> {
        self.encode_cancellable(bgra, timestamp, keyframe, &Arc::new(AtomicBool::new(false)))
    }
    pub(crate) fn encode_cancellable(
        &mut self,
        bgra: &[u8],
        timestamp: i64,
        keyframe: bool,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Vec<Encoded>> {
        ensure!(
            !cancel.load(std::sync::atomic::Ordering::Acquire),
            "编码已取消"
        );
        self.frame_rate.input(timestamp);
        let pitch = self.size.0 as usize * 4;
        ensure!(
            bgra.len() >= pitch * self.size.1 as usize,
            "采集帧尺寸与编码器不一致"
        );
        let color = format::Color::Sdr.space(None);
        match &mut self.inner {
            Inner::Software(core) => {
                if keyframe {
                    core.request_keyframe();
                }
                core.prepare(bgra, pitch, cancel)?;
                Ok(core
                    .encode(timestamp, keyframe, cancel)?
                    .into_iter()
                    .map(|p| Encoded {
                        data: p.data,
                        keyframe: p.keyframe,
                        timestamp_100ns: p.timestamp_100ns,
                        is_new: true,
                        timing: None,
                        format: self.format,
                        color,
                    })
                    .collect())
            }
            Inner::Vaapi(v) => Ok(v
                .encode(bgra, timestamp, keyframe)?
                .into_iter()
                .map(|p| Encoded {
                    data: p.data,
                    keyframe: p.keyframe,
                    timestamp_100ns: timestamp,
                    is_new: true,
                    timing: None,
                    format: self.format,
                    color,
                })
                .collect()),
        }
    }
}
