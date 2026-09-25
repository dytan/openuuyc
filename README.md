# OpenUUYC

![OpenUUYC](assets/banner.png)

OpenUUYC 是用 Rust 编写的 UU 远程第三方客户端。使用已有的 UU 账号登录，连接和控制远端设备。

This repository ([dytan/openuuyc](https://github.com/dytan/openuuyc)) is a collaboration fork that adds a **native Linux controller** (viewer/controller only — not a Linux host/agent). Windows support from upstream is unchanged.

## 下载与使用 / Downloads

- **Windows**: download the x64 client from upstream [Releases](https://github.com/djkcyl/openuuyc/releases), run it, sign in (QR or SMS), then connect. The remote device must be running UU Remote.
- **Linux**: build from source (see [Linux controller](#linux-controller-this-fork) below). There is no packaged Linux release yet.

## 功能 / Features (upstream)

- **设备管理**：设备列表与详情、修改别名、远程开关机和重启；支持通过设备 ID 连接，以及最近连接和收藏。
- **远程控制**：键鼠操作、多种鼠标模式、自定义快捷键和设备快速切换。
- **麦克风**：将本机麦克风发送到远端 Windows 设备。
- **文件传输**：双栏浏览、文件和文件夹双向传输。
- **剪贴板同步**：文字、图片及文件复制粘贴。
- **端口转发**、**多显示器**、**画质设置**、**批注**、**插件**：见上游说明；具体以所用版本为准。

目前支持 **Windows x64** 与 **Linux x64 控制器**；暂不支持本机被控（Linux host）。具体功能以所用版本的 Release / 本分支文档为准。

## Linux controller (this fork)

Native Linux GUI controller built on egui + winit. Highlights that exist on this branch:

| Area | Status |
|------|--------|
| GUI shell | **wgpu** present (Vulkan / GLES) |
| Video decode | **VA-API H.264** and **VA-API H.265/HEVC Main** (8-bit 4:2:0 NV12) when the driver exposes VLD; **Rust H.264 software** fallback. Main10 / 4:4:4 are not advertised (no NV12-compatible readback yet). If HEVC VA-API is unavailable, capability negotiation falls back toward H.264 |
| Fonts / UI | Noto Sans CJK SC (TTC face index 2) and related candidates |
| Connect options | `gui` flags for codec, hardware decode, transport, auto mouse control, clipboard files |
| Clipboard | **arboard** text/image + **FUSE** file offer (`clipboard_files`) |
| Lock / SecureDesktop | SecureDesktop-aware HID + CAD path; decoder **unpark after present** so lock UI stays live |
| Auth / paths | Secret Service (keyring); XDG data under `~/.local/share/openuuyc` (or `$XDG_DATA_HOME/openuuyc`); session flock under `$XDG_RUNTIME_DIR` |

Deeper status and postmortems (do not duplicate here):

- [`LINUX.md`](LINUX.md) — status overview
- [`LINUX-BUILD.md`](LINUX-BUILD.md) — distro package lists and build notes
- [`docs/linux-clipboard-fuse.md`](docs/linux-clipboard-fuse.md) — FUSE / clipboard verify
- [`docs/linux-lock-video-freeze.md`](docs/linux-lock-video-freeze.md) — lock-screen freeze fix

### Build (Linux)

Needs Rust stable and a C/C++ toolchain, plus X11/Wayland, Vulkan/Mesa, **libva**, **fuse3**, and CJK fonts. Distro-specific package lists: [`LINUX-BUILD.md`](LINUX-BUILD.md).

```bash
git clone https://github.com/dytan/openuuyc.git
cd openuuyc
cargo build                 # debug → target/debug/OpenUUYC
cargo build --release       # release → target/release/OpenUUYC
```

### Run (Linux)

```bash
./target/debug/OpenUUYC gui \
  --codec h264 \
  --hardware-decode true \
  --transport auto \
  --auto-mouse-control true

# Force software H.264:
./target/debug/OpenUUYC gui --codec h264 --hardware-decode false

# Enable clipboard file copy by default (also available in the player menu):
./target/debug/OpenUUYC gui --clipboard-files true

./target/debug/OpenUUYC gui --help
```

Logs (rotated): `~/.local/share/openuuyc/logs/` (or `$XDG_DATA_HOME/openuuyc/logs/`). Override with `--log-file`.

Linux needs a D-Bus user session and an unlocked Secret Service (e.g. gnome-keyring / KWallet). Ensure `DBUS_SESSION_BUS_ADDRESS` is set in the session you launch from.

### Windows build (upstream)

Needs Rust stable (MSVC), Visual Studio C++ build tools, Windows SDK, CMake, and UPX on `PATH`:

```powershell
git clone https://github.com/djkcyl/openuuyc.git
cd openuuyc
cargo dist
```

Binaries land under `target/dist/upx/`. See `--help` on the binary for CLI usage.

## Credits / 致谢

- Original project: [djkcyl/openuuyc](https://github.com/djkcyl/openuuyc)
- Linux port foundation: [DoyoDia](https://github.com/DoyoDia) — [PR #1 (linux-port)](https://github.com/djkcyl/openuuyc/pull/1) on upstream (`feat: Linux 客户端移植与 VA-API 硬件解码`)

This fork continues that Linux controller work (wgpu present, VA-API + software fallback, clipboard FUSE parity, SecureDesktop / decoder-unpark fixes, docs). Windows `cfg` paths are intentionally left intact.

## 反馈与许可 / Feedback and license

Issues and suggestions: upstream [Issues](https://github.com/djkcyl/openuuyc/issues) (or this fork’s Issues for Linux-controller work). When reporting bugs, include version, steps, and relevant logs — strip account credentials, verification codes, and other private data.

**OpenUUYC is not a NetEase official project.** Source is published, but **the project as a whole is not released under an open-source license** (not MIT/Apache/GPL/AGPL). Per [LICENSE](LICENSE):

- Rights in original project material that the rightsholders may license are **reserved** except where law, existing licenses, written authorization, or LICENSE §4 apply. Visibility of the source alone does **not** grant permission to use, copy, modify, distribute, sublicense, sell, or commercially exploit that original material.
- **Third-party** code and derivatives remain under **their own** licenses (see [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES)); those terms take precedence in their scope. The tree includes FFmpeg-derived Rust code under **LGPL-2.1-or-later**, with LICENSE §4 stating the LGPL §6-style modification / reverse-engineering / relink allowances for combined programs.
- This notice grants **no** right to use NetEase / UU Remote services, bypass auth, or control a device without authorization from the person entitled to control it.

Forks and redistributors must read [LICENSE](LICENSE) and fulfill third-party obligations themselves. Do not assume rights we do not have.
