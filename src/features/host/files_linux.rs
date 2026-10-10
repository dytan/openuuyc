//! Host-side file transfer runs in the Windows host agent; Linux only keeps the
//! status shape shared with access reporting.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Status {
    pub active: bool,
    pub error: Option<String>,
}
