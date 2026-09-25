//! Platform video decode contract.
//!
//! Windows: DXVA11 hardware + software H.264.
//! Linux: software H.264 (see `software`); hardware decode TODO(linux).

#![allow(unsafe_code)]

mod error;
mod video;

pub use error::DecodeError;
pub use video::{DecoderNotification, VideoDecoder, VideoDecoderConfig, VideoOutputPreference};

#[cfg(windows)]
pub mod windows;

#[cfg(any(windows, target_os = "linux"))]
pub mod software;
