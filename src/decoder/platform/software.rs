//! Portable CPU H.264 software decoder backed by `openuuyc-h264`.
use crate::decoder::platform::{
    DecodeError, DecoderNotification, VideoDecoder, VideoDecoderConfig, VideoOutputPreference,
};
use mediaway_common::{Bytes, CodecKind, Packet};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuFormat {
    Nv12,
    I444,
}

pub struct CpuVideoFrame {
    pub pts: i64,
    pub width: u32,
    pub height: u32,
    pub format: CpuFormat,
    pub data: Bytes,
}

pub enum DecodedFrame {
    Cpu(CpuVideoFrame),
}

pub struct SoftwareVideoDecoder {
    decoder: openuuyc_h264::stream::Decoder,
    pending: VecDeque<DecodedFrame>,
    notification: DecoderNotification,
}

fn software_error(error: openuuyc_h264::Error) -> DecodeError {
    use openuuyc_h264::Error;
    match error {
        Error::Cancelled | Error::Closed => DecodeError::Closed,
        Error::Unsupported(_) => DecodeError::Unsupported,
        Error::Allocation => DecodeError::Backend,
        Error::NeedKeyframe | Error::Truncated | Error::Invalid(_) => {
            tracing::debug!(%error, "Rust H264 input requires a new keyframe");
            DecodeError::NeedKeyframe
        }
    }
}

impl SoftwareVideoDecoder {
    pub fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        if config.output != VideoOutputPreference::CpuFramesOk {
            return Err(DecodeError::Unsupported);
        }
        if config.codec != CodecKind::H264 {
            return Err(DecodeError::Unsupported);
        }
        if config.width == 0
            || config.height == 0
            || config.width > 16384
            || config.height > 16384
        {
            return Err(DecodeError::InvalidInput);
        }
        let mut decoder = openuuyc_h264::stream::Decoder::new();
        decoder.seed(&config.extra_data).map_err(software_error)?;
        Ok(Self {
            decoder,
            pending: VecDeque::new(),
            notification: DecoderNotification::default(),
        })
    }

    pub fn poll_owned_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        if self.notification.is_cancelled() {
            self.pending.clear();
            return Err(DecodeError::Closed);
        }
        Ok(self.pending.pop_front())
    }

    pub fn poll_dropped_token(&mut self) -> Option<i64> {
        None
    }

    pub fn reset_for_keyframe(&mut self) -> Result<(), DecodeError> {
        self.pending.clear();
        self.decoder.reset().map_err(software_error)
    }
}

impl VideoDecoder for SoftwareVideoDecoder {
    fn set_notification(&mut self, notification: DecoderNotification) {
        self.notification = notification;
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if packet.payload.len() > i32::MAX as usize || packet.duration > i64::MAX as u64 {
            return Err(DecodeError::InvalidInput);
        }
        let outputs = self
            .decoder
            .submit_with_cancel(
                &packet.payload,
                packet.pts as u64,
                self.notification.cancellation(),
            )
            .map_err(software_error)?;
        for output in outputs {
            let picture = output.picture;
            let mut packed = Vec::new();
            picture.pack_into(&mut packed).map_err(software_error)?;
            if self.notification.is_cancelled() {
                self.pending.clear();
                self.decoder.reset().map_err(software_error)?;
                return Err(DecodeError::Closed);
            }
            self.pending.push_back(DecodedFrame::Cpu(CpuVideoFrame {
                pts: output.token as i64,
                width: picture.crop.width as u32,
                height: picture.crop.height as u32,
                format: if picture.chroma == openuuyc_h264::picture::Chroma::Yuv444 {
                    CpuFormat::I444
                } else {
                    CpuFormat::Nv12
                },
                data: Bytes::from(packed),
            }));
        }
        Ok(())
    }
}
