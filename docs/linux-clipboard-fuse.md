# Linux clipboard + FUSE (vs PR1)

Date: 2026-09-26 (Asia/Shanghai)

## Inventory (parity)

| Piece | Path | vs PR1 |
|-------|------|--------|
| FUSE file offer FS | `src/clipboard/fuse_linux.rs` | identical |
| Native Linux clipboard | `src/clipboard/native_linux.rs` | identical |
| X11 file offer helper | `src/clipboard/x11_offer_linux.rs` | identical |
| Wire protocol | `src/clipboard/protocol.rs` | identical |
| Format registry | `src/clipboard/formats.rs` | identical (process-local CF_* on Linux) |
| arboard + fuser deps | `Cargo.toml` | same features |

Mount root: `$XDG_RUNTIME_DIR/openuuyc/clipboard/<generation>/` (RO FUSE,
`fusermount3 -u -z` on retire). Text/image use arboard; file lists mount via
FUSE and publish `text/uri-list` / X11 offer.

## Gaps closed this pass

- **`clipboard_files` connection default** (was missing vs PR1): media options +
  profile, CLI `--clipboard-files`, center UI 「文件复制」, and
  `clipboard.set_files(profile.clipboard_files)` at stream-control create.
- Text/image 「剪贴板同步」 defaults on; player menu can still disable it. 「文件复制」 stays opt-in.
- Clipboard-ready diagnostic (`剪贴板同步条件`) when the gate flips.

## Remaining

- No AutoUnmount (needs AllowOther); stale mounts cleared on next start.
- Wayland file-offer polish beyond arboard `wayland-data-control` + X11 helper.
- `formats_linux.rs` / `formats_windows.rs` are leftover stubs (unused).

## How to verify

1. Rebuild and relaunch GUI (soft-kill only):
   ```bash
   pkill -x OpenUUYC || true
   ./target/debug/OpenUUYC gui \
     --codec h264 --hardware-decode false --transport auto \
     --auto-mouse-control true --clipboard-files true
   ```
2. Connect with 键鼠控制 on. Text/image 「剪贴板同步」 is on by default;
   turn on 「文件复制」 in the player menu (or start with `--clipboard-files true`)
   when you need file paste.
3. **Text**: copy on host → paste in a local editor; reverse direction.
4. **Image**: copy a PNG/bitmap on host → paste locally (arboard path).
5. **Files**: copy file(s) on host → paste into a local file manager. Expect a
   mount under `$XDG_RUNTIME_DIR/openuuyc/clipboard/` while the offer is live;
   `mount | grep openuuyc-clipboard` should show it. Drop clears the mount.
6. Logs: look for `剪贴板同步条件` (info) and FUSE root lines when a file
   offer is published. Missing `fuse3` / FUSE permission → paste fails; see
   `LINUX-BUILD.md`.
