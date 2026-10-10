//! Host loopback audio capture is Windows-only.
use anyhow::Result;

pub(crate) struct Endpoint {
    pub id: String,
    name: std::convert::Infallible,
}
impl Endpoint {
    pub fn name(&self) -> String {
        match self.name {}
    }
}
#[derive(Default)]
pub(crate) struct Change {
    pub refresh: bool,
    pub rebuild: bool,
}
pub(crate) struct Devices {
    _never: std::convert::Infallible,
}
impl Devices {
    pub fn new() -> Result<Self> {
        crate::platform::linux::unsupported()
    }
    pub fn changes(&self, _selected: Option<&str>) -> Change {
        match self._never {}
    }
    pub fn endpoint(&self, _selected: Option<&str>) -> Result<Option<Endpoint>> {
        match self._never {}
    }
    pub fn list(&self) -> Result<Vec<Endpoint>> {
        match self._never {}
    }
}
