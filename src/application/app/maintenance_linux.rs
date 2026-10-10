//! Linux installs through the system package manager (deb, Arch package,
//! AppImage or install.sh), so the Windows in-place installer, updater and
//! uninstaller have no counterpart here.
use anyhow::Result;

pub(crate) fn route_gui() -> Result<bool> {
    Ok(false)
}
pub(crate) fn update_error_code(error: &anyhow::Error) -> i32 {
    crate::application::component_error_code(error).unwrap_or(1)
}
pub(crate) fn update(_silent: bool, _no_elevate: bool) -> Result<bool> {
    anyhow::bail!("Linux 版本请通过软件包管理器或新版 AppImage 更新")
}
pub(crate) fn uninstall(_parent: Option<u32>) -> Result<()> {
    anyhow::bail!("Linux 版本请通过软件包管理器卸载")
}
pub(crate) fn report_error(error: &anyhow::Error) {
    tracing::error!("{error:#}");
    eprintln!("OpenUUYC: {error:#}");
}
