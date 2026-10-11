# Linux viewer: upstream feature parity

The Linux player (winit + wgpu, `src/application/viewer/linux_presenter.rs`)
follows the Windows player wherever X11/Wayland allow it.

| Feature | Linux | Notes |
|---|---|---|
| Caption row (device, tabs, metrics, quality, annotation, 1:1, microphone, input) | yes | `linux_chrome.rs`, same shared caption controls as Windows. No plugin button: the plugin host is Windows-only. |
| Screen tabs | yes, single window | Click a tab to switch the window to that screen; right-click for the per-screen display menu; `+` adds a virtual screen, `×` removes one. Follows remote topology changes. |
| Detach a tab into its own window | no | Needs pointer capture outside the window during a tab drag; Wayland never grants it. |
| Device switching | yes | Click the device name in the caption; occupied devices ask for takeover confirmation. |
| Annotation board | yes | Same tool panel as Windows. Remote input is blocked while annotation is on. |
| Listen-only (audio) view | yes | Fixed-size audio panel; the video window geometry comes back afterwards. Some tiling compositors ignore fixed sizes. |
| Display-transition overlay | yes | Shown while a resolution or topology change is pending; input is blocked meanwhile. |
| Drop files onto the viewer | yes | Sent through the official clipboard file drop at the pointer position. |
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
