//! 设备详情页的远端终端入口。

impl super::DeviceCenterApp {
    pub(super) fn start_terminal_service(&mut self, device: crate::account::api::DeviceInfo) {
        if self.logout_pending || self.mutation_pending {
            return;
        }
        if self
            .worker
            .commands
            .send(super::GuiCommand::Terminal {
                generation: self.login_generation,
                device,
            })
            .is_err()
        {
            self.status = super::StatusMessage::error("设备后台服务已停止");
        }
    }

    /// 在系统终端里打开 CLI，让它经控制中心的终端服务接入对端。
    /// `session` 为 None 表示新建一个 shell 再接入。
    pub(super) fn launch_remote_terminal(&mut self, target: &str, session: Option<u32>) {
        if !crate::features::terminal::relay::available() {
            self.status = super::StatusMessage::error(
                "没有设置 XDG_RUNTIME_DIR，终端窗口无法接到控制中心；请改用命令行 OpenUUYC terminal",
            );
            return;
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                self.status = super::StatusMessage::error(format!("找不到当前程序路径：{error}"));
                return;
            }
        };
        let mut args = vec![
            exe.into_os_string(),
            "terminal".into(),
            "--device-id".into(),
            target.into(),
            "--via-gui".into(),
        ];
        match session {
            Some(handle) => args.push(handle.to_string().into()),
            None => args.push("--new".into()),
        }
        if let Err(error) = crate::features::terminal::launch::open(args) {
            self.status = super::StatusMessage::error(format!("{error:#}"));
        }
    }

    pub(super) fn terminal_popup(&mut self, ctx: &egui::Context, controller_id: &str, target: &str) {
        use crate::features::terminal::service::{self, Phase};
        ctx.request_repaint_after(std::time::Duration::from_millis(400));
        service::touch(controller_id, target);
        let snapshot = service::snapshot(controller_id, target);
        let mut close = false;
        egui::Window::new("远端终端")
            .id(egui::Id::new(("remote-terminal", target)))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                match snapshot.as_ref().map(|state| &state.phase) {
                    Some(Phase::Connecting) | None => {
                        ui.label("正在连接终端服务…");
                    }
                    Some(Phase::Failed) => {
                        ui.colored_label(
                            crate::ui::theme::AMBER,
                            snapshot
                                .as_ref()
                                .and_then(|state| state.error.clone())
                                .unwrap_or_else(|| "连接终端服务失败".to_owned()),
                        );
                    }
                    Some(Phase::Ready) => {
                        let listed = snapshot.as_ref().is_some_and(|state| state.listed);
                        let sessions = snapshot
                            .as_ref()
                            .map(|state| state.sessions.clone())
                            .unwrap_or_default();
                        if ui.button("新建终端").clicked() {
                            self.launch_remote_terminal(target, None);
                        }
                        if !listed {
                            ui.label("正在读取对端终端列表…");
                        } else if sessions.is_empty() {
                            ui.label("对端没有活跃终端会话");
                        } else {
                            for session in sessions {
                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "{}  {}",
                                        if session.name.is_empty() {
                                            format!("#{}", session.session_id)
                                        } else {
                                            session.name.clone()
                                        },
                                        session.state_label()
                                    ));
                                    if ui.button("接入").clicked() {
                                        self.launch_remote_terminal(target, Some(session.session_id));
                                    }
                                });
                            }
                        }
                    }
                }
                if ui.button("关闭").clicked() {
                    close = true;
                }
            });
        if close {
            self.center_ui.terminal_popup_target = None;
        }
    }
}
