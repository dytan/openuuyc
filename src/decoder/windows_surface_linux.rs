//! Linux placeholder for the Windows D3D11 surface writer types.
//! Video presentation on Linux will use CPU RGBA (and later Vulkan/GL) instead.
use anyhow::{Result, bail};

/// Placeholder so shared viewer types compile; hardware zero-copy is Windows-only today.
#[derive(Clone, Debug)]
pub(crate) struct D3D11SurfaceWriter;

#[derive(Debug)]
pub(crate) struct D3D11Surface;

impl D3D11SurfaceWriter {
    pub(crate) fn new() -> Result<Self> {
        bail!("TODO(linux): D3D11 surfaces are Windows-only")
    }

    pub(crate) fn available() -> Result<Vec<Self>> {
        Ok(Vec::new())
    }
}
