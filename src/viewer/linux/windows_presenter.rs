//! Linux viewer presenter: connection progress + CPU RGBA video + winit input.
#![allow(unsafe_code)]

use super::{
    ConnectionProgress, ConnectionProgressApp, DecodedVideoFrame, NativeViewerSession,
    ViewerDisplayHandle, ViewerWindowEvent, configure_viewer_visuals, install_system_cjk_font,
    mutex_lock, take_next_frame,
};
use crate::decoder::{RenderSurface, Rgba8};
use crate::remote_input::{BUTTONS, MouseMode, RemoteInput};
use crate::ui::window_manager::{Event as UiEvent, Repaint as UiRepaintEvent};
use anyhow::{Context, Result, anyhow, bail};
use glutin::prelude::{GlDisplay, GlSurface as _, PossiblyCurrentGlContext};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowAttributes, WindowId};

pub struct ConnectingWindowsRunConfig {
    pub alias: String,
    pub progress: std_mpsc::Receiver<ConnectionProgress>,
    pub session: std_mpsc::Receiver<ViewerWindowEvent>,
    pub display_sender: tokio::sync::oneshot::Sender<ViewerDisplayHandle>,
}

pub fn run_connecting(config: ConnectingWindowsRunConfig) -> Result<()> {
    let event_loop = EventLoop::<UiEvent>::with_user_event()
        .build()
        .context("create Linux viewer event loop")?;
    let mut runner = ConnectingWindowsRunner::new(config, true, event_loop.create_proxy(), false);
    event_loop
        .run_app(&mut runner)
        .context("run Linux viewer event loop")?;
    if let Some(error) = runner.error() {
        bail!(error);
    }
    Ok(())
}

pub fn run(session: NativeViewerSession) -> Result<()> {
    // Standalone playback without a prior connecting window.
    let (progress_tx, progress_rx) = std_mpsc::channel();
    let mut ready = ConnectionProgress::working(13, "已连接", "播放窗口已打开");
    ready.state = super::ConnectionProgressState::Ready;
    let _ = progress_tx.send(ready);
    let (session_tx, session_rx) = std_mpsc::channel();
    let _ = session_tx.send(ViewerWindowEvent::Playing(Box::new(session)));
    let (display_tx, _display_rx) = tokio::sync::oneshot::channel();
    run_connecting(ConnectingWindowsRunConfig {
        alias: "OpenUUYC".into(),
        progress: progress_rx,
        session: session_rx,
        display_sender: display_tx,
    })
}

struct GlowHostSurface {
    window: Window,
    gl_context: glutin::context::PossiblyCurrentContext,
    gl_surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
}

impl GlowHostSurface {
    unsafe fn create(
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
    ) -> Result<(Self, Arc<glow::Context>)> {
        use glutin::context::NotCurrentGlContext;
        use glutin::display::{GetGlDisplay, GlDisplay};
        use glutin::surface::GlSurface;

        let template = glutin::config::ConfigTemplateBuilder::new()
            .prefer_hardware_accelerated(None)
            .with_depth_size(0)
            .with_stencil_size(0)
            .with_transparency(false);
        let (mut window_slot, gl_config) = glutin_winit::DisplayBuilder::new()
            .with_preference(glutin_winit::ApiPreference::FallbackEgl)
            .with_window_attributes(Some(attributes.clone()))
            .build(event_loop, template, |mut configs| {
                configs.next().expect("no matching GL config")
            })
            .map_err(|error| anyhow!("create viewer GL display: {error}"))?;
        let gl_display = gl_config.display();
        let raw = window_slot
            .as_ref()
            .map(|w| w.window_handle().expect("handle").as_raw());
        let attrs = glutin::context::ContextAttributesBuilder::new().build(raw);
        let fallback = glutin::context::ContextAttributesBuilder::new()
            .with_context_api(glutin::context::ContextApi::Gles(None))
            .build(raw);
        let not_current = unsafe {
            gl_display
                .create_context(&gl_config, &attrs)
                .or_else(|_| gl_display.create_context(&gl_config, &fallback))
                .context("create viewer GL context")?
        };
        let window = match window_slot.take() {
            Some(window) => window,
            None => glutin_winit::finalize_window(event_loop, attributes, &gl_config)
                .context("finalize viewer window")?,
        };
        let (width, height): (u32, u32) = window.inner_size().into();
        let surface_attributes =
            glutin::surface::SurfaceAttributesBuilder::<glutin::surface::WindowSurface>::new()
                .build(
                    window.window_handle().expect("handle").as_raw(),
                    NonZeroU32::new(width).unwrap_or(NonZeroU32::MIN),
                    NonZeroU32::new(height).unwrap_or(NonZeroU32::MIN),
                );
        let gl_surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surface_attributes)
                .context("create viewer surface")?
        };
        let gl_context = not_current
            .make_current(&gl_surface)
            .context("make viewer GL current")?;
        let _ = gl_surface.set_swap_interval(
            &gl_context,
            glutin::surface::SwapInterval::Wait(NonZeroU32::MIN),
        );
        let gl = Arc::new(unsafe {
            glow::Context::from_loader_function_cstr(|s| gl_display.get_proc_address(s))
        });
        Ok((
            Self {
                window,
                gl_context,
                gl_surface,
            },
            gl,
        ))
    }

    fn resize(&self, size: PhysicalSize<u32>) {
        if let (Ok(w), Ok(h)) = (size.width.try_into(), size.height.try_into()) {
            self.gl_surface.resize(&self.gl_context, w, h);
        }
    }

    fn make_current(&self) -> Result<()> {
        self.gl_context
            .make_current(&self.gl_surface)
            .context("make viewer GL context current")
    }

    fn swap(&self) -> Result<()> {
        self.make_current()?;
        self.gl_surface
            .swap_buffers(&self.gl_context)
            .context("viewer swap buffers")
    }
}

struct VideoState {
    session: NativeViewerSession,
    texture: Option<egui::TextureHandle>,
    frame_size: (u32, u32),
    pointer_in_video: bool,
    last_pos: Option<egui::Pos2>,
    video_rect: egui::Rect,
    owner: u64,
}

impl VideoState {
    fn new(session: NativeViewerSession) -> Self {
        Self {
            session,
            texture: None,
            frame_size: (0, 0),
            pointer_in_video: false,
            last_pos: None,
            video_rect: egui::Rect::NOTHING,
            owner: 1,
        }
    }

    fn input(&self) -> &RemoteInput {
        self.session.stream_control.mouse()
    }

    fn pull_frame(&mut self, ctx: &egui::Context) {
        use crate::performance::RenderedFrameTiming;
        let Some(frame) = take_next_frame(
            &mut mutex_lock(&self.session.frame_queue),
            &self.session.performance,
        ) else {
            return;
        };
        let RenderSurface::CpuRgba8(pixels) = frame.surface else {
            // Linux has no D3D11 surfaces; drop unexpected frames.
            return;
        };
        let rgba = rgba8_to_bytes(&pixels);
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &rgba,
        );
        match &mut self.texture {
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.texture = Some(ctx.load_texture(
                    "openuuyc-linux-video",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        self.frame_size = (frame.width, frame.height);
        self.session.performance.record_rendered_frame(RenderedFrameTiming {
            is_new_picture: frame.is_new_picture,
            width: frame.width,
            height: frame.height,
            decoded_at: frame.decoded_at,
            local: Duration::ZERO,
            assembly: frame.assembly_delay,
            input_queue: frame.input_queue_delay,
            decode_pipeline: frame.decode_pipeline_delay,
            surface_transfer: Duration::ZERO,
            present_wait: Duration::ZERO,
            render_queue: frame.decoded_at.elapsed(),
            sender_capture_at: frame.sender_timing.capture_at,
            sender_capture: frame.sender_timing.capture_delay,
            sender_encode: frame.sender_timing.encode_delay,
            sender_pacer: frame.sender_timing.pacer_delay,
            sender_total: frame.sender_timing.sending_delay,
            transport: frame.sender_timing.transport_delay,
        });
        // Wake the decode worker after draining the presentation queue
        // (same contract as Windows Video Render / linux_presenter).
        self.session.manager_wake.unpark();
    }
}

fn rgba8_to_bytes(pixels: &[Rgba8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for px in pixels {
        out.extend_from_slice(&px.0);
    }
    out
}

/// Placeholder type kept for window_manager type-checks.
pub struct ConnectingWindowsRunner {
    embedded: bool,
    close_requested: bool,
    attributes: WindowAttributes,
    alias: String,
    progress: Option<std_mpsc::Receiver<ConnectionProgress>>,
    session: std_mpsc::Receiver<ViewerWindowEvent>,
    display_sender: Option<tokio::sync::oneshot::Sender<ViewerDisplayHandle>>,
    surface: Option<GlowHostSurface>,
    painter: Option<egui_glow::Painter>,
    context: Option<egui::Context>,
    input: Option<egui_winit::State>,
    viewport: egui::ViewportInfo,
    connecting: Option<ConnectionProgressApp>,
    playing: Option<VideoState>,
    fatal_error: Option<String>,
    next_repaint: Option<Instant>,
    repaint_proxy: EventLoopProxy<UiEvent>,
    ui_generation: u64,
    modifiers: u8,
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
                .with_decorations(true)
                .with_inner_size(LogicalSize::new(1280.0, 760.0))
                .with_min_inner_size(LogicalSize::new(760.0, 520.0)),
            alias: config.alias,
            progress: Some(config.progress),
            session: config.session,
            display_sender: needs_display.then_some(config.display_sender),
            surface: None,
            painter: None,
            context: None,
            input: None,
            viewport: egui::ViewportInfo::default(),
            connecting: None,
            playing: None,
            fatal_error: None,
            next_repaint: Some(Instant::now()),
            repaint_proxy: proxy,
            embedded,
            ui_generation: 1,
            close_requested: false,
            modifiers: 0,
        }
    }

    fn exit(&mut self, event_loop: &ActiveEventLoop) {
        self.close_requested = true;
        if !self.embedded {
            event_loop.exit();
        }
    }

    pub(crate) fn closed(&self) -> bool {
        self.close_requested
    }

    pub(crate) fn error(&self) -> Option<String> {
        self.fatal_error.clone()
    }

    pub(crate) fn owns(&self, id: WindowId) -> bool {
        self.surface.as_ref().is_some_and(|s| s.window.id() == id)
    }

    pub(crate) fn focus(&self) {
        if let Some(surface) = &self.surface {
            surface.window.set_visible(true);
            surface.window.focus_window();
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        self.fatal_error = Some(error);
        self.exit(event_loop);
    }

    fn receive_session_events(&mut self) -> Result<()> {
        while let Ok(event) = self.session.try_recv() {
            if self.close_requested {
                continue;
            }
            match event {
                ViewerWindowEvent::Close => self.close_requested = true,
                ViewerWindowEvent::Playing(session) => {
                    self.connecting = None;
                    let title = session.title.clone();
                    if let Some(surface) = &self.surface {
                        surface.window.set_title(&title);
                    }
                    let mut video = VideoState::new(*session);
                    video.session.frame_wake.install_render_thread(std::thread::current());
                    self.playing = Some(video);
                    self.next_repaint = Some(Instant::now());
                }
                ViewerWindowEvent::Reconnect {
                    alias,
                    progress,
                    display,
                    ..
                } => {
                    self.alias = alias.clone();
                    self.playing = None;
                    self.connecting = Some(ConnectionProgressApp::new(alias, progress));
                    if let Some(sender) = self.display_sender.take() {
                        let _ = sender.send(ViewerDisplayHandle::default());
                    } else {
                        let _ = display.send(ViewerDisplayHandle::default());
                    }
                    if let Some(surface) = &self.surface {
                        surface.window.set_title(&format!(
                            "{}{}",
                            crate::VIEWER_TITLE_PREFIX,
                            self.alias
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn render(&mut self) -> Result<()> {
        use glow::HasContext as _;

        let Some(surface) = self.surface.as_mut() else {
            return Ok(());
        };
        let Some(painter) = self.painter.as_mut() else {
            return Ok(());
        };
        let Some(context) = self.context.as_mut() else {
            return Ok(());
        };
        let Some(input) = self.input.as_mut() else {
            return Ok(());
        };

        surface.make_current()?;
        self.next_repaint = None;
        egui_winit::update_viewport_info(&mut self.viewport, context, &surface.window, false);
        let mut raw = input.take_egui_input(&surface.window);
        raw.viewports
            .insert(egui::ViewportId::ROOT, self.viewport.clone());
        if self.close_requested {
            raw.viewports
                .get_mut(&egui::ViewportId::ROOT)
                .expect("root")
                .events
                .push(egui::ViewportEvent::Close);
        }
        self.viewport.events.clear();

        if let Some(video) = &mut self.playing {
            video.pull_frame(context);
        }

        let mut pointer_in_video = false;
        let mut video_rect = egui::Rect::NOTHING;
        let mut close = false;
        let alias = self.alias.clone();
        let output = context.run_ui(raw, |ui| {
            if let Some(progress) = &mut self.connecting {
                progress.draw(ui);
            } else if let Some(video) = &mut self.playing {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(egui::Color32::from_rgb(12, 14, 18)))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&alias)
                                    .size(crate::ui::theme::SMALL)
                                    .color(crate::ui::theme::MUTED),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("关闭").clicked() {
                                    close = true;
                                }
                                let mode = video.input().mode();
                                ui.label(format!("鼠标模式: {mode:?}"));
                            });
                        });
                        ui.separator();
                        let available = ui.available_rect_before_wrap();
                        if let Some(texture) = &video.texture {
                            let (fw, fh) = video.frame_size;
                            let fit = fit_rect(available, fw as f32, fh as f32);
                            let response = ui.allocate_rect(fit, egui::Sense::click_and_drag());
                            ui.painter().image(
                                texture.id(),
                                fit,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                            pointer_in_video = response.hovered();
                            video_rect = fit;
                            if response.hovered() {
                                ui.ctx().request_repaint();
                            }
                        } else {
                            ui.centered_and_justified(|ui| {
                                ui.label("等待画面…");
                            });
                            ui.ctx().request_repaint_after(Duration::from_millis(33));
                        }
                    });
            } else {
                ui.centered_and_justified(|ui| ui.label("正在准备观看窗口…"));
            }
            crate::ui::controls::show_notices(ui.ctx());
        });

        if let Some(video) = &mut self.playing {
            video.pointer_in_video = pointer_in_video;
            video.video_rect = video_rect;
        }
        if close {
            self.close_requested = true;
        }

        let egui::FullOutput {
            platform_output,
            mut textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = output;
        input.handle_platform_output(&surface.window, platform_output);
        if let Some(root) = viewport_output.get(&egui::ViewportId::ROOT) {
            for command in &root.commands {
                if matches!(command, egui::ViewportCommand::Close) {
                    self.close_requested = true;
                }
            }
            if let Some(when) = Instant::now().checked_add(root.repaint_delay) {
                self.next_repaint = Some(self.next_repaint.map_or(when, |old| old.min(when)));
            }
        }

        for (id, deltas) in std::mem::take(&mut textures_delta.set) {
            for delta in deltas {
                painter.set_texture(id, &delta);
            }
        }
        let clipped = context.tessellate(shapes, pixels_per_point);
        let size = surface.window.inner_size();
        unsafe {
            painter.gl().clear_color(0.05, 0.06, 0.08, 1.0);
            painter.gl().clear(glow::COLOR_BUFFER_BIT);
        }
        painter.paint_primitives([size.width, size.height], pixels_per_point, &clipped);
        for id in std::mem::take(&mut textures_delta.free) {
            painter.free_texture(id);
        }
        surface.swap()?;
        if !surface.window.is_visible().unwrap_or(true) {
            surface.window.set_visible(true);
        }
        Ok(())
    }

}

fn fit_rect(available: egui::Rect, frame_w: f32, frame_h: f32) -> egui::Rect {
    if frame_w <= 0.0 || frame_h <= 0.0 {
        return available;
    }
    let scale = (available.width() / frame_w)
        .min(available.height() / frame_h)
        .max(0.0);
    let size = egui::vec2(frame_w * scale, frame_h * scale);
    egui::Rect::from_center_size(available.center(), size)
}

fn mouse_button_code(button: MouseButton) -> Option<u32> {
    match button {
        MouseButton::Left => Some(BUTTONS[0]),
        MouseButton::Right => Some(BUTTONS[1]),
        MouseButton::Middle => Some(BUTTONS[2]),
        MouseButton::Back => Some(BUTTONS[3]),
        MouseButton::Forward => Some(BUTTONS[4]),
        MouseButton::Other(_) => None,
    }
}

impl ApplicationHandler<UiEvent> for ConnectingWindowsRunner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_some() {
            return;
        }
        let attributes = self.attributes.clone();
        let created = unsafe { GlowHostSurface::create(event_loop, attributes) };
        let (surface, gl) = match created {
            Ok(value) => value,
            Err(error) => {
                self.fail(event_loop, format!("initialize Linux viewer GL: {error:#}"));
                return;
            }
        };
        let painter = match egui_glow::Painter::new(Arc::clone(&gl), "", None, true) {
            Ok(painter) => painter,
            Err(error) => {
                self.fail(event_loop, format!("create viewer painter: {error}"));
                return;
            }
        };
        let context = egui::Context::default();
        context.set_embed_viewports(true);
        install_system_cjk_font(&context);
        configure_viewer_visuals(&context);
        let input = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            &surface.window,
            Some(surface.window.scale_factor() as f32),
            surface.window.theme(),
            Some(painter.max_texture_side()),
        );
        let mut viewport = egui::ViewportInfo::default();
        egui_winit::update_viewport_info(&mut viewport, &context, &surface.window, true);
        let proxy = self.repaint_proxy.clone();
        let window_id = surface.window.id();
        let generation = self.ui_generation;
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

        let progress = self
            .progress
            .take()
            .expect("connecting window is created only once");
        self.connecting = Some(ConnectionProgressApp::new(self.alias.clone(), progress));
        if let Some(sender) = self.display_sender.take() {
            // Software decode path: no D3D11 surface writer on Linux.
            if sender.send(ViewerDisplayHandle::default()).is_err() {
                self.close_requested = true;
                self.exit(event_loop);
                return;
            }
        }
        self.surface = Some(surface);
        self.painter = Some(painter);
        self.context = Some(context);
        self.input = Some(input);
        self.viewport = viewport;
        if let Err(error) = self.render() {
            self.fail(event_loop, format!("draw first viewer frame: {error:#}"));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if !self.owns(id) {
            return;
        }
        if let Some(input) = self.input.as_mut()
            && let Some(surface) = self.surface.as_ref()
        {
            let response = input.on_window_event(&surface.window, &event);
            if response.repaint {
                self.next_repaint = Some(Instant::now());
            }
        }

        match &event {
            WindowEvent::CloseRequested => {
                self.close_requested = true;
            }
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                if let Some(surface) = &self.surface {
                    surface.resize(*size);
                }
                self.next_repaint = Some(Instant::now());
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.receive_session_events() {
                    self.fail(event_loop, error.to_string());
                    return;
                }
                if let Err(error) = self.render() {
                    self.fail(event_loop, error.to_string());
                    return;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pos = egui::pos2(position.x as f32, position.y as f32);
                // Scale: winit physical → egui points approximately via pixels_per_point.
                let ppp = self
                    .context
                    .as_ref()
                    .map(|c| c.pixels_per_point())
                    .unwrap_or(1.0);
                let pos = egui::pos2(pos.x / ppp, pos.y / ppp);
                if let Some(video) = self.playing.as_mut() {
                    video.pointer_in_video = video.video_rect.contains(pos);
                    if video.pointer_in_video && video.input().mode() != MouseMode::View {
                        let rect = video.video_rect;
                        let x = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64;
                        let y = ((pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0) as f64;
                        video.input().absolute(video.owner, 0, x, y);
                        video.last_pos = Some(pos);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(video) = self.playing.as_ref()
                    && video.pointer_in_video
                    && video.input().mode() != MouseMode::View
                    && let Some(code) = mouse_button_code(*button)
                {
                    video
                        .input()
                        .button(video.owner, code, *state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(video) = self.playing.as_ref()
                    && video.pointer_in_video
                    && video.input().mode() != MouseMode::View
                {
                    let (dx, dy) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => ((*x * 120.0) as i32, (*y * 120.0) as i32),
                        MouseScrollDelta::PixelDelta(p) => (p.x as i32, p.y as i32),
                    };
                    if dx != 0 {
                        video.input().wheel(video.owner, dx, true);
                    }
                    if dy != 0 {
                        video.input().wheel(video.owner, dy, false);
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = crate::viewer_shortcuts::modifiers(modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(video) = self.playing.as_ref()
                    && video.input().mode() != MouseMode::View
                    && let Some(vk) = crate::viewer_shortcuts::physical_key(event.physical_key)
                {
                    let down = event.state == ElementState::Pressed;
                    let _ = video.input().key(video.owner, vk, down, None);
                    // Also honor local shortcut releases.
                    if down
                        && let Some(action) =
                            crate::viewer_shortcuts::match_key(vk, self.modifiers)
                    {
                        match action {
                            crate::viewer_shortcuts::Action::Close => {
                                self.close_requested = true;
                            }
                            crate::viewer_shortcuts::Action::ReleaseMouse => {
                                let _ = video
                                    .session
                                    .stream_control
                                    .set_mouse_mode(MouseMode::View);
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }

        if self.close_requested {
            self.exit(event_loop);
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UiEvent) {
        let UiEvent::Repaint(event) = event else {
            return;
        };
        if !self.owns(event.window) {
            return;
        }
        if let Some(context) = &self.context {
            let current = context.cumulative_pass_nr();
            if current == event.pass || current == event.pass.saturating_add(1) {
                self.next_repaint = Some(
                    self.next_repaint
                        .map_or(event.when, |old| old.min(event.when)),
                );
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.receive_session_events() {
            self.fail(event_loop, error.to_string());
            return;
        }
        if self.close_requested {
            self.exit(event_loop);
            return;
        }
        if let Some(when) = self.next_repaint {
            if when <= Instant::now() {
                self.next_repaint = None;
                if let Some(surface) = &self.surface {
                    surface.window.request_redraw();
                }
            }
            event_loop.set_control_flow(self.next_repaint.map_or_else(
                || ControlFlow::Wait,
                ControlFlow::WaitUntil,
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(mut video) = self.playing.take() {
            video.session.frame_wake.install_render_thread(std::thread::current());
            drop(video);
        }
        self.connecting = None;
        self.input = None;
        self.context = None;
        if let Some(mut painter) = self.painter.take() {
            if let Some(surface) = &self.surface {
                let _ = surface.make_current();
            }
            painter.destroy();
        }
        self.surface = None;
        self.close_requested = true;
    }
}
