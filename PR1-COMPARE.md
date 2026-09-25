# OpenUUYC Linux ports: OUR tree vs PR1 (DoyoDia linux-port)

Date: 2026-09-25 (Asia/Shanghai).  
OUR tip (pre-adapt): `856d9d3` (`/home/di/src/openuuyc`, egui-glow + software H.264).  
PR1 tip: `eb51212` (`/home/di/src/openuuyc-pr1`, wgpu + VA-API).

This note shapes upstream collaboration. It is not a fork brand statement.

## Architecture

| Area | OUR | PR1 |
|------|-----|-----|
| GUI shell | `egui_glow` + glutin/EGL (X11/Wayland) | `egui-wgpu` + wgpu (Vulkan/GLES) |
| Video present | CPU RGBA upload into glow textures | wgpu video path (`ui/wgpu_video.rs`) with RGBA or NV12 planes |
| Decode | Software H.264 (`openuuyc-h264` / oxideav); HW decode default **off** on Linux | VA-API via vendored `cros-libva` + software fallback; HW decode default **on** |
| Clipboard | Stub / TODO (`native_linux.rs` ~40 LOC) | `arboard` + FUSE file offer + X11 file offer |
| Plugins | Linux stub (`plugins_linux.rs`) | Closer to Windows plugin host wiring |
| Instance lock | flock under XDG data dir | flock under `$XDG_RUNTIME_DIR` (uid-tagged `/tmp` fallback) |
| Fonts | CJK candidates with **TTC face index** (Noto SC = 2) | Broader path list, **no face index** (SC can become JP) |

## Decode

- **OUR:** intentional CPU path while glow stack is the Linux UI. `--hardware-decode false` is the supported soak default.
- **PR1:** real VA-API surfaces, NV12 readback into wgpu. Substantial new code under `decoder/platform/linux/vaapi/` plus `vendor/cros-libva`.
- **Merge risk:** swapping stacks means replacing presenter, decoder pool surface types, and Linux Cargo deps in one go.

## Clipboard

- **PR1 has** text/image via arboard, clipboard file copy gated by `clipboard_files`, FUSE mount under runtime dir, X11 offer helper.
- **OUR lacks** all of that on Linux (explicit TODO). Porting files/clipboard is a separate project (fuser + arboard + Wayland/X11 policy), not a drive-by.

## Fonts

- **OUR advantage:** `FontData.index = 2` for `NotoSansCJK-Regular.ttc` (SC). Without it, Simplified Chinese UI often renders with Japanese forms.
- **PR1 advantage:** extra candidate paths (Serif CJK, `truetype/noto`, Source Han, Arphic uming, `truetype/wqy`).
- **Portable take:** merge path lists; **keep face index**.

## Paths / flock / XDG

- Both use XDG data for app state; APIs differ (`app_data_dir` vs `local_app_data`).
- PR1 session locks on `$XDG_RUNTIME_DIR` are better (cleared on logout; no stale data-dir locks).
- PR1 `user-dirs.dirs` parsing for Desktop/Downloads/Documents is better than hard-coded `~/Desktop` style places.

## Input / settings

- **PR1:** `auto_mouse_control` (default on) + CLI + center-panel switch + `stream_control` auto take-over with decline tracking; also `clipboard_files`; relative-pointer denial tracking; richer shortcuts/`virtual_keys`.
- **OUR:** manual 键鼠 control only; Linux shortcuts split file; glow mouse path without PR1 relative-denied flag.

## What PR1 has that we lack (high level)

- wgpu presenter + NV12 path
- VA-API / cros-libva
- Working Linux clipboard (incl. files)
- `auto_mouse_control` / `clipboard_files` settings
- `$XDG_RUNTIME_DIR` locks; xdg-user-dirs places
- chrome_native_linux, virtual_keys consolidation, more complete plugins on Linux

## What we have that PR1 lacks

- Working egui-glow Linux GUI already in use on om-xps
- Correct Noto CJK **SC face index**
- Linux-default software decode (honest until HW exists)
- Auth/GL soak hardening notes and handoff docs on this branch
- cfg-split Windows/Linux modules kept explicit (`*_linux.rs` / `*_windows.rs`)

## Risks of a wholesale merge

1. **Presenter rewrite** (glow ↔ wgpu) touches viewer, UI shell, and frame hand-off.
2. **VA-API + vendored cros-libva** adds native build deps and failure modes we are not soaking yet.
3. **Clipboard/FUSE** needs package deps (`fuser`, fuse group perms) and session semantics.
4. **Default HW decode on** would break our current “software only” Linux promise.
5. Large conflict surface vs our local commits ahead of `origin/main`.

## This pass (portable only)

Adapted into OUR tree without ripping glow/VA-API:

- CJK candidate path merge (keep SC index 2)
- `auto_mouse_control` setting + CLI + stream_control wiring
- Session flock under `$XDG_RUNTIME_DIR`
- `linux_place` reads `user-dirs.dirs`

Deferred: wgpu, VA-API/cros-libva, clipboard_files/FUSE/arboard, relative_denied pointer grab.
