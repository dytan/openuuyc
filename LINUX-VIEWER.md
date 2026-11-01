# Linux viewer: upstream feature parity

The Linux player (winit + wgpu, `src/application/viewer/linux_presenter.rs`)
follows the Windows player wherever X11/Wayland allow it.

| Feature | Linux | Notes |
|---|---|---|
| Caption row (device, tabs, metrics, quality, annotation, 1:1, clipboard, microphone, input) | yes | `linux_chrome.rs`, same shared caption controls as Windows. The clipboard button toggles sync (see below). No plugin button: the plugin host is Windows-only; clipboard uses that caption slot. |
| Screen tabs | yes, single window | Click a tab to switch the window to that screen; right-click for the per-screen display menu; `+` adds a virtual screen, `×` removes one. Follows remote topology changes. |
| Detach a tab into its own window | no | Needs pointer capture outside the window during a tab drag; Wayland never grants it. |
| Device switching | yes | Click the device name in the caption; occupied devices ask for takeover confirmation. |
| Annotation board | yes | Same tool panel as Windows. Remote input is blocked while annotation is on. |
| Listen-only (audio) view | yes | Fixed-size audio panel; the video window geometry comes back afterwards. Some tiling compositors ignore fixed sizes. |
| Display-transition overlay | yes | Shown while a resolution or topology change is pending; input is blocked meanwhile. |
| Drop files onto the viewer | yes | Sent through the official clipboard file drop at the pointer position. Requires clipboard sync to be on (the drop uses the active clipboard session). |
| Clipboard sync toggle | yes | Title-bar button, to the right of the microphone. Same per-device preference as 串流设置 → 剪贴板同步. Off suspends text, image, and file-clipboard sync. 「文件复制」 stays a settings-only sub-switch. Hidden in fullscreen along with the rest of the caption. |
| Drag files out of the viewer to the desktop | no | See below. |
| Secure desktop (lock screen/UAC) | yes | Hint in the stream menu plus one automatic Ctrl+Alt+Del per lock transition while keyboard control is on. `OPENUUYC_NO_AUTO_CAD=1` turns the automatic SAS off. |

## Why dragging files out is not supported

On Windows the viewer is an OLE drag source. It negotiates the native
drag channel with the host and streams file contents on demand while the
drop target reads them. Doing the same on Linux would need two things that
do not exist today:

1. **A native drag channel on Linux.** `drag_drop_linux.rs` keeps the
   channel inert, so hosts never see the capability. The Windows controller
   depends on OLE `IDataObject`/`IStream` virtual files (`CFSTR_FILEDESCRIPTOR`
   and `CFSTR_FILECONTENTS`), and nothing on Linux plays that role.
2. **A way to start an outgoing drag from the player window.** winit only
   accepts incoming file drops. Starting a drag needs XDND on X11 or
   `wl_data_device.start_drag` on Wayland, with the serial of the pointer
   press that started it. winit owns that press and never exposes it, so a
   second Wayland connection cannot start a drag for the player surface.
   XDND also expects the files at drop time, but the remote file has not
   been fetched yet.

Workaround: copy the files on the remote desktop and paste them locally.
Clipboard file sync is backed by a FUSE file offer, which is why the
packages depend on fuse3. The file transfer browser can also fetch remote
files.

## Clipboard toggle retest

Windowed session (the caption is hidden in fullscreen):

1. Linux viewer to a Windows host. Turn on key/mouse control. Click the
   clipboard icon to the right of the microphone. Copy text on each side and
   confirm it arrives only while the button is on. Repeat with an image, and
   confirm a new text copy replaces a previous image (sticky-image sequencing).
2. With sync on and 「文件复制」 on, copy a file and confirm the offer. Turn
   the title-bar button off and confirm text, image, and file clipboard stop.
   Turn it back on and confirm 「文件复制」 is still whatever it was.
3. Reverse direction if a Windows viewer is available: this fork's Windows
   caption has no clipboard button; use 串流设置 → 剪贴板同步 against a Linux
   host. Host Send / drag-and-drop into `~/Downloads` should be unchanged.
