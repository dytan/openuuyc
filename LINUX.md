# OpenUUYC on Linux

Status for the native Linux controller (egui + winit). This is an upstream
collaboration track for [djkcyl/openuuyc](https://github.com/djkcyl/openuuyc),
not a fork product. Windows cfg paths stay intact.

## What works

| Area | Status |
|------|--------|
| GUI shell | **wgpu** presenter (Vulkan/GLES) with egui; glow path retired on this branch |
| Video decode | **VA-API H.264 + HEVC Main** (8-bit 4:2:0) when the driver exposes VLD; **Rust H.264 software** fallback. Main10 not advertised (NV12 readback only) |
| Video present | wgpu upload (RGBA or NV12 shader path); VA-API still readbacks to CPU NV12 (no dmabuf zero-copy yet) |
| Login / devices | QR + GUI login; Secret Service / gnome-keyring (or compatible) |
| 连接设置 | Codec / HW-decode / transport / **键鼠控制** (`auto_mouse_control`, default on) |
| Fonts | Noto Sans CJK SC via TTC **face index 2** (JP/KR/SC/TC/HK order) |
| Instance lock | flock under `$XDG_RUNTIME_DIR` (uid-tagged `/tmp` fallback) |
| Places | XDG user-dirs (`user-dirs.dirs`) for Desktop/Downloads/Documents |
| Clipboard | **arboard** text/image + **FUSE** file offer (`clipboard_files`) adapted from DoyoDia PR1 |

## Deferred / remaining

- **dmabuf zero-copy** from VA-API into wgpu (today: surface → packed NV12 → GPU upload)
- **HEVC Main10 / 4:4:4** readback (Main 8-bit NV12 is wired; Main10 needs a non-NV12 path)
- Broader Wayland compositor quirks soak
- Plugin host parity with Windows

## Soak flags

```bash
./target/debug/OpenUUYC gui \
  --codec h264 \
  --hardware-decode true \
  --transport auto \
  --auto-mouse-control true
```

`--hardware-decode false` forces software H.264. Default is **true** (VA-API first, software fallback).

## Milestones

| ID | Goal | Status |
|----|------|--------|
| M0 | Linux compile + GUI shell | **done** (wgpu) |
| M1 | Software H.264 view path | **done** |
| M2 | Portable PR1 adapts (mouse, fonts, XDG) | **done** |
| M3 | HW decode when available + software fallback | **done** (VA-API + pool fallback) |
| M4 | Clipboard text + file (FUSE) | **done** (arboard + FUSE) |
| M5 | Zero-copy VA↔wgpu / deeper soak | **next** |

## Docs

- [`LINUX-BUILD.md`](LINUX-BUILD.md) — distro build guides
- [`LINUX-SUMMARY.md`](LINUX-SUMMARY.md) — handoff / commit list
- [`PR1-COMPARE.md`](PR1-COMPARE.md) — OUR tree vs DoyoDia linux-port
- [`docs/linux-clipboard-fuse.md`](docs/linux-clipboard-fuse.md) — FUSE/clipboard audit + verify
- [`docs/linux-lock-video-freeze.md`](docs/linux-lock-video-freeze.md) — lock UI freeze postmortem (f0067ff)

## Auth note

Linux needs a D-Bus user session and Secret Service. Ensure
`DBUS_SESSION_BUS_ADDRESS` (often `unix:path=$XDG_RUNTIME_DIR/bus`) and an
unlocked keyring.
