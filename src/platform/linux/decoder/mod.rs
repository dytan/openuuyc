//! Linux video decode behind the same contract as the Windows decoder.
//!
//! Hardware mode is VA-API (H.264 and HEVC, 8-bit 4:2:0) through the local
//! driver: Intel, AMD, or NVIDIA via nvidia-vaapi-driver. Software mode is the
//! shared Rust codec core. Both hand frames over on the CPU; VA-API surfaces
//! are read back as NV12, so there is no zero-copy GPU frame type here.
use crate::media::VideoCodec;
use crate::media::decode_api::{DecodeError, DecoderMode, DecoderNotification, VideoDecoderConfig};
use bytes::Bytes;
use std::collections::VecDeque;

mod vaapi;

/// GPU frames only exist with D3D11; Linux never produces one.
pub enum WindowsGpuVideoFrame {}

impl WindowsGpuVideoFrame {
    pub const fn pts(&self) -> i64 {
        match *self {}
    }
    pub const fn width(&self) -> u32 {
        match *self {}
    }
    pub const fn height(&self) -> u32 {
        match *self {}
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsCpuFormat {
    Nv12,
    I444,
    P010,
    Ayuv,
    Y410,
}

pub struct WindowsCpuVideoFrame {
    pub pts: i64,
    pub width: u32,
    pub height: u32,
    pub coded_width: u32,
    pub coded_height: u32,
    pub format: WindowsCpuFormat,
    pub data: Bytes,
}

pub enum WindowsDecodedFrame {
    Gpu(WindowsGpuVideoFrame),
    Cpu(WindowsCpuVideoFrame),
}

enum Backend {
    /// VA-API decodes one access unit per call.
    Hardware(Box<vaapi::Session>),
    Software(Box<openuuyc_codec::decoder::Decoder>),
}

pub struct WindowsVideoDecoder {
    backend: Backend,
    pending: VecDeque<WindowsDecodedFrame>,
    notification: DecoderNotification,
    dropped: VecDeque<i64>,
}

fn software_error(error: openuuyc_codec::decoder::Error) -> DecodeError {
    use openuuyc_codec::decoder::Error;
    match error {
        Error::Closed => DecodeError::Closed,
        Error::Unsupported => DecodeError::Unsupported,
        Error::Allocation => DecodeError::Backend,
        Error::InvalidInput => DecodeError::InvalidInput,
        Error::NeedKeyframe => DecodeError::NeedKeyframe,
    }
}

fn software_frame(frame: openuuyc_codec::decoder::Frame) -> Result<WindowsDecodedFrame, DecodeError> {
    use openuuyc_codec::PixelFormat as F;
    Ok(WindowsDecodedFrame::Cpu(WindowsCpuVideoFrame {
        pts: frame.pts,
        width: frame.width,
        height: frame.height,
        coded_width: frame.coded_width,
        coded_height: frame.coded_height,
        data: frame.data,
        format: match frame.format {
            F::Nv12 => WindowsCpuFormat::Nv12,
            F::I444 => WindowsCpuFormat::I444,
            F::P010 => WindowsCpuFormat::P010,
            F::Ayuv => WindowsCpuFormat::Ayuv,
            F::Y410 => WindowsCpuFormat::Y410,
            F::Bgra => return Err(DecodeError::Unsupported),
        },
    }))
}

impl WindowsVideoDecoder {
    pub fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        let backend = match config.mode {
            DecoderMode::Software => {
                if config.width == 0
                    || config.height == 0
                    || config.width > 16384
                    || config.height > 16384
                {
                    return Err(DecodeError::InvalidInput);
                }
                Backend::Software(Box::new(
                    openuuyc_codec::decoder::Decoder::new(config.codec, &config.extra_data)
                        .map_err(software_error)?,
                ))
            }
            _ => Backend::Hardware(Box::new(vaapi::Session::open(config)?)),
        };
        let mut decoder = Self {
            backend,
            pending: VecDeque::new(),
            notification: DecoderNotification::default(),
            dropped: VecDeque::new(),
        };
        decoder.set_notification(decoder.notification.clone());
        Ok(decoder)
    }

    /// Windows-shaped probe: there is never a D3D11 device on Linux.
    pub fn probe_format(
        device: crate::platform::surface::ID3D11Device,
        _codec: VideoCodec,
        _width: u32,
        _height: u32,
        _depth: u8,
        _chroma: u8,
    ) -> bool {
        match device {}
    }

    pub(crate) fn check_format(
        device: crate::platform::surface::ID3D11Device,
        _codec: VideoCodec,
        _width: u32,
        _height: u32,
        _depth: u8,
        _chroma: u8,
    ) -> anyhow::Result<()> {
        match device {}
    }

    /// Whether the local VA-API driver decodes this format in hardware.
    pub fn probe_hardware(codec: VideoCodec, width: u32, height: u32, depth: u8, chroma: u8) -> bool {
        vaapi::probe(codec, width, height, depth, chroma)
    }

    pub fn poll_owned_frame(&mut self) -> Result<Option<WindowsDecodedFrame>, DecodeError> {
        if self.notification.is_cancelled() {
            self.pending.clear();
            return Err(DecodeError::Closed);
        }
        Ok(self.pending.pop_front())
    }

    pub fn poll_dropped_token(&mut self) -> Option<i64> {
        self.dropped.pop_front()
    }

    pub fn reset_for_keyframe(&mut self) -> Result<(), DecodeError> {
        self.pending.clear();
        self.dropped.clear();
        match &mut self.backend {
            Backend::Hardware(session) => {
                session.reset();
                Ok(())
            }
            Backend::Software(session) => session.reset().map_err(software_error),
        }
    }

    pub fn set_notification(&mut self, notification: DecoderNotification) {
        if let Backend::Software(session) = &mut self.backend {
            session.set_cancellation(notification.shared_cancellation());
        }
        self.notification = notification;
    }

    pub fn push_packet(&mut self, payload: &[u8], token: i64) -> Result<(), DecodeError> {
        match &mut self.backend {
            Backend::Hardware(session) => {
                let frame = session.decode(payload, self.notification.cancellation())?;
                self.pending
                    .push_back(WindowsDecodedFrame::Cpu(WindowsCpuVideoFrame {
                        pts: token,
                        width: frame.width,
                        height: frame.height,
                        coded_width: frame.width,
                        coded_height: frame.height,
                        format: WindowsCpuFormat::Nv12,
                        data: Bytes::from(frame.data),
                    }));
                Ok(())
            }
            Backend::Software(session) => {
                let frames = session.push(payload, token).map_err(software_error)?;
                if frames.is_empty() {
                    self.dropped.push_back(token);
                }
                for frame in frames {
                    self.pending.push_back(software_frame(frame)?);
                }
                Ok(())
            }
        }
    }
}

impl Drop for WindowsVideoDecoder {
    fn drop(&mut self) {
        self.pending.clear();
    }
}

/// Decode a short Annex-B access-unit sequence with the VA-API H.264 decoder,
/// returning the number of frames produced. Used to validate the host VA-API
/// encoder's output. Returns an error if VA-API decode is unavailable.
pub(crate) fn vaapi_decode_roundtrip(
    width: u32,
    height: u32,
    units: &[Vec<u8>],
) -> anyhow::Result<usize> {
    vaapi::decode_roundtrip(width, height, units).map_err(|e| anyhow::anyhow!("{e:?}"))
}
