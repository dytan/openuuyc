//! Capture through the XDG ScreenCast portal.
//!
//! The compositor hands each shared monitor over as a PipeWire stream. This
//! is the path for Wayland sessions that do not implement `wlr-screencopy`
//! (GNOME, KDE). On wlroots-family compositors it stays behind that protocol
//! unless `OPENUUYC_CAPTURE` selects it.
//!
//! The first `Start` shows the desktop's share dialog. Sharing is requested
//! as persistent (`ExplicitlyRevoked`), and the restore token the portal
//! returns is stored in the app data directory so later sessions can start
//! without asking, until the user revokes the grant.
//!
//! One portal session serves every monitor of this process. Its cursor mode
//! is fixed when it starts: embedded, unless the separate Hyprland cursor
//! channel is available, in which case the cursor is hidden in the stream.

use super::pipewire::Stream;
use crate::media::capture::Screen;
use anyhow::{Context, Result, anyhow, bail};
use ashpd::desktop::PersistMode;
use ashpd::desktop::screencast::{
    CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
    StartCastOptions,
};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// One shared monitor as the portal described it.
#[derive(Clone, Debug)]
struct Source {
    node: u32,
    /// Portal stream id when the compositor sets one. Often opaque.
    id: Option<String>,
    position: Option<(i32, i32)>,
    size: Option<(i32, i32)>,
}

/// Logical rectangle from xdg-output, which is the space the portal uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OutputRect {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

struct Session {
    runtime: tokio::runtime::Runtime,
    proxy: Screencast,
    session: ashpd::desktop::Session<Screencast>,
    sources: Vec<Source>,
    /// `true` when the stream embeds the pointer.
    cursor: bool,
}

/// Run portal I/O on a thread of its own so `block_on` is never entered from
/// whatever runtime the capture worker is already on.
fn on_portal<T: Send>(
    runtime: &tokio::runtime::Runtime,
    work: impl std::future::Future<Output = T> + Send,
) -> T {
    std::thread::scope(|scope| {
        scope
            .spawn(|| runtime.block_on(work))
            .join()
            .expect("portal thread panicked")
    })
}

fn token_path() -> Result<std::path::PathBuf> {
    Ok(crate::paths::app_data_dir()?.join("screencast-restore-token"))
}

/// Whether a previous grant stored a restore token.
pub(crate) fn remembered() -> bool {
    token_path()
        .and_then(|path| Ok(std::fs::read_to_string(path)?))
        .is_ok_and(|token| !token.trim().is_empty())
}

impl Session {
    fn start(cursor: bool) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("screencast-portal")
            .enable_all()
            .build()
            .context("无法创建 ScreenCast 门户运行时")?;
        let token = std::fs::read_to_string(token_path()?)
            .ok()
            .map(|token| token.trim().to_owned())
            .filter(|token| !token.is_empty());
        let started = on_portal(&runtime, async {
            let proxy = Screencast::new()
                .await
                .context("桌面没有提供 ScreenCast 门户")?;
            let session = proxy
                .create_session(Default::default())
                .await
                .context("无法创建 ScreenCast 会话")?;
            proxy
                .select_sources(
                    &session,
                    SelectSourcesOptions::default()
                        .set_sources(Some(SourceType::Monitor.into()))
                        .set_multiple(Some(true))
                        .set_cursor_mode(Some(if cursor {
                            CursorMode::Embedded
                        } else {
                            CursorMode::Hidden
                        }))
                        .set_persist_mode(Some(PersistMode::ExplicitlyRevoked))
                        .set_restore_token(token.as_deref()),
                )
                .await
                .context("无法配置 ScreenCast 源")?
                .response()?;
            // Without a usable restore token this shows the sharing dialog
            // and waits for someone at this machine.
            let streams = proxy
                .start(&session, None, StartCastOptions::default())
                .await
                .context("无法开始屏幕共享")?
                .response()
                .context("本机用户没有同意屏幕共享")?;
            let sources = streams
                .streams()
                .iter()
                .map(|stream| Source {
                    node: stream.pipe_wire_node_id(),
                    id: stream.id().map(str::to_owned),
                    position: stream.position(),
                    size: stream.size(),
                })
                .collect::<Vec<_>>();
            let restore = streams.restore_token().map(str::to_owned);
            anyhow::Ok((proxy, session, sources, restore))
        })?;
        let (proxy, session, sources, restore) = started;
        if let Some(restore) = restore.filter(|token| !token.is_empty()) {
            write_token(&restore)?;
        }
        if sources.is_empty() {
            bail!("屏幕共享没有包含任何显示器");
        }
        tracing::info!(?sources, cursor, "ScreenCast portal session started");
        Ok(Self {
            runtime,
            proxy,
            session,
            sources,
            cursor,
        })
    }

    /// The shared session with this cursor mode, started if none is alive.
    fn shared(cursor: bool) -> Result<Arc<Self>> {
        static SESSIONS: Mutex<Vec<Weak<Session>>> = Mutex::new(Vec::new());
        // Held across start: a restore token is single-use, so two sessions
        // starting together would leave one of them asking again.
        let mut sessions = SESSIONS.lock().unwrap_or_else(|error| error.into_inner());
        sessions.retain(|session| session.strong_count() > 0);
        if let Some(session) = sessions
            .iter()
            .filter_map(Weak::upgrade)
            .find(|session| session.cursor == cursor)
        {
            return Ok(session);
        }
        let session = Arc::new(Self::start(cursor)?);
        sessions.push(Arc::downgrade(&session));
        Ok(session)
    }

    fn source(&self, screen: &Screen) -> Result<&Source> {
        let logical = output_logical(screen);
        let fallback = logical.is_none().then_some((
            (screen.left, screen.top),
            (screen.width as i32, screen.height as i32),
        ));
        if let Some(index) = pick(&self.sources, &screen.device_name, logical, fallback) {
            return Ok(&self.sources[index]);
        }
        tracing::warn!(
            screen = %screen.device_name,
            left = screen.left,
            top = screen.top,
            width = screen.width,
            height = screen.height,
            sources = ?self.sources,
            logical = ?logical,
            "门户共享的屏幕里找不到所选显示器"
        );
        Err(anyhow!(unmatched(&self.sources, logical.is_some())))
    }
}

fn write_token(restore: &str) -> Result<()> {
    let path = token_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // The token restores a screen-share grant. Keep it private to the user.
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("无法保存屏幕共享恢复令牌 {}", path.display()))?;
    file.write_all(restore.as_bytes())?;
    Ok(())
}

/// xdg-output logical rectangle for `screen`, when this is a Wayland session
/// and the compositor named the output. That is the coordinate space the
/// portal describes a monitor in. Scaled desktops disagree with the physical
/// rectangle on `Screen`, so the physical rectangle is not a fallback here.
fn output_logical(screen: &Screen) -> Option<OutputRect> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return None;
    }
    let outputs = super::wayland::outputs().ok()?;
    let output = outputs
        .iter()
        .find(|output| output.name == screen.device_name)?;
    let (left, top, width, height) = output.logical;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(OutputRect {
        left,
        top,
        width,
        height,
    })
}

fn unmatched(sources: &[Source], have_layout: bool) -> String {
    if sources.len() > 1 && sources.iter().all(|source| source.position.is_none()) {
        return "门户没有给出共享屏幕的位置，无法区分共享的多块屏".to_owned();
    }
    if !have_layout && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return "读不到合成器的输出布局（需要 xdg-output），缩放的桌面上无法确认门户共享的是哪块屏"
            .to_owned();
    }
    "所选显示器不在本机授权共享的屏幕中".to_owned()
}

/// Which portal stream is this monitor.
///
/// `logical` is the compositor rectangle (xdg-output). It wins whenever it
/// is known: on a scaled desktop the display server's rectangle can be
/// exactly some other monitor's portal rectangle. `fallback` is that display
/// rectangle, used only when there is no logical layout (an X11 session).
/// A stream id that equals the output name is accepted when the rectangle
/// does not, and a single stream with no geometry can only be this monitor.
fn pick(
    sources: &[Source],
    screen_name: &str,
    logical: Option<OutputRect>,
    fallback: Option<((i32, i32), (i32, i32))>,
) -> Option<usize> {
    if let Some(rect) = logical {
        if let Some(index) = sources
            .iter()
            .position(|source| rectangle_matches(source, rect))
        {
            return Some(index);
        }
    } else if let Some((position, size)) = fallback
        && let Some(index) = sources
            .iter()
            .position(|source| source.position == Some(position) && source.size == Some(size))
    {
        return Some(index);
    }
    if let Some(index) = sources
        .iter()
        .position(|source| source.id.as_deref() == Some(screen_name))
    {
        return Some(index);
    }
    match sources {
        [only] if only.position.is_none() => Some(0),
        _ => None,
    }
}

/// Position must match. Size may be a unit off: the portal and xdg-output
/// do not always round a fractional logical size the same way.
fn rectangle_matches(source: &Source, rect: OutputRect) -> bool {
    source.position == Some((rect.left, rect.top))
        && source.size.is_some_and(|(width, height)| {
            (i64::from(width) - i64::from(rect.width)).abs() <= 1
                && (i64::from(height) - i64::from(rect.height)).abs() <= 1
        })
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = on_portal(&self.runtime, self.session.close());
        let _ = &self.proxy;
    }
}

/// One captured portal frame: tightly packed BGRA.
pub(crate) struct Shot {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub(crate) struct Grab {
    stream: Stream,
    session: Arc<Session>,
}

impl Grab {
    /// `cursor` embeds the pointer in the stream. Pass false when the
    /// separate cursor channel is publishing it.
    pub(crate) fn open(screen: &Screen, cursor: bool) -> Result<Self> {
        let session = Session::shared(cursor)?;
        let node = session.source(screen)?.node;
        let fd = on_portal(
            &session.runtime,
            session
                .proxy
                .open_pipe_wire_remote(&session.session, OpenPipeWireRemoteOptions::default()),
        )
        .context("无法打开屏幕共享的 PipeWire 连接")?;
        let stream = Stream::connect(fd, node)?;
        Ok(Self { stream, session })
    }

    pub(crate) fn next(&mut self, timeout: Duration) -> Result<Option<Shot>> {
        Ok(self.stream.next(timeout)?.map(|picture| Shot {
            width: picture.width,
            height: picture.height,
            pixels: picture.pixels,
        }))
    }

    pub(crate) fn stage(&self) -> String {
        self.stream.stage()
    }
}

#[cfg(test)]
mod tests {
    use super::{OutputRect, Source, pick, rectangle_matches, unmatched};

    fn stream(x: i32, y: i32, width: i32, height: i32) -> Source {
        Source {
            node: 0,
            id: None,
            position: Some((x, y)),
            size: Some((width, height)),
        }
    }

    fn rect(left: i32, top: i32, width: i32, height: i32) -> OutputRect {
        OutputRect {
            left,
            top,
            width,
            height,
        }
    }

    /// GNOME with three 4K monitors, the first two at 200% and the third at
    /// 100%. XWayland scales the whole desktop by 2, so the second monitor's
    /// server rectangle is exactly the third one's portal rectangle. Pairing
    /// uses the logical layout, not that server rectangle.
    #[test]
    fn mixed_scales_pair_by_the_logical_rectangle() {
        let sources = [
            stream(0, 0, 1920, 1080),
            stream(1920, 0, 1920, 1080),
            stream(3840, 0, 3840, 2160),
        ];
        let layouts = [
            rect(0, 0, 1920, 1080),
            rect(1920, 0, 1920, 1080),
            rect(3840, 0, 3840, 2160),
        ];
        // Server rectangles, scaled by 2. The second collides with the third
        // portal rectangle and must not win.
        let servers = [
            ((0, 0), (3840, 2160)),
            ((3840, 0), (3840, 2160)),
            ((7680, 0), (7680, 4320)),
        ];
        for (index, layout) in layouts.iter().enumerate() {
            assert_eq!(
                pick(&sources, "DP-2", Some(*layout), Some(servers[index])),
                Some(index)
            );
        }
    }

    #[test]
    fn an_unscaled_desktop_matches_the_display_rectangle() {
        let sources = [stream(0, 0, 1920, 1080), stream(1920, 0, 1920, 1080)];
        assert_eq!(
            pick(&sources, "DP-2", None, Some(((1920, 0), (1920, 1080)))),
            Some(1)
        );
    }

    #[test]
    fn a_rounded_size_pairs_but_a_shifted_position_does_not() {
        let layout = rect(1920, 0, 1920, 1080);
        assert_eq!(
            pick(&[stream(1920, 0, 1919, 1079)], "DP-2", Some(layout), None),
            Some(0)
        );
        assert!(rectangle_matches(&stream(1920, 0, 1919, 1079), layout));
        assert_eq!(
            pick(&[stream(1921, 0, 1920, 1080)], "DP-2", Some(layout), None),
            None
        );
    }

    #[test]
    fn a_connector_id_pairs_when_the_rectangle_does_not() {
        let mut named = stream(0, 0, 1920, 1080);
        named.id = Some("DP-2".to_owned());
        assert_eq!(
            pick(&[named], "DP-2", Some(rect(100, 0, 1920, 1080)), None),
            Some(0)
        );
    }

    #[test]
    fn a_scaled_desktop_without_a_layout_is_not_guessed_from_a_foreign_rectangle() {
        // No logical layout: the caller passes no fallback when it knows the
        // spaces disagree. Nothing is guessed.
        let sources = [stream(0, 0, 1920, 1080), stream(3840, 0, 1920, 1080)];
        assert_eq!(pick(&sources, "DP-2", None, None), None);
    }

    #[test]
    fn a_lone_stream_without_geometry_is_the_monitor_asked_for() {
        let only = [Source {
            node: 7,
            id: None,
            position: None,
            size: None,
        }];
        assert_eq!(pick(&only, "DP-1", None, None), Some(0));
        let two = [
            Source {
                node: 0,
                id: None,
                position: None,
                size: None,
            },
            Source {
                node: 1,
                id: None,
                position: None,
                size: None,
            },
        ];
        assert_eq!(pick(&two, "DP-1", None, None), None);
        assert!(unmatched(&two, true).contains("没有给出共享屏幕的位置"));
    }

    #[test]
    fn a_miss_says_what_was_missing() {
        let sources = [stream(0, 0, 1920, 1080)];
        assert_eq!(
            unmatched(&sources, true),
            "所选显示器不在本机授权共享的屏幕中"
        );
    }
}
