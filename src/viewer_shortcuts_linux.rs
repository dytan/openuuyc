//! User-local player shortcuts (Linux).
//! TODO(linux): richer key naming and global hotkey capture parity with Windows.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    ReleaseMouse,
    Fullscreen,
    Close,
    Performance,
}
impl Action {
    pub const ALL: [Self; 4] = [
        Self::ReleaseMouse,
        Self::Fullscreen,
        Self::Close,
        Self::Performance,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::ReleaseMouse => "退出键鼠控制",
            Self::Fullscreen => "切换全屏",
            Self::Close => "关闭串流窗口",
            Self::Performance => "切换性能监控",
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub key: u16,
    pub modifiers: u8,
}
impl Binding {
    pub fn label(self) -> String {
        crate::plugins::hotkeys::label(&crate::plugins::hotkeys::Binding {
            key: self.key,
            modifiers: self.modifiers,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    schema: u8,
    pub bindings: [Option<Binding>; 4],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 1,
            bindings: [90, 70, 81, 83].map(|key| Some(Binding { key, modifiers: 7 })),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == 1, "快捷键配置格式不支持");
        for binding in &self.bindings {
            if let Some(b) = binding {
                ensure!(
                    (8..=254).contains(&b.key)
                        && !matches!(b.key, 16..=18 | 91..=92 | 160..=165 | 229 | 231)
                        && b.modifiers <= 15,
                    "快捷键无效"
                );
            }
        }
        Ok(())
    }
    fn packed(&self) -> u64 {
        let mut value = 1u64;
        for (i, binding) in self.bindings.iter().enumerate() {
            if let Some(b) = binding {
                value |= (u64::from(b.key) << (16 * i + 8)) | (u64::from(b.modifiers) << (16 * i));
            }
        }
        value
    }
    fn unpack(value: u64) -> Self {
        let mut bindings = [None; 4];
        for (i, slot) in bindings.iter_mut().enumerate() {
            let key = ((value >> (16 * i + 8)) & 0xff) as u16;
            let modifiers = ((value >> (16 * i)) & 0xff) as u8;
            if key != 0 {
                *slot = Some(Binding { key, modifiers });
            }
        }
        Self { schema: 1, bindings }
    }
}

struct Cache {
    bytes: Option<Vec<u8>>,
    error: Option<String>,
    checked: Option<Instant>,
}
struct Runtime {
    active: AtomicU64,
    cache: Mutex<Cache>,
    text_owner: AtomicU64,
}
fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime {
        active: AtomicU64::new(Settings::default().packed()),
        cache: Mutex::new(Cache {
            bytes: None,
            error: None,
            checked: None,
        }),
        text_owner: AtomicU64::new(0),
    })
}

pub(crate) fn revision() -> u64 {
    runtime().active.load(Ordering::Acquire)
}
pub(crate) fn match_key(key: u16, modifiers: u8) -> Option<Action> {
    let settings = Settings::unpack(revision());
    settings
        .bindings
        .iter()
        .enumerate()
        .find_map(|(i, b)| b.filter(|b| b.key == key && b.modifiers == modifiers).map(|_| Action::ALL[i]))
}
pub(crate) fn label(action: Action) -> String {
    Settings::unpack(revision()).bindings[action.index()]
        .map(|b| b.label())
        .unwrap_or_else(|| "未设置".into())
}
pub(crate) fn capture_active() -> bool {
    false
}
pub(crate) fn set_text_owner(owner: u64, active: bool) {
    if active {
        runtime().text_owner.store(owner, Ordering::Release);
    } else {
        let _ = runtime().text_owner.compare_exchange(owner, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}
pub(crate) fn suspended() -> bool {
    runtime().text_owner.load(Ordering::Acquire) != 0
}
pub(crate) fn physical_key(key: winit::keyboard::PhysicalKey) -> Option<u16> {
    use winit::keyboard::{KeyCode, PhysicalKey};
    let PhysicalKey::Code(code) = key else {
        return None;
    };
    Some(match code {
        KeyCode::KeyA => 0x41,
        KeyCode::KeyB => 0x42,
        KeyCode::KeyC => 0x43,
        KeyCode::KeyD => 0x44,
        KeyCode::KeyE => 0x45,
        KeyCode::KeyF => 0x46,
        KeyCode::KeyG => 0x47,
        KeyCode::KeyH => 0x48,
        KeyCode::KeyI => 0x49,
        KeyCode::KeyJ => 0x4A,
        KeyCode::KeyK => 0x4B,
        KeyCode::KeyL => 0x4C,
        KeyCode::KeyM => 0x4D,
        KeyCode::KeyN => 0x4E,
        KeyCode::KeyO => 0x4F,
        KeyCode::KeyP => 0x50,
        KeyCode::KeyQ => 0x51,
        KeyCode::KeyR => 0x52,
        KeyCode::KeyS => 0x53,
        KeyCode::KeyT => 0x54,
        KeyCode::KeyU => 0x55,
        KeyCode::KeyV => 0x56,
        KeyCode::KeyW => 0x57,
        KeyCode::KeyX => 0x58,
        KeyCode::KeyY => 0x59,
        KeyCode::KeyZ => 0x5A,
        KeyCode::Digit0 => 0x30,
        KeyCode::Digit1 => 0x31,
        KeyCode::Digit2 => 0x32,
        KeyCode::Digit3 => 0x33,
        KeyCode::Digit4 => 0x34,
        KeyCode::Digit5 => 0x35,
        KeyCode::Digit6 => 0x36,
        KeyCode::Digit7 => 0x37,
        KeyCode::Digit8 => 0x38,
        KeyCode::Digit9 => 0x39,
        KeyCode::Escape => 0x1B,
        KeyCode::Space => 0x20,
        KeyCode::Enter => 0x0D,
        KeyCode::Tab => 0x09,
        _ => return None,
    })
}
pub(crate) fn modifiers(m: winit::keyboard::ModifiersState) -> u8 {
    u8::from(m.control_key())
        | (u8::from(m.shift_key()) << 1)
        | (u8::from(m.alt_key()) << 2)
        | (u8::from(m.super_key()) << 3)
}
fn path() -> Result<PathBuf> {
    let base = crate::paths::app_data_dir().context("无法确定本地设置目录")?;
    Ok(base.join("shortcuts.json"))
}
fn read(path: &std::path::Path) -> Result<Option<Vec<u8>>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(8193).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 8192, "快捷键配置过大");
    Ok(Some(bytes))
}
pub(crate) fn refresh() {
    {
        let mut cache = runtime().cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.checked.is_some_and(|t| t.elapsed() < Duration::from_millis(500)) {
            return;
        }
        cache.checked = Some(Instant::now());
    }
    let result = path().and_then(|p| read(&p));
    let mut cache = runtime().cache.lock().unwrap_or_else(|e| e.into_inner());
    match result {
        Ok(bytes) => {
            if bytes == cache.bytes && cache.error.is_none() {
                return;
            }
            let parsed = bytes.as_ref().map_or_else(
                || Ok(Settings::default()),
                |b| serde_json::from_slice::<Settings>(b).context("快捷键配置无效"),
            )
            .and_then(|s| {
                s.validate()?;
                Ok(s)
            });
            cache.bytes = bytes;
            match parsed {
                Ok(s) => {
                    runtime().active.store(s.packed(), Ordering::Release);
                    cache.error = None;
                }
                Err(error) => cache.error = Some(format!("{error:#}")),
            }
        }
        Err(error) => cache.error = Some(format!("{error:#}")),
    }
}

#[derive(Default)]
pub(crate) struct Editor {
    draft: Option<Settings>,
    baseline: Option<Settings>,
    expected: Option<Vec<u8>>,
    error: Option<String>,
    recording: Option<usize>,
}
impl Editor {
    pub fn reset(&mut self) {
        self.cancel_recording();
        self.draft = Some(Settings::default());
    }
    pub fn cancel_recording(&mut self) {
        self.recording = None;
    }
    fn reload(&mut self) {
        self.cancel_recording();
        refresh();
        let cache = runtime().cache.lock().unwrap_or_else(|e| e.into_inner());
        let s = Settings::unpack(revision());
        self.draft = Some(s.clone());
        self.baseline = Some(s);
        self.expected = cache.bytes.clone();
        self.error = cache.error.clone();
    }
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        if self.draft.is_none() {
            self.reload();
        }
        ui.label("快捷键（Linux 预览）");
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        for action in Action::ALL {
            ui.horizontal(|ui| {
                ui.label(action.label());
                let text = draft.bindings[action.index()]
                    .map(|b| b.label())
                    .unwrap_or_else(|| "未设置".into());
                ui.label(text);
            });
        }
        if ui.button("恢复默认").clicked() {
            *draft = Settings::default();
        }
        if ui.button("保存更改").clicked() {
            if let Err(error) = (|| -> Result<()> {
                draft.validate()?;
                let path = path()?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let bytes = serde_json::to_vec_pretty(draft)?;
                let mut file = std::fs::File::create(&path)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                runtime().active.store(draft.packed(), Ordering::Release);
                Ok(())
            })() {
                self.error = Some(format!("{error:#}"));
            } else {
                self.error = None;
                self.reload();
            }
        }
    }
}
