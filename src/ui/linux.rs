//! Linux egui + winit + glow/glutin shell (replaces D3D11 presenter).
#![allow(unsafe_code)]

use super::window_manager::{self, Event, Repaint, Request};
use super::{AppFactory, AppSession, WindowConfig};
use crate::viewer::windows_presenter::ConnectingWindowsRunner;
use anyhow::{Context, Result, anyhow, bail};
use glutin::prelude::{GlDisplay, GlSurface as _, PossiblyCurrentGlContext};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowAttributes, WindowId};

pub(super) fn run(config: WindowConfig, factory: AppFactory) -> Result<()> {
    let event_loop = EventLoop::<Event>::with_user_event()
        .build()
        .context("create Linux desktop event loop")?;
    let mut runner = LinuxApp {
        main: Runner {
            config,
            factory: Some(factory),
            state: None,
            error: None,
            proxy: event_loop.create_proxy(),
            root: true,
            closed: false,
        },
        windows: HashMap::new(),
        viewers: HashMap::new(),
    };
    window_manager::install(Some(event_loop.create_proxy()));
    event_loop
        .run_app(&mut runner)
        .context("run Linux egui/glow event loop")?;
    window_manager::install(None);
    crate::clipboard::shutdown();
    if let Some(error) = runner.main.error.take() {
        bail!(error);
    }
    Ok(())
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
                configs
                    .next()
                    .expect("no matching GL config for OpenUUYC Linux UI")
            })
            .map_err(|error| anyhow!("create GL display/config: {error}"))?;

        let gl_display = gl_config.display();
        let raw_window_handle = window_slot.as_ref().map(|window| {
            window
                .window_handle()
                .expect("window handle")
                .as_raw()
        });

        let context_attributes =
            glutin::context::ContextAttributesBuilder::new().build(raw_window_handle);
        let fallback = glutin::context::ContextAttributesBuilder::new()
            .with_context_api(glutin::context::ContextApi::Gles(None))
            .build(raw_window_handle);
        let not_current = unsafe {
            gl_display
                .create_context(&gl_config, &context_attributes)
                .or_else(|_| gl_display.create_context(&gl_config, &fallback))
                .context("create OpenGL context")?
        };

        let window = match window_slot.take() {
            Some(window) => window,
            None => glutin_winit::finalize_window(event_loop, attributes, &gl_config)
                .context("finalize GL window")?,
        };

        let (width, height): (u32, u32) = window.inner_size().into();
        let width = NonZeroU32::new(width).unwrap_or(NonZeroU32::MIN);
        let height = NonZeroU32::new(height).unwrap_or(NonZeroU32::MIN);
        let surface_attributes =
            glutin::surface::SurfaceAttributesBuilder::<glutin::surface::WindowSurface>::new()
                .build(
                    window.window_handle().expect("window handle").as_raw(),
                    width,
                    height,
                );
        let gl_surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surface_attributes)
                .context("create GL window surface")?
        };
        let gl_context = not_current
            .make_current(&gl_surface)
            .context("make GL context current")?;
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

    fn resize(&self, size: winit::dpi::PhysicalSize<u32>) {
        if let (Ok(w), Ok(h)) = (size.width.try_into(), size.height.try_into()) {
            self.gl_surface.resize(&self.gl_context, w, h);
        }
    }

    fn make_current(&self) -> Result<()> {
        self.gl_context
            .make_current(&self.gl_surface)
            .context("make GL context current")
    }

    fn swap(&self) -> Result<()> {
        self.make_current()?;
        self.gl_surface
            .swap_buffers(&self.gl_context)
            .context("swap GL buffers")
    }
}

fn window_attributes(config: &WindowConfig) -> WindowAttributes {
    let viewport = &config.viewport;
    let mut attributes = WindowAttributes::default()
        .with_visible(viewport.visible.unwrap_or(false))
        .with_title(
            viewport
                .title
                .clone()
                .unwrap_or_else(|| crate::APP_NAME.to_owned()),
        )
        .with_window_icon(Some(super::branding::window_icon()))
        .with_decorations(true)
        .with_resizable(viewport.resizable.unwrap_or(true));
    if let Some(size) = viewport.inner_size {
        attributes = attributes.with_inner_size(LogicalSize::new(size.x as f64, size.y as f64));
    }
    if let Some(size) = viewport.min_inner_size {
        attributes = attributes.with_min_inner_size(LogicalSize::new(size.x as f64, size.y as f64));
    }
    if let Some(size) = viewport.max_inner_size {
        attributes = attributes.with_max_inner_size(LogicalSize::new(size.x as f64, size.y as f64));
    }
    attributes
}

struct DesktopWindow {
    app: AppSession,
    surface: GlowHostSurface,
    painter: egui_glow::Painter,
    context: egui::Context,
    input: egui_winit::State,
    viewport: egui::ViewportInfo,
    close_requested: bool,
    show_after_present: bool,
    next_repaint: Option<Instant>,
    last_frame: Option<Instant>,
    interval: Duration,
}

impl Drop for DesktopWindow {
    fn drop(&mut self) {
        self.surface.window.set_visible(false);
        let _ = self.surface.make_current();
        self.painter.destroy();
    }
}

impl DesktopWindow {
    fn schedule(&mut self, when: Instant) {
        let when = self
            .last_frame
            .map_or(when, |last| when.max(last + self.interval));
        self.next_repaint = Some(self.next_repaint.map_or(when, |old| old.min(when)));
    }

    fn render(&mut self) -> Result<()> {
        use glow::HasContext as _;

        self.surface.make_current()?;
        self.next_repaint = None;
        self.last_frame = Some(Instant::now());
        egui_winit::update_viewport_info(
            &mut self.viewport,
            &self.context,
            &self.surface.window,
            false,
        );
        let mut raw = self.input.take_egui_input(&self.surface.window);
        raw.viewports
            .insert(egui::ViewportId::ROOT, self.viewport.clone());
        if self.close_requested {
            raw.viewports
                .get_mut(&egui::ViewportId::ROOT)
                .expect("root viewport")
                .events
                .push(egui::ViewportEvent::Close);
        }
        self.viewport.events.clear();

        let output = self.context.run_ui(raw, |ui| {
            self.app.0.ui(ui);
            super::controls::show_notices(ui.ctx());
        });

        let egui::FullOutput {
            platform_output,
            mut textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = output;

        self.input
            .handle_platform_output(&self.surface.window, platform_output);

        if let Some(root) = viewport_output.get(&egui::ViewportId::ROOT) {
            for command in &root.commands {
                match command {
                    egui::ViewportCommand::Close => self.close_requested = true,
                    egui::ViewportCommand::CancelClose => self.close_requested = false,
                    _ => {}
                }
            }
            let mut actions = Vec::new();
            egui_winit::process_viewport_commands(
                &self.context,
                &mut self.viewport,
                root.commands.iter().cloned().filter(|command| {
                    !matches!(
                        command,
                        egui::ViewportCommand::Close | egui::ViewportCommand::CancelClose
                    )
                }),
                &self.surface.window,
                &mut actions,
            );
            if let Some(when) = Instant::now().checked_add(root.repaint_delay) {
                self.schedule(when);
            }
        }

        if self.close_requested && self.app.0.on_close_requested() {
            self.surface.window.set_visible(false);
            return Ok(());
        }
        self.close_requested = false;

        for (id, deltas) in std::mem::take(&mut textures_delta.set) {
            for delta in deltas {
                self.painter.set_texture(id, &delta);
            }
        }

        let clipped = self.context.tessellate(shapes, pixels_per_point);
        let size = self.surface.window.inner_size();
        unsafe {
            self.painter.gl().clear_color(0.08, 0.09, 0.12, 1.0);
            self.painter.gl().clear(glow::COLOR_BUFFER_BIT);
        }
        self.painter
            .paint_primitives([size.width, size.height], pixels_per_point, &clipped);
        for id in std::mem::take(&mut textures_delta.free) {
            self.painter.free_texture(id);
        }
        self.surface.swap()?;
        if self.show_after_present {
            self.surface.window.set_visible(true);
            self.show_after_present = false;
        }
        Ok(())
    }
}

struct Runner {
    root: bool,
    closed: bool,
    config: WindowConfig,
    factory: Option<AppFactory>,
    state: Option<DesktopWindow>,
    error: Option<String>,
    proxy: EventLoopProxy<Event>,
}

impl Runner {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(format!("{error:#}"));
        self.state.take();
        self.closed = true;
        if self.root {
            event_loop.exit();
        }
    }

    fn create(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.state.is_some() {
            return Ok(());
        }
        let attributes = window_attributes(&self.config).with_visible(false);
        let (surface, gl) = unsafe { GlowHostSurface::create(event_loop, attributes)? };
        if self.config.centered
            && let Some(monitor) = surface.window.current_monitor()
        {
            let outer = surface.window.outer_size();
            let size = monitor.size();
            let origin = monitor.position();
            surface.window.set_outer_position(PhysicalPosition::new(
                origin.x + (size.width as i64 - outer.width as i64).max(0) as i32 / 2,
                origin.y + (size.height as i64 - outer.height as i64).max(0) as i32 / 2,
            ));
        }

        let painter = egui_glow::Painter::new(Arc::clone(&gl), "", None, true)
            .map_err(|error| anyhow!("create egui glow painter: {error}"))?;
        let context = egui::Context::default();
        context.set_embed_viewports(true);
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
        let proxy = self.proxy.clone();
        let window_id = surface.window.id();
        context.set_request_repaint_callback(move |info| {
            if info.viewport_id == egui::ViewportId::ROOT
                && let Some(when) = Instant::now().checked_add(info.delay)
            {
                let _ = proxy.send_event(Event::Repaint(Repaint {
                    window: window_id,
                    generation: 0,
                    pass: info.current_cumulative_pass_nr,
                    when,
                }));
            }
        });
        let factory = self
            .factory
            .take()
            .context("desktop factory already consumed")?;
        let graphics = format!("OpenGL (glow) — {}", glow_renderer_string(&gl));
        let app = AppSession(factory(&context, Some(graphics)));
        let refresh = surface
            .window
            .current_monitor()
            .and_then(|monitor| monitor.refresh_rate_millihertz())
            .filter(|rate| *rate != 0)
            .unwrap_or(60_000);
        let show_after_present = self.config.viewport.visible.unwrap_or(true);
        self.state = Some(DesktopWindow {
            app,
            surface,
            painter,
            context,
            input,
            viewport,
            close_requested: false,
            show_after_present,
            next_repaint: Some(Instant::now()),
            last_frame: None,
            interval: Duration::from_secs_f64(1000.0 / f64::from(refresh)),
        });
        let state = self.state.as_mut().expect("created desktop state");
        state.render()?;
        state.app.0.on_focus_changed(state.surface.window.has_focus());
        self.finish_close(event_loop);
        Ok(())
    }

    fn finish_close(&mut self, event_loop: &ActiveEventLoop) {
        if self
            .state
            .as_ref()
            .is_some_and(|state| state.close_requested)
        {
            self.closed = true;
            if self.root {
                event_loop.exit();
            }
        }
    }
}

fn glow_renderer_string(gl: &glow::Context) -> String {
    use glow::HasContext as _;
    unsafe {
        let vendor = gl.get_parameter_string(glow::VENDOR);
        let renderer = gl.get_parameter_string(glow::RENDERER);
        format!("{vendor} / {renderer}")
    }
}

impl ApplicationHandler<Event> for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.create(event_loop) {
            self.fail(event_loop, error);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = self
            .state
            .as_mut()
            .filter(|state| state.surface.window.id() == id)
        else {
            return;
        };
        let response = state.input.on_window_event(&state.surface.window, &event);
        let result = match event {
            WindowEvent::Focused(focused) => {
                state.app.0.on_focus_changed(focused);
                state.schedule(Instant::now());
                Ok(())
            }
            WindowEvent::CloseRequested => {
                state.close_requested = true;
                state.render()
            }
            WindowEvent::Destroyed => {
                self.closed = true;
                if self.root {
                    event_loop.exit();
                }
                Ok(())
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    state.surface.resize(size);
                }
                state.schedule(Instant::now());
                Ok(())
            }
            WindowEvent::RedrawRequested => state.render(),
            _ => {
                if response.repaint {
                    state.schedule(Instant::now());
                }
                Ok(())
            }
        };
        if let Err(error) = result {
            self.fail(event_loop, error);
            return;
        }
        self.finish_close(event_loop);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: Event) {
        let Event::Repaint(event) = event else {
            return;
        };
        if let Some(state) = &mut self.state {
            if state.surface.window.id() != event.window {
                return;
            }
            let current = state.context.cumulative_pass_nr();
            if current == event.pass || current == event.pass.saturating_add(1) {
                state.schedule(event.when);
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = &mut self.state {
            if let Some(when) = state.next_repaint {
                if when <= Instant::now() {
                    state.next_repaint = None;
                    if state.show_after_present {
                        if let Err(error) = state.render() {
                            self.fail(event_loop, error);
                            return;
                        }
                    } else {
                        state.surface.window.request_redraw();
                    }
                    event_loop.set_control_flow(
                        state
                            .next_repaint
                            .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
                    );
                } else {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(when));
                }
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
        self.finish_close(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.state.take();
    }
}

struct ManagedViewer {
    runner: ConnectingWindowsRunner,
    done: Option<tokio::sync::oneshot::Sender<Result<()>>>,
}

impl ManagedViewer {
    fn finish(&mut self, event_loop: &ActiveEventLoop) {
        self.runner.exiting(event_loop);
        if let Some(done) = self.done.take() {
            let _ = done.send(
                self.runner
                    .error()
                    .map_or(Ok(()), |error| Err(anyhow!(error))),
            );
        }
    }
}

struct LinuxApp {
    windows: HashMap<String, Runner>,
    viewers: HashMap<String, ManagedViewer>,
    main: Runner,
}

impl LinuxApp {
    fn retire(&mut self, event_loop: &ActiveEventLoop) {
        self.windows.retain(|_, window| !window.closed);
        self.viewers.retain(|_, viewer| {
            if viewer.runner.closed() {
                viewer.finish(event_loop);
                false
            } else {
                true
            }
        });
    }

    fn focus(&self, key: &str) {
        if let Some(window) = self.windows.get(key).and_then(|r| r.state.as_ref()) {
            window.surface.window.set_minimized(false);
            window.surface.window.set_visible(true);
            window.surface.window.focus_window();
        }
        if let Some(viewer) = self.viewers.get(key) {
            viewer.runner.focus();
        }
    }
}

impl ApplicationHandler<Event> for LinuxApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.main.resumed(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self
            .main
            .state
            .as_ref()
            .is_some_and(|s| s.surface.window.id() == id)
        {
            self.main.window_event(event_loop, id, event);
        } else if let Some(window) = self
            .windows
            .values_mut()
            .find(|w| w.state.as_ref().is_some_and(|s| s.surface.window.id() == id))
        {
            window.window_event(event_loop, id, event);
        } else if let Some(viewer) = self.viewers.values_mut().find(|v| v.runner.owns(id)) {
            viewer.runner.window_event(event_loop, id, event);
        }
        self.retire(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Repaint(event) => {
                if self
                    .main
                    .state
                    .as_ref()
                    .is_some_and(|s| s.surface.window.id() == event.window)
                {
                    self.main.user_event(event_loop, Event::Repaint(event));
                } else if let Some(window) = self.windows.values_mut().find(|w| {
                    w.state
                        .as_ref()
                        .is_some_and(|s| s.surface.window.id() == event.window)
                }) {
                    window.user_event(event_loop, Event::Repaint(event));
                } else if let Some(viewer) = self
                    .viewers
                    .values_mut()
                    .find(|v| v.runner.owns(event.window))
                {
                    viewer.runner.user_event(event_loop, Event::Repaint(event));
                }
            }
            Event::Request(Request::Focus(key)) => self.focus(&key),
            Event::Request(Request::Open {
                key,
                config,
                factory,
            }) => {
                if self.windows.contains_key(&key) {
                    self.focus(&key);
                    return;
                }
                let mut window = Runner {
                    config,
                    factory: Some(factory),
                    state: None,
                    error: None,
                    proxy: self.main.proxy.clone(),
                    root: false,
                    closed: false,
                };
                window.resumed(event_loop);
                if !window.closed {
                    self.windows.insert(key, window);
                }
            }
            Event::Request(Request::Viewer { key, config, done }) => {
                if self.viewers.contains_key(&key) {
                    self.focus(&key);
                    let _ = done.send(Err(anyhow!("观看窗口已打开")));
                    return;
                }
                let mut viewer = ManagedViewer {
                    runner: ConnectingWindowsRunner::new(
                        config,
                        true,
                        self.main.proxy.clone(),
                        true,
                    ),
                    done: Some(done),
                };
                viewer.runner.resumed(event_loop);
                self.viewers.insert(key, viewer);
            }
        }
        self.retire(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let mut wake = None::<Instant>;
        self.main.about_to_wait(event_loop);
        if let ControlFlow::WaitUntil(at) = event_loop.control_flow() {
            wake = Some(at);
        }
        for window in self.windows.values_mut() {
            event_loop.set_control_flow(ControlFlow::Wait);
            window.about_to_wait(event_loop);
            if let ControlFlow::WaitUntil(at) = event_loop.control_flow() {
                wake = Some(wake.map_or(at, |old| old.min(at)));
            }
        }
        for viewer in self.viewers.values_mut() {
            event_loop.set_control_flow(ControlFlow::Wait);
            viewer.runner.about_to_wait(event_loop);
            if let ControlFlow::WaitUntil(at) = event_loop.control_flow() {
                wake = Some(wake.map_or(at, |old| old.min(at)));
            }
        }
        self.retire(event_loop);
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        window_manager::install(None);
        for viewer in self.viewers.values_mut() {
            viewer.finish(event_loop);
        }
        self.viewers.clear();
        self.windows.clear();
        self.main.exiting(event_loop);
    }
}
