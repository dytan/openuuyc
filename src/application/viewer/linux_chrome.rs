//! Player caption for X11 and Wayland: the same row as the Windows player
//! (device switcher, screen tabs, stream metrics, annotation, 1:1, microphone,
//! input control) drawn with the shared caption controls.
//!
//! Screen tabs switch the single player window between screens. Detaching a
//! tab into its own window is Windows-only: it relies on pointer capture
//! outside the window, which Wayland does not grant.
use crate::application::viewer::StreamControlUi;
use crate::application::viewer_shortcuts::Action as ViewerShortcut;
use crate::diagnostics::performance::PerformanceMonitor;
use crate::features::stream_control::StreamControlHandle;
use crate::ui::chrome::{
    WindowMoveState, paint_brand_logo, update_nonmodal_window_move, window_buttons,
};
use crate::ui::controls::{
    ViewerCaptionIcon as TitleIcon, viewer_caption_button as title_icon_button,
};
use winit::window::Window;

#[derive(Default)]
pub(super) struct ScreenTabBar {
    pub(super) device_switch: Option<super::device_switch::DeviceSwitcher>,
    pub(super) tabs: Vec<(i32, String)>,
    pub(super) selected: i32,
    pub(super) pending: Option<i32>,
    pub(super) error: Option<String>,
    /// A tab the user picked; the player opens it on its next update.
    pub(super) command: Option<i32>,
}

impl ScreenTabBar {
    pub(super) fn is_pending(&self) -> bool {
        self.pending.is_some()
            || self
                .device_switch
                .as_ref()
                .is_some_and(|switcher| switcher.is_switching())
    }

    fn draw(&mut self, ui: &mut egui::Ui, window: &Window, control: &StreamControlHandle) {
        let snapshot = control.snapshot();
        let width = ui.available_width();
        let monitor = window
            .current_monitor()
            .map(|m| (m.size().width, m.size().height));
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(ui.max_rect())
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 3.0;
        egui::ScrollArea::horizontal()
            .id_salt("screen-tabs")
            .auto_shrink([false, false])
            .max_width(width)
            .scroll_source(
                egui::scroll_area::ScrollSource::SCROLL_BAR
                    | egui::scroll_area::ScrollSource::MOUSE_WHEEL,
            )
            .show(&mut row, |ui| {
                ui.horizontal_centered(|ui| {
                    for (id, name) in &self.tabs {
                        let selected = self.selected == *id;
                        let pending =
                            self.pending == Some(*id) || control.display_change_pending(*id);
                        let closable = snapshot.ready
                            && snapshot.dpi_settings_supported
                            && snapshot.screens.len() > 1
                            && (!snapshot.topology.pending || snapshot.topology.dismissed)
                            && snapshot
                                .screens
                                .iter()
                                .any(|s| s.id == *id && s.display.screen_type == 1);
                        let (response, close_clicked) = ui
                            .push_id(("screen-tab", *id), |ui| {
                                crate::ui::controls::screen_tab(
                                    ui, name, selected, pending, true, closable,
                                )
                            })
                            .inner;
                        let response = response.on_hover_text(name);
                        if close_clicked {
                            super::stream_menu::topology_menu::request(
                                ui.ctx(),
                                control,
                                *id,
                                crate::features::stream_control::DisplayTopologyAction::Remove {
                                    screen_id: *id,
                                },
                            );
                        } else if response.clicked() {
                            self.command = Some(*id);
                        }
                        response.context_menu(|ui| {
                            super::stream_menu::menu_style(ui);
                            ui.set_width(crate::ui::theme::CONTEXT_MENU_WIDTH);
                            if let Some(error) =
                                super::stream_menu::tab_display_menu(ui, control, *id, monitor)
                            {
                                self.error = Some(error);
                            }
                        });
                    }
                    if snapshot.topology_support.create_visible
                        && crate::ui::controls::screen_add(
                            ui,
                            snapshot.ready
                                && snapshot.topology_support.create
                                && (!snapshot.topology.pending || snapshot.topology.dismissed),
                        )
                        .on_hover_text("添加虚拟屏")
                        .on_disabled_hover_text(
                            snapshot
                                .topology_support
                                .create_unavailable
                                .unwrap_or("正在处理显示器操作"),
                        )
                        .clicked()
                    {
                        let (width, height, _) =
                            super::stream_menu::topology_menu::local_parameters(
                                ui.ctx(),
                                control,
                                monitor,
                            );
                        super::stream_menu::topology_menu::request(
                            ui.ctx(),
                            control,
                            self.selected,
                            crate::features::stream_control::DisplayTopologyAction::Create {
                                width,
                                height,
                            },
                        );
                    }
                });
            });
        crate::ui::controls::observe_notice(
            ui.ctx(),
            "linux-screen-error",
            "显示器操作失败",
            crate::ui::controls::DialogIcon::Error,
            self.error.as_deref(),
        );
    }
}

pub(super) struct PlayerTitleBar<'a> {
    pub(super) screens: &'a mut ScreenTabBar,
    pub(super) window: &'a Window,
    pub(super) title: &'a str,
    pub(super) performance: &'a PerformanceMonitor,
    pub(super) stream_control: &'a StreamControlHandle,
    pub(super) stream_control_ui: &'a mut StreamControlUi,
    pub(super) move_state: &'a mut WindowMoveState,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PlayerChromeAction {
    pub(super) close: bool,
    pub(super) one_to_one: bool,
    pub(super) toggle_mouse: bool,
    pub(super) toggle_annotation: bool,
}

pub(super) fn player_title_bar(ui: &mut egui::Ui, bar: PlayerTitleBar<'_>) -> PlayerChromeAction {
    ui.set_min_height(crate::ui::theme::WINDOW_TITLE_CONTENT_HEIGHT);
    let mut action = PlayerChromeAction::default();
    let stats = bar.performance.snapshot();
    let rect = egui::Rect::from_min_size(
        ui.available_rect_before_wrap().min,
        egui::vec2(
            ui.available_width(),
            crate::ui::theme::WINDOW_TITLE_CONTENT_HEIGHT,
        ),
    );
    // No plugin host on Linux: one caption slot fewer than Windows.
    const VIEW_ACTIONS_WIDTH: f32 = crate::ui::theme::VIEWER_ACTIONS_WIDTH;
    let title = bar.title.trim_start_matches(crate::VIEWER_TITLE_PREFIX);
    let title_width = if bar.screens.device_switch.is_some() {
        crate::ui::controls::viewer_device_button_width(ui, title)
    } else {
        ui.painter()
            .layout_no_wrap(
                title.into(),
                egui::FontId::proportional(crate::ui::theme::BODY),
                crate::ui::theme::TEXT,
            )
            .size()
            .x
    };
    let identity_width =
        (crate::ui::theme::WINDOW_LOGO_SIZE + ui.spacing().item_spacing.x + title_width)
            .min(crate::ui::theme::VIEWER_IDENTITY_MAX_WIDTH);
    let controls_rect = egui::Rect::from_min_max(
        egui::pos2(
            rect.max.x - crate::ui::theme::WINDOW_CONTROLS_WIDTH,
            rect.min.y,
        ),
        rect.max,
    );
    let actions_rect = egui::Rect::from_min_max(
        egui::pos2(controls_rect.min.x - VIEW_ACTIONS_WIDTH, rect.min.y),
        egui::pos2(controls_rect.min.x, rect.max.y),
    );
    let identity_rect = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(
            identity_width.min((actions_rect.min.x - rect.min.x).max(0.0)),
            rect.height(),
        ),
    );
    let content_rect = egui::Rect::from_min_max(
        egui::pos2(
            identity_rect.max.x + crate::ui::theme::VIEWER_IDENTITY_GAP,
            rect.min.y,
        ),
        egui::pos2(actions_rect.min.x, rect.max.y),
    );
    let metrics_rect = egui::Rect::from_min_max(
        egui::pos2(
            (content_rect.max.x - 184.0).max(content_rect.min.x),
            rect.min.y,
        ),
        content_rect.max,
    );
    let tabs_rect =
        egui::Rect::from_min_max(content_rect.min, egui::pos2(metrics_rect.min.x, rect.max.y));

    // The caption background is registered first so the tabs and the device
    // button above it take their own clicks; the rest moves the window.
    let drag = ui.interact(
        egui::Rect::from_min_max(
            egui::pos2(
                if bar.screens.device_switch.is_some() {
                    identity_rect.max.x
                } else {
                    rect.min.x
                },
                rect.min.y,
            ),
            metrics_rect.max,
        ),
        ui.id().with("player-title-drag"),
        egui::Sense::click_and_drag(),
    );
    update_nonmodal_window_move(ui.ctx(), bar.window, &drag, bar.move_state);

    let mut identity = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(identity_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    identity.set_clip_rect(identity_rect);
    paint_brand_logo(&mut identity);
    if let Some(switcher) = &bar.screens.device_switch {
        let response = crate::ui::controls::viewer_device_button(&mut identity, title);
        switcher.menu(&response, bar.window.id());
    } else {
        identity.add(
            egui::Label::new(
                egui::RichText::new(title)
                    .size(crate::ui::theme::BODY)
                    .strong()
                    .color(crate::ui::theme::TEXT),
            )
            .truncate(),
        );
    }
    crate::ui::controls::viewer_caption_separator(
        ui,
        egui::pos2(
            identity_rect.right() + crate::ui::theme::VIEWER_IDENTITY_GAP / 2.0,
            rect.center().y,
        ),
    );

    if !bar.screens.tabs.is_empty() {
        let mut tabs = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(tabs_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        tabs.set_clip_rect(tabs_rect);
        bar.screens.draw(&mut tabs, bar.window, bar.stream_control);
    }
    let codec_info = if stats.video_format.contains("264") || stats.video_format.contains("265") {
        &stats.video_format
    } else {
        &stats.video_codec
    };
    let codec = if codec_info.contains("AV1") {
        "AV1"
    } else if codec_info.contains("265") || codec_info.contains("HEVC") {
        "H.265"
    } else if codec_info.contains("264") || codec_info.contains("AVC") {
        "H.264"
    } else {
        "—"
    };
    let parameters = stats.decoded_resolution.map_or_else(
        || "等待画面".to_owned(),
        |(width, height)| {
            format!(
                "{width}×{height} · {:.0} FPS · {codec}",
                stats.actual_fps.max(1.0)
            )
        },
    );
    let mut metrics = ui.new_child(egui::UiBuilder::new().max_rect(metrics_rect).layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
    ));
    metrics.set_clip_rect(metrics_rect);
    metrics
        .add(
            egui::Label::new(
                egui::RichText::new(parameters)
                    .size(crate::ui::theme::TINY)
                    .color(crate::ui::theme::MUTED),
            )
            .truncate(),
        )
        .on_hover_text(&stats.video_format);

    let mut actions = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(actions_rect.shrink2(egui::vec2(4.0, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    actions.spacing_mut().item_spacing.x = 4.0;
    if title_icon_button(
        &mut actions,
        TitleIcon::Quality,
        bar.stream_control_ui.open,
        "画质与串流设置",
    )
    .clicked()
    {
        bar.stream_control_ui.open = !bar.stream_control_ui.open;
    }
    let annotation = bar.stream_control.annotation_snapshot();
    action.toggle_annotation = actions
        .add_enabled_ui(
            !annotation.toggling
                && (annotation.enabled
                    || annotation.supported
                    || bar.stream_control.snapshot().ready
                        && bar.stream_control.remote_upgrade().is_some()),
            |ui| title_icon_button(ui, TitleIcon::Annotation, annotation.enabled, "批注"),
        )
        .inner
        .on_disabled_hover_text("当前设备暂不支持批注，或仍在连接中")
        .clicked();
    action.one_to_one = actions
        .add_enabled_ui(
            !bar.window.is_maximized() && bar.window.fullscreen().is_none(),
            |ui| {
                title_icon_button(
                    ui,
                    TitleIcon::OneToOne,
                    false,
                    "1:1 实际像素（空间不足时等比缩小）",
                )
            },
        )
        .inner
        .on_disabled_hover_text("请先还原窗口")
        .clicked();
    let control = bar.stream_control.snapshot();
    let mic = bar.stream_control.microphone().snapshot();
    let mic_hint = if mic.pending {
        "麦克风：等待远端确认"
    } else if let Some(error) = mic.error.as_deref() {
        error
    } else if mic.capturing {
        "正在将麦克风发送到远端，点击关闭"
    } else if mic.enabled {
        "麦克风已开启，等待远端应用使用，点击关闭"
    } else {
        "将所选麦克风发送到远端（设备可在高级设置中切换）"
    };
    if actions
        .add_enabled_ui(
            mic.enabled
                || (bar.stream_control.microphone_available()
                    && control.ready
                    && control.mouse_mode != crate::features::remote_input::MouseMode::View),
            |ui| title_icon_button(ui, TitleIcon::Microphone, mic.enabled, mic_hint),
        )
        .inner
        .on_disabled_hover_text("请先连接支持麦克风的 Windows 设备并开启键鼠控制")
        .clicked()
    {
        crate::ui::controls::clear_notice(actions.ctx(), "microphone-request-error");
        match bar.stream_control.set_microphone_enabled(!mic.enabled) {
            Ok(()) => crate::ui::controls::clear_notice(actions.ctx(), "microphone-error"),
            Err(error) => crate::ui::controls::notice(
                actions.ctx(),
                "microphone-request-error",
                "麦克风",
                crate::ui::controls::DialogIcon::Error,
                error.to_string(),
            ),
        }
    }
    let enabled = control.mouse_mode != crate::features::remote_input::MouseMode::View
        || control.mouse_pending;
    action.toggle_mouse = actions
        .add_enabled_ui(enabled || control.ready, |ui| {
            let hint = if bar.stream_control.mouse().transport_recovering() {
                "键鼠控制正在自动恢复；点击取消".into()
            } else if bar.stream_control.mouse().waiting_for_neutral() {
                format!(
                    "等待松开全部键鼠；退出控制快捷键：{}",
                    crate::application::viewer_shortcuts::label(ViewerShortcut::ReleaseMouse)
                )
            } else if enabled {
                format!(
                    "退出控制（{}）",
                    crate::application::viewer_shortcuts::label(ViewerShortcut::ReleaseMouse)
                )
            } else if bar.stream_control.mouse().keyboard_supported() {
                "开启键鼠控制".into()
            } else {
                "开启鼠标控制（此平台暂未适配键盘）".into()
            };
            title_icon_button(ui, TitleIcon::Mouse, enabled, &hint)
        })
        .inner
        .clicked();

    let mut controls = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(controls_rect.shrink2(egui::vec2(4.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    controls.spacing_mut().item_spacing.x = 4.0;
    action.close = window_buttons(&mut controls, bar.window);
    ui.painter().line_segment(
        [
            egui::pos2(controls_rect.min.x, rect.min.y + 8.0),
            egui::pos2(controls_rect.min.x, rect.max.y - 8.0),
        ],
        egui::Stroke::new(1.0, crate::ui::theme::LINE),
    );
    ui.advance_cursor_after_rect(rect);
    action
}
