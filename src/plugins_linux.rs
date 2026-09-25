//! Linux plugin host stub.
//! TODO(linux): port DLL/node-graph host (dlopen + ABI) after the viewer shell lands.
use anyhow::{Result, bail};
use std::path::Path;

pub mod graph {}
pub mod video {
    use anyhow::{Result, bail};
    pub fn host() -> Result<()> {
        bail!("TODO(linux): plugin video host not implemented")
    }
}

pub(crate) mod hotkeys {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Binding {
        pub key: u16,
        pub modifiers: u8,
    }

    pub fn key_event(_owner: u64, _key: u16, _down: bool) -> bool {
        false
    }

    pub fn key_code(key: egui::Key) -> Option<u16> {
        // Rough egui → Windows VK mapping for shortcut UI compatibility.
        Some(match key {
            egui::Key::A => 0x41,
            egui::Key::B => 0x42,
            egui::Key::C => 0x43,
            egui::Key::D => 0x44,
            egui::Key::E => 0x45,
            egui::Key::F => 0x46,
            egui::Key::G => 0x47,
            egui::Key::H => 0x48,
            egui::Key::I => 0x49,
            egui::Key::J => 0x4A,
            egui::Key::K => 0x4B,
            egui::Key::L => 0x4C,
            egui::Key::M => 0x4D,
            egui::Key::N => 0x4E,
            egui::Key::O => 0x4F,
            egui::Key::P => 0x50,
            egui::Key::Q => 0x51,
            egui::Key::R => 0x52,
            egui::Key::S => 0x53,
            egui::Key::T => 0x54,
            egui::Key::U => 0x55,
            egui::Key::V => 0x56,
            egui::Key::W => 0x57,
            egui::Key::X => 0x58,
            egui::Key::Y => 0x59,
            egui::Key::Z => 0x5A,
            egui::Key::Num0 => 0x30,
            egui::Key::Num1 => 0x31,
            egui::Key::Num2 => 0x32,
            egui::Key::Num3 => 0x33,
            egui::Key::Num4 => 0x34,
            egui::Key::Num5 => 0x35,
            egui::Key::Num6 => 0x36,
            egui::Key::Num7 => 0x37,
            egui::Key::Num8 => 0x38,
            egui::Key::Num9 => 0x39,
            egui::Key::F1 => 0x70,
            egui::Key::F2 => 0x71,
            egui::Key::F3 => 0x72,
            egui::Key::F4 => 0x73,
            egui::Key::F5 => 0x74,
            egui::Key::F6 => 0x75,
            egui::Key::F7 => 0x76,
            egui::Key::F8 => 0x77,
            egui::Key::F9 => 0x78,
            egui::Key::F10 => 0x79,
            egui::Key::F11 => 0x7A,
            egui::Key::F12 => 0x7B,
            egui::Key::Escape => 0x1B,
            egui::Key::Enter => 0x0D,
            egui::Key::Space => 0x20,
            egui::Key::Tab => 0x09,
            _ => return None,
        })
    }

    pub fn label(binding: &Binding) -> String {
        let mut names = Vec::new();
        for (bit, name) in [(1, "Ctrl"), (2, "Shift"), (4, "Alt"), (8, "Super")] {
            if binding.modifiers & bit != 0 {
                names.push(name.to_owned());
            }
        }
        names.push(format!("VK_{}", binding.key));
        names.join(" + ")
    }
}

#[derive(Default)]
pub(crate) struct Manager;

impl Manager {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.label("插件宿主尚未在 Linux 上实现（TODO(linux)）。");
    }
}

pub(crate) struct Controller;
impl Controller {
    pub fn new(_ctx: egui::Context) -> Self {
        Self
    }
}

pub(crate) fn capturing_shortcut(_ctx: &egui::Context) -> bool {
    false
}

pub(crate) struct Sample;
pub(crate) struct Shared;
pub(crate) struct ChainShared;
pub(crate) struct Graph;
pub(crate) struct Tap;

pub(crate) fn paint_plugin_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let center = rect.center();
    let stroke = egui::Stroke::new(1.5, color);
    painter.rect_stroke(
        egui::Rect::from_center_size(center, egui::vec2(12.0, 9.0)),
        2.5,
        stroke,
        egui::StrokeKind::Inside,
    );
}

pub fn host(_path: &Path) -> Result<()> {
    bail!("TODO(linux): plugin host not implemented")
}
