//! Capture-ahead producer for the Linux controlled side.
//!
//! A dedicated thread owns the Wayland/X11 `Desktop` and continuously grabs and
//! scales the newest frame into a single-slot mailbox. The encode loop then
//! overlaps with capture instead of blocking on it, and the mailbox only ever
//! holds the latest frame: an unconsumed frame is overwritten (dropped), never
//! queued, so the controller never falls behind on stale pictures.
use crate::features::host::VideoConfig;
use crate::features::host::{Lease, lock};
use crate::platform::capture::{self, Desktop, Frame, Screen};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

type Cursor = Option<crate::platform::windows::cursor_shape::Snapshot>;

/// The latest captured frame and the capture-source state the encode loop reads.
pub(super) struct Snapshot {
    pub frame: Option<Frame>,
    pub screen: Screen,
    pub generation: u64,
    pub cursor: Cursor,
    pub available: bool,
    pub backend_name: &'static str,
    pub hdr_available: bool,
    pub grab: Duration,
    pub scale: Duration,
}

#[derive(Default)]
struct Slot {
    snap: Option<Snapshot>,
    seq: u64,
    opened: bool,
    fatal: Option<String>,
}

struct Shared {
    slot: Mutex<Slot>,
    cv: Condvar,
}

pub(super) struct Feed {
    shared: Arc<Shared>,
    cancel: CancellationToken,
    thread: Option<JoinHandle<()>>,
    last_seq: u64,
}

impl Feed {
    pub(super) fn start(
        screen: Screen,
        config: Arc<Mutex<VideoConfig>>,
        handle: Lease,
        cancel: CancellationToken,
    ) -> Self {
        let shared = Arc::new(Shared {
            slot: Mutex::new(Slot::default()),
            cv: Condvar::new(),
        });
        let worker = shared.clone();
        let worker_cancel = cancel.clone();
        let thread = std::thread::Builder::new()
            .name("host-screen-grab".into())
            .spawn(move || produce(&worker, &worker_cancel, &config, &handle, screen))
            .ok();
        Self {
            shared,
            cancel,
            thread,
            last_seq: 0,
        }
    }

    /// A fatal open error from the producer, if any (mirrors the old inline
    /// `Desktop::open_selected` failure that stopped the worker).
    pub(super) fn fatal(&self) -> Option<String> {
        lock(&self.shared.slot).fatal.clone()
    }

    /// Whether the producer has opened the capture source at least once.
    pub(super) fn opened(&self) -> bool {
        lock(&self.shared.slot).opened
    }

    /// Take the newest captured snapshot, waiting up to `wait_ms` for a fresh
    /// one. Returns `None` when no new frame arrived in time.
    pub(super) fn wait(&mut self, wait_ms: u32) -> Option<Snapshot> {
        let deadline = Instant::now() + Duration::from_millis(u64::from(wait_ms.max(1)));
        let mut slot = lock(&self.shared.slot);
        loop {
            if slot.seq != self.last_seq {
                self.last_seq = slot.seq;
                if let Some(snap) = slot.snap.take() {
                    return Some(snap);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (guard, _) = self
                .shared
                .cv
                .wait_timeout(slot, deadline - now)
                .unwrap_or_else(|e| e.into_inner());
            slot = guard;
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.shared.cv.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn publish(shared: &Shared, snap: Snapshot) {
    let mut slot = lock(&shared.slot);
    slot.opened = true;
    slot.seq = slot.seq.wrapping_add(1);
    // Overwrite any unconsumed frame: only the latest picture matters.
    slot.snap = Some(snap);
    drop(slot);
    shared.cv.notify_one();
}

fn produce(
    shared: &Shared,
    cancel: &CancellationToken,
    config: &Arc<Mutex<VideoConfig>>,
    handle: &Lease,
    mut screen: Screen,
) {
    let mut desktop: Option<Desktop> = None;
    let mut next = Instant::now();
    while !cancel.is_cancelled() && handle.requested() {
        let wanted = *lock(config);
        let fps = wanted.fps.clamp(1, 240);
        let period = Duration::from_secs_f64(1.0 / f64::from(fps));

        if desktop.is_none() {
            match Desktop::open_selected(&screen) {
                Ok(d) => {
                    screen = d.screen.clone();
                    desktop = Some(d);
                }
                Err(error) => {
                    // Match the old behaviour: a hard open failure stops the
                    // pipeline with a reported reason instead of spinning.
                    let mut slot = lock(&shared.slot);
                    slot.fatal = Some(format!("{error:#}"));
                    drop(slot);
                    shared.cv.notify_all();
                    return;
                }
            }
        }

        // Pace to the target frame rate: the grab itself is the main limiter,
        // but this avoids burning the compositor on captures the encode loop
        // would only drop.
        let now = Instant::now();
        if now < next {
            std::thread::sleep((next - now).min(Duration::from_millis(20)));
            continue;
        }
        next = if now.saturating_duration_since(next) >= period {
            now + period
        } else {
            next + period
        };

        let wait_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);
        let desktop_ref = match desktop.as_mut() {
            Some(d) => d,
            None => continue,
        };
        match desktop_ref.next_timed(
            wait_ms,
            wanted.quality,
            wanted.cursor_capture,
            wanted.color.is_hdr(),
            wanted.maximum,
        ) {
            Ok(result) => {
                let (frame, grab, scale) = match result {
                    Some((frame, grab, scale)) => (Some(frame), grab, scale),
                    None => (None, Duration::ZERO, Duration::ZERO),
                };
                screen = desktop_ref.screen.clone();
                let snap = Snapshot {
                    frame,
                    screen: desktop_ref.screen.clone(),
                    generation: desktop_ref.generation,
                    cursor: desktop_ref.cursor.clone(),
                    available: desktop_ref.available,
                    backend_name: desktop_ref.backend_name(),
                    hdr_available: desktop_ref.hdr_available(),
                    grab,
                    scale,
                };
                publish(shared, snap);
            }
            Err(error) => {
                // A transient capture loss (e.g. source gone) reopens next loop;
                // publish an unavailable snapshot so the encode loop can react.
                if error.is::<capture::SourceGone>() {
                    if let Ok(refreshed) = capture::refresh(&screen) {
                        screen = refreshed;
                    }
                }
                let snap = Snapshot {
                    frame: None,
                    screen: screen.clone(),
                    generation: desktop.as_ref().map_or(0, |d| d.generation),
                    cursor: None,
                    available: false,
                    backend_name: desktop.as_ref().map_or("Linux", Desktop::backend_name),
                    hdr_available: false,
                    grab: Duration::ZERO,
                    scale: Duration::ZERO,
                };
                desktop = None;
                publish(shared, snap);
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        // Release the CPU briefly so a cancelled/paused worker exits promptly.
        if !handle.requested() {
            break;
        }
        let _ = Ordering::Acquire;
    }
}
