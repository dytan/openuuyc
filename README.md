# OpenUUYC

![OpenUUYC](assets/banner.png)

本仓库（[`dytan/openuuyc`](https://github.com/dytan/openuuyc) 的 `linux/host`）是上游项目的 Linux 移植分支。上游已更名为 **Farwing**，见下文。本分支的应用二进制与界面仍为 **OpenUUYC**。Cargo 版本保持上游 `1.0.0-rc.2`（也是程序自报的引擎版本）；Linux 打包使用独立的 `v0.x` 线。当前说明：[v0.8.1 发行说明](packaging/linux/RELEASE-NOTES.md)。

| 文档 | 内容 |
| --- | --- |
| [LINUX.md](LINUX.md) | Linux 主控与被控状态 |
| [LINUX-HOST.md](LINUX-HOST.md) | 被控 / host |
| [LINUX-BUILD.md](LINUX-BUILD.md) | 从源码构建 |
| [packaging/linux/RELEASE-NOTES.md](packaging/linux/RELEASE-NOTES.md) | v0.8.1 发行说明 |

Linux 预编译包在本仓库 [Releases](https://github.com/dytan/openuuyc/releases)。`v0.8.1` 在合并进 `linux/host` 后由维护者打标签并发布。

## 上游已更名为 Farwing

上游仓库 [`djkcyl/openuuyc`](https://github.com/djkcyl/openuuyc) 现解析到 [djkcyl/farwing](https://github.com/djkcyl/farwing)。以下正文与上游 `next` 分支 README 一致（`docs: introduce Farwing`）。本 Linux 分支仍交付后面能力表所描述的 OpenUUYC 客户端。

**远方，随手可及。**

Farwing 是一个用 Rust 编写的远程桌面与串流项目。我们想让你在另一台设备上舒服地使用自己的电脑：继续工作、玩一局游戏，或者邀请别人一起看、一起操作。

家里的主机可以继续承担繁重的计算，出门只带一台轻便的设备。在书房打开的东西，换到客厅、办公室或旅途中，还能接着用。这是 Farwing 想带来的便利。

### 目标

**在远程桌面与游戏串流的整体体验上，超越 RustDesk、Moonlight 等现有开源产品。**

这个目标要落到每天会碰到的小事上。办公时，文字要清楚，多屏切换要顺手，复制内容、拖动文件要符合平时的习惯。玩游戏时，按键要跟手，运动中的画面要看得清，声音和画面要保持同步。我们希望这些需求能在同一个客户端里得到认真对待。

网络也很少一直理想。Wi-Fi 会波动，异地连接会遇到丢包，设备还会休眠和切换网络。我们在意这些时候能否继续操作、多久恢复画面，以及恢复以后能不能接着刚才的事做下去。延迟、画质、稳定性，都需要放到这些实际场景里衡量。

### 怎么连接，由你选择

我们希望连接自己的设备是一件很平常的事。

同一个局域网里的两台电脑，可以不登录账号，直接通过地址连接。经常使用的设备，可以放在同一个账号下，打开客户端就能找到。希望自己管理服务的人，也应该能部署自己的 Farwing Server，掌握账号、设备目录和连接协调。

自建本身也要足够简单：配置容易理解，出了问题能找到原因，更新和维护不用每次重新折腾一遍。服务端的使用体验同样属于这个项目。

### 一张桌面，也可以一起用

有时只是想给朋友看一段画面，有时需要帮家人处理电脑问题，也有时想和同事一起改点东西。我们希望这些事都能从一次简单的邀请开始。

观看和操作分别授权。别人加入时，主人能看清谁在连接、有什么权限；需要交接操作时，可以明确授予，也可以随时收回。多人共享应该方便，自己的电脑也始终由自己做主。

如果有一天，出门前只需要想带哪台轻便的设备，而不用再想家里的电脑上还有什么没带走，Farwing 就做成了一件有用的事。

### 开发方向

优先完善 Windows 客户端与自建服务端，打磨音视频、连接稳定性和日常操作体验，逐步扩展多人协作与跨平台支持。

## 本分支客户端

OpenUUYC 是用 Rust 编写的 UU 远程第三方 Windows 客户端，支持主控、被控和远程协助。本 Linux 分支在此之上提供 Linux 主控，以及自 v0.8.0 起的 Linux 被控（host），见 [LINUX.md](LINUX.md) 与 [LINUX-HOST.md](LINUX-HOST.md)。

Windows 1.0 已进入 **RC** 阶段，重点完善兼容性、恢复和稳定性。上游问题请提交到 [Farwing Issues](https://github.com/djkcyl/farwing/issues)。

## 下载与使用

Windows x64 客户端从上游 [Farwing Releases](https://github.com/djkcyl/farwing/releases) 下载，扫码或短信登录后连接设备，支持 UU 官方客户端；无需登录也可在登录页开启“接受远程协助”。

上游最新稳定版为 [v0.7.0](https://github.com/djkcyl/farwing/releases/tag/v0.7.0)，最新预发布为 [v1.0.0-rc.2](https://github.com/djkcyl/farwing/releases/tag/v1.0.0-rc.2)。Linux 预编译包在本仓库 [Releases](https://github.com/dytan/openuuyc/releases)；发行说明见 [v0.8.1](packaging/linux/RELEASE-NOTES.md)。

接收测试版更新提醒：在“关于”中开启“允许测试版”（默认关闭）。

**从 alpha.6 或更早版本升级需重新登录，设备 ID 会改变，旧设备条目需手动删除。**

## 能力表

以下对应 **v1.0.0-rc.2**，功能取决于双方能力与权限。Linux 被控的已实现范围以 [LINUX-HOST.md](LINUX-HOST.md) 为准，窄于下表的 Windows 被控端。

| 功能 | 主控端 | 被控端 | 说明 |
| --- | --- | --- | --- |
| 同账号远程连接 | 支持同时连接多台设备 | 支持 | 本机在连接设置中开启“允许被控” |
| 设备 ID / 验证码协助 | 支持主动连接 | 支持账号及游客协助 | 随机 / 自定义密码、本机确认；主控支持最近连接和收藏 |
| 画面传输 | 接收与播放 | 采集与编码 | H.264 / H.265 / AV1、真彩及 HDR，可调画质、码率和帧率；AV1 与 SDR 自动 10 位需双方 OpenUUYC 支持 |
| 编解码 | DXVA11 硬解 / Rust 软解 | NVENC、AMF、QSV 硬编 / Rust H.264 软编 | 可选格式与显卡首选；软解支持 H.264 / AV1，整个客户端最多一个软解播放窗口 |
| 键鼠与触摸 | 键盘、鼠标控制 | 键鼠及移动端原生触摸接收 | 相对/绝对鼠标、组合键、光标同步；可选 125 / 500 / 1000 Hz 鼠标节流（默认关闭） |
| 多显示器与显示设置 | 切屏、多窗口、分辨率及 DPI 调整 | 多屏采集与设置接收 | 屏幕标签可拖出独立窗口，显示设置按目标支持的配置应用 |
| 虚拟屏、超级屏与无屏接管 | 支持操作远端 | 支持 | 需安装显示驱动；自动兜底屏断线保留，其他可用屏幕接入后回收 |
| 桌面声音 | 播放与远程音质调整 | 采集与发送 | 默认或指定设备，设备变化后恢复；64 / 128 / 192 / 256 kbps，远程调音质需双方 alpha.6+ |
| 仅音频连接 | 支持 | 支持 | 默认 256 kbps，提供波形、电平、网络 RTT、精简模式，可恢复画面 |
| 麦克风与虚拟声卡 | 选择麦克风、调整音质并发送 | 虚拟扬声器及远程麦克风接收 | 被控端需选装虚拟音频驱动，可配置默认音频设备切换策略 |
| 剪贴板与文件拖放 | 支持 | 支持 | 文字、富文本、图片及文件复制；OpenUUYC 双端支持双向拖放，与官方端连接时使用其支持的发送方式 |
| 独立文件传输 | 支持 | 支持官方 Windows / Android / iOS 收发 | 目录浏览、收发、文件管理及暂停续传；被控端需登录 Windows |
| 端口转发 | 支持 | 支持 | TCP 与官方互通；UDP 需两端 OpenUUYC，复用可靠通道；被控需开启许可 |
| 批注与指示工具 | 支持发送 | 支持接收 | 画笔、形状、撤销重做、白板、激光笔和鼠标指示；被控端需登录 Windows |
| 远程电源操作 | 开机、关机和重启 | 关机、重启、远程开机设置与局域网唤醒协助 | 提供有线网卡检查/配置；WoL 需 BIOS 开启及同网协助设备或已配置的 UU 路由器 |

被控通知可选程序浮窗或 Windows 通知，支持连接确认、断开及文件收发提醒。

向官方被控端拖入文件，需开启键鼠控制、剪贴板同步和文件复制。

另支持设备管理、自定义快捷键、本机诊断、性能监控、异常图标及[插件和节点图](plugins/README.md)。

支持本机导出及远端获取脱敏诊断包（ZIP / zstd，最多 32 MiB）；远端获取需双方 beta.3+，并开启被控端文件访问权限。

## 安装与服务

主页“安装服务”会将程序安装到 `Program Files\OpenUUYC`，安装后台服务、输入和显示驱动，创建快捷方式并配置托盘自启动。安装需管理员授权及驱动证书信任；关闭主窗口进入托盘，托盘“退出”结束运行。

安装服务并开启被控后，支持锁屏、PIN 界面及重启后未登录 Windows 时接入。

更新时运行新版 EXE，点击“更新并打开”即可更新已有安装。虚拟声卡在“连接设置 → 服务管理”中选装；卸载可选择保留虚拟显示驱动、虚拟声卡及用户数据。

### 虚拟音频驱动的使用条件

**虚拟音频驱动使用测试签名，尚未取得 Microsoft 正式签名。** 使用虚拟扬声器或接收远程麦克风，需要手动准备测试启动环境；普通画面、键鼠和桌面声音不需要此设置。

- 临时测试：Shift + 重启 → 疑难解答 → 高级选项 → 启动设置 → 重启 → **7 / F7 禁用驱动程序强制签名**，仅本次启动有效。[Windows 说明](https://support.microsoft.com/en-us/windows/experience/startup-boot/windows-startup-settings)
- 持续测试：管理员执行 `bcdedit /set testsigning on` 后重启；恢复时执行 `bcdedit /set testsigning off` 并重启。Secure Boot、BitLocker 或组织策略可能限制操作。[微软说明](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/the-testsigning-boot-configuration-option)

测试启动会降低驱动加载保护，程序不会自动修改这些设置。

## 构建

需要 Rust stable（MSVC）、Visual Studio C++ 构建工具、Windows SDK、CMake 和 NASM，命令行工具需加入 PATH。H.264 软件编解码和 AV1 软件解码使用项目 Rust 核心及 SIMD 汇编。Linux 构建见 [LINUX-BUILD.md](LINUX-BUILD.md)。

上游 Windows 树：

```powershell
git clone https://github.com/djkcyl/farwing.git
cd farwing
cargo dist
```

产物为 `target/dist/` 下的单文件 EXE。仅构建原生 EXE 可用 `cargo dist --native`；命令行参数见 `--help`。

源码已包含签名驱动包和公钥证书，构建主程序不需要签名私钥。修改驱动及发布前验证见 [驱动构建说明](drivers/README.md)。

## 命令行更新

从新下载的程序包运行 `OpenUUYC.exe update --silent`，静默更新已有安装。会中断当前连接，完成后不自动打开控制中心或重启 Windows；非管理员运行时仍需确认系统 UAC。

无人值守脚本使用 `update --silent --no-elevate`，由安装所属用户的管理员终端执行；权限不足返回 740，不弹 UAC。退出码：0 成功、3010 需重启、170 已有安装操作、1605 未安装、2404 驱动占用，其他失败为 1。不要同时保留其他更新窗口。

## 反馈与许可

问题和建议请提交到上游 [Issues](https://github.com/djkcyl/farwing/issues)。报告问题时附上双方版本、操作步骤、大致发生时间及导出的诊断包，分享前检查是否含私人信息。

OpenUUYC 非网易官方项目。源码公开，但项目整体未采用开源许可证，使用与分发条件见 [LICENSE](LICENSE)。第三方及派生代码保留各自的许可，详见 [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES)。

## 鸣谢

感谢以下团队、项目和贡献者为 OpenUUYC 提供的参考、技术基础与开发支持：

- [网易 UU 远程](https://uuyc.163.com/)：本项目的协议兼容目标，也是远程连接功能与交互行为的参考。
- [SudoVDA](https://github.com/SudoMaker/SudoVDA) 与 [Microsoft Windows Driver Samples](https://github.com/microsoft/Windows-driver-samples)：虚拟显示驱动的源码基础，以及虚拟音频驱动的示例参考。
- [FFmpeg](https://ffmpeg.org/)：视频编解码基础算法及硬件解码适配的重要来源。
- [rav1d](https://github.com/memorysafety/rav1d) / dav1d：AV1 软件解码核心及 SIMD 实现。
- [Cisco OpenH264](licenses/openh264-algorithms.txt)：软件编码器的码率控制、屏幕变化检测与运动搜索等算法参考。
- [WebRTC](https://webrtc.googlesource.com/src/+/refs/branch-heads/5481/)、[WebRTC-rs](https://github.com/webrtc-rs/webrtc) 与 [goog_cc](https://github.com/kixelated/goog_cc)：实时传输、音频接收处理与拥塞控制基础。
- Opus / [SpeexDSP](src/media/audio/COPYING.SpeexDSP)：音频编解码与重采样算法。
- egui / [egui-directx11](https://github.com/NekomaruQwQ/egui-directx11)：图形界面与渲染基础。
- [OpenAI](https://openai.com/)：Codex 为本项目的开发提供 AI 编程辅助。
- [sisi0318](https://github.com/sisi0318)：提供两个 Codex x20 账号，支持本项目开发。
- ~~[Tibo（Thibault Sottiaux）](https://x.com/thsottiaux)：感谢他与 Codex 团队提供的额外额度重置。~~

也感谢[项目贡献者](https://github.com/djkcyl/farwing/graphs/contributors)及提供测试反馈和改进建议的用户。完整的第三方来源与许可说明见 [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES)。
