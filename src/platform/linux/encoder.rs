//! Linux host video encoding: the in-tree Rust H.264 software encoder fed with
//! CPU BGRA frames. Hardware (VA-API) encoding is not wired up yet.
use crate::features::host::format::{self, Backend, Capability, Format, Rate};
use crate::media::encoding::rate::Controller;
use anyhow::{Context, Result, ensure};
use openuuyc_codec::encoder::{Config, Encoder as Core};
use std::sync::{Arc, atomic::AtomicBool};

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

/// Verify the software H.264 path against one real captured frame.
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
    let mut encoder = Encoder::software_format(&(), (frame.width, frame.height), Format::AVC, rate)?;
    let output = encoder.encode(&frame.texture, 0, true)?;
    let parsed = output
        .iter()
        .find_map(|p| crate::media::video_format::parse_stream_format(Format::AVC.codec.media(), &p.data))
        .context("软件编码器未输出可验证的序列头")?;
    ensure!(
        (parsed.visible_width, parsed.visible_height) == (frame.width, frame.height),
        "软件编码器输出尺寸与输入不一致"
    );
    Ok(vec![Capability {
        adapter: 0,
        backend: Backend::Software,
        format: Format::AVC,
        maximum: encoder.maximum_size(),
    }])
}

pub(crate) struct Encoder {
    core: Core,
    size: (u32, u32),
    format: Format,
    rate: Rate,
    frame_rate: Controller,
}
impl Encoder {
    pub(crate) fn hardware_format(
        _device: &(),
        _size: (u32, u32),
        _format: Format,
        _rate: Rate,
        _color: format::Color,
    ) -> Result<Self> {
        anyhow::bail!("Linux 被控暂无硬件编码")
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
            core,
            size,
            format,
            rate,
            frame_rate: Controller::new(rate),
        })
    }
    pub(crate) fn implementation(&self) -> i32 {
        5
    }
    pub(crate) fn maximum_size(&self) -> (u32, u32) {
        openuuyc_codec::encoder::maximum_size(self.format.codec.media())
    }
    pub(crate) fn configure_rate(&mut self, rate: Rate) -> Result<bool> {
        let Some(update) = self.frame_rate.decide(rate) else {
            return Ok(false);
        };
        self.core.configure(update.rate.fps, update.rate.target)?;
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
        if keyframe {
            self.core.request_keyframe();
        }
        let pitch = self.size.0 as usize * 4;
        ensure!(
            bgra.len() >= pitch * self.size.1 as usize,
            "采集帧尺寸与编码器不一致"
        );
        self.core.prepare(bgra, pitch, cancel)?;
        Ok(self
            .core
            .encode(timestamp, keyframe, cancel)?
            .into_iter()
            .map(|p| Encoded {
                data: p.data,
                keyframe: p.keyframe,
                timestamp_100ns: p.timestamp_100ns,
                is_new: true,
                timing: None,
                format: self.format,
                color: format::Color::Sdr.space(None),
            })
            .collect())
    }
}
