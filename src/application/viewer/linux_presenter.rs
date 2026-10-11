//! Player window for X11 and Wayland.
//!
//! The Windows player composes DXVA surfaces through DirectComposition and
//! takes input from a raw-input hook. Neither exists here, so this presenter is
//! built the other way round: frames land on the CPU (software decode), the
//! wgpu shell draws them under the egui chrome, and input comes from the winit
//! events the compositor delivers to the focused window.
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::media::decoder::RenderSurface;
use crate::features::remote_input::MouseMode;
use crate::ui::chrome::{
    WindowMoveState, WindowResizeState, cancel_pointer_operation, configure_dwm_window,
    resize_regions, title_bar_height, title_bar_panel, update_nonmodal_window_resize,
    window_title_bar,
};
use crate::ui::gfx::{UiPresenter, UiTimingAudit, split_output};
use crate::ui::wgpu_video::VideoPlacement;
use crate::ui::window_manager::{Event as UiEvent, Repaint as UiRepaintEvent};

use super::linux_chrome::{PlayerChromeAction, PlayerTitleBar, ScreenTabBar, player_title_bar};
use super::screens::ScreenPlayback;
use crate::features::stream_control::RemoteScreen;

use super::{
    AudioView, ConnectionProgress, ConnectionProgressApp, DecodedVideoFrame, NativeViewerSession,
    PerformancePanelMode, StreamControlUi, ViewerDisplayHandle, ViewerWindowEvent,
    configure_viewer_visuals, mutex_lock, show_stream_control_window,
    take_next_frame,
};

pub(super) use crate::application::viewer_shortcuts::Action as ViewerShortcut;

pub(crate) struct ConnectingWindowsRunConfig {
    pub alias: String,
    pub progress: std_mpsc::Receiver<ConnectionProgress>,
    pub session: std_mpsc::Receiver<ViewerWindowEvent>,
    pub display_sender: tokio::sync::oneshot::Sender<ViewerDisplayHandle>,
}

pub(super) fn run(session: NativeViewerSession) -> Result<()> {
    let (sender, receiver) = std_mpsc::channel();
    let alias = session
        .title
        .strip_prefix(crate::VIEWER_TITLE_PREFIX)
        .unwrap_or(&session.title)
        .to_owned();
    sender
        .send(ViewerWindowEvent::Playing(Box::new(session)))
        .map_err(|_| anyhow!("queue existing viewer session"))?;
    run_player(
        ConnectingWindowsRunConfig {
            alias,
            progress: std_mpsc::channel().1,
            session: receiver,
            display_sender: tokio::sync::oneshot::channel().0,
        },
        false,
    )
}

pub(super) fn run_connecting(config: ConnectingWindowsRunConfig) -> Result<()> {
    run_player(config, true)
}

fn run_player(config: ConnectingWindowsRunConfig, needs_display: bool) -> Result<()> {
    let event_loop = EventLoop::<UiEvent>::with_user_event()
        .build()
        .context("create player event loop")?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut runner =
        ConnectingWindowsRunner::new(config, needs_display, event_loop.create_proxy(), false);
    event_loop
        .run_app(&mut runner)
        .map_err(|error| anyhow!("run connection/player event loop: {error}"))?;
    if let Some(error) = runner.fatal_error.take() {
        bail!(error);
    }
    Ok(())
}

fn ui_frame_interval(window: &Window) -> Duration {
    let millihertz = window
        .current_monitor()
        .and_then(|monitor| monitor.refresh_rate_millihertz())
        .filter(|rate| *rate != 0)
        .unwrap_or(60_000);
    Duration::from_secs_f64(1000.0 / f64::from(millihertz))
}

pub(crate) struct ConnectingWindowsRunner {
    embedded: bool,
    alias: String,
    attributes: WindowAttributes,
    progress: Option<std_mpsc::Receiver<ConnectionProgress>>,
    session: std_mpsc::Receiver<ViewerWindowEvent>,
    display_sender: Option<tokio::sync::oneshot::Sender<ViewerDisplayHandle>>,
    window: Option<Arc<Window>>,
    shell: Option<Shell>,
    stage: Stage,
    close_requested: bool,
    fatal_error: Option<String>,
    next_repaint: Option<Instant>,
    last_frame: Option<Instant>,
    ui_frame_interval: Duration,
    proxy: EventLoopProxy<UiEvent>,
    generation: u64,
    /// Window size and maximized state to restore when listening ends.
    audio_restore: Option<(winit::dpi::PhysicalSize<u32>, bool)>,
}

enum Stage {
    Connecting(Box<ConnectionProgressApp>),
    /// Audio-only connection: the compact listening panel.
    Listening(Box<AudioView>),
    Playing(Box<Player>),
    Finished,
}

/// egui plus the wgpu surface for the one player window.
struct Shell {
    context: egui::Context,
    input: egui_winit::State,
    presenter: UiPresenter,
    move_state: WindowMoveState,
    resize_state: WindowResizeState,
    timing_audit: Option<UiTimingAudit>,
}

impl ConnectingWindowsRunner {
    pub(crate) fn new(
        config: ConnectingWindowsRunConfig,
        needs_display: bool,
        proxy: EventLoopProxy<UiEvent>,
        embedded: bool,
    ) -> Self {
        Self {
            attributes: WindowAttributes::default()
                .with_visible(false)
                .with_title(format!("{}{}", crate::VIEWER_TITLE_PREFIX, config.alias))
                .with_window_icon(Some(crate::ui::branding::window_icon()))
                .with_decorations(false)
                .with_inner_size(LogicalSize::new(1280.0, 760.0))
                .with_min_inner_size(LogicalSize::new(760.0, 520.0)),
            alias: config.alias.clone(),
            progress: Some(config.progress),
            session: config.session,
            display_sender: needs_display.then_some(config.display_sender),
            window: None,
            shell: None,
            stage: Stage::Connecting(Box::new(ConnectionProgressApp::new(
                config.alias,
                std_mpsc::channel().1,
            ))),
            close_requested: false,
            fatal_error: None,
            next_repaint: Some(Instant::now()),
            last_frame: None,
            ui_frame_interval: Duration::from_secs_f64(1.0 / 60.0),
            proxy,
            embedded,
            generation: 1,
            audio_restore: None,
        }
    }

    pub(crate) fn closed(&self) -> bool {
        self.close_requested
    }

    pub(crate) fn error(&self) -> Option<String> {
        self.fatal_error.clone()
    }

    pub(crate) fn owns(&self, id: WindowId) -> bool {
        self.window.as_ref().is_some_and(|window| window.id() == id)
    }

    pub(crate) fn focus(&self) {
        if let Some(window) = &self.window {
            window.set_minimized(false);
            window.set_visible(true);
            window.focus_window();
        }
    }

    fn exit(&mut self, event_loop: &ActiveEventLoop) {
        self.close_requested = true;
        self.stage = Stage::Finished;
        if !self.embedded {
            event_loop.exit();
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: &anyhow::Error) {
        self.fatal_error = Some(format!("{error:#}"));
        self.exit(event_loop);
    }

    fn create(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.window.is_some() {
            return Ok(());
        }
        let window = Arc::new(
            event_loop
                .create_window(self.attributes.clone())
                .context("create player window")?,
        );
        crate::ui::branding::set_taskbar_icon(&window);
        configure_dwm_window(&window);
        let context = egui::Context::default();
        context.set_embed_viewports(true);
        crate::ui::fonts::install(&context);
        configure_viewer_visuals(&context);
        let window_id = window.id();
        let proxy = self.proxy.clone();
        let generation = self.generation;
        context.set_request_repaint_callback(move |info| {
            if info.viewport_id == egui::ViewportId::ROOT
                && let Some(when) = Instant::now().checked_add(info.delay)
            {
                let _ = proxy.send_event(UiEvent::Repaint(UiRepaintEvent {
                    window: window_id,
                    generation,
                    pass: info.current_cumulative_pass_nr,
                    when,
                }));
            }
        });
        let input = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        );
        let graphics = crate::ui::gfx::create_device()?;
        let presenter = UiPresenter::new(window.clone(), &graphics)?;
        self.ui_frame_interval = ui_frame_interval(&window);
        tracing::debug!(
            interval_ms = self.ui_frame_interval.as_secs_f64() * 1000.0,
            scale = window.scale_factor(),
            "player window created"
        );
        self.shell = Some(Shell {
            context,
            input,
            presenter,
            move_state: WindowMoveState::default(),
            resize_state: WindowResizeState::default(),
            timing_audit: None,
        });
        // Only the D3D11 path can hand a decoder its own surfaces; a Linux
        // session always decodes into CPU frames, so the handle stays empty.
        if let Some(sender) = self.display_sender.take() {
            let _ = sender.send(ViewerDisplayHandle::default());
        }
        if let Some(receiver) = self.progress.take() {
            self.stage = Stage::Connecting(Box::new(ConnectionProgressApp::new(
                self.alias.clone(),
                receiver,
            )));
        }
        self.window = Some(window);
        Ok(())
    }

    /// Session hand-offs arrive on a channel: play, close, or reconnect.
    fn poll_session(&mut self, event_loop: &ActiveEventLoop) {
        loop {
            let event = match self.session.try_recv() {
                Ok(event) => event,
                Err(std_mpsc::TryRecvError::Empty) => return,
                Err(std_mpsc::TryRecvError::Disconnected) => return,
            };
            match event {
                ViewerWindowEvent::Close => {
                    self.exit(event_loop);
                    return;
                }
                ViewerWindowEvent::Listening(audio) => {
                    let Some(window) = self.window.clone() else {
                        return;
                    };
                    self.audio_restore
                        .get_or_insert_with(|| (window.inner_size(), window.is_maximized()));
                    configure_audio_window(&window, false);
                    self.stage = Stage::Listening(Box::new(audio));
                    window.set_visible(true);
                    self.schedule_now();
                }
                ViewerWindowEvent::Playing(session) => {
                    let Some(window) = self.window.clone() else {
                        return;
                    };
                    self.restore_video_window(&window);
                    match Player::new(*session, &window, self.proxy.clone()) {
                        Ok(player) => {
                            window.set_visible(true);
                            self.stage = Stage::Playing(Box::new(player));
                        }
                        Err(error) => {
                            self.fail(event_loop, &error);
                            return;
                        }
                    }
                }
                ViewerWindowEvent::Reconnect {
                    alias,
                    window: _,
                    progress,
                    display,
                } => {
                    self.alias = alias.clone();
                    self.generation += 1;
                    if let Some(window) = self.window.clone() {
                        self.restore_video_window(&window);
                        window.set_title(&format!("{}{}", crate::VIEWER_TITLE_PREFIX, alias));
                    }
                    let _ = display.send(ViewerDisplayHandle::default());
                    if let Some(shell) = self.shell.as_mut() {
                        shell.presenter.clear_video();
                    }
                    self.stage =
                        Stage::Connecting(Box::new(ConnectionProgressApp::new(alias, progress)));
                }
            }
        }
    }

    /// Undo the fixed listening layout before video or a reconnect takes over.
    fn restore_video_window(&mut self, window: &Window) {
        if let Some((size, maximized)) = self.audio_restore.take() {
            restore_video_window_constraints(window);
            let _ = window.request_inner_size(size);
            window.set_maximized(maximized);
        }
    }

    /// Pace ordinary UI repaints at the display refresh rate.
    fn schedule(&mut self, when: Instant) {
        let when = self
            .last_frame
            .map_or(when, |last| when.max(last + self.ui_frame_interval));
        self.next_repaint = Some(self.next_repaint.map_or(when, |old| old.min(when)));
    }

    /// A decoded frame is already late; waiting for the next paced tick costs
    /// it most of a refresh interval on the way to the screen.
    fn schedule_now(&mut self) {
        self.next_repaint = Some(Instant::now());
    }

    fn render(&mut self) -> Result<()> {
        let (Some(window), Some(shell)) = (self.window.clone(), self.shell.as_mut()) else {
            return Ok(());
        };
        self.next_repaint = None;
        let started = Instant::now();
        self.last_frame = Some(started);
        let title = self.alias.clone();
        let mut close_requested = false;
        let mut placement = None;
        let input = shell.input.take_egui_input(&window);
        let stage = &mut self.stage;
        let output = shell.context.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            let resizable = !matches!(stage, Stage::Listening(_));
            resize_regions(ui, &window, |response, direction| {
                if !resizable {
                    return;
                }
                update_nonmodal_window_resize(
                    &ctx,
                    &window,
                    response,
                    direction,
                    &mut shell.resize_state,
                    None,
                );
            });
            if window.fullscreen().is_none() {
                title_bar_panel(ui, "player-window-chrome", title_bar_height(), |ui| {
                    if let Stage::Playing(player) = stage {
                        close_requested |= player.title_bar(ui, &window, &mut shell.move_state);
                    } else {
                        let listening = matches!(stage, Stage::Listening(_));
                        close_requested |= window_title_bar(
                            ui,
                            &window,
                            &title,
                            Some(&mut shell.move_state),
                            !listening,
                        );
                    }
                });
            }
            match stage {
                Stage::Connecting(app) => app.draw(ui),
                Stage::Listening(audio) => close_requested |= audio.show(ui),
                Stage::Playing(player) => placement = player.draw(ui, &window),
                Stage::Finished => {}
            }
            crate::ui::controls::show_notices(ui.ctx());
        });
        // A minimised window never submits, and `write_texture` only reaches the
        // GPU with the next submission, so uploading here would strand the
        // staging copy inside the device -- one video frame's worth, at the
        // video's frame rate, until an allocation finally fails. The frame is
        // still taken, to keep the decoder's queue from growing instead.
        if let Stage::Listening(audio) = &mut self.stage
            && let Some(compact) = audio.take_layout_change()
        {
            configure_audio_window(&window, compact);
        }
        if let Stage::Playing(player) = &mut self.stage
            && std::mem::take(&mut player.video_reset)
        {
            shell.presenter.clear_video();
        }
        let drawing = window.is_minimized() != Some(true);
        if let Stage::Playing(player) = &mut self.stage
            && let Some(frame) = player.take_frame()
        {
            let upload = Instant::now();
            if drawing {
                match &frame.surface {
                    RenderSurface::CpuNv12 { data, color } => shell.presenter.upload_video_nv12(
                        frame.width,
                        frame.height,
                        data,
                        color.transform(8),
                    )?,
                    RenderSurface::CpuRgba8(pixels) => shell.presenter.upload_video(
                        frame.width,
                        frame.height,
                        bytemuck::cast_slice(pixels),
                    )?,
                    // The D3D11 surface type is uninhabited off Windows.
                    RenderSurface::D3D11(surface) => match *surface {},
                }
            }
            player.publish(&frame, upload.elapsed());
            player.current = Some(frame);
        }
        let placement = placement.filter(|_| shell.presenter.has_video());
        shell.presenter.set_video_placement(placement);
        let (ui_output, platform, viewports) = split_output(output);
        shell.input.handle_platform_output(&window, platform);
        close_requested |= viewports
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|viewport| {
                viewport
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::ViewportCommand::Close))
            });
        let repaint_at = viewports
            .get(&egui::ViewportId::ROOT)
            .and_then(|viewport| Instant::now().checked_add(viewport.repaint_delay));
        let layout = started.elapsed();
        let mut presented = false;
        if !drawing {
            shell.presenter.defer_output(ui_output);
        } else {
            presented = shell.presenter.render(&shell.context, ui_output, false)?;
            if presented && window.is_visible() == Some(false) {
                window.set_visible(true);
            }
        }
        if let Some(audit) = UiTimingAudit::active(&mut shell.timing_audit, started) {
            audit.record(
                started,
                layout,
                started.elapsed().saturating_sub(layout),
                repaint_at.is_some_and(|when| when <= started),
                presented,
            );
        }
        self.close_requested |= close_requested;
        if let Some(when) = repaint_at {
            self.schedule(when);
        }
        Ok(())
    }
}

impl ApplicationHandler<UiEvent> for ConnectingWindowsRunner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.create(event_loop) {
            self.fail(event_loop, &error);
            return;
        }
        self.poll_session(event_loop);
        if let Err(error) = self.render() {
            self.fail(event_loop, &error);
            return;
        }
        if let Some(window) = &self.window {
            window.set_visible(true);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if !self.owns(id) {
            return;
        }
        let (Some(window), Some(shell)) = (self.window.clone(), self.shell.as_mut()) else {
            return;
        };
        let response = shell.input.on_window_event(&window, &event);
        let consumed = response.consumed;
        if let Stage::Playing(player) = &mut self.stage {
            player.on_window_event(&window, &shell.context, &event, consumed);
        }
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.exit(event_loop);
                return;
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    cancel_pointer_operation(&mut shell.move_state, &mut shell.resize_state);
                }
                if let Stage::Playing(player) = &mut self.stage {
                    player.on_focus_changed(&window, focused);
                }
                crate::application::viewer_shortcuts::set_focused_window(if focused {
                    u64::from(id)
                } else {
                    0
                });
                self.schedule(Instant::now());
            }
            WindowEvent::Resized(size) => {
                if let Err(error) = shell.presenter.resize(size) {
                    self.fail(event_loop, &error);
                    return;
                }
                self.schedule(Instant::now());
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.render() {
                    self.fail(event_loop, &error);
                    return;
                }
            }
            _ => {
                if response.repaint {
                    self.schedule(Instant::now());
                }
            }
        }
        if self.close_requested {
            self.exit(event_loop);
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device: DeviceId,
        event: DeviceEvent,
    ) {
        // Raw motion is the only pointer source once the pointer is locked.
        if let Stage::Playing(player) = &mut self.stage
            && player.on_device_event(&event)
        {
            self.schedule(Instant::now());
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UiEvent) {
        let UiEvent::Repaint(event) = event else {
            return;
        };
        if !self.owns(event.window) {
            return;
        }
        self.schedule(event.when);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.poll_session(event_loop);
        if self.close_requested {
            self.exit(event_loop);
            return;
        }
        if let Stage::Playing(player) = &mut self.stage {
            match player.poll() {
                Ok(true) => self.schedule_now(),
                // Nothing decoded: egui asks for the repaints it needs and the
                // wake bridge covers the next frame, so the loop can sleep.
                // Repainting the whole UI at the display rate instead would
                // leave an arriving frame waiting for the pass in progress.
                Ok(false) => {}
                Err(error) => {
                    self.fail(event_loop, &error);
                    return;
                }
            }
        }
        match self.next_repaint {
            Some(when) if when <= Instant::now() => {
                if let Err(error) = self.render() {
                    self.fail(event_loop, &error);
                    return;
                }
            }
            _ => {}
        }
        event_loop.set_control_flow(
            self.next_repaint
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.stage = Stage::Finished;
        self.shell = None;
        self.window = None;
    }
}

/// The playing half of the window: video, input forwarding and the menus.
struct Player {
    /// The screen this window shows; replaced when a screen tab is picked.
    session: Arc<NativeViewerSession>,
    /// Opens the other screens of this connection and owns their decoders.
    screens: Option<ScreenPlayback>,
    tabs: ScreenTabBar,
    catalog: Vec<RemoteScreen>,
    selected: Option<i32>,
    pending: Option<PendingScreen>,
    /// The last screen that failed to open, as (id, track, type), so the
    /// catalog refresh does not retry it in a loop.
    failed_screen: Option<(i32, i32, i32)>,
    next_refresh: Instant,
    topology_sequence: Option<i64>,
    topology_focused: Option<i32>,
    /// Set after a screen switch; the shell drops the old screen's texture.
    video_reset: bool,
    annotation: super::annotation::AnnotationUi,
    display_transition_visible: bool,
    /// Local UI owns the picture (annotation, display change, menus).
    input_blocked: bool,
    proxy: EventLoopProxy<UiEvent>,
    window_id: WindowId,
    /// Keeps the decoder publishing frames and nudges the event loop; held for
    /// its lifetime, which is what registers this window with the session.
    #[allow(dead_code, reason = "Owned for its registration and its Drop.")]
    wake: FrameWakeBridge,
    /// Identifies this window to `RemoteInput`, which arbitrates between windows.
    owner: u64,
    stream_control_ui: StreamControlUi,
    performance_mode: PerformancePanelMode,
    intercept_shortcuts: bool,
    current: Option<DecodedVideoFrame>,
    video_size: Option<(u32, u32, u16)>,
    /// Content area of the last frame, used to map pointer positions.
    video_rect: Option<egui::Rect>,
    pointer_in_video: bool,
    last_position: Option<PhysicalPosition<f64>>,
    modifiers: winit::keyboard::ModifiersState,
    held_keys: Vec<u16>,
    /// Everything physically down right now, whether or not it was forwarded.
    /// Entering control waits for this to empty before the remote accepts input.
    physical_keys: Vec<u16>,
    physical_buttons: u8,
    /// Pixels per point of the last frame, for pointer mapping.
    scale: f32,
    /// Last failure from a control request, shown next to the toolbar.
    control_error: Option<String>,
    /// Files dropped onto the window; winit reports one event per file.
    pending_drop: Vec<std::path::PathBuf>,
    /// The remote cursor shape, decoded once per distinct image.
    cursor: Option<(usize, egui::TextureHandle, [u32; 2], [u32; 2])>,
    /// Whether the pointer is currently locked to the window for raw motion.
    pointer_locked: bool,
    /// The compositor only lets a focused window hold the pointer.
    focused: bool,
    /// Sub-pixel motion carried between raw events.
    motion_remainder: [f64; 2],
    /// Throttles the pointer diagnostics, which stay off unless
    /// `openuuyc::viewer::input` is enabled at debug.
    diagnostics_at: [Option<Instant>; 4],
    /// When the remote first said its pointer was hidden, for [`CURSOR_HIDE_GRACE`].
    hidden_since: Option<Instant>,
}

impl Player {
    fn new(
        mut session: NativeViewerSession,
        window: &Arc<Window>,
        proxy: EventLoopProxy<UiEvent>,
    ) -> Result<Self> {
        session.ensure_running()?;
        let owner = u64::from(window.id());
        let screens = session.take_screen_playback();
        let session = Arc::new(session);
        if let Some(factory) = &screens {
            factory.register(Arc::clone(&session));
        }
        let wake = FrameWakeBridge::install(&session, proxy.clone(), window.id())?;
        let performance_mode = session.stream_control.device_preferences().performance_mode;
        let annotation =
            super::annotation::AnnotationUi::new(session.stream_control.clone(), owner);
        let selected = Some(session.screen_id());
        Ok(Self {
            catalog: screens.as_ref().map(ScreenPlayback::screens).unwrap_or_default(),
            screens,
            tabs: ScreenTabBar::default(),
            selected,
            pending: None,
            failed_screen: None,
            next_refresh: Instant::now(),
            topology_sequence: None,
            topology_focused: None,
            video_reset: false,
            annotation,
            display_transition_visible: false,
            input_blocked: false,
            proxy,
            window_id: window.id(),
            session,
            wake,
            owner,
            stream_control_ui: StreamControlUi::default(),
            performance_mode,
            intercept_shortcuts: true,
            current: None,
            video_size: None,
            video_rect: None,
            pointer_in_video: false,
            last_position: None,
            modifiers: winit::keyboard::ModifiersState::empty(),
            held_keys: Vec::new(),
            physical_keys: Vec::new(),
            physical_buttons: 0,
            scale: window.scale_factor() as f32,
            control_error: None,
            pending_drop: Vec::new(),
            cursor: None,
            pointer_locked: false,
            focused: true,
            motion_remainder: [0.0, 0.0],
            diagnostics_at: [None; 4],
            hidden_since: None,
        })
    }

    fn control(&self) -> &crate::features::stream_control::StreamControlHandle {
        &self.session.stream_control
    }

    fn input(&self) -> &crate::features::remote_input::RemoteInput {
        self.session.stream_control.mouse()
    }

    /// True when a decoded frame is waiting to go up.
    fn poll(&mut self) -> Result<bool> {
        self.confirm_neutral();
        self.update_screens();
        self.session.ensure_running()?;
        Ok(mutex_lock(&self.session.frame_queue).is_some())
    }

    /// Take the newest decoded frame, dropping anything the UI cadence skipped.
    fn take_frame(&mut self) -> Option<DecodedVideoFrame> {
        let mut queue = mutex_lock(&self.session.frame_queue);
        let mut frame = take_next_frame(&mut queue, &self.session.performance)?;
        while let Some(newer) = take_next_frame(&mut queue, &self.session.performance) {
            self.session.performance.record_dropped_present_frame();
            frame = newer;
        }
        drop(queue);
        // Windows Video Render unparks manager_wake after every dequeue.
        // DECODER_ADMISSION_TOKENS=1: the decode worker parks while the
        // presentation queue is non-empty. Without this unpark, Linux keeps
        // showing the first frame forever (lock UI clock/dots freeze) even
        // though RTP/FEC keep arriving — exactly the SecureDesktop soak.
        self.session.manager_wake.unpark();
        Some(frame)
    }

    fn publish(&mut self, frame: &DecodedVideoFrame, transfer: Duration) {
        self.video_size = Some((frame.width, frame.height, frame.rotation));
        self.session
            .performance
            .record_rendered_frame(crate::diagnostics::performance::RenderedFrameTiming {
                is_new_picture: frame.is_new_picture,
                width: frame.width,
                height: frame.height,
                decoded_at: frame.decoded_at,
                local: frame.received_at.elapsed(),
                assembly: frame.assembly_delay,
                input_queue: frame.input_queue_delay,
                decode_pipeline: frame.decode_pipeline_delay,
                surface_transfer: transfer,
                present_wait: Duration::ZERO,
                render_queue: frame.decoded_at.elapsed().saturating_sub(transfer),
                sender_capture_at: frame.sender_timing.capture_at,
                sender_capture: frame.sender_timing.capture_delay,
                sender_encode: frame.sender_timing.encode_delay,
                sender_pacer: frame.sender_timing.pacer_delay,
                sender_total: frame.sender_timing.sending_delay,
                transport: frame.sender_timing.transport_delay,
            });
    }

    /// Rotation swaps the aspect the video should be fitted to.
    fn display_size(&self) -> Option<(u32, u32)> {
        let (width, height, rotation) = self.video_size?;
        Some(if matches!(rotation % 360, 90 | 270) {
            (height, width)
        } else {
            (width, height)
        })
    }

    fn draw(&mut self, ui: &mut egui::Ui, window: &Arc<Window>) -> Option<VideoPlacement> {
        // The title bar is a panel above this ui; max_rect would include it and
        // push the first rows of the picture underneath it.
        let content = ui.available_rect_before_wrap();
        self.video_rect = None;
        let scale = ui.ctx().pixels_per_point();
        self.scale = scale;
        let placement = self.display_size().map(|(width, height)| {
            let available = content.size();
            let video_aspect = (width as f32).max(1.0) / (height as f32).max(1.0);
            let area_aspect = available.x / available.y.max(1.0);
            let size = if video_aspect > area_aspect {
                egui::vec2(available.x, available.x / video_aspect)
            } else {
                egui::vec2(available.y * video_aspect, available.y)
            };
            let rect = egui::Rect::from_center_size(content.center(), size);
            self.video_rect = Some(rect);
            VideoPlacement {
                x: rect.min.x * scale,
                y: rect.min.y * scale,
                width: rect.width() * scale,
                height: rect.height() * scale,
                rotation: self.video_size.map_or(0, |size| size.2),
            }
        });
        if placement.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("正在等待画面…")
                        .color(crate::ui::theme::MUTED)
                        .size(crate::ui::theme::BODY),
                );
            });
        }
        self.flush_drop();
        let ctx = ui.ctx().clone();
        let screen_id = self.session.screen_id();
        self.display_transition_visible =
            super::display_transition::show(&ctx, &self.session.stream_control, screen_id, content);
        let mut view = super::stream_menu::LocalViewSettings {
            performance_mode: self.performance_mode,
            performance_always_visible: self
                .session
                .stream_control
                .device_preferences()
                .performance_always_visible,
            intercept_shortcuts: self.intercept_shortcuts,
            send_ctrl_alt_del: false,
        };
        let display_size = self.display_size();
        show_stream_control_window(
            ui.ctx(),
            &self.session.stream_control,
            &mut self.stream_control_ui,
            &mut view,
            screen_id,
            display_size,
        );
        if self.performance_mode != view.performance_mode {
            self.session
                .stream_control
                .set_performance_mode(view.performance_mode);
            self.performance_mode = view.performance_mode;
        }
        self.session
            .stream_control
            .set_performance_always_visible(view.performance_always_visible);
        self.intercept_shortcuts = view.intercept_shortcuts;
        if view.send_ctrl_alt_del {
            let _ = self
                .session
                .stream_control
                .mouse()
                .send_ctrl_alt_del(self.owner);
        }
        // Health alerts always show; the panel follows "性能监控常驻".
        super::show_performance_overlay(
            ui.ctx(),
            &self.session.performance,
            &self.session.stream_control.audio(),
            view.performance_mode,
            view.performance_always_visible,
            self.video_rect.unwrap_or(content),
            "linux-player",
        );
        let control = self.session.stream_control.clone();
        let display_blocked =
            control.display_input_blocked(self.session.track_index, self.session.screen_binding());
        self.annotation.draw(
            ui,
            screen_id,
            self.video_rect,
            self.display_transition_visible
                || self.tabs.is_pending()
                || self.stream_control_ui.open
                || display_blocked,
            self.focused,
        );
        if let Some(upgrade) = control.remote_upgrade() {
            upgrade.show(&ctx, window.id(), &control);
        }
        crate::ui::controls::observe_notice(
            &ctx,
            "linux-player-error",
            "播放器",
            crate::ui::controls::DialogIcon::Warning,
            self.control_error.as_deref(),
        );
        if self.focused && !self.stream_control_ui.open {
            super::polling_warning::show(&ctx, control.mouse(), content);
        }
        // Local UI that covers the picture owns the input, as on Windows.
        let blocked = control.annotation_snapshot().enabled
            || control
                .remote_upgrade()
                .is_some_and(|upgrade| upgrade.owns_input(window.id()))
            || self.display_transition_visible
            || super::stream_menu::topology_menu::owns_input(&ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some())
            || self.tabs.is_pending()
            || display_blocked
            || control.pending_display_resolution(screen_id).is_some();
        if blocked && !self.input_blocked {
            self.release_keys();
            self.input().pause_owner(self.owner);
        }
        self.input_blocked = blocked;
        if self.focused
            && !blocked
            && !self.stream_control_ui.open
            && !ctx.any_popup_open()
            && !ctx.text_edit_focused()
            && self.current.is_some()
            && let Err(error) = control.restore_input_control()
        {
            control.mouse().fail(error.to_string());
        }
        self.update_pointer_lock(window);
        self.draw_remote_cursor(ui, window);
        placement
    }

    /// Paint the remote pointer over the video and hide the local one.
    ///
    /// In absolute mode the local pointer already stands for the remote one, so
    /// the shape is drawn where the local pointer is rather than at the position
    /// the remote sampled with it.
    fn draw_remote_cursor(&mut self, ui: &mut egui::Ui, window: &Arc<Window>) {
        if self.input_blocked {
            window.set_cursor_visible(true);
            self.cursor_note("local UI owns the picture");
            return;
        }
        if self.pointer_locked {
            // The remote draws its own pointer into the video in this mode.
            self.cursor_note("locked");
            return;
        }
        let controlling = self.session.stream_control.mouse().mode() != MouseMode::View;
        // An ordinary Windows desktop flaps this flag: it hides the pointer for
        // a keystroke and shows it on the next mouse move, several times a
        // second while someone types. Taking each report at face value makes
        // the pointer blink between the remote shape and the local arrow, so a
        // hide only counts once it has lasted.
        let hidden = match (
            self.session.stream_control.remote_cursor_hidden(),
            self.hidden_since,
        ) {
            (false, _) => {
                self.hidden_since = None;
                false
            }
            (true, Some(since)) => since.elapsed() >= CURSOR_HIDE_GRACE,
            (true, None) => {
                self.hidden_since = Some(Instant::now());
                false
            }
        };
        let pointer = ui.ctx().pointer_latest_pos();
        let Some((rect, position)) = self.video_rect.zip(pointer) else {
            window.set_cursor_visible(true);
            self.cursor_note("no rect or pointer");
            return;
        };
        // Over a panel, menu or the caption the local pointer is the one that
        // matters, and the remote shape would be painted underneath them.
        if !controlling || !self.hit_video_at(ui.ctx(), position) {
            window.set_cursor_visible(true);
            self.cursor_note("not over video");
            return;
        }
        // Exactly one pointer belongs over the picture. The host draws its own
        // into the frames while it is capturing the cursor, so a second one
        // painted here would double it. While it is not, this client owns the
        // job, and hiding the local pointer before there is a shape to put in
        // its place just leaves the picture with nothing to aim with -- which
        // is what a remote Windows desktop causes every time it hides its
        // pointer for someone typing.
        if self.session.stream_control.remote_cursor_captured() {
            window.set_cursor_visible(false);
            self.cursor_note("host draws it");
            return;
        }
        if hidden {
            window.set_cursor_visible(true);
            self.cursor = None;
            self.cursor_note("remote hid it");
            return;
        }
        // A hide that has not lasted long enough leaves no shape behind: the
        // report that carried it also cleared the one before it. The texture
        // decoded earlier carries the pointer across that gap.
        if let Some(cursor) = self.session.stream_control.remote_cursor() {
            let identity = Arc::as_ptr(&cursor.image) as usize;
            if self.cursor.as_ref().is_none_or(|(id, ..)| *id != identity) {
                match decode_cursor(ui.ctx(), &cursor.image) {
                    Some(entry) => self.cursor = Some((identity, entry.0, entry.1, entry.2)),
                    None => {
                        window.set_cursor_visible(true);
                        self.cursor = None;
                        self.cursor_note("undecodable");
                        return;
                    }
                }
            }
        }
        let Some((_, texture, size, hotspot)) = &self.cursor else {
            window.set_cursor_visible(true);
            self.cursor_note("no texture");
            return;
        };
        window.set_cursor_visible(false);
        // The cursor is authored in remote pixels; scale it with the video.
        let scale = self
            .session
            .stream_control
            .mouse_screen(self.session.track_index)
            .map_or(1.0, |(_, remote_width, _)| {
                rect.width() / (remote_width.max(1) as f32)
            });
        let size = egui::vec2(size[0] as f32 * scale, size[1] as f32 * scale);
        let origin = position - egui::vec2(hotspot[0] as f32 * scale, hotspot[1] as f32 * scale);
        // Painted straight into a foreground layer rather than through an
        // `Area`: an area restarts its fade-in whenever it skips a pass, and
        // this one skips every pass the pointer spends on a panel or the
        // caption, which left the shape at a fraction of its opacity. A bare
        // layer also registers no `AreaState`, so it cannot shadow the video in
        // `hit_video_at`.
        ui.ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("remote-cursor"),
            ))
            .image(
                texture.id(),
                egui::Rect::from_min_size(origin, size),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        self.cursor_note("painted");
    }

    /// The player caption (the Windows player's row); true asks to close.
    fn title_bar(
        &mut self,
        ui: &mut egui::Ui,
        window: &Arc<Window>,
        move_state: &mut WindowMoveState,
    ) -> bool {
        let control = self.session.stream_control.clone();
        let performance = self.session.performance.clone();
        let title = self.session.title.clone();
        let action: PlayerChromeAction = player_title_bar(
            ui,
            PlayerTitleBar {
                screens: &mut self.tabs,
                window,
                title: &title,
                performance: &performance,
                stream_control: &control,
                stream_control_ui: &mut self.stream_control_ui,
                move_state,
            },
        );
        if action.toggle_annotation {
            self.release_keys();
            self.release_pointer(window);
            self.stream_control_ui.open = false;
            let annotation = control.annotation_snapshot();
            if !annotation.supported
                && !annotation.enabled
                && let Some(upgrade) = control.remote_upgrade()
            {
                upgrade.prompt(window.id(), "批注");
            } else {
                self.annotation.toggle();
            }
        }
        if action.toggle_mouse {
            let state = control.snapshot();
            let enable = state.mouse_mode == MouseMode::View && !state.mouse_pending;
            if enable {
                crate::ui::controls::clear_notice(ui.ctx(), "remote-input-error");
            }
            self.set_control(enable);
        }
        if action.one_to_one {
            self.resize_one_to_one(window);
        }
        if action.close {
            self.release_keys();
        }
        action.close
    }

    /// Size the window so one video pixel is one screen pixel, shrinking to
    /// the monitor when it does not fit. Compositors may still adjust it.
    fn resize_one_to_one(&self, window: &Window) {
        let Some((width, height)) = self.display_size() else {
            return;
        };
        let title = crate::ui::chrome::title_bar_height_pixels(window);
        let (max_width, max_height) = window
            .current_monitor()
            .map(|m| (m.size().width.max(1), m.size().height.max(1)))
            .unwrap_or((width, height + title));
        let scale = 1.0_f64
            .min(f64::from(max_width) / f64::from(width.max(1)))
            .min(f64::from(max_height.saturating_sub(title).max(1)) / f64::from(height.max(1)));
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(
            (f64::from(width) * scale).round().max(1.0) as u32,
            (f64::from(height) * scale).round().max(1.0) as u32 + title,
        ));
    }

    fn set_control(&mut self, enabled: bool) {
        let mode = if enabled {
            match self.control().snapshot().mouse_preference {
                MouseMode::View => MouseMode::Smart,
                preference => preference,
            }
        } else {
            MouseMode::View
        };
        if let Err(error) = self.control().set_mouse_mode(mode) {
            self.control_error = Some(format!("{error:#}"));
        }
        if !enabled {
            self.release_keys();
        }
    }

    fn release_pointer(&mut self, window: &Arc<Window>) {
        if self.pointer_locked {
            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
            window.set_cursor_visible(true);
            self.pointer_locked = false;
        }
    }

    /// Raw pointer motion, used while the remote drives its own cursor.
    fn on_device_event(&mut self, event: &DeviceEvent) -> bool {
        let DeviceEvent::MouseMotion { delta } = event else {
            return false;
        };
        {
            let input = self.input();
            if !self.pointer_locked
                || !input.relative_mode()
                || input.mode() == MouseMode::View
                || input.waiting_for_neutral()
            {
                return false;
            }
        }
        // Whole pixels go out now; the fraction waits for the next event so slow
        // movement is not rounded away.
        let x = delta.0 + self.motion_remainder[0];
        let y = delta.1 + self.motion_remainder[1];
        self.motion_remainder = [x.fract(), y.fract()];
        let (x, y) = (x.trunc(), y.trunc());
        if x == 0.0 && y == 0.0 {
            return false;
        }
        self.input().relative(self.owner, x as i32, y as i32);
        true
    }

    /// Keep the pointer inside the window while the remote owns it, so the
    /// local cursor cannot wander onto another window mid-game.
    fn update_pointer_lock(&mut self, window: &Arc<Window>) {
        let input = self.input();
        let wanted = self.focused
            && !self.input_blocked
            && input.relative_mode()
            && input.mode() != MouseMode::View;
        if wanted == self.pointer_locked {
            return;
        }
        if wanted {
            // Relative motion on Wayland only arrives while the pointer is
            // Locked. Confined keeps CursorMoved alive but blocks absolute()
            // once relative_mode is set — a silent dead zone on Hyprland/Sway.
            // Require Locked; anything less falls back to absolute coordinates.
            match window.set_cursor_grab(winit::window::CursorGrabMode::Locked) {
                Ok(()) => {
                    window.set_cursor_visible(false);
                    self.pointer_locked = true;
                }
                Err(error) => {
                    tracing::warn!(%error, "无法锁定指针，改用绝对坐标");
                    self.deny_relative("当前桌面不允许锁定指针，已改用绝对坐标");
                }
            }
        } else {
            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
            window.set_cursor_visible(true);
            self.pointer_locked = false;
            self.motion_remainder = [0.0, 0.0];
        }
    }

    /// Relative mode without an exclusive grab drifts; force absolute now, not
    /// on the next policy refresh (which may be a long time away).
    fn deny_relative(&mut self, message: &str) {
        self.input().set_relative_available(false);
        self.input().set_relative_mode(false);
        self.control_error = Some(message.to_owned());
        if self.pointer_locked {
            // Caller releases the grab when wanted flips; keep state coherent.
            self.pointer_locked = false;
            self.motion_remainder = [0.0, 0.0];
        }
    }

    /// The remote refuses input until every key and button held when control
    /// was granted has been released; nothing else reports that on Linux.
    fn confirm_neutral(&self) {
        let input = self.input();
        if !input.waiting_for_neutral() {
            return;
        }
        if self.physical_keys.is_empty() && self.physical_buttons == 0 {
            input.confirm_neutral(input.activation_generation());
        }
    }

    fn release_keys(&mut self) {
        for key in std::mem::take(&mut self.held_keys) {
            self.input().key(self.owner, key, false, None);
        }
    }

    fn on_focus_changed(&mut self, window: &Arc<Window>, focused: bool) {
        self.focused = focused;
        if !focused {
            self.annotation.finish();
            self.release_pointer(window);
            // A key released while another window has focus never reaches us.
            self.release_keys();
            self.physical_keys.clear();
            self.physical_buttons = 0;
            self.modifiers = winit::keyboard::ModifiersState::empty();
            self.input().pause_owner(self.owner);
        }
    }

    /// Returns true when the event was consumed by remote input.
    fn on_window_event(
        &mut self,
        window: &Arc<Window>,
        context: &egui::Context,
        event: &WindowEvent,
        consumed_by_ui: bool,
    ) -> bool {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                false
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.repeat {
                    return false;
                }
                let down = event.state == ElementState::Pressed;
                if let Some(key) = crate::application::viewer_shortcuts::physical_key(event.physical_key) {
                    if down {
                        if !self.physical_keys.contains(&key) {
                            self.physical_keys.push(key);
                        }
                    } else {
                        self.physical_keys.retain(|held| *held != key);
                    }
                    self.confirm_neutral();
                }
                if down && let Some(action) = viewer_shortcut(self.modifiers, event.physical_key) {
                    self.run_shortcut(action, window);
                    return true;
                }
                // Caption buttons leave egui focus behind; that must not swallow
                // every key destined for the remote Windows desktop. Only a real
                // text field should keep keyboard events local.
                let ui_owns_keys = context.text_edit_focused();
                if ui_owns_keys || self.input_blocked || self.input().mode() == MouseMode::View {
                    return false;
                }
                let Some(key) = crate::application::viewer_shortcuts::physical_key(event.physical_key) else {
                    return false;
                };
                if down {
                    if !self.held_keys.contains(&key) {
                        self.held_keys.push(key);
                    }
                } else {
                    self.held_keys.retain(|held| *held != key);
                }
                self.input().key(self.owner, key, down, None)
            }
            WindowEvent::HoveredFile(_) | WindowEvent::HoveredFileCancelled => false,
            WindowEvent::DroppedFile(path) => {
                self.pending_drop.push(path.clone());
                window.request_redraw();
                true
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.last_position = Some(*position);
                self.pointer_in_video = self.hit_video(context, *position);
                self.send_position(*position);
                false
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer_in_video = false;
                self.last_position = None;
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(bit) = mouse_button(*button).map(button_bit) {
                    if *state == ElementState::Pressed {
                        self.physical_buttons |= bit;
                    } else {
                        self.physical_buttons &= !bit;
                    }
                    self.confirm_neutral();
                }
                tracing::info!(target: "openuuyc::viewer::input",
                    ?button, pressed = *state == ElementState::Pressed,
                    consumed_by_ui, in_video = self.pointer_in_video,
                    mode = ?self.input().mode(),
                    relative = self.input().relative_mode(),
                    neutral_wait = self.input().waiting_for_neutral(),
                    ready = self.input().transport_ready(),
                    platform = self.input().keyboard_platform_code(),
                    owner = self.owner,
                    "mouse button");
                // Over the picture, forward clicks even when egui reports the
                // event as consumed (hovering a previously focused caption
                // control). Still yield while egui is mid-drag on a widget.
                let ui_drag = context.egui_is_using_pointer();
                if self.input_blocked || self.input().mode() == MouseMode::View {
                    return false;
                }
                if (consumed_by_ui && (ui_drag || !self.pointer_in_video))
                    || (!self.pointer_in_video && *state == ElementState::Pressed)
                {
                    return false;
                }
                let Some(code) = mouse_button(*button) else {
                    return false;
                };
                let pressed = *state == ElementState::Pressed;
                self.input().button(self.owner, code, pressed);
                tracing::info!(
                    target: "openuuyc::viewer::input",
                    code,
                    pressed,
                    ready = self.input().transport_ready(),
                    mode = ?self.input().mode(),
                    "mouse button forwarded to RemoteInput"
                );
                true
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if consumed_by_ui || self.input_blocked || !self.pointer_in_video {
                    return false;
                }
                let (horizontal, amount) = match delta {
                    // One notch is 120 units on the wire, as Windows reports it.
                    MouseScrollDelta::LineDelta(x, y) => {
                        if x.abs() > y.abs() {
                            (true, *x * 120.0)
                        } else {
                            (false, *y * 120.0)
                        }
                    }
                    MouseScrollDelta::PixelDelta(position) => {
                        if position.x.abs() > position.y.abs() {
                            (true, position.x as f32 * 2.0)
                        } else {
                            (false, position.y as f32 * 2.0)
                        }
                    }
                };
                let amount = amount.round() as i32;
                if amount == 0 {
                    return false;
                }
                self.input().wheel(self.owner, amount, horizontal);
                true
            }
            _ => false,
        }
    }

    fn run_shortcut(&mut self, action: ViewerShortcut, window: &Arc<Window>) {
        match action {
            ViewerShortcut::ReleaseMouse => self.set_control(false),
            ViewerShortcut::Fullscreen => toggle_fullscreen(window),
            ViewerShortcut::Close => {
                self.release_keys();
                window.set_visible(false);
                self.session.close_handle().close();
            }
            ViewerShortcut::Performance => {
                self.performance_mode = self
                    .session
                    .stream_control
                    .device_preferences()
                    .performance_mode
                    .next();
                self.session
                    .stream_control
                    .set_performance_mode(self.performance_mode);
            }
        }
    }

    /// True at most once every half second, so a diagnostic on a per-frame path
    /// does not flood the log. `slot` keeps the callers independent.
    fn diagnose(&mut self, slot: usize) -> bool {
        if !tracing::enabled!(target: "openuuyc::viewer::input", tracing::Level::DEBUG) {
            return false;
        }
        let now = Instant::now();
        if self.diagnostics_at[slot]
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(500))
        {
            return false;
        }
        self.diagnostics_at[slot] = Some(now);
        true
    }

    /// Why the remote pointer is or is not on screen. Every path out of
    /// `draw_remote_cursor` names itself here: the two ways this goes wrong --
    /// no pointer at all, and a pointer that does not aim where it looks -- are
    /// indistinguishable from a screenshot.
    fn cursor_note(&mut self, outcome: &str) {
        if !self.diagnose(0) {
            return;
        }
        tracing::debug!(target: "openuuyc::viewer::input",
            outcome,
            locked = self.pointer_locked,
            relative = self.input().relative_mode(),
            relative_available = self.input().relative_available(),
            mode = ?self.input().mode(),
            hidden = self.session.stream_control.remote_cursor_hidden(),
            hidden_for_ms = self.hidden_since.map(|since| since.elapsed().as_millis()),
            captured = self.session.stream_control.remote_cursor_captured(),
            shape = self.session.stream_control.remote_cursor().is_some(),
            in_video = self.pointer_in_video,
            "remote cursor");
    }

    fn hit_video(&self, context: &egui::Context, position: PhysicalPosition<f64>) -> bool {
        let scale = f64::from(context.pixels_per_point());
        self.hit_video_at(
            context,
            egui::pos2((position.x / scale) as f32, (position.y / scale) as f32),
        )
    }

    /// True when this point lands on the picture and not on any UI layer above it.
    fn hit_video_at(&self, context: &egui::Context, point: egui::Pos2) -> bool {
        self.video_rect.is_some_and(|rect| rect.contains(point))
            && context
                .layer_id_at(point)
                .is_none_or(|layer| layer.order == egui::Order::Background)
    }

    /// Send files dropped on the window through the official clipboard file
    /// drop. X11/Wayland report no drop position to winit, so the last pointer
    /// position over the video is used (the video center otherwise).
    fn flush_drop(&mut self) {
        if self.pending_drop.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut self.pending_drop);
        let Some(rect) = self.video_rect else {
            self.control_error = Some("画面尚未就绪，无法拖放文件".into());
            return;
        };
        let Some((screen, _, _)) = self.control().mouse_screen(self.session.track_index) else {
            self.control_error = Some("当前画面不支持拖放文件".into());
            return;
        };
        let scale = f64::from(self.scale_factor());
        let (x, y) = self
            .last_position
            .filter(|_| self.pointer_in_video)
            .map(|p| {
                (
                    ((p.x / scale - f64::from(rect.min.x)) / f64::from(rect.width())).clamp(0.0, 1.0),
                    ((p.y / scale - f64::from(rect.min.y)) / f64::from(rect.height())).clamp(0.0, 1.0),
                )
            })
            .unwrap_or((0.5, 0.5));
        let point = crate::protocol::drag_drop::Point { screen, x, y };
        match self.session.stream_control.drop_files(paths, point) {
            Ok(_submission) => self.control_error = None,
            Err(error) => self.control_error = Some(format!("{error:#}")),
        }
    }

    fn send_position(&mut self, position: PhysicalPosition<f64>) {
        let input = self.input();
        if self.input_blocked || input.mode() == MouseMode::View || input.waiting_for_neutral() {
            return;
        }
        // Relative mode owns motion only while the pointer is actually locked.
        // Without the grab, absolute() would be dropped and the cursor freezes.
        if input.relative_mode() && self.pointer_locked {
            return;
        }
        let dragging = input.owner_holds_buttons(self.owner);
        if !dragging && !self.pointer_in_video {
            return;
        }
        let Some(rect) = self.video_rect else {
            return;
        };
        let Some((screen, remote_width, remote_height)) =
            self.control().mouse_screen(self.session.track_index)
        else {
            return;
        };
        let local_x = (position.x / f64::from(self.scale_factor())) - f64::from(rect.min.x);
        let local_y = (position.y / f64::from(self.scale_factor())) - f64::from(rect.min.y);
        let width = f64::from(rect.width());
        let height = f64::from(rect.height());
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        // The host multiplies by the screen size and rounds, so the last
        // physical pixel must stay inside the screen.
        let x = (local_x.clamp(0.0, width - 1.0) / width)
            .min(1.0 - 1.0 / f64::from(remote_width.max(1)));
        let y = (local_y.clamp(0.0, height - 1.0) / height)
            .min(1.0 - 1.0 / f64::from(remote_height.max(1)));
        let relative_unlocked = input.relative_mode() && !self.pointer_locked;
        if relative_unlocked {
            input.absolute_without_relative_gate(self.owner, screen, x, y);
        } else {
            input.absolute(self.owner, screen, x, y);
        }
        if self.diagnose(3) {
            tracing::debug!(
                target: "openuuyc::viewer::input",
                screen,
                abs_x = x,
                abs_y = y,
                remote_width,
                remote_height,
                rect = ?rect,
                local_x,
                local_y,
                "absolute mapped inside letterboxed video_rect"
            );
        }
    }

    fn scale_factor(&self) -> f32 {
        self.scale.max(0.1)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.release_keys();
        self.input().disable();
        self.annotation.finish();
        if let Some(switcher) = &self.tabs.device_switch {
            switcher.cancel_takeover(self.window_id);
        }
        self.pending.take();
        self.session
            .stream_control
            .cancel_display_change(self.session.screen_id());
        // Every screen decoder this connection opened goes with the window.
        self.session.close_handle().close();
        if let Some(factory) = &self.screens {
            factory.close_decoders();
        }
    }
}

/// A screen tab being opened in the background.
struct PendingScreen {
    id: i32,
    task: tokio::task::JoinHandle<()>,
    result: tokio::sync::oneshot::Receiver<Result<Arc<NativeViewerSession>>>,
}

impl Drop for PendingScreen {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Player {
    /// Screen tabs: finish a pending switch, run the picked tab, follow the
    /// remote topology, and publish the tab row.
    fn update_screens(&mut self) {
        if self.screens.is_none() {
            return;
        }
        let ready = self
            .pending
            .as_mut()
            .and_then(|pending| match pending.result.try_recv() {
                Ok(result) => Some(result),
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    Some(Err(anyhow!("屏幕加载已取消")))
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
            });
        if let Some(result) = ready {
            let pending = self.pending.take().expect("pending screen");
            match result {
                Ok(session) => self.finish_screen(pending.id, session),
                Err(error) => {
                    self.failed_screen = Some(
                        self.catalog
                            .iter()
                            .find(|s| s.id == pending.id)
                            .map_or((pending.id, -1, -1), |s| {
                                (s.id, s.video_track_index, s.display.screen_type)
                            }),
                    );
                    self.tabs.error = Some(format!("{error:#}"));
                }
            }
        }
        if let Some(screen) = self.tabs.command.take()
            && let Err(error) = self.begin_screen(screen)
        {
            self.tabs.error = Some(format!("{error:#}"));
        }
        let now = Instant::now();
        if now >= self.next_refresh {
            self.refresh_catalog();
            self.next_refresh = now + Duration::from_millis(250);
        }
        self.sync_tabs();
    }

    fn finish_screen(&mut self, id: i32, session: Arc<NativeViewerSession>) {
        let Some(factory) = &self.screens else {
            return;
        };
        let current = factory.screens().iter().any(|s| {
            s.id == id
                && s.video_track_index == session.track_index
                && session.screen_binding()
                    == (s.id as u32 as u64 | ((s.display.screen_type as u32 as u64) << 32))
        });
        if !current {
            // The mapping moved while the screen opened; the refresh retries.
            self.next_refresh = Instant::now();
            return;
        }
        if !Arc::ptr_eq(&session, &self.session) {
            match FrameWakeBridge::install(&session, self.proxy.clone(), self.window_id) {
                // The old bridge releases the previous screen's registration.
                Ok(wake) => self.wake = wake,
                Err(error) => {
                    self.tabs.error = Some(format!("{error:#}"));
                    return;
                }
            }
            self.session = Arc::clone(&session);
        }
        self.annotation.finish();
        self.annotation.bind(session.stream_control.clone());
        self.current = None;
        self.video_size = None;
        self.video_rect = None;
        self.cursor = None;
        self.video_reset = true;
        self.selected = Some(id);
        self.failed_screen = None;
        factory.focus(id);
        tracing::debug!(screen_id = id, "screen switch bound");
    }

    fn begin_screen(&mut self, screen_id: i32) -> Result<()> {
        let factory = self.screens.clone().context("当前连接不支持切换显示器")?;
        self.catalog
            .iter()
            .find(|screen| screen.id == screen_id)
            .context("显示器已断开")?;
        self.failed_screen = None;
        if self.selected == Some(screen_id) {
            self.pending.take();
            return Ok(());
        }
        if self.pending.as_ref().is_some_and(|p| p.id == screen_id) {
            return Ok(());
        }
        let cancelled = self.pending.take().map(|pending| pending.id);
        self.session
            .stream_control
            .cancel_display_change(self.session.screen_id());
        self.release_keys();
        let proxy = self.proxy.clone();
        let window = self.window_id;
        let (task, result) = factory.open(
            screen_id,
            ViewerDisplayHandle::default(),
            self.selected,
            cancelled,
            move || {
                let _ = proxy.send_event(UiEvent::Repaint(UiRepaintEvent {
                    window,
                    generation: 0,
                    pass: 0,
                    when: Instant::now(),
                }));
            },
        );
        self.pending = Some(PendingScreen {
            id: screen_id,
            task,
            result,
        });
        Ok(())
    }

    /// Follow screens appearing, disappearing or being remapped remotely.
    fn refresh_catalog(&mut self) {
        let Some(factory) = &self.screens else {
            return;
        };
        let topology = factory.topology();
        if topology.sequence != self.topology_sequence {
            self.topology_sequence = topology.sequence;
            self.topology_focused = None;
        }
        let actual = factory.screens();
        self.catalog = actual.clone();
        for screen in &topology.transient_screens {
            if !self.catalog.iter().any(|s| s.id == screen.id) {
                self.catalog.push(screen.clone());
            }
        }
        if self.catalog.is_empty() {
            return;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| !actual.iter().any(|s| s.id == p.id))
        {
            self.pending.take();
        }
        // A screen the remote just created or selected comes to the front.
        if self.topology_focused != topology.selected_screen
            && topology.reported
            && !matches!(
                topology.action,
                Some(crate::features::stream_control::DisplayTopologyAction::Remove { .. })
            )
            && let Some(target) = topology
                .selected_screen
                .filter(|id| actual.iter().any(|s| s.id == *id))
        {
            self.topology_focused = Some(target);
            if self.selected != Some(target)
                && let Err(error) = self.begin_screen(target)
            {
                self.tabs.error = Some(format!("{error:#}"));
            }
            return;
        }
        if self.pending.is_some() {
            return;
        }
        let selected = self
            .selected
            .and_then(|id| actual.iter().find(|s| s.id == id));
        let bound = selected.is_some_and(|screen| {
            self.session.track_index == screen.video_track_index
                && self.session.screen_binding()
                    == (screen.id as u32 as u64 | ((screen.display.screen_type as u32 as u64) << 32))
        });
        if bound {
            return;
        }
        // The shown screen went away or moved to another track.
        let target = selected
            .map(|screen| screen.id)
            .or_else(|| actual.first().map(|screen| screen.id));
        let Some(target) = target else {
            return;
        };
        if actual.iter().any(|s| {
            s.id == target
                && self.failed_screen == Some((s.id, s.video_track_index, s.display.screen_type))
        }) {
            return;
        }
        self.selected = None;
        if let Err(error) = self.begin_screen(target) {
            self.failed_screen = actual
                .iter()
                .find(|s| s.id == target)
                .map(|s| (s.id, s.video_track_index, s.display.screen_type));
            self.tabs.error = Some(error.to_string());
        }
    }

    fn sync_tabs(&mut self) {
        let Some(factory) = &self.screens else {
            return;
        };
        self.tabs.device_switch = factory.device_switch.clone();
        self.tabs.tabs = self
            .catalog
            .iter()
            .map(|screen| (screen.id, screen.label(&self.catalog)))
            .collect();
        self.tabs.selected = self.selected.unwrap_or(-1);
        self.tabs.pending = self.pending.as_ref().map(|pending| pending.id);
        let mut visible: Vec<_> = self
            .selected
            .into_iter()
            .chain(self.pending.as_ref().map(|pending| pending.id))
            .collect();
        visible.sort_unstable();
        visible.dedup();
        factory.set_visible(visible);
    }
}

/// Audio-only connections use a small fixed window, as on Windows.
fn configure_audio_window(window: &Window, compact: bool) {
    window.set_fullscreen(None);
    window.set_maximized(false);
    window.set_resizable(false);
    window.set_enabled_buttons(
        winit::window::WindowButtons::CLOSE | winit::window::WindowButtons::MINIMIZE,
    );
    let [width, height] = if compact {
        crate::ui::theme::AUDIO_COMPACT_SIZE
    } else {
        crate::ui::theme::AUDIO_WINDOW_SIZE
    };
    let size = LogicalSize::new(width, height);
    window.set_min_inner_size(None::<LogicalSize<f32>>);
    window.set_max_inner_size(None::<LogicalSize<f32>>);
    let _ = window.request_inner_size(size);
    window.set_min_inner_size(Some(size));
    window.set_max_inner_size(Some(size));
}

fn restore_video_window_constraints(window: &Window) {
    window.set_max_inner_size(None::<LogicalSize<f32>>);
    window.set_resizable(true);
    window.set_enabled_buttons(winit::window::WindowButtons::all());
    window.set_min_inner_size(Some(LogicalSize::new(760.0, 520.0)));
}

/// The decoder drops every decoded frame unless a render thread is registered
/// with the session, which is how the Windows player signals that its window is
/// showing video. Here the frames are drawn by the UI thread, so this thread
/// holds that registration and turns each decoded frame into a repaint request.
struct FrameWakeBridge {
    wake: super::FrameWake,
    running: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FrameWakeBridge {
    fn install(
        session: &NativeViewerSession,
        proxy: EventLoopProxy<UiEvent>,
        window: WindowId,
    ) -> Result<Self> {
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let worker_running = Arc::clone(&running);
        let thread = std::thread::Builder::new()
            .name("Video wake".to_owned())
            .spawn(move || {
                while worker_running.load(std::sync::atomic::Ordering::Acquire) {
                    std::thread::park();
                    if !worker_running.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    let _ = proxy.send_event(UiEvent::Repaint(UiRepaintEvent {
                        window,
                        generation: 0,
                        pass: 0,
                        when: Instant::now(),
                    }));
                }
            })
            .context("create video wake thread")?;
        let handle = thread.thread().clone();
        session.frame_wake.install_render_thread(handle.clone());
        handle.unpark();
        Ok(Self {
            wake: session.frame_wake.clone(),
            running,
            thread: Some(thread),
        })
    }
}

impl Drop for FrameWakeBridge {
    fn drop(&mut self) {
        // Stop the decoder queueing frames nothing will draw, then release the
        // registration before the thread goes away.
        self.wake
            .visible
            .store(false, std::sync::atomic::Ordering::Release);
        *mutex_lock(&self.wake.render_thread) = None;
        self.running
            .store(false, std::sync::atomic::Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

fn toggle_fullscreen(window: &Arc<Window>) {
    if window.fullscreen().is_some() {
        window.set_fullscreen(None);
    } else {
        window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
    }
}

/// The wire codes `RemoteInput` accepts, which are not the 1..=6 numbering the
/// plugin hotkeys use.
fn mouse_button(button: MouseButton) -> Option<u32> {
    Some(match button {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 16,
        MouseButton::Back => 32,
        MouseButton::Forward => 64,
        MouseButton::Other(_) => return None,
    })
}

/// One bit per wire code, for tracking what is physically held.
fn button_bit(code: u32) -> u8 {
    match code {
        1 => 1,
        2 => 2,
        16 => 4,
        32 => 8,
        64 => 16,
        _ => 0,
    }
}

fn viewer_shortcut(
    modifiers: winit::keyboard::ModifiersState,
    key: winit::keyboard::PhysicalKey,
) -> Option<ViewerShortcut> {
    if crate::application::viewer_shortcuts::suspended() {
        return None;
    }
    crate::application::viewer_shortcuts::match_key(
        crate::application::viewer_shortcuts::physical_key(key)?,
        crate::application::viewer_shortcuts::modifiers(modifiers),
    )
}

/// How long the remote has to keep saying its pointer is hidden before this
/// client believes it. Long enough to swallow the flap a desktop produces while
/// someone types, short enough that a real hide still feels immediate.
const CURSOR_HIDE_GRACE: Duration = Duration::from_millis(600);

/// Decode the remote cursor PNG into a texture, with its size and hotspot.
fn decode_cursor(
    ctx: &egui::Context,
    image: &crate::features::remote_cursor::CursorImage,
) -> Option<(egui::TextureHandle, [u32; 2], [u32; 2])> {
    if image.png.is_empty() {
        return None;
    }
    let decoded = image::load_from_memory_with_format(&image.png, image::ImageFormat::Png)
        .map_err(|error| tracing::debug!(%error, "远端光标图像无法解码"))
        .ok()?
        .to_rgba8();
    let size = [decoded.width() as usize, decoded.height() as usize];
    let texture = ctx.load_texture(
        "remote-cursor",
        egui::ColorImage::from_rgba_unmultiplied(size, decoded.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    Some((texture, [decoded.width(), decoded.height()], image.hotspot))
}
