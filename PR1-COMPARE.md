# OpenUUYC Linux ports: OUR tree vs DoyoDia PR1 (linux-port)

Date: 2026-09-25 (Asia/Shanghai), post wgpu/VA-API/clipboard integration.

OUR tip: local `main` on your Linux checkout (see `git log origin/main..HEAD`).
PR1 tip: `eb51212` in a sibling checkout of the DoyoDia linux-port branch.

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

## Adopted from farwing beta.5 / linux-port (selective)

- Linux player title bar: clipboard sync toggle, wired to the existing
  `set_clipboard_enabled` path (same device preference as 串流设置). Not a
  rebase of the linux-port tree. Windows caption in this fork is unchanged
  (stream settings only). Fullscreen still hides the whole caption.

## What PR1 still suggests next

- dmabuf zero-copy (both trees still read back VA surfaces to CPU NV12)
- Relative-pointer denial polish / richer virtual keys (partially present)
- Plugin host depth

## Merge risk notes

Windows paths and D3D11 presenter remain the Windows cfg path. Linux no longer
uses egui-glow; do not reintroduce glutin without a feature flag.
