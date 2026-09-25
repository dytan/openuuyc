# Linux lock-screen video freeze (f0067ff)

Date: 2026-09-26 (Asia/Shanghai)

## Symptom

On the Windows host lock / SecureDesktop UI, the Linux controller showed a
frozen frame (clock stuck, password dots appeared only after reconnect). HID
still reached the host; RTP/FEC kept arriving.

## Misdiagnosis

First suspicion was SecureDesktop blocking HID injection. HID timestamps were
realtime; the pathology was on the **viewer decode / present** path, not input.

## Root cause

`DECODER_ADMISSION_TOKENS = 1`: the decode worker parks while the presentation
queue is non-empty and waits for `manager_wake.unpark()`.

- Windows Video Render unparks after every dequeue.
- Linux `Player::take_frame` (`src/viewer/linux_presenter.rs`) and the
  windows-compat presenter path (`src/viewer/linux/windows_presenter.rs`) did
  **not** unpark after presenting.

Result after the first decoded frame: `rendered_frames = 1`, `received_fps = 0`,
while FEC/RTP continued — classic admission deadlock, not a missing keyframe.

## Fix

Commit `f0067ff` (`fix(linux): unpark decoder after presenting so lock UI stays live`):

- `linux_presenter.rs`: after draining newer frames in `take_frame`, drop the
  queue lock and call `self.session.manager_wake.unpark()`.
- `windows_presenter.rs` (Linux cfg): replace `frame_wake.notify()`-only with
  `manager_wake.unpark()` after present (same contract as Windows).

## Lesson

Correlate **frame / admission stats** with HID timestamps before blaming
injection. Frozen UI + live HID + live FEC points at present/unpark, not CAD.
