# OpenUUYC Linux controller support (WIP)

Local milestone on native Linux (`x86_64-unknown-linux-gnu`). Identity remains **OpenUUYC** Linux support — not a competing fork brand.

## What works now

Verified on this tree with Rust **1.98+** (edition 2024 crates such as `egui` 0.36 / `mediaway-common` need a recent rustc; 1.85 is too old):

```bash
# toolchain: rustup (1.98+)
source "$HOME/.cargo/env"   # if needed
cargo check
cargo build

./target/debug/OpenUUYC native-status
./target/debug/OpenUUYC contracts
./target/debug/OpenUUYC rtc-selftest
./target/debug/OpenUUYC auth-status

# GUI control center (egui-winit + egui_glow / EGL)
# Needs a working DISPLAY (or Wayland) and runtime libs below.
./target/debug/OpenUUYC gui
```

Smoke-tested: `OpenUUYC gui` opens a decorated window titled **「OpenUUYC · 控制中心」** and keeps the event loop alive (device list / login may still be incomplete depending on auth state).

CLI paths that do not need a GUI shell compile and run: transport/status, REST contract listing, local WebRTC offer self-test, auth/keyring status, login/logout scaffolding (needs network + keyring backend at runtime).

App data lives under `$XDG_DATA_HOME/openuuyc` or `~/.local/share/openuuyc` (see `src/paths.rs`).

### Runtime packages (Debian/Ubuntu-ish)

```bash
sudo apt-get install -y \
  libegl1 libegl-dev \
  libxkbcommon0 libxkbcommon-x11-0 \
  libxcb-xkb1 \
  libwayland-client0
```

Missing `libxkbcommon-x11.so` causes an immediate panic from `xkbcommon-dl` when opening the GUI on X11.

## Graphical path (this milestone)

| Piece | Location | Notes |
|-------|----------|--------|
| Main UI event loop | `src/ui/linux.rs` | glutin-winit EGL + `egui_glow::Painter`; mirrors Windows `AppFactory` / `WindowConfig` / `window_manager` requests (`Open`, `Viewer`, Focus, Repaint) |
| Viewer / connecting | `src/viewer/linux/windows_presenter.rs` | Connecting progress UI + playing shell; pulls `RenderSurface::CpuRgba8` from the session frame queue into an egui texture |
| Input | same presenter | Absolute mouse + buttons/wheel + keyboard via existing `RemoteInput` / `stream_control` shapes (no Win32 hooks) |
| Decode | software H.264 | Already on Linux via `openuuyc-h264`; presenter does not use D3D11 surfaces |
| Windows | unchanged | Still `cfg(windows)` for D3D11 / Win32 UI |

Deps (Linux target): `egui_glow` (winit), `glow` 0.17, `glutin` / `glutin-winit` (egl, x11, wayland), `egui-winit`, `raw-window-handle`.

## Status by area

| Area | Status |
|------|--------|
| GUI device center | **Opens**; login/device refresh still depend on auth + network |
| Viewer connecting / play window | Wired; needs live connect soak to validate end-to-end |
| CPU RGBA / software H.264 present | Presenter path implemented; not soak-tested on a real session yet |
| Hardware decode | Windows DXVA only; Linux software for now |
| H.265 software | Not wired on Linux yet |
| Global input hooks | Windows hooks stubbed; in-window winit input is used for remote control |
| Clipboard native sync | OLE path stubbed; protocol retained |
| Plugins / DLL host | `src/plugins_linux.rs` stub |
| File-transfer Win32 bits | Portable FS path; places use XDG/home |
| NetEq | Built with `WEBRTC_POSIX` + `WEBRTC_LINUX` |

## Remaining blockers for connect soak

1. **Auth / device list in GUI** — complete login (keyring + network) so the control center can list and start a connect.
2. **End-to-end connect** — exercise `Request::Viewer` → connecting progress → soft-decode frames → `CpuRgba8` texture; confirm no GL/context sharing issues across multi-window.
3. **Input soak** — absolute mouse + keyboard under real remote session; relative mouse / modifier edge cases; IME.
4. **Audio** — `cpal` + NetEq on Linux hosts (mute first for video-only soak).
5. **H.265 / hardware decode** — strategy (software vs VA-API) undecided.
6. **Clipboard / plugins / file transfer polish** — stubs or partial ports.

## Suggested next milestone

1. GUI login → device list refresh with a real account.
2. Connect soak: muted audio, H.264 software, auto transport; confirm frames + input.
3. Harden multi-window GL (shared glow contexts / make_current) and document Wayland vs X11 quirks.
4. Optional: VA-API or better software H.265 path.

## Build notes

- `build.rs` allows `windows` and `linux`; embeds Windows resources only on Windows; NetEq uses POSIX/Linux defines on Linux (C++ exceptions enabled for `bad_alloc`).
- Do not strip `LICENSE` / `THIRD_PARTY_NOTICES`.
- Upstream contribution should stay under OpenUUYC naming.
- Do not push unless asked; keep Windows `cfg` paths intact.
