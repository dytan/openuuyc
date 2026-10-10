//! The Windows privileged host service (resident account owner, input/capture
//! agents, credential vault). None of it exists on Linux: there is never a
//! resident owner, the vault never applies, and agent connections fail.
use anyhow::Result;

pub(crate) mod activity {
    //! Counts owned work until teardown has completed.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ACTIVE: AtomicUsize = AtomicUsize::new(0);
    pub(crate) struct Work;
    impl Work {
        pub fn new() -> Self {
            ACTIVE.fetch_add(1, Ordering::AcqRel);
            Self
        }
    }
    impl Drop for Work {
        fn drop(&mut self) {
            ACTIVE.fetch_sub(1, Ordering::AcqRel);
        }
    }
    pub(crate) fn idle() -> bool {
        ACTIVE.load(Ordering::Acquire) == 0
    }
}

pub(crate) mod resident {
    use crate::{account::auth::NativeIdentity, features::host, session::presence::PresenceState};
    use anyhow::Result;
    use serde::{Deserialize, Serialize};

    pub(crate) fn is_owner() -> bool {
        false
    }
    pub(crate) fn managed() -> bool {
        false
    }
    #[derive(Serialize, Deserialize)]
    pub(crate) enum Request {
        Resume,
        Pause,
        Quiescent,
        DeploymentReady,
        FinishMigration {
            target: String,
        },
        PrepareUpdate,
        CancelUpdate,
        Snapshot {
            ui: bool,
            #[serde(default)]
            device_cursor: Option<crate::account::device_change::relay::Cursor>,
        },
        Assist {
            account: String,
            action: host::assist::Action,
        },
        RefreshPublication,
        RefreshWol,
        WolSetup {
            account: String,
            action: host::wol::setup::Action,
        },
        Initialize {
            force: bool,
        },
        Controllable(bool),
        Name {
            device: String,
            value: String,
        },
        Settings {
            account: String,
            allowed: bool,
            encoding: host::EncodingSettings,
            audio_device: Option<host::audio::Device>,
            audio_defaults: host::audio::DefaultDevices,
            audio_quality: crate::media::audio::encoder::Quality,
            assistance: host::assist::Settings,
            clipboard: host::clipboard::Settings,
            file_transfer: bool,
            port_mapping: bool,
            remote_power: bool,
            wol: bool,
        },
        Disconnect {
            account: String,
            session: String,
        },
        Retry {
            account: String,
        },
        Retire {
            account: String,
        },
    }
    #[derive(Serialize, Deserialize)]
    pub(crate) struct Snapshot {
        #[serde(default)]
        pub device_changes: Option<crate::account::device_change::relay::Batch>,
        pub assistance: host::assist::Snapshot,
        pub publication: crate::account::reporting::Snapshot,
        pub account: String,
        pub online: PresenceState,
        pub allowed: bool,
        pub encoding: host::EncodingSettings,
        #[serde(default)]
        pub audio_device: Option<host::audio::Device>,
        #[serde(default)]
        pub audio_defaults: host::audio::DefaultDevices,
        #[serde(default)]
        pub audio_quality: crate::media::audio::encoder::Quality,
        #[serde(default)]
        pub clipboard: host::clipboard::Settings,
        #[serde(default)]
        pub file_transfer: bool,
        #[serde(default)]
        pub port_mapping: bool,
        #[serde(default)]
        pub remote_power: bool,
        #[serde(default)]
        pub wol: bool,
        pub status: host::Status,
        pub capabilities: Option<host::desktop::Capabilities>,
    }
    #[derive(Serialize, Deserialize)]
    pub(crate) enum Reply {
        Done,
        Migration(crate::platform::windows::components::migration::Report),
        Snapshot(Box<Snapshot>),
        Identity(Box<NativeIdentity>),
        Error(String),
        ApiError { code: i32, message: String },
    }
    impl Reply {
        pub fn checked(self) -> Result<Self> {
            match self {
                Self::Error(e) => anyhow::bail!(e),
                Self::ApiError { code, message } => {
                    Err(crate::account::api::ApiFailure { code, message }.into())
                }
                other => Ok(other),
            }
        }
    }
    pub(crate) fn call(_request: Request) -> Result<Reply> {
        super::super::unsupported()
    }
    pub(crate) async fn request(request: Request) -> Result<Reply> {
        call(request)
    }
    pub(crate) async fn pause_for_exit() -> Result<()> {
        Ok(())
    }
    pub(crate) fn paused() -> Result<bool> {
        Ok(false)
    }
    pub(crate) fn run(_parent: u32) -> Result<()> {
        super::super::unsupported()
    }
}

pub(crate) mod vault {
    use anyhow::Result;
    use std::path::PathBuf;
    pub(crate) fn root() -> Result<PathBuf> {
        crate::paths::credential_coord_dir()
    }
    pub(crate) fn sid(_pid: u32) -> Result<String> {
        // SAFETY: getuid has no preconditions.
        Ok(unsafe { libc::getuid() }.to_string())
    }
    pub(crate) fn owner() -> Result<Option<String>> {
        Ok(None)
    }
    pub(crate) fn applies() -> Result<bool> {
        Ok(false)
    }
    pub(crate) fn key(service: &str, account: &str) -> String {
        format!("{service}\u{1f}{account}")
    }
    pub(crate) fn read(_key: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    pub(crate) fn write(_key: &str, _value: Option<&[u8]>) -> Result<()> {
        super::super::unsupported()
    }
}

pub(crate) mod user_backend {
    use anyhow::Result;
    use serde::{Deserialize, Serialize};
    #[derive(Clone, Copy, Debug, Serialize, Deserialize)]
    pub(crate) enum Role {
        Clipboard,
        Files,
        Annotation,
    }
    pub(crate) struct Observer;
    impl Observer {
        pub fn alive(&self) -> bool {
            false
        }
    }
    pub(crate) fn observe() -> Result<Option<Observer>> {
        Ok(None)
    }
    pub(crate) fn active() -> bool {
        false
    }
    pub(crate) fn permitted() -> bool {
        false
    }
    pub(crate) struct Server;
    impl Server {
        pub fn start() -> Result<Self> {
            Ok(Self)
        }
    }
    pub(crate) struct Lease {
        pub pid: u32,
    }
    impl Lease {
        pub fn connect(
            _role: Role,
            _channel: &str,
            _session: u32,
            _allowed: impl Fn() -> bool,
        ) -> Result<Self> {
            super::super::unsupported()
        }
        pub fn alive(&self) -> bool {
            false
        }
        pub fn finish(&self) -> Result<()> {
            Ok(())
        }
    }
}

pub(crate) mod pipe {
    /// Owned OS handle placeholder; Linux never creates one.
    pub(crate) struct Handle(pub i64);
}

pub(crate) mod install {
    use anyhow::Result;
    #[derive(Debug)]
    pub(crate) struct NeedsUpdate;
    impl std::fmt::Display for NeedsUpdate {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("被控服务需要更新")
        }
    }
    impl std::error::Error for NeedsUpdate {}
    #[derive(Debug)]
    pub(crate) struct ActiveSession;
    impl std::fmt::Display for ActiveSession {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("存在活动的被控会话")
        }
    }
    impl std::error::Error for ActiveSession {}
    pub(crate) fn running() -> Result<bool> {
        Ok(false)
    }
    pub(crate) fn verify_running(_pid: u32) -> Result<()> {
        super::super::unsupported()
    }
    pub(crate) fn verify_client(_pid: u32) -> Result<()> {
        super::super::unsupported()
    }
}

pub(crate) mod service {
    use anyhow::Result;
    pub(crate) fn secure_attention() -> Result<()> {
        super::super::unsupported()
    }
    pub(crate) fn run() -> Result<()> {
        super::super::unsupported()
    }
}

pub(crate) mod startup {
    use anyhow::Result;
    pub(crate) fn set(_enabled: bool) -> Result<()> {
        super::super::unsupported()
    }
    pub(crate) fn set_image(_enabled: bool, _path: &std::path::Path) -> Result<()> {
        super::super::unsupported()
    }
}

pub(crate) mod process {
    use anyhow::{Context, Result};
    pub(crate) fn image(pid: u32) -> Result<std::path::PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/exe")).context("读取进程路径失败")
    }
}

#[allow(dead_code)]
fn _result() -> Result<()> {
    Ok(())
}
