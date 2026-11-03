//! The Windows privileged host service (resident account owner, input/capture
//! agents, credential vault). None of it exists on Linux: there is never a
//! resident owner, the vault never applies, and agent connections fail.
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

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
            #[serde(default)]
            terminal: bool,
            #[serde(default)]
            terminal_local: bool,
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
        pub terminal: bool,
        #[serde(default)]
        pub terminal_local: bool,
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
    use super::*;
    use std::path::Path;
    use std::sync::OnceLock;

    /// The `app-<desktop id>` name lets xdg-desktop-portal identify the host
    /// process as `openuuyc` from its cgroup, the same as a launch from the
    /// menu, so portal grants such as KDE's screen-sharing approval persist.
    const UNIT: &str = "app-openuuyc.service";
    /// The name earlier versions enabled; packages now ship it as a unit that
    /// only starts [`UNIT`], and [`migrate`] moves an enabled one over.
    const LEGACY_UNIT: &str = "openuuyc.service";
    /// systemd 的用户单元搜索路径里由发行版安装的那几处。包装的单元是权威版本，
    /// 便携运行时系统里没有，才按当前可执行文件另写一份到用户配置目录。
    const PACKAGED_DIRS: [&str; 3] = [
        "/usr/lib/systemd/user",
        "/usr/local/lib/systemd/user",
        "/etc/systemd/user",
    ];
    /// 两种机制都要用户配置目录：XDG 条目写在那里，便携运行时的单元也写在那里。
    const NO_CONFIG_DIR: &str = "无法确定用户配置目录（HOME 与 XDG_CONFIG_HOME 都没有设置），写不了 systemd 用户单元，也写不了 XDG 自启动条目";
    /// 本进程就在这个图形会话里跑，答案在运行期间不会变，所以只查一次。
    static REACHABLE: OnceLock<bool> = OnceLock::new();

    fn config_dir() -> Result<PathBuf> {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .context("无法确定用户配置目录")
    }

    /// 桌面有没有把自己接进 `graphical-session.target`：接了用 systemd 单元，没接
    /// 用 XDG 条目。
    fn session_reachable() -> bool {
        *REACHABLE.get_or_init(|| {
            std::process::Command::new("systemctl")
                .arg("--user")
                .args(["is-active", "graphical-session.target"])
                .output()
                .is_ok_and(|output| output.status.success())
        })
    }

    fn checked(args: &[&str], output: &std::process::Output) -> Result<()> {
        // 有的子命令（is-enabled）把状态写在标准输出上、错误时标准错误是空的。
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let reason = if stderr.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        } else {
            stderr
        };
        anyhow::ensure!(
            output.status.success(),
            "systemctl --user {} 失败：{reason}",
            args.join(" ")
        );
        Ok(())
    }

    /// 两种机制都用不了时的原因，界面据此把开关置灰并说明。桌面没接
    /// `graphical-session.target` 不算：那里改用 XDG 条目。
    pub(crate) fn unavailable() -> Option<String> {
        config_dir().err().map(|_| NO_CONFIG_DIR.to_owned())
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(crate) enum State {
        Disabled,
        Enabled,
        /// Windows keeps a separate Startup Apps approval the program cannot
        /// override. A desktop's disable switch is the unit or entry itself,
        /// which the settings switch rewrites, so it reads as plain `Disabled`.
        #[allow(dead_code)]
        DisabledByWindows,
    }

    /// Whether login starts OpenUUYC on this desktop: the unit is enabled and
    /// the desktop reaches its target, or the XDG entry is present and not
    /// skipped. The settings switch and `systemctl --user enable/disable` are
    /// the same thing where the unit is the mechanism.
    pub(crate) fn state() -> Result<State> {
        Ok(if enabled()? {
            State::Enabled
        } else {
            State::Disabled
        })
    }

    /// The settings switch. There is no installed copy apart from the running
    /// one, so login starts this program, as enabling remote wake does.
    pub(crate) fn configure(enabled: bool) -> Result<()> {
        if !enabled {
            return set_image(false, Path::new(""));
        }
        set_image(true, &std::env::current_exe()?)
    }

    /// Older call sites enable or disable login startup of the running binary.
    pub(crate) fn set(enabled: bool) -> Result<()> {
        configure(enabled)
    }

    pub(crate) fn set_image(enabled: bool, path: &Path) -> Result<()> {
        let places = Places::current()?;
        if !enabled {
            return places.disable();
        }
        anyhow::ensure!(path.is_file(), "尚未部署自启动程序");
        let image = path.to_str().context("程序路径不是 UTF-8")?;
        anyhow::ensure!(!image.contains(['\n', '\r']), "程序路径含换行");
        places.enable(image, session_reachable())
    }

    pub(crate) fn enabled() -> Result<bool> {
        Places::current()?.enabled(session_reachable())
    }

    /// Moves login startup enabled by an earlier version from [`LEGACY_UNIT`]
    /// to [`UNIT`]. A portable run's own legacy unit is a full copy that would
    /// keep starting the program under the old name; the packaged one only
    /// forwards, but is retired too.
    pub(crate) fn migrate() {
        let result = (|| -> Result<()> {
            let places = Places::current()?;
            if !session_reachable() || !places.unit_name_enabled(LEGACY_UNIT)? {
                return Ok(());
            }
            let image = std::env::current_exe()?;
            places.enable_unit(image.to_str().context("程序路径不是 UTF-8")?)
        })();
        if let Err(error) = result {
            tracing::warn!(error = %format!("{error:#}"), "旧版自启动单元迁移失败");
        }
    }

    /// 自启读写的位置。测试换成临时目录，碰不到真实的用户配置与系统里的单元。
    struct Places<'a> {
        config: PathBuf,
        /// Directories a package installs user units into.
        packaged: &'a [&'a str],
        /// 测试指向一个不存在的程序，当作没有 systemd 的系统，免得读到、甚至停用
        /// 跑测试的人自己启用的单元。
        systemctl: &'a str,
    }

    impl Places<'static> {
        fn current() -> Result<Self> {
            Ok(Self {
                config: config_dir()?,
                packaged: &PACKAGED_DIRS,
                systemctl: "systemctl",
            })
        }
    }

    impl Places<'_> {
        /// 跑一次 `systemctl --user`。没有 systemd 的发行版上根本没有 systemctl，那时
        /// 返回 `None`，由调用方决定这算不算错。
        fn run_systemctl(&self, args: &[&str]) -> Result<Option<std::process::Output>> {
            match std::process::Command::new(self.systemctl)
                .arg("--user")
                .args(args)
                .output()
            {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                result => result.context("无法执行 systemctl").map(Some),
            }
        }

        fn systemctl(&self, args: &[&str]) -> Result<()> {
            let output = self.run_systemctl(args)?.context("找不到 systemctl")?;
            checked(args, &output)
        }

        /// 我们自己写的那份单元，与包装的同名单元不同路径。
        fn generated_unit(&self) -> PathBuf {
            self.config.join("systemd/user").join(UNIT)
        }

        fn generated_legacy_unit(&self) -> PathBuf {
            self.config.join("systemd/user").join(LEGACY_UNIT)
        }

        /// systemd 现在能不能看到这个单元，包装的或我们自己写的都算。
        fn unit_name_installed(&self, name: &str) -> bool {
            self.config.join("systemd/user").join(name).exists()
                || self
                    .packaged
                    .iter()
                    .any(|directory| Path::new(directory).join(name).exists())
        }

        fn entry(&self) -> PathBuf {
            self.config.join("autostart/openuuyc.desktop")
        }

        /// 自启在这个桌面上是否开着。`systemd` 是桌面接没接 `graphical-session.target`：
        /// 没接的桌面上 enable 了的单元登录时不会被拉起，不算。XDG 条目在接了的桌面
        /// （GNOME、KDE 的会话管理器或 systemd 的 xdg-autostart 生成器）上照样会启动
        /// 程序，所以哪里都算——升级前写入的条目也因此显示为开。
        fn enabled(&self, systemd: bool) -> Result<bool> {
            Ok(self.entry_runs()? || (systemd && self.unit_enabled()?))
        }

        fn enable(&self, image: &str, systemd: bool) -> Result<()> {
            if systemd {
                self.enable_unit(image)?;
                if let Err(error) = self.remove_entry() {
                    tracing::warn!(error = %format!("{error:#}"), "XDG 自启动条目没能删除");
                }
            } else {
                self.write_entry(image)?;
                // 先前在接了 target 的桌面上开过单元：留着它，回到那个桌面时会起两次。
                if let Err(error) = self.disable_unit() {
                    tracing::warn!(error = %format!("{error:#}"), "自启动单元没能停用");
                }
            }
            Ok(())
        }

        fn disable(&self) -> Result<()> {
            self.disable_unit()?;
            self.remove_entry()
        }

        /// XDG 条目会不会在登录时启动程序：桌面被明确告知跳过它就不算。
        fn entry_runs(&self) -> Result<bool> {
            let entry = self.entry();
            let text = match std::fs::read_to_string(&entry) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                result => {
                    result.with_context(|| format!("无法读取自启动设置 {}", entry.display()))?
                }
            };
            Ok(!text.lines().map(str::trim).any(|line| {
                line.eq_ignore_ascii_case("Hidden=true")
                    || line.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
            }))
        }

        fn write_entry(&self, image: &str) -> Result<()> {
            let entry = self.entry();
            if let Some(directory) = entry.parent() {
                std::fs::create_dir_all(directory)?;
            }
            std::fs::write(&entry, entry_body(image))
                .with_context(|| format!("无法写入自启动设置 {}", entry.display()))
        }

        fn remove_entry(&self) -> Result<()> {
            match std::fs::remove_file(self.entry()) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(error).context("无法删除本程序自启动设置")
                }
                _ => Ok(()),
            }
        }

        /// 单元现在是否已启用，查的是 systemd 的实际状态。
        fn unit_enabled(&self) -> Result<bool> {
            Ok(self.unit_name_enabled(UNIT)? || self.unit_name_enabled(LEGACY_UNIT)?)
        }

        fn unit_name_enabled(&self, name: &str) -> Result<bool> {
            // 单元没装时不必问 systemctl：v253 起 `is-enabled` 才输出 `not-found`，
            // systemd 249（Ubuntu 22.04，deb 只投这个）对不存在的单元标准输出什么都不
            // 打、只往标准错误写一行就非零退出。
            if !self.unit_name_installed(name) {
                return Ok(false);
            }
            // 包里的单元文件在、系统却没有 systemd（systemctl 不存在）时，单元不会被
            // 任何人启动，就是没启用；别让开关读成「无法读取」，旧条目也还得能关掉。
            let Some(output) = self.run_systemctl(&["is-enabled", name])? else {
                return Ok(false);
            };
            // 未启用、被 mask 都是非零退出，状态只写在标准输出上。
            let state = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            match state.as_str() {
                "enabled" | "enabled-runtime" | "alias" | "static" | "indirect" | "generated" => {
                    Ok(true)
                }
                "disabled" | "linked" | "linked-runtime" | "masked" | "not-found" => Ok(false),
                // 状态查不到时标准输出是空的，退出码非零同样只是「没启用」。
                "" if !output.status.success() => Ok(false),
                "" => bail!(
                    "systemctl --user is-enabled 没有给出状态：{}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
                other => bail!("无法识别的单元状态：{other}"),
            }
        }

        fn enable_unit(&self, image: &str) -> Result<()> {
            check_image(image)?;
            let generated = self.generated_unit();
            // 包装的单元已经把可执行文件位置写死成安装路径；只有在没有包装单元、或者
            // 之前那份本来就是我们自己写的时候，才按当前可执行文件重写一份，免得便携
            // 运行时换了个位置后单元指向一个已经不存在的文件。
            if generated.exists() || !self.unit_name_installed(UNIT) {
                if let Some(directory) = generated.parent() {
                    std::fs::create_dir_all(directory)?;
                }
                std::fs::write(&generated, unit_body(image))
                    .with_context(|| format!("无法写入自启动单元 {}", generated.display()))?;
                self.systemctl(&["daemon-reload"])?;
            }
            // 这里只 enable 不 --now：正在跑的就是本进程，再起一份会被实例锁挡住。
            self.systemctl(&["enable", UNIT])?;
            self.disable_legacy_unit()?;
            Ok(())
        }

        fn disable_unit(&self) -> Result<()> {
            // 没启用（包括单元不在）时不必让 systemctl 再报一次错，那就是我们要的结果。
            for name in [UNIT, LEGACY_UNIT] {
                if self.unit_name_enabled(name)? {
                    self.systemctl(&["disable", name])?;
                }
            }
            // 只删我们自己写的那份；包装的单元归包管。
            let mut removed = false;
            for generated in [self.generated_unit(), self.generated_legacy_unit()] {
                if generated.exists() {
                    std::fs::remove_file(&generated).context("删除自启动单元失败")?;
                    removed = true;
                }
            }
            if removed {
                // 没有 systemd 时也就没有要重新加载的用户管理器。
                if let Some(output) = self.run_systemctl(&["daemon-reload"])? {
                    checked(&["daemon-reload"], &output)?;
                }
            }
            Ok(())
        }

        fn disable_legacy_unit(&self) -> Result<()> {
            if self.unit_name_enabled(LEGACY_UNIT)? {
                self.systemctl(&["disable", LEGACY_UNIT])?;
            }
            let generated = self.generated_legacy_unit();
            if generated.exists() {
                std::fs::remove_file(&generated).context("删除旧版自启动单元失败")?;
                if let Some(output) = self.run_systemctl(&["daemon-reload"])? {
                    checked(&["daemon-reload"], &output)?;
                }
            }
            Ok(())
        }
    }

    /// XDG 自启动条目，与改用 systemd 单元之前写的一模一样。
    fn entry_body(image: &str) -> String {
        // Desktop Entry quoting: reserved characters get a backslash inside the
        // quoted argument, and the file's string escaping doubles every backslash.
        let mut quoted = String::new();
        for c in image.chars() {
            match c {
                '\\' => quoted.push_str(r"\\\\"),
                '"' | '`' | '$' => {
                    quoted.push_str(r"\\");
                    quoted.push(c);
                }
                '%' => quoted.push_str("%%"),
                _ => quoted.push(c),
            }
        }
        format!(
            "[Desktop Entry]\nType=Application\nName=OpenUUYC\n\
             Exec=\"{quoted}\" gui --background\nX-GNOME-Autostart-enabled=true\n"
        )
    }

    /// 单元内容。包里的 `packaging/linux/app-openuuyc.service` 是同一份定义，改这里时
    /// 要一起改。
    fn unit_body(image: &str) -> String {
        let specifier = image.replace('%', "%%");
        format!(
            "[Unit]\n\
             Description=OpenUUYC 远程被控\n\
             # 被控要有画面可采，所以跟着图形会话走：会话结束就停，也不在没人登录时硬撑。\n\
             PartOf=graphical-session.target\n\
             After=graphical-session.target xdg-desktop-portal.service\n\
             # 程序不在时（例如已卸载，只剩这份写在 ~/.config 里的单元）跳过这次启动：\n\
             # 条件不满足是 skipped 而不是 failed，Restart= 不会再试。\n\
             ConditionFileIsExecutable={}\n\
             # 与 Sunshine 的用户单元相同：偶尔崩溃照样拉起，一启动就崩的约 25 秒后停下，\n\
             # 不会无休止地重启、刷日志。\n\
             StartLimitIntervalSec=500\n\
             StartLimitBurst=5\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart={} gui --background\n\
             # 崩了自动回来；从托盘退出是正常退出，不会又被拉起来。\n\
             Restart=on-failure\n\
             RestartSec=5\n\
             # 默认的 control-group 会把子进程一起杀掉：显示恢复守护（`process_group(0)`）\n\
             # 本该跨崩溃活着，`xdg-open` 打开的浏览器也不该跟着退。\n\
             KillMode=process\n\
             \n\
             [Install]\n\
             WantedBy=graphical-session.target\n",
            specifier,
            exec_quote(image)
        )
    }

    /// 单元里表达不了的可执行路径：换行会把一行写成两行，`"` 与 `\` 在 systemd 的
    /// 命令行解析里是特殊字符（`Executable path contains special characters`）。
    fn check_image(image: &str) -> Result<()> {
        anyhow::ensure!(!image.contains(['\n', '\r']), "程序路径含换行");
        anyhow::ensure!(
            !image.contains(['"', '\\']),
            "程序路径含引号或反斜杠，无法写入自启动单元"
        );
        Ok(())
    }

    /// `ExecStart` 里 `%` 是说明符，字面量要写两遍；`$` 在可执行路径上既不展开也不
    /// 折叠（`ExecStart="/x/a$$b"` 找的就是 `a$$b`），写两遍反而找不到程序。`"` 与
    /// `\` 由 [`check_image`] 拒绝。
    fn exec_quote(image: &str) -> String {
        format!("\"{}\"", image.replace('%', "%%"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn quotes_and_escapes_the_executable_path() {
            assert_eq!(exec_quote("/usr/bin/OpenUUYC"), r#""/usr/bin/OpenUUYC""#);
            // 说明符要写成字面量；变量展开在可执行路径上不存在，`$` 不能双写
            assert_eq!(exec_quote("/opt/a%b/c$d"), r#""/opt/a%%b/c$d""#);
            // 引号与反斜杠在单元里表达不了，只能拒绝
            assert!(check_image("/usr/bin/OpenUUYC").is_ok());
            assert!(check_image(r#"/opt/a"b\c"#).is_err());
            assert!(check_image("/opt/a\nb").is_err());
        }

        /// 包里的单元与程序自己写的必须说同一件事：包装时用前者，便携运行时用后者。
        #[test]
        fn the_shipped_unit_agrees_with_the_generated_one() {
            let shipped = include_str!("../../../packaging/linux/app-openuuyc.service");
            let generated = unit_body("/usr/bin/OpenUUYC");
            for line in [
                "PartOf=graphical-session.target",
                "After=graphical-session.target",
                "ConditionFileIsExecutable=/usr/bin/OpenUUYC",
                "StartLimitIntervalSec=500",
                "StartLimitBurst=5",
                "Type=simple",
                "Restart=on-failure",
                "RestartSec=5",
                "KillMode=process",
                "WantedBy=graphical-session.target",
            ] {
                assert!(shipped.contains(line), "单元文件缺少 {line}");
                assert!(generated.contains(line), "生成的单元缺少 {line}");
            }
            assert!(shipped.contains("ExecStart=/usr/bin/OpenUUYC gui --background"));
        }

        #[test]
        fn portal_identity_uses_the_desktop_entry_id() {
            let desktop = include_str!("../../../packaging/linux/openuuyc.desktop");
            let compatibility = include_str!("../../../packaging/linux/openuuyc.service");
            assert_eq!(UNIT, "app-openuuyc.service");
            assert!(desktop.contains("Name=OpenUUYC"));
            assert!(compatibility.contains(
                "ExecStart=/usr/bin/systemctl --user --no-block start app-openuuyc.service"
            ));
            assert!(compatibility.contains(
                "ExecStop=/usr/bin/systemctl --user --no-block stop app-openuuyc.service"
            ));
        }

        #[test]
        fn unit_follows_the_graphical_session() {
            let body = unit_body("/usr/bin/OpenUUYC");
            assert!(body.contains("ExecStart=\"/usr/bin/OpenUUYC\" gui --background"));
            assert!(body.contains("WantedBy=graphical-session.target"));
            assert!(body.contains("PartOf=graphical-session.target"));
            assert!(body.contains("Restart=on-failure"));
        }

        /// 临时配置目录、没有包装单元：碰不到真实的用户配置，也不会去问 systemctl。
        fn scratch<'a>(name: &str) -> Places<'a> {
            let config = std::env::temp_dir()
                .join(format!("openuuyc-startup-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&config);
            Places {
                config,
                packaged: &[],
                systemctl: "/nonexistent/openuuyc-test/systemctl",
            }
        }

        /// 到不了 `graphical-session.target` 的桌面（XFCE、MATE、Cinnamon）上开启写的
        /// 是 XDG 条目，状态读回为开，关闭时删掉。
        #[test]
        fn falls_back_to_the_xdg_entry_without_the_session_target() {
            let places = scratch("xdg");
            assert!(!places.enabled(false).unwrap());
            places.enable("/opt/Open UUYC/a%b$c", false).unwrap();
            let text = std::fs::read_to_string(places.entry()).unwrap();
            assert!(text.contains(r#"Exec="/opt/Open UUYC/a%%b\\$c" gui --background"#));
            assert!(!places.generated_unit().exists());
            assert!(places.enabled(false).unwrap());
            // 接了 target 的桌面上会话管理器同样会运行这个条目。
            assert!(places.enabled(true).unwrap());
            places.disable().unwrap();
            assert!(!places.entry().exists());
            assert!(!places.enabled(false).unwrap());
            // 已经关着时再关一次不是错误。
            places.disable().unwrap();
            let _ = std::fs::remove_dir_all(&places.config);
        }

        /// 包里的单元文件在而 systemd 不在（或这个用户的管理器里没有它）时，读作关而
        /// 不是「无法读取」，XDG 条目照样能开能关。只读地问一次 `is-enabled`。
        #[test]
        fn a_packaged_unit_without_systemd_reads_as_off() {
            let mut places = scratch("packaged");
            std::fs::create_dir_all(&places.config).unwrap();
            let directory = places.config.to_str().unwrap().to_owned();
            let packaged = [directory.as_str()];
            places.packaged = &packaged;
            // A package directory without the unit does not count.
            assert!(!places.unit_name_installed(UNIT));
            std::fs::write(places.config.join(UNIT), "").unwrap();
            assert!(places.unit_name_installed(UNIT));
            assert!(!places.enabled(true).unwrap());
            places.enable("/usr/bin/OpenUUYC", false).unwrap();
            assert!(places.enabled(false).unwrap());
            places.disable().unwrap();
            assert!(!places.entry().exists());
            let _ = std::fs::remove_dir_all(&places.config);
        }

        /// 桌面被告知跳过的条目登录时不会运行，读作关；关闭开关时照样删掉。
        #[test]
        fn a_skipped_entry_reads_as_off() {
            let places = scratch("skipped");
            for skip in ["Hidden=true", "X-GNOME-Autostart-enabled=false"] {
                places.enable("/usr/bin/OpenUUYC", false).unwrap();
                let text = std::fs::read_to_string(places.entry()).unwrap();
                std::fs::write(
                    places.entry(),
                    text.replace("X-GNOME-Autostart-enabled=true", skip),
                )
                .unwrap();
                assert!(!places.enabled(false).unwrap(), "{skip}");
                places.disable().unwrap();
                assert!(!places.entry().exists());
            }
            let _ = std::fs::remove_dir_all(&places.config);
        }
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
