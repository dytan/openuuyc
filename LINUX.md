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
./target/debug/OpenUUYC gui --codec h264 --hardware-decode false
```

Smoke-tested: `OpenUUYC gui` opens a decorated window titled **「OpenUUYC · 控制中心」** and keeps the event loop alive (device list / login may still be incomplete depending on auth state).

CLI paths that do not need a GUI shell compile and run: transport/status, REST contract listing, local WebRTC offer self-test, auth/keyring status, login/logout scaffolding (needs **Secret Service + network + UU phone QR** at runtime).

App data lives under `$XDG_DATA_HOME/openuuyc` or `~/.local/share/openuuyc` (see `src/paths.rs`).

### Runtime packages (Debian/Ubuntu-ish)

```bash
sudo apt-get install -y \
  libegl1 libegl-dev \
  libxkbcommon0 libxkbcommon-x11-0 \
  libxcb-xkb1 \
  libwayland-client0 \
  gnome-keyring libsecret-1-0
```

Missing `libxkbcommon-x11.so` causes an immediate panic from `xkbcommon-dl` when opening the GUI on X11.

**Credential store:** login/session use Linux **Secret Service** via D-Bus (`keyring` → `zbus-secret-service`). There is **no plaintext fallback**. You need:

- A user D-Bus session (`echo "$DBUS_SESSION_BUS_ADDRESS"` should be set; often `unix:path=$XDG_RUNTIME_DIR/bus`)
- An unlocked keyring provider (e.g. `gnome-keyring-daemon`)

Without those, `auth-status` / `login` / `devices` / `connect` fail with `native secure credential store is unavailable` and a Linux-specific hint.

## Connect soak checklist

Prep for a real UU account on a Linux desktop (this CI/box run cannot supply credentials).

### Prereqs

1. Rust **1.98+**, `cargo build` succeeds.
2. Display: `DISPLAY` (X11) or Wayland; packages above installed.
3. Secret Service unlocked (see above). Confirm:

```bash
./target/debug/OpenUUYC auth-status
# expect:
#   platform credential store: available
#   saved login session: absent|present
```

4. Network reachability to UU/NRD endpoints (same as Windows client).
5. UU mobile app available to scan the CLI/GUI QR code.

### Recommended soak defaults (Linux)

| Setting | Flag | Why |
|---------|------|-----|
| Mute local audio | `--mute` (CLI connect) | Isolate video/input first |
| Codec | `--codec h264` | Software H.264 only on Linux today |
| Hardware decode | `--hardware-decode false` | **Linux CLI/GUI default is already `false`** |
| Transport | `--transport auto` | Default |

### Exact commands

```bash
# 1) Credential / session probe (headless OK if Secret Service works)
./target/debug/OpenUUYC auth-status

# 2) Interactive QR login (prints Unicode QR in the terminal; scan with UU phone)
./target/debug/OpenUUYC login
# no extra flags; restores a saved session if still valid, else QR flow

# 3) List devices (script-friendly text; requires saved session)
./target/debug/OpenUUYC devices

# 4a) Headless-ish connect (still opens viewer window — needs DISPLAY)
# DEVICE = exact full device name from `devices` (must be unique)
./target/debug/OpenUUYC connect "$DEVICE" \
  --mute --codec h264 --hardware-decode false --transport auto
# optional disambiguation:
#   --device-id <verified-id>

# 4b) GUI path (same media knobs; login/connect from control center UI)
./target/debug/OpenUUYC gui --codec h264 --hardware-decode false --transport auto

# 5) Tear down local session only (keeps virtual device identity in keyring)
./target/debug/OpenUUYC logout-local
```

### Expected windows / behavior

| Step | Expect |
|------|--------|
| `gui` | Window **「OpenUUYC · 控制中心」**; login/device UI once session exists |
| `connect …` | Connecting/progress window, then playback titled with viewer prefix + device alias |
| Video | Soft-decoded **CPU RGBA** frames in the Linux presenter (not D3D11) |
| Input | In-window mouse/keyboard → existing `RemoteInput` path |
| Ctrl+C on `connect` | Ordered leave / window close |

### What this environment proved vs what still needs a real UU account

| Ready without account | Needs real UU account + display + Secret Service |
|----------------------|--------------------------------------------------|
| `cargo build` | `login` QR confirm → session in keyring |
| `native-status`, `contracts`, `rtc-selftest` | `devices` listing bound hosts |
| `gui` opens empty/login shell | End-to-end `connect` / GUI connect soak |
| CLI help + soak flags documented | Audio unmute soak; H.265; P2P vs relay matrix |
| Clearer auth errors when D-Bus/keyring missing | Multi-window GL under live reconnect |

## Graphical path

| Piece | Location | Notes |
|-------|----------|--------|
| Main UI event loop | `src/ui/linux.rs` | glutin-winit EGL + `egui_glow::Painter`; `make_current` before paint/swap/destroy |
| Viewer / connecting | `src/viewer/linux/windows_presenter.rs` | Connecting + play; `CpuRgba8` → egui texture; winit → `RemoteInput` |
| Decode | software H.264 | `openuuyc-h264`; no D3D11 surfaces |
| Windows | unchanged | `cfg(windows)` D3D11 / Win32 UI |

## Status by area

| Area | Status |
|------|--------|
| GUI device center | **Opens**; login/device refresh need auth + network |
| Viewer connecting / play window | Wired; needs live connect soak |
| CPU RGBA / software H.264 present | Presenter path implemented; not soak-tested live |
| Credential store (Linux) | Secret Service; improved unavailable hints |
| Hardware decode | Windows DXVA; Linux software (CLI default off) |
| H.265 software | Not wired on Linux yet |
| Global input hooks | Stubbed; in-window winit used for remote control |
| Clipboard / plugins | Stubbed / partial |

## Remaining blockers after a successful login

1. End-to-end connect → frames + input under real session.
2. Audio (`cpal` + NetEq); mute first.
3. H.265 / VA-API strategy.
4. Clipboard / plugins / file-transfer polish.

## Build notes

- `build.rs` allows `windows` and `linux`; NetEq uses POSIX/Linux defines on Linux.
- Do not strip `LICENSE` / `THIRD_PARTY_NOTICES`.
- Do not push unless asked; keep Windows `cfg` paths intact.
