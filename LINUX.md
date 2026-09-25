# OpenUUYC Linux controller support (WIP)

Local milestone on native Linux (`x86_64-unknown-linux-gnu`). Identity remains **OpenUUYC** Linux support — not a competing fork brand.

## What works now

Verified on this tree with Rust **1.98+** (edition 2024 crates such as `egui` 0.36 / `mediaway-common` need a recent rustc; 1.85 is too old):

```bash
cargo check
cargo build
./target/debug/OpenUUYC native-status
./target/debug/OpenUUYC contracts
./target/debug/OpenUUYC rtc-selftest
./target/debug/OpenUUYC auth-status
```

CLI paths that do not need a GUI shell compile and run: transport/status, REST contract listing, local WebRTC offer self-test, auth/keyring status, login/logout scaffolding (needs network + keyring backend at runtime).

App data lives under `$XDG_DATA_HOME/openuuyc` or `~/.local/share/openuuyc` (see `src/paths.rs`).

## Intentionally stubbed / incomplete

| Area | Status |
|------|--------|
| GUI device center / viewer | `src/ui/linux.rs` returns TODO; no egui-winit+glow/wgpu shell yet |
| Video present (D3D11 path) | Windows-only; Linux stub presenter |
| Hardware decode | Windows DXVA11 only; Linux uses Rust H.264 software (`openuuyc-h264`) |
| H.265 software | Not wired on Linux yet |
| Input capture (global hooks) | Windows hooks stubbed |
| Clipboard native sync | OLE path stubbed; protocol retained |
| Plugins / DLL host | `src/plugins_linux.rs` stub |
| File-transfer Win32 bits | Portable FS path; places use XDG/home |
| NetEq | Built with `WEBRTC_POSIX` + `WEBRTC_LINUX` |

## Remaining blockers for a usable connect session (severity)

1. **Critical — Viewer / windowing:** egui + winit + OpenGL/Vulkan presenter to show decoded frames and host the device center (`ui::run`, `viewer` presenter).
2. **Critical — Input:** keyboard/mouse capture and remote injection path without Win32 hooks (`viewer` input modules).
3. **High — Video decode display path:** software H.264 → CPU RGBA (or VA-API later) into the Linux presenter; H.265 strategy undecided.
4. **High — Signaling + connect orchestration:** mostly portable already (`signal`, `rtc`, `controller`); blocked on viewer lifecycle / progress UI.
5. **Medium — Login UX:** CLI login should work with keyring; GUI QR/SMS still needs UI shell.
6. **Medium — Audio:** `cpal` + NetEq likely close; needs end-to-end session soak on Linux hosts.
7. **Lower — Clipboard / plugins / file transfer polish:** stubs or partial ports.

## Suggested next milestone

1. Implement `src/ui/linux.rs` with **egui-winit + egui_glow** (deps already sketched in `Cargo.toml`) enough to run the existing device-center `App` and a connecting/progress window.
2. Present software-decoded **CPU RGBA** frames in that shell (skip zero-copy GPU initially).
3. Wire **winit** keyboard/mouse events into the existing remote-input protocol (no global hooks required for first connect).
4. Soak-test: `login` → `devices` → `connect` with muted audio, H.264 software, auto transport.

## Build notes

- `build.rs` allows `windows` and `linux`; embeds Windows resources only on Windows; NetEq uses POSIX/Linux defines on Linux (C++ exceptions enabled for `bad_alloc`).
- Do not strip `LICENSE` / `THIRD_PARTY_NOTICES`.
- Upstream contribution should stay under OpenUUYC naming.
