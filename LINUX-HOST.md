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
  - Wayland, wlroots-family (Hyprland, Sway, river, labwc, Wayfire):
    `zwlr_screencopy_v1` into shared memory. This stays the default whenever
    the compositor offers the protocol. The portal is not opened.
  - Wayland without that protocol (GNOME, KDE): XDG ScreenCast portal, frames
    read from PipeWire shared memory. KDE buffers that arrive as an unmapped
    MemFd are mapped for the copy. DMA-BUF is not imported; negotiation asks
    only for MemPtr/MemFd.
  - X11: `GetImage` on the root window. Unchanged. The portal is not an
    automatic fallback on Xorg.
  - Override with `OPENUUYC_CAPTURE` (comma-separated, first usable wins):
    `wlr`, `portal`, `x11`. Examples: `portal` forces the portal even on
    Hyprland; `wlr,portal` tries screencopy and then the portal; `wlr` alone
    fails on GNOME/KDE instead of switching. Unset keeps the default above.
  - **Portal permission.** The first `Start` shows the desktop's screen-share
    dialog. Someone at the machine has to allow the monitor; a remote session
    cannot click that dialog. The portal is asked to persist the grant until
    it is explicitly revoked, and the restore token is stored in the app data
    directory (`$XDG_DATA_HOME/openuuyc/screencast-restore-token`, or
    `~/.local/share/openuuyc/screencast-restore-token` when `XDG_DATA_HOME` is
    unset). Later starts pass the token back and should not ask again until
    the user revokes the share in the desktop's privacy settings.
  - A DPMS-blanked panel (idle laptop with no local input) is woken on connect
    via compositor IPC on Hyprland, so a remote session never stalls on a black
    screen. Best-effort elsewhere. That wake still runs if Hyprland is told to
    use the portal.
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
  hi-res, and keys). Printable US-ASCII `text_input` uses the same `uinput`
  key codes. Needs permission to `/dev/uinput`.
- **Non-ASCII / IME text.** On a Wayland compositor that advertises
  `zwp_virtual_keyboard_manager_v1` (wlroots family: Hyprland, Sway, river,
  labwc, Wayfire), a `text_input` commit that contains anything outside
  US-ASCII is injected with a one-shot xkb keymap on `zwp_virtual_keyboard_v1`.
  That is the DoyoDia linux-port text path. The whole commit goes through the
  virtual keyboard, so CJK and the ASCII around it stay in order and do not
  depend on the host keymap. ASCII **key** events (`kbd_press` / `kbd_release`)
  stay on `uinput`. A commit that is entirely US-ASCII stays on `uinput` too.
  Each character is paced by a few milliseconds so a host input method does not
  drop the burst. No extra udev rule: the compositor exposes the protocol.
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
- **Viewer file drop.** Dragging a file from a controller onto the session also
  lands in `~/Downloads` (same folder as Send, with a desktop notification when
  the session can show one). Controllers that negotiate native drag are answered
  on that channel; a clipboard OleDrop is saved the same way. The file is not
  placed under the pointer. Dragging files *out* of the Linux host is not offered.
- **Desktop audio.** Captured from the default sink monitor through `parec`
  (PulseAudio / PipeWire-pulse).
- **Zero-copy GPU capture (experimental, opt-in).** `OPENUUYC_ZEROCOPY=1` enables
  dmabuf `zwlr_screencopy` → VA surface import → VPP scale/CSC → VA-API encode,
  with automatic fallback to shm+VPP and then CPU. **Off by default.**

## Not done yet (gracefully degraded)

- Virtual / headless displays, resolution & DPI changes, multi-monitor topology
  journaling (physical outputs only).
- Remote microphone playback into the host (accepted on the wire, dropped).
- **Non-ASCII text where virtual-keyboard is missing.** X11, XWayland windows,
  and Wayland compositors that do not implement `zwp_virtual_keyboard_v1`
  (GNOME/Mutter does not) still type US-ASCII through `uinput` and skip other
  characters. A clipboard Ctrl+V fallback was not added: it would overwrite the
  synced clipboard and miss terminals and other clients that do not paste with
  Ctrl+V. Upstream's X11 answer is an XTest keysym rebind; this host does not
  have that injector, and XWayland clients ignore a custom virtual keymap.
- Windows system chords (Win+D, Task View, Ctrl+Alt+Del on the host).
- Portal streams that only offer DMA-BUF (this fallback reads shared memory).
- Confirming pointer mapping on a scaled GNOME/KDE desktop: the frame size
  follows the portal stream and can differ from the `wl_output` mode. The
  host updates the published size when the first frame disagrees.

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

Enumerates outputs, captures a frame, encodes → decodes it, probes
`/dev/uinput`, and reports whether `zwp_virtual_keyboard_manager_v1` is
advertised. The report names the capture order (`wlr-screencopy`,
`portal-screencast`, or `X11`). On a portal session with no restore token
yet, it warns that the desktop will show a share dialog before the first
frame. Use it to confirm the local half before asking a controller to
connect.

## Connection test plan (needs a second device)

1. On the Linux box: log in, open *连接设置* and turn on *允许被控*. The status
   should move to “等待连接”.
2. From a Windows or Mac controller signed into the same account, connect to the
   Linux device.
3. Check, in order: video appears; mouse moves and clicks land where expected;
   keyboard types ASCII; scroll works; desktop audio is heard (if enabled).
   For CJK / IME, use the retest below.
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
6. **File drop.** From the controller, drag a small file onto the remote session
   (the video window). It should appear in the host's `~/Downloads`. Then use
   file-transfer **Send** with another small file and confirm that path still
   writes to `~/Downloads` as well.
7. Toggle *允许被控* off and confirm the controller is dropped.

## Caveat: device platform id

The device registers with the generic (Windows) platform id, so the server lists
it like a Windows host. This is what currently lets the server mark it
controllable; whether the server fully honours a non-Windows host, and whether a
controller interoperates with this H.264 stream and input, can only be confirmed
by the connection test above.

## Live retest (GNOME and KDE)

A headless build machine cannot open a GNOME or KDE session. On a computer
logged into that desktop, with `xdg-desktop-portal` (and the GNOME or KDE
backend) and PipeWire installed:

1. Leave `OPENUUYC_CAPTURE` unset. Confirm the compositor does not advertise
   `zwlr_screencopy_manager_v1`.
2. Run `OpenUUYC host-selftest`. The first run should show the screen-share
   dialog. Share a monitor. The report should say `portal-screencast` and a
   non-empty frame. A restore token file appears in the app data directory
   (see above).
3. Run `host-selftest` again. It should capture without asking.
4. From a controller: video, mouse, keys, and (if enabled) desktop audio.
   On a scaled display, check that clicks land on the picture.
5. Revoke the share in the desktop's privacy settings and confirm the next
   start asks again.
6. If the report says buffers were delivered but could not be read, and the
   reason mentions DMA-BUF, that desktop refused shared memory. That case is
   still unsupported.

Hyprland (or another wlroots compositor): leave `OPENUUYC_CAPTURE` unset.
`host-selftest` must still report `wlr-screencopy`, with DPMS wake and the
separate cursor channel unchanged. `OPENUUYC_CAPTURE=portal` is the opt-in
to try the portal there. Leave `OPENUUYC_ZEROCOPY` unset; the experimental
dmabuf path stays wlr-only and off by default.

## v0.8.1

The notes in this section are the v0.8.1 release. Functional host features in
that release are unchanged from v0.8.0 aside from one clipboard fix. Viewer
file drop and the ScreenCast portal fallback, both described under **What
works**, landed after that tag. Release notes:
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
- Viewer file drop saves into `~/Downloads`, not at the pointer on the desktop.
  Drag-out from the Linux host is not offered.
- Linux → controller file clipboard is partial (text/images reliable).
- GNOME/KDE Wayland capture needed the ScreenCast portal at v0.8.0. That
  fallback is now implemented (shared memory / KDE MemFd); DMA-BUF-only
  streams are still unsupported. See **What works**.
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

## Retest: non-ASCII / IME text

A cloud VM has no controller and no compositor to type into. On a real
wlroots session (Hyprland is the one this host is aimed at):

1. Log in, confirm `/dev/uinput` is writable, turn on *允许被控*.
   `OpenUUYC host-selftest` should report `zwp_virtual_keyboard: 可用`.
2. From a Windows, Mac, or phone controller on the same account, connect and
   focus a **native Wayland** text field (a terminal or browser on Wayland,
   not an XWayland app).
3. Type ASCII from the controller keyboard: letters, Shift, and shortcuts still
   go through `uinput` and should land as they did before.
4. Commit CJK from the controller IME, for example `你好`, then a mixed commit
   such as `你好abc`. The characters should appear in that order in the focused
   field. A longer paste (more than 56 distinct characters) should still arrive
   whole.
5. Repeat on X11, or on GNOME Wayland: ASCII still types; non-ASCII is skipped
   and the host log says `zwp_virtual_keyboard_v1 is not available`. The input
   session itself should stay up (mouse and keys keep working).
6. If a modifier is still held on the controller when the IME commits, release
   it first. The virtual keyboard clears its own modifiers and does not lift
   keys held on the `uinput` device.
