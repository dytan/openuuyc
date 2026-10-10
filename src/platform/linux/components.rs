//! Windows installer/driver component management. Linux installs through its
//! own packages (AppImage/deb/Arch), so every operation reports unsupported.
use anyhow::Result;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Kind {
    Application,
    Suite,
    #[value(skip)]
    HostService,
    #[value(skip)]
    InputDriver,
    #[value(skip)]
    DisplayDriver,
    AudioDriver,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Operation {
    Install,
    Uninstall,
}
pub(crate) struct Status {
    pub label: String,
    pub installed: bool,
    pub removable: bool,
    pub ready: bool,
}
#[derive(Clone, Copy, Debug, Default, clap::Args)]
pub struct RemovalOptions {
    #[arg(long)]
    pub remove_display_driver: bool,
    #[arg(long)]
    pub remove_audio_driver: bool,
    #[arg(long)]
    pub remove_data: bool,
}

impl Kind {
    pub(crate) const ALL: [Self; 4] = [
        Self::HostService,
        Self::InputDriver,
        Self::DisplayDriver,
        Self::AudioDriver,
    ];
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Application => "程序",
            Self::Suite => "服务",
            Self::HostService => "被控服务",
            Self::InputDriver => "输入驱动",
            Self::DisplayDriver => "虚拟显示驱动",
            Self::AudioDriver => "虚拟音频驱动",
        }
    }
}
pub(crate) fn maintaining() -> bool {
    false
}
pub(crate) fn elevated() -> Result<bool> {
    // SAFETY: geteuid has no preconditions.
    Ok(unsafe { libc::geteuid() } == 0)
}
pub(crate) fn status(kind: Kind) -> Result<Status> {
    Ok(Status {
        label: kind.label().into(),
        installed: false,
        removable: false,
        ready: false,
    })
}
pub(crate) fn execute(
    _kind: Kind,
    _operation: Operation,
    _allow_sas: bool,
    _owner: Option<&str>,
    _removal: RemovalOptions,
) -> Result<bool> {
    anyhow::bail!("Linux 版本通过发行版软件包安装和卸载")
}
pub(crate) fn request(
    kind: Kind,
    operation: Operation,
    allow_sas: bool,
    removal: RemovalOptions,
) -> Result<bool> {
    execute(kind, operation, allow_sas, None, removal)
}
pub(crate) fn request_update(_resume: bool) -> Result<bool> {
    anyhow::bail!("Linux 版本请通过软件包或 AppImage 更新")
}

pub(crate) mod files {
    use anyhow::{Result, ensure};
    use std::path::Path;
    /// Refuse symlinks where Windows refuses reparse points.
    pub(crate) fn reject_reparse(path: &Path) -> Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        ensure!(!metadata.file_type().is_symlink(), "不处理符号链接");
        Ok(())
    }
}

pub(crate) mod application {
    use anyhow::{Context, Result};
    use std::path::PathBuf;
    pub(crate) fn purge_machine_data() -> Result<()> {
        Ok(())
    }
    pub(crate) fn image() -> Result<PathBuf> {
        std::env::current_exe().context("读取程序路径失败")
    }
    pub(crate) fn start_installed(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<()> {
        std::process::Command::new(image()?)
            .args(arguments)
            .spawn()
            .context("启动程序失败")?;
        Ok(())
    }
}

pub(crate) mod migration {
    use serde::{Deserialize, Serialize};
    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    pub(crate) struct Report {
        pub complete: bool,
        pub pending: Vec<String>,
        pub preserved: Vec<String>,
    }
    pub(crate) struct UserRecovery;
    impl UserRecovery {
        pub(crate) fn start() -> Self {
            Self
        }
    }
    pub(crate) fn registered_client(_path: &std::path::Path) -> anyhow::Result<bool> {
        Ok(false)
    }
}

#[allow(dead_code)]
fn _unused(_: &Path) {}
