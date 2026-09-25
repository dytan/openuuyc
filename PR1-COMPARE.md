# OpenUUYC Linux ports: OUR tree vs DoyoDia PR1 (linux-port)

Date: 2026-09-25 (Asia/Shanghai), post wgpu/VA-API/clipboard integration.

OUR tip: local `main` on `/home/di/src/openuuyc` (see `git log origin/main..HEAD`).
PR1 tip: `eb51212` at `/home/di/src/openuuyc-pr1`.

## Architecture (current)

| Area | OUR (now) | PR1 |
|------|-----------|-----|
| GUI shell | **egui-wgpu** + winit (Vulkan/GLES) | egui-wgpu + winit |
| Video present | wgpu (`ui/wgpu_video.rs`); NV12 or RGBA | same family |
| Decode | VA-API via vendored `cros-libva` + software H.264 fallback | same |
| Clipboard | arboard + FUSE file offer + X11 offer + `clipboard_files` connect default | same |
| Instance lock | `$XDG_RUNTIME_DIR` flock | `$XDG_RUNTIME_DIR` flock |
| Fonts | CJK candidates **with TTC face index 2** for SC | broader paths, **no** face index |

## What we kept that PR1 lacked

- Correct Noto CJK **SC face index**
- Explicit cfg-split Windows/Linux modules where we already had them
- Auth/GL soak notes and handoff docs on this branch
- `cros-libva` patch for newer libva headers (`seg_id_block_size` / `va_reserved8`)

## What PR1 still suggests next

- dmabuf zero-copy (both trees still read back VA surfaces to CPU NV12)
- Relative-pointer denial polish / richer virtual keys (partially present)
- Plugin host depth

## Merge risk notes

Windows paths and D3D11 presenter remain the Windows cfg path. Linux no longer
uses egui-glow; do not reintroduce glutin without a feature flag.
