//! Separate-cursor support for the Linux controlled side.
//!
//! The Windows host never bakes the pointer into the video frames: it samples
//! the cursor's position and shape and publishes them on the lightweight cursor
//! control channel, so moving the mouse costs nothing on the (slow) video path.
//! We do the same on Hyprland: screencopy is told `overlay_cursor = 0`, a small
//! background thread reads the global pointer position over the Hyprland IPC
//! socket at ~60 Hz, and the shared pointer snapshot the cursor task already
//! publishes is filled with that position plus a default arrow shape (Wayland
//! clients cannot read the compositor's current cursor image).
//!
//! When the IPC socket is absent (not Hyprland, or a plain X11 session) this
//! module reports unavailable and the capture path falls back to compositing
//! the cursor into the frame as before, so no controller ever loses its cursor.

use crate::platform::linux::cursor_shape::{Image, Snapshot};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Locate the Hyprland IPC `.socket.sock`, preferring the signature in the
/// environment and falling back to the sole instance directory if unset.
fn socket_path() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let base = PathBuf::from(&runtime).join("hypr");
    if let Some(his) = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE") {
        let candidate = base.join(his).join(".socket.sock");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let mut found = None;
    for entry in std::fs::read_dir(&base).ok()?.flatten() {
        let candidate = entry.path().join(".socket.sock");
        if candidate.exists() {
            // Prefer a single match; if several instances exist we cannot tell
            // which owns our session, so decline rather than guess.
            if found.is_some() {
                return None;
            }
            found = Some(candidate);
        }
    }
    found
}

/// Whether the separate-cursor path can run (Hyprland IPC reachable).
pub(crate) fn available() -> bool {
    socket_path().is_some()
}

/// Current global pointer position in logical (layout) coordinates, or `None`
/// if the socket is gone or the reply did not parse.
fn position() -> Option<(i32, i32)> {
    let mut stream = UnixStream::connect(socket_path()?).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(50)));
    stream.write_all(b"cursorpos").ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    let (x, y) = reply.trim().split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// A default arrow drawn once: white fill, black outline, hotspot at the tip.
/// Wayland offers no way to read the compositor's live cursor image, so every
/// shape is this arrow (matching "default arrow if unavailable").
fn default_arrow() -> Option<Image> {
    static ARROW: OnceLock<Option<Image>> = OnceLock::new();
    ARROW.get_or_init(build_arrow).clone()
}

fn build_arrow() -> Option<Image> {
    use base64::Engine;
    use image::ImageEncoder;
    // 'B' outline (black), 'W' fill (white), '.' transparent. 12x19.
    const ROWS: [&str; 19] = [
        "B...........",
        "BB..........",
        "BWB.........",
        "BWWB........",
        "BWWWB.......",
        "BWWWWB......",
        "BWWWWWB.....",
        "BWWWWWWB....",
        "BWWWWWWWB...",
        "BWWWWWWWWB..",
        "BWWWWWWWWWB.",
        "BWWWWWWWWWWB",
        "BWWWWWBBBBBB",
        "BWWBWWB.....",
        "BWB.BWWB....",
        "BB..BWWB....",
        "B....BWWB...",
        ".....BWWB...",
        "......BB....",
    ];
    let height = ROWS.len() as u32;
    let width = ROWS[0].len() as u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in ROWS {
        for px in row.bytes() {
            let [r, g, b, a] = match px {
                b'B' => [0u8, 0, 0, 255],
                b'W' => [255, 255, 255, 255],
                _ => [0, 0, 0, 0],
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(Image {
        width,
        height,
        hotspot: [0, 0],
        kind: 0,
        png: base64::engine::general_purpose::STANDARD.encode(png),
    })
}

/// Background pointer sampler. While alive it keeps the shared pointer snapshot
/// up to date at ~60 Hz, independent of the video capture loop, so cursor-only
/// moves update the controller's cursor without any capture/encode work.
pub(crate) struct Sampler {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Sampler {
    /// Start sampling into `pointer`, mapping logical coordinates to the
    /// physical (scaled) space the screen catalog uses. Returns `None` when the
    /// separate-cursor path is unavailable, leaving the caller on the
    /// composite-cursor fallback.
    pub(crate) fn start(pointer: Arc<Mutex<Option<Snapshot>>>, scale: f64) -> Option<Self> {
        if !available() {
            return None;
        }
        let arrow = default_arrow();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let join = std::thread::Builder::new()
            .name("host-cursor".into())
            .spawn(move || {
                let mut last: Option<Snapshot> = None;
                while !flag.load(Ordering::Relaxed) {
                    if let Some((lx, ly)) = position() {
                        let snapshot = Snapshot {
                            showing: true,
                            x: (f64::from(lx) * scale).round() as i32,
                            y: (f64::from(ly) * scale).round() as i32,
                            image: arrow.clone(),
                        };
                        if last.as_ref() != Some(&snapshot) {
                            if let Ok(mut guard) = pointer.lock() {
                                *guard = Some(snapshot.clone());
                            }
                            last = Some(snapshot);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(16));
                }
            })
            .ok()?;
        Some(Self {
            stop,
            join: Some(join),
        })
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
