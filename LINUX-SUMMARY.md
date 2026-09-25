# Linux controller — handoff summary

Date: 2026-09-25 (Asia/Shanghai). Host: Omarchy on XPS. **Do not push** until
the collab path is agreed.

## Commits ahead of `origin/main`

```
1b6da60 feat(linux): wgpu presenter, VA-API decode, arboard+FUSE clipboard
dbe4610 feat(linux): adopt portable PR1 bits (auto mouse, fonts, XDG locks)
856d9d3 fix(linux): load Noto Sans CJK SC for egui labels
5c9de57 docs(linux): continue-on-new-box handoff + commit bundle
7fe1e6e docs(linux): connect soak checklist; auth/GL harden
8c04fef feat(linux): egui-glow GUI shell and CPU RGBA viewer path
b12086a feat(linux): initial controller compile path for native Linux
```

## Portable PR1 adapts (done)

- `auto_mouse_control` + CLI + 连接设置 **键鼠控制** switch + stream_control auto take-over
- CJK Noto SC **face index 2** + merged font candidate paths
- `$XDG_RUNTIME_DIR` session flock
- `user-dirs.dirs` places for the file browser

## GPU path (done this pass)

- Vendored `cros-libva` + `decoder/platform/linux` VA-API (CPU NV12 readback)
- `DecoderCandidate::LinuxVaapi` with software H.264 fallback in the pool
- GUI shell swapped to **egui-wgpu** (Vulkan/GLES); glow/glutin Linux shell retired
- Default `--hardware-decode` is **true** on Linux; false still forces software

## Clipboard (done this pass)

- arboard text/image path
- FUSE file offer (`clipboard/fuse_linux.rs`) + X11 offer helper from DoyoDia PR1

## Still open

- VA-API → wgpu **dmabuf zero-copy** (current path copies NV12 to CPU then uploads)
- Longer connect soak on real devices
- Upstream PR shape (single stack vs stacked PRs)

## How to continue

```bash
cd /home/di/src/openuuyc
cargo build
pkill -x OpenUUYC || true
./target/debug/OpenUUYC gui --codec h264 --hardware-decode true --transport auto --auto-mouse-control true &
```

Next engineering slice: zero-copy presenter **or** shape the upstream PR without
rewriting Windows paths.
