### Linux host milestone

This release makes a Linux box usable as a **controlled device** (被控 / host),
driven from a Windows or macOS controller — not just a controller itself.

**New since v0.7.0**
- **VA-API hardware H.264 encode**, selected automatically after an
  encode→decode round-trip validates on the machine; **software H.264** is the
  automatic fallback. Override with `OPENUUYC_HOST_ENCODER=software|vaapi`.
- **Separate cursor channel** on Hyprland (mouse moves no longer cost a video
  frame), like the Windows host.
- **Clipboard sync both ways** — text, images, and files — using the Windows
  clipboard dialect so Windows *and* macOS controllers interoperate.
- **Host-side file receive** into `~/Downloads` via the file-transfer Send path.
- **Wake a DPMS-blanked panel on connect**, so an idle laptop with no local input
  never serves a black screen.
- Robustness: the capture/encode worker degrades (shm → CPU) instead of tearing
  the session down when a frame path hiccups.

**Experimental: zero-copy GPU capture (opt-in, OFF by default)**
- `OPENUUYC_ZEROCOPY=1` enables dmabuf `zwlr_screencopy` → VA import → VPP
  scale/CSC → VA-API encode, with automatic fallback (dmabuf → shm+VPP → CPU).
- Not yet soak-tested broadly. Leave it **off** for routine use; opt in only to
  test and turn it off if you see quality dips or stalls.

**Known limitations**
- Viewer drag-drop onto the video window does not write files yet — use the
  file-transfer **Send** button.
- Linux → controller file clipboard is partial (text/images are reliable).
- GNOME/KDE Wayland capture needs the ScreenCast portal (not supported yet).
- Non-ASCII / IME text injection and some Windows system chords are unsupported.
- Input injection needs access to `/dev/uinput` (see `LINUX-HOST.md`).

**Recommended defaults:** `OPENUUYC_ZEROCOPY` unset/0; `OPENUUYC_HOST_ENCODER`
unset (auto). Full host notes: `LINUX-HOST.md`.

> The Linux port keeps its own release line (`v0.1.0 … v0.8.0`); the underlying
> app engine is upstream `1.0.0-rc.2`, which is the version the app reports.
