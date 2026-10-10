//! Placeholder for the D3D11 zero-copy surface path. The types stay in the
//! shared decoder signatures; they can never be constructed on Linux, so every
//! frame takes the CPU/VA-API path.
use anyhow::{Result, bail};

#[derive(Clone)]
pub(crate) enum D3D11SurfaceWriter {}

#[derive(Debug)]
pub(crate) enum D3D11Surface {}

/// Stand-in for the D3D11 device handle carried in shared decoder config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ID3D11Device {}

impl D3D11SurfaceWriter {
    pub(crate) fn new() -> Result<Self> {
        bail!("当前平台没有 D3D11 零拷贝解码表面")
    }
    pub(crate) fn available() -> Result<Vec<Self>> {
        Ok(Vec::new())
    }
    pub(crate) fn adapter_ids() -> Result<Vec<u64>> {
        Ok(Vec::new())
    }
    pub(crate) fn for_adapter(_id: u64) -> Result<Self> {
        Self::new()
    }
    pub(crate) fn diagnostic_adapters() -> Result<Vec<(String, Result<Self>)>> {
        Ok(Vec::new())
    }
    pub(crate) fn adapter_id(&self) -> u64 {
        match *self {}
    }
    pub(crate) fn supports_software_av1(&self, _depth: u8, _chroma: u8) -> bool {
        match *self {}
    }
    pub(crate) fn device_handle(&self) -> ID3D11Device {
        match *self {}
    }
    pub(crate) fn upload_cpu(
        &self,
        _frame: &crate::platform::decoder::WindowsCpuVideoFrame,
    ) -> Result<D3D11Surface> {
        match *self {}
    }
    pub(crate) fn wrap_decoded_surface(
        &self,
        frame: crate::platform::decoder::WindowsGpuVideoFrame,
    ) -> Result<D3D11Surface> {
        match frame {}
    }
}

impl D3D11Surface {
    pub(crate) fn coded_size(&self) -> (u32, u32) {
        match *self {}
    }
    pub(crate) const fn visible_origin(&self) -> (u32, u32) {
        match *self {}
    }
    pub(crate) fn adapter_id(&self) -> u64 {
        match *self {}
    }
}
