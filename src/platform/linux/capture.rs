//! Screen capture is a host feature; Linux does not host.
use anyhow::Result;
pub(crate) use crate::media::capture::{Screen, SourceGone};

pub(crate) fn session_locked() -> Option<bool> {
    None
}
pub(crate) fn screens() -> Result<Vec<Screen>> {
    Ok(Vec::new())
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct EncodingAdapter {
    #[serde(default)]
    pub id: Option<crate::media::selection::GpuId>,
    pub luid: u64,
    pub vendor: u32,
    pub name: String,
}
/// No hardware encoders are used on Linux.
pub(crate) fn encoding_adapters() -> Result<Vec<EncodingAdapter>> {
    Ok(Vec::new())
}
