# Linux controlled side (被控 / host) — MVP

Historically the Linux port was controller-only: it could view and drive a
remote Windows/Mac machine, but could not itself be controlled. This branch adds
a minimum viable **host** so a Linux box can be driven from a Windows or Mac
controller.

## What works

- **Register as controllable.** When you turn on *允许被控* the client calls the
  account `set_controllable(true)` API and keeps room presence as a host, exactly
  like Windows. (The device is registered with the generic platform id; see the
  caveat below.)
- **Screen capture.**
  - Wayland: `zwlr_screencopy_v1` into shared memory. Works on wlroots-family
    compositors (Hyprland, Sway, river, labwc, Wayfire).
  - X11: `GetImage` on the root window.
  - GNOME/KDE Wayland are **not** supported yet (they need the ScreenCast
    portal; see below).
  - A DPMS-blanked panel (idle laptop with no local input) is woken on connect
    via compositor IPC on Hyprland, so a remote session never stalls on a black
    screen. Best-effort elsewhere.
- **Encode.**
  - **VA-API hardware H.264** (Intel/AMD/anything with a working libva driver),
    selected automatically after an encode→decode round-trip validates on the
    machine. Low-latency rate control fills the negotiated bitrate.
  - The uploaded picture is never the reconstructed `CurrPic`. AMD VCN 5
    (RDNA4-class, radeonsi) replaces that surface's buffer and frees it when
    the picture is unreferenced; encoding into the source crashes the probe.
  - Each IDR is submitted with this encoder's SPS/PPS as a packed header.
    radeonsi parses that header into its slices but does not write it unless a
    packed slice header is also sent, so an IDR that comes back without an SPS
    is prefixed with the same bytes. Drivers that already emit the sets are
    left as they are.
  - **Software H.264** (the in-tree Rust `openuuyc-codec`, BGRA → Annex-B) is the
    automatic fallback and is always offered, with live bitrate/FPS control,
    keyframe requests and the same quality/scaling policy the Windows path uses.
  - Override with `OPENUUYC_HOST_ENCODER=software|vaapi` (default: auto).
- **Separate cursor.** On Hyprland the pointer is published on the lightweight
  cursor channel (screencopy `overlay_cursor=0`), like the Windows host, so mouse
  movement does not cost a video frame. Falls back to a composited cursor
  elsewhere.
- **Input injection.** A virtual mouse+keyboard through `uinput`
  (absolute pointer over the full layout, relative motion, buttons, wheel with
  hi-res, keys, and US-ASCII text). Needs permission to `/dev/uinput`.
- **Clipboard sync (text, images, files).** Bidirectional between the Linux host
  and the controller, using the Windows clipboard dialect so Windows **and** macOS
  controllers interoperate. Text and images paste both ways. A file copied on
  the Linux host is advertised as `FileGroupDescriptorW` + `FileContents` (no
  extra shell formats). The descriptor blob keeps Windows FILETIME ticks; the
  protobuf file list sends whole Unix seconds, which is what official
  controllers read. Answering the blob also pins clipboard task 0 so a
  contents read can follow without `FileDescListRequest`. Backed by
  `wl-clipboard` / `arboard` on the host. Confirm with the retest below; this
  has not been pasted against a live Windows controller in CI.
- **Host-side file receive.** Files sent from the controller land in `~/Downloads`
  (name collisions are resolved) via the file-transfer **Send** path.
- **Desktop audio.** Captured from the default sink monitor through `parec`
  (PulseAudio / PipeWire-pulse).
- **Zero-copy GPU capture (experimental, opt-in).** `OPENUUYC_ZEROCOPY=1` enables
  dmabuf `zwlr_screencopy` → VA surface import → VPP scale/CSC → VA-API encode,
  with automatic fallback to shm+VPP and then CPU. **Off by default.**

## Not done yet (gracefully degraded)

- **Viewer drag-drop onto the video window** (controller → Linux host) does not
  write files yet — the OleDrop path is not wired. Use the file-transfer **Send**
  button instead; that works.
- Virtual / headless displays, resolution & DPI changes, multi-monitor topology
  journaling (physical outputs only).
- Remote microphone playback into the host (accepted on the wire, dropped).
- Non-ASCII / IME text injection (evdev is layout-unaware).
- Windows system chords (Win+D, Task View, Ctrl+Alt+Del on the host).
- GNOME/KDE Wayland capture (portal ScreenCast).

## Required setup for input injection

`uinput` needs write access to `/dev/uinput`. Pick one:

Load the module (if `/dev/uinput` is missing):

    sudo modprobe uinput
    echo uinput | sudo tee /etc/modules-load.d/uinput.conf

Grant your user access with a udev rule (preferred):

    echo 'KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"' \
      | sudo tee /etc/udev/rules.d/99-openuuyc-uinput.rules
    sudo usermod -aG input "$USER"
    sudo udevadm control --reload-rules && sudo udevadm trigger

Then log out and back in (group change) and confirm:

    ls -l /dev/uinput          # should be group 'input', mode 0660
    id | tr ',' '\n' | grep input

Screen capture and audio need no root.

## Self-test (no peer required)

    OpenUUYC host-selftest

Enumerates outputs, captures a frame, encodes → decodes it, and probes
`/dev/uinput`, printing a short report. Use it to confirm the local half before
asking a controller to connect.

## Connection test plan (needs a second device)

1. On the Linux box: log in, open *连接设置* and turn on *允许被控*. The status
   should move to “等待连接”.
2. From a Windows or Mac controller signed into the same account, connect to the
   Linux device.
3. Check, in order: video appears; mouse moves and clicks land where expected;
   keyboard types; scroll works; desktop audio is heard (if enabled).
4. **File paste, Linux → controller.** On the Hyprland host, copy one small file
   in the file manager (not a screenshot). On the Windows controller, paste into
   Explorer. The file should appear with its name, bytes, and a modification
   time near the original (protobuf mtimes are whole Unix seconds; the OLE
   descriptor still carries FILETIME ticks). If paste does nothing, the host log
   should show `已登记剪贴板文件任务并回传描述符块` and then a contents read on
   task 0 — a `文件任务已失效` line means the controller asked for a different
   task id. Repeat the paste on a Mac controller if one is connected (same
   Windows clipboard dialect).
5. **Reverse smoke.** Copy a small file on the controller and confirm it still
   arrives on the host (`~/Downloads` via Send, or a clipboard file offer).
   Then copy text and an image each way so the sticky-image fix still holds:
   an image followed by text must paste as text.
6. Toggle *允许被控* off and confirm the controller is dropped.

## Caveat: device platform id

The device registers with the generic (Windows) platform id, so the server lists
it like a Windows host. This is what currently lets the server mark it
controllable; whether the server fully honours a non-Windows host, and whether a
controller interoperates with this H.264 stream and input, can only be confirmed
by the connection test above.

## v0.8.1

Functional host features in this document are unchanged from v0.8.0 aside from
one clipboard fix. Release notes:
[`packaging/linux/RELEASE-NOTES.md`](packaging/linux/RELEASE-NOTES.md).

- **Sticky-image clipboard** (`0b9cfad`). After an image sync, a later text copy
  no longer pastes as the old image. Remote clipboard accepts are sequenced, and
  a new text copy is advertised as text only.
- **Linux → controller file paste** (after v0.8.1). Host file offers match the
  Windows pair (`FileGroupDescriptorW` / `FileContents`). The binary descriptor
  keeps FILETIME; protobuf `last_write_time` is Unix seconds. The descriptor
  answer pins task 0 so contents can be read without a descriptor-list request.
  Retest: copy a file on the Hyprland host and paste on a Windows controller;
  then copy a small file on the controller and confirm it still lands in
  `~/Downloads` or via FUSE. Text and image both ways after a file copy.
- **Docs.** Upstream is now [Farwing](https://github.com/djkcyl/farwing)
  (`djkcyl/openuuyc` resolves there). This branch's binary and UI stay
  OpenUUYC. [`LINUX.md`](LINUX.md) matches host mode.

## v0.8.0 review (Linux host milestone)

This release turns the minimum-viable host into a usable one: hardware encode,
separate cursor, full clipboard, file receive, and robustness fixes for idle
laptops and the zero-copy path.

### Highlights since v0.7.0
- VA-API hardware H.264 encode with automatic software fallback.
- Separate cursor channel on Hyprland (mouse moves no longer cost a frame).
- Bidirectional clipboard: text, images, and files (Windows dialect, so Windows
  and macOS controllers both work).
- Host-side file receive into `~/Downloads`.
- Wake a DPMS-blanked panel on connect so a remote session never stalls black.
- Experimental zero-copy dmabuf → VA-API path (`OPENUUYC_ZEROCOPY=1`, off by
  default) with layered fallback (dmabuf → shm+VPP → CPU).

### Known gaps / risks
- **Zero-copy is experimental and off by default.** It has fallback for static
  screens and VPP/dmabuf errors, but has not had wide soak time; keep it off for
  routine use and opt in only to test. Leave it off if you see quality dips or
  stalls.
- Viewer drag-drop onto the video window does not write files yet (use **Send**).
- Linux → controller file clipboard is partial (text/images reliable).
- GNOME/KDE Wayland capture still needs the ScreenCast portal.
- The device still registers with the generic (Windows) platform id; see the
  caveat below.

### Recommended defaults
- `OPENUUYC_ZEROCOPY` **unset / 0** (software or VA-API shm path, which is the
  stable default).
- `OPENUUYC_HOST_ENCODER` **unset** (auto: VA-API when it validates, else
  software).

### Versioning
The Linux port keeps its own release line (`v0.8.1` current; `v0.8.0` is the
host milestone). The underlying app engine is upstream **1.0.0-rc.2**, which is
what the app reports as its version string. `Cargo.toml` stays at that engine
version. `v0.8.1` is the Linux packaging tag, not the engine version. The
upstream project is now **Farwing**
([djkcyl/farwing](https://github.com/djkcyl/farwing)).
