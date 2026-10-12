### Linux v0.8.1

Documentation release plus one host clipboard fix since v0.8.0. Functional host
features are otherwise unchanged from v0.8.0.

**Since v0.8.0**

- **Sticky-image clipboard fix** (`0b9cfad`). After an image sync, a later text
  copy could still paste as the old image: a slow in-flight image fetch could
  finish after a newer text offer and overwrite the local clipboard, and the
  host kept advertising the leftover image. Remote accepts are now sequenced and
  discarded when stale, and a new text copy is advertised as text only.
- **Upstream rename.** The upstream project is now **Farwing**
  ([djkcyl/farwing](https://github.com/djkcyl/farwing)). `djkcyl/openuuyc`
  resolves to that repository. This fork's app binary and UI remain OpenUUYC.
  The README carries the upstream Farwing introduction and the project
  acknowledgements.
- **Linux status docs.** `LINUX.md` matches host mode (v0.8.0+). Linux is a
  controller and a controlled host.

**Unchanged from v0.8.0**

A Linux machine can still be a controlled host: VA-API or software H.264 encode,
separate cursor on Hyprland, bidirectional clipboard (text, images, and files),
file receive into `~/Downloads`, DPMS wake on connect, and experimental
zero-copy (`OPENUUYC_ZEROCOPY=1`, off by default). The same limits apply:
GNOME/KDE Wayland capture is unsupported, viewer drag-drop onto the video
window does not write files (use file-transfer **Send**), Linux → controller
file clipboard is partial, and input injection needs `/dev/uinput`. Full notes:
`LINUX-HOST.md`.

**Recommended defaults:** `OPENUUYC_ZEROCOPY` unset/0; `OPENUUYC_HOST_ENCODER`
unset (auto).

> The Linux port keeps its own release line. `v0.8.1` is this packaging tag.
> `Cargo.toml` stays at upstream `1.0.0-rc.2`, which is the version the app
> reports.
