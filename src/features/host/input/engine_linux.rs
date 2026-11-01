//! Linux controlled-side input injection.
//!
//! Pointer, buttons, wheel and key events go through `uinput`, as do printable
//! US-ASCII `text_input` commits, so an ASCII session keeps the path that
//! already works. A commit that contains any other character is injected with
//! `zwp_virtual_keyboard_v1` when the compositor offers it. Without that
//! protocol the ASCII subset still uses `uinput` and the other characters are
//! skipped. Absolute coordinates use the full compositor layout bounding box.
#![allow(dead_code)]
use super::wire::{Event, KeyOrButton};
use crate::platform::linux::host::{input::Uinput, keymap, virtual_keyboard};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Screen {
    pub id: i32,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}
impl Screen {
    pub fn rect(&self) -> Result<(i32, i32, i32, i32)> {
        ensure!(
            self.width > 0 && self.height > 0 && self.width <= 65536 && self.height <= 65536,
            "屏幕尺寸无效"
        );
        Ok((
            self.left,
            self.top,
            self.left
                .checked_add(self.width as i32)
                .ok_or_else(|| anyhow::anyhow!("屏幕坐标溢出"))?,
            self.top
                .checked_add(self.height as i32)
                .ok_or_else(|| anyhow::anyhow!("屏幕坐标溢出"))?,
        ))
    }
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Geometry {
    pub current: i32,
    pub screens: Vec<Screen>,
}
impl Geometry {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.screens.len() <= 32, "输入屏幕数量无效");
        let mut ids = BTreeSet::new();
        for screen in &self.screens {
            screen.rect()?;
            ensure!(ids.insert(screen.id), "重复输入屏幕");
        }
        Ok(())
    }
    pub fn screen(&self, id: Option<i32>) -> Result<&Screen> {
        self.screens
            .iter()
            .find(|s| s.id == id.unwrap_or(self.current))
            .ok_or_else(|| anyhow::anyhow!("输入目标屏幕已失效"))
    }
    /// The bounding box of every screen (the uinput ABS space).
    fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let mut it = self.screens.iter();
        let first = it.next()?.rect().ok()?;
        let mut bounds = first;
        for s in it {
            if let Ok(r) = s.rect() {
                bounds.0 = bounds.0.min(r.0);
                bounds.1 = bounds.1.min(r.1);
                bounds.2 = bounds.2.max(r.2);
                bounds.3 = bounds.3.max(r.3);
            }
        }
        Some(bounds)
    }
}

pub(crate) struct Engine {
    device: Option<Uinput>,
    keys: BTreeSet<u16>,
    buttons: BTreeSet<u16>,
    geometry: Geometry,
    unsupported_text: bool,
    virtual_keyboard: Option<virtual_keyboard::Keyboard>,
    /// Primary contact followed as the left button. This host has no uinput
    /// touchscreen, so a native frame becomes pointer motion plus one click.
    touch: TouchFollow,
}
impl Engine {
    pub fn new(
        _hardware: bool,
        _policy: super::wire::Policy,
        _origin: u32,
        _privileged: bool,
    ) -> Result<Self> {
        // Open eagerly so a permission problem is reported before the controller
        // believes it has input.
        let device = Uinput::open()?;
        Ok(Self {
            device: Some(device),
            keys: BTreeSet::new(),
            buttons: BTreeSet::new(),
            geometry: Geometry::default(),
            unsupported_text: false,
            virtual_keyboard: virtual_keyboard::Keyboard::connect_optional(),
            touch: TouchFollow::default(),
        })
    }
    pub fn backend(&self) -> &'static str {
        "uinput"
    }
    pub fn take_sas(&mut self) -> bool {
        false
    }
    pub fn configure(&mut self, _configuration: super::config::Configuration) -> Result<()> {
        Ok(())
    }
    pub fn mouse_policy(&self) -> super::config::MousePolicy {
        super::config::MousePolicy::default()
    }
    pub fn synchronize(&mut self, geometry: Geometry) -> Result<()> {
        geometry.validate()?;
        self.geometry = geometry;
        Ok(())
    }
    fn device(&mut self) -> Result<&mut Uinput> {
        self.device
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("uinput 设备未就绪"))
    }
    fn key(&mut self, vk: u16, down: bool) -> Result<()> {
        let Some(code) = keymap::evdev(vk) else {
            tracing::debug!(vk, "忽略无法映射的按键");
            return Ok(());
        };
        self.device()?.key(code, down)?;
        if down {
            self.keys.insert(code);
        } else {
            self.keys.remove(&code);
        }
        Ok(())
    }
    fn button(&mut self, button: u8, down: bool) -> Result<()> {
        let Some(code) = keymap::button(button) else {
            bail!("无效鼠标按键")
        };
        self.device()?.button(code, down)?;
        if down {
            self.buttons.insert(code);
        } else {
            self.buttons.remove(&code);
        }
        Ok(())
    }
    fn absolute(&mut self, screen: Option<i32>, x: f64, y: f64) -> Result<()> {
        let rect = self.geometry.screen(screen)?.rect()?;
        let bounds = self
            .geometry
            .bounds()
            .ok_or_else(|| anyhow::anyhow!("桌面范围无效"))?;
        let (bw, bh) = ((bounds.2 - bounds.0) as f64, (bounds.3 - bounds.1) as f64);
        ensure!(bw > 0.0 && bh > 0.0, "桌面范围无效");
        let px = rect.0 as f64 + (rect.2 - rect.0) as f64 * x;
        let py = rect.1 as f64 + (rect.3 - rect.1) as f64 * y;
        self.device()?
            .move_abs((px - bounds.0 as f64) / bw, (py - bounds.1 as f64) / bh)
    }
    fn text(&mut self, text: &str, permitted: &dyn Fn() -> bool) -> Result<()> {
        if virtual_keyboard::needs_layout_independent_text(text) {
            if let Some(keyboard) = self.virtual_keyboard.as_mut() {
                match keyboard.commit(text, permitted) {
                    virtual_keyboard::Commit::Done => return Ok(()),
                    virtual_keyboard::Commit::Failed {
                        committed: true,
                        error,
                    } => {
                        tracing::warn!(
                            %error,
                            "virtual keyboard text interrupted; not replaying through uinput"
                        );
                        return Ok(());
                    }
                    virtual_keyboard::Commit::Failed {
                        committed: false,
                        error,
                    } => {
                        tracing::warn!(
                            %error,
                            "virtual keyboard text failed; falling back to US-ASCII uinput"
                        );
                        self.unsupported_text = true;
                    }
                }
            } else if !self.unsupported_text {
                self.unsupported_text = true;
                tracing::warn!("non-ASCII text skipped: zwp_virtual_keyboard_v1 is not available");
            }
        }
        self.type_ascii(text, permitted)
    }
    fn type_ascii(&mut self, text: &str, permitted: &dyn Fn() -> bool) -> Result<()> {
        for ch in text.chars() {
            if !permitted() {
                return Ok(());
            }
            let Some((code, shift)) = keymap::ascii(ch) else {
                if !self.unsupported_text {
                    self.unsupported_text = true;
                    tracing::warn!("远端键入了非 US ASCII 字符，Linux 被控暂无法注入");
                }
                continue;
            };
            let device = self
                .device
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("uinput 设备未就绪"))?;
            if shift {
                device.key(42, true)?;
            }
            device.key(code, true)?;
            device.key(code, false)?;
            if shift {
                device.key(42, false)?;
            }
        }
        Ok(())
    }
    pub fn apply(
        &mut self,
        event: Event,
        geometry: Geometry,
        permitted: impl Fn() -> bool,
    ) -> Result<()> {
        ensure!(permitted(), "被控输入许可已撤销");
        self.synchronize(geometry)?;
        match event {
            Event::Heartbeat => {}
            Event::Command(_) => {
                // Desktop/TaskView/Lock/TaskManager shortcuts are Windows chords.
                tracing::debug!("忽略 Linux 不支持的系统快捷命令");
            }
            Event::Key { key, down, .. } => self.key(key, down)?,
            Event::Button { button, down } => self.button(button, down)?,
            Event::Click { button } => {
                self.button(button, true)?;
                self.button(button, false)?;
            }
            Event::Relative { x, y } => self.device()?.move_rel(x, y)?,
            Event::RelativeScaled { screen, x, y } => {
                // Convert the fraction to a pixel delta on the target screen.
                let rect = self.geometry.screen(screen)?.rect()?;
                let dx = ((rect.2 - rect.0) as f64 * x).round() as i32;
                let dy = ((rect.3 - rect.1) as f64 * y).round() as i32;
                self.device()?.move_rel(dx, dy)?;
            }
            Event::Absolute { screen, x, y } => self.absolute(screen, x, y)?,
            Event::Wheel { x, y } => {
                let (horizontal, delta) = if x.unsigned_abs() > y.unsigned_abs() {
                    (true, x)
                } else {
                    (false, y)
                };
                if delta != 0 {
                    let detents = delta / 120;
                    let hi_res = delta;
                    self.device()?.wheel(horizontal, detents, hi_res)?;
                }
            }
            Event::Text(text) => self.text(&text, &permitted)?,
            Event::Combination { down, keys } => {
                for item in keys {
                    ensure!(permitted(), "组合输入已取消");
                    match item {
                        KeyOrButton::Key(k) => self.key(k, down)?,
                        KeyOrButton::Button(b) => self.button(b, down)?,
                    }
                }
            }
            Event::Touch { kind, points } => {
                let samples: Vec<(u32, f64, f64)> =
                    points.iter().map(|p| (p.id, p.x, p.y)).collect();
                match self.touch.step(kind, &samples) {
                    TouchFollowAction::Idle => {}
                    TouchFollowAction::Release => self.button(1, false)?,
                    TouchFollowAction::Move { x, y } => {
                        self.absolute(Some(self.geometry.current), x, y)?;
                    }
                    TouchFollowAction::Down { x, y } => {
                        self.absolute(Some(self.geometry.current), x, y)?;
                        self.button(1, true)?;
                    }
                }
            }
        }
        Ok(())
    }
    pub fn tick(&mut self) -> Result<()> {
        Ok(())
    }
    pub fn release(&mut self) -> Result<()> {
        let keys: Vec<u16> = self.keys.iter().copied().collect();
        let buttons: Vec<u16> = self.buttons.iter().copied().collect();
        let mut failure = None;
        if let Some(device) = self.device.as_mut() {
            for code in keys {
                if let Err(e) = device.key(code, false) {
                    failure = Some(e);
                }
            }
            for code in buttons {
                if let Err(e) = device.button(code, false) {
                    failure = Some(e);
                }
            }
        }
        self.keys.clear();
        self.buttons.clear();
        self.touch = TouchFollow::default();
        failure.map_or(Ok(()), Err)
    }
}

/// One contact followed as the pointer. Extra fingers in the same frame are
/// ignored. A real multitouch device (ABS_MT) is not created here.
#[derive(Default)]
struct TouchFollow {
    id: Option<u32>,
}

enum TouchFollowAction {
    Idle,
    Down { x: f64, y: f64 },
    Move { x: f64, y: f64 },
    Release,
}

impl TouchFollow {
    fn step(&mut self, kind: u8, points: &[(u32, f64, f64)]) -> TouchFollowAction {
        if kind >= 3 || points.is_empty() {
            return if self.id.take().is_some() {
                TouchFollowAction::Release
            } else {
                TouchFollowAction::Idle
            };
        }
        let chosen = self
            .id
            .and_then(|id| points.iter().find(|point| point.0 == id))
            .or(points.first());
        let Some(&(id, x, y)) = chosen else {
            return TouchFollowAction::Idle;
        };
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            return TouchFollowAction::Idle;
        }
        let was_down = self.id.is_some();
        self.id = Some(id);
        if was_down {
            TouchFollowAction::Move { x, y }
        } else {
            TouchFollowAction::Down { x, y }
        }
    }
}

#[cfg(test)]
mod touch_follow_tests {
    use super::*;

    #[test]
    fn primary_contact_presses_moves_and_lifts() {
        let mut follow = TouchFollow::default();
        assert!(matches!(
            follow.step(1, &[(4, 0.25, 0.5)]),
            TouchFollowAction::Down { x, y } if x == 0.25 && y == 0.5
        ));
        // A second finger does not replace the contact already held.
        assert!(matches!(
            follow.step(2, &[(4, 0.3, 0.4), (9, 0.9, 0.9)]),
            TouchFollowAction::Move { x, y } if x == 0.3 && y == 0.4
        ));
        assert!(matches!(follow.step(3, &[]), TouchFollowAction::Release));
        assert!(matches!(follow.step(3, &[]), TouchFollowAction::Idle));
    }

    #[test]
    fn a_lift_with_no_contact_is_idle() {
        let mut follow = TouchFollow::default();
        assert!(matches!(follow.step(4, &[]), TouchFollowAction::Idle));
    }
}
