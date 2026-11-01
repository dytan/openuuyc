//! Layout-independent text via Wayland `zwp_virtual_keyboard_v1`.
//!
//! `uinput` reports evdev codes. The compositor then applies the user's keymap,
//! so it cannot commit an arbitrary Unicode string. wlroots-family compositors
//! (Hyprland, Sway, river, labwc, Wayfire) advertise
//! `zwp_virtual_keyboard_manager_v1`, which accepts a client-supplied xkb
//! keymap. This is the text path from the DoyoDia linux-port: each commit
//! creates a fresh virtual keyboard (Hyprland only honours the first keymap
//! on an object), uploads just the characters in that commit, and taps the
//! matching evdev-style keycode.
//!
//! The protocol keycode is evdev-style. The compositor and the client add 8
//! before looking it up in the xkb keymap, so a keymap entry of `9` is tapped
//! as key `1`.
//!
//! X11 has no virtual-keyboard protocol. A clipboard Ctrl+V fallback is not
//! used: it would overwrite the synced clipboard and miss terminals and other
//! clients that do not paste with Ctrl+V. Upstream types into X11 by rebinding
//! an X keysym over XTest; this host does not have that injector, so X11 keeps
//! the US-ASCII `uinput` path and skips everything else. The same fallback
//! applies on Wayland when the compositor does not offer the protocol
//! (GNOME/Mutter does not). XWayland windows also ignore a custom virtual
//! keymap; native Wayland clients are the ones that receive the commit.

use anyhow::{Context, Result};
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::time::Duration;
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1 as manager, zwp_virtual_keyboard_v1 as keyboard,
};

use super::keymap;

/// `zwp_virtual_keyboard_v1.keymap` format. The misc protocol XML types this
/// argument as a plain uint; `1` is xkb v1 (`keymap_format.xkb_v1`).
pub(crate) const KEYMAP_FORMAT_XKB_V1: u32 = 1;
/// `wl_keyboard.key_state.pressed`.
pub(crate) const KEY_DOWN: u32 = 1;
/// `wl_keyboard.key_state.released`.
pub(crate) const KEY_UP: u32 = 0;
/// First evdev-style keycode sent to the compositor. Matches wtype.
pub(crate) const FIRST_CODE: u32 = 1;
/// evdev keycode → xkb keycode. Hyprland applies `event.key + 8`.
pub(crate) const KEYCODE_OFFSET: u32 = 8;
/// Unique characters per keymap object. The xkb maximum stays at 64
/// (`8 + 56`). Longer commits are split across fresh keyboard objects.
pub(crate) const MAX_UNIQUE: usize = 56;
/// Gap between characters. Host input methods drop a burst that arrives faster
/// than they can forward.
const KEY_GAP: Duration = Duration::from_millis(4);

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
wayland_client::delegate_noop!(State: ignore manager::ZwpVirtualKeyboardManagerV1);
wayland_client::delegate_noop!(State: ignore keyboard::ZwpVirtualKeyboardV1);
wayland_client::delegate_noop!(State: ignore wl_seat::WlSeat);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stroke {
    /// evdev-style code passed to `zwp_virtual_keyboard_v1.key`.
    pub code: u32,
    pub down: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Batch {
    /// xkb keymap text, NUL-terminated. The protocol `size` counts the NUL.
    pub keymap: Vec<u8>,
    pub strokes: Vec<Stroke>,
}

pub(crate) enum Commit {
    Done,
    Failed {
        /// At least one key-down was already handed to the compositor.
        committed: bool,
        error: anyhow::Error,
    },
}

/// xkb keysym for one committed character.
///
/// Controls use the same keysyms as upstream's X11 text path (`XK_Return`,
/// `XK_Tab`, `XK_BackSpace`) so a newline in a commit is Enter rather than a
/// raw linefeed. Latin-1 keysyms equal their code point. Everything else is
/// the Unicode keysym range `0x01000000 | codepoint`.
pub(crate) fn keysym(c: char) -> u32 {
    match c {
        '\n' | '\r' => 0xff0d,
        '\t' => 0xff09,
        '\u{8}' => 0xff08,
        ' '..='~' | '\u{a0}'..='\u{ff}' => u32::from(c),
        _ => 0x0100_0000 | u32::from(c),
    }
}

/// Characters to tap, collapsing a Windows CRLF into one Enter.
fn logical_chars(text: &str) -> Vec<char> {
    let mut out = Vec::new();
    let mut prev_cr = false;
    for c in text.chars() {
        if c == '\n' && prev_cr {
            prev_cr = false;
            continue;
        }
        prev_cr = c == '\r';
        out.push(c);
    }
    out
}

fn keymap_text(chars: &[char]) -> String {
    let minimum = FIRST_CODE + KEYCODE_OFFSET;
    let maximum = minimum + chars.len().saturating_sub(1) as u32;
    let mut out = String::from("xkb_keymap {\n  xkb_keycodes \"vk\" {\n");
    out.push_str(&format!("    minimum = {minimum};\n"));
    out.push_str(&format!("    maximum = {maximum};\n"));
    for index in 0..chars.len() {
        let code = minimum + index as u32;
        out.push_str(&format!("    <VK{index:02}> = {code};\n"));
    }
    out.push_str("  };\n  xkb_types \"vk\" { include \"complete\" };\n");
    out.push_str("  xkb_compat \"vk\" { include \"complete\" };\n");
    out.push_str("  xkb_symbols \"vk\" {\n");
    for (index, c) in chars.iter().enumerate() {
        let sym = keysym(*c);
        out.push_str(&format!("    key <VK{index:02}> {{ [ 0x{sym:08x} ] }};\n"));
    }
    out.push_str("  };\n};\n");
    out
}

fn keymap_bytes(chars: &[char]) -> Vec<u8> {
    let mut bytes = keymap_text(chars).into_bytes();
    bytes.push(0);
    bytes
}

/// Keymap uploads and press/release pairs for `text`. No Wayland connection.
pub(crate) fn plan(text: &str) -> Vec<Batch> {
    let mut batches = Vec::new();
    let mut chars: Vec<char> = Vec::new();
    let mut strokes: Vec<Stroke> = Vec::new();
    for c in logical_chars(text) {
        let position = if let Some(found) = chars.iter().position(|seen| *seen == c) {
            found
        } else {
            if chars.len() == MAX_UNIQUE {
                push_batch(&mut batches, &mut chars, &mut strokes);
            }
            chars.push(c);
            chars.len() - 1
        };
        let code = FIRST_CODE + position as u32;
        strokes.push(Stroke { code, down: true });
        strokes.push(Stroke { code, down: false });
    }
    push_batch(&mut batches, &mut chars, &mut strokes);
    batches
}

fn push_batch(batches: &mut Vec<Batch>, chars: &mut Vec<char>, strokes: &mut Vec<Stroke>) {
    if chars.is_empty() {
        return;
    }
    batches.push(Batch {
        keymap: keymap_bytes(chars),
        strokes: std::mem::take(strokes),
    });
    chars.clear();
}

/// True when `text` contains a character the US-ASCII `uinput` path cannot type.
///
/// `\r` alone does not count: that path already skips it and turns `\n` into
/// Enter, including a Windows CRLF. A commit that also contains a real
/// non-ASCII character is sent through the virtual keyboard as a whole so the
/// ASCII around it stays in order and layout-independent.
pub(crate) fn needs_layout_independent_text(text: &str) -> bool {
    text.chars()
        .any(|c| keymap::ascii(c).is_none() && c != '\r')
}

fn keymap_fd(bytes: &[u8]) -> Result<OwnedFd> {
    use std::io::{Seek, SeekFrom, Write};
    let name = c"openuuyc-vk";
    let raw = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    anyhow::ensure!(
        raw >= 0,
        "创建 keymap 内存文件失败: {}",
        std::io::Error::last_os_error()
    );
    let mut file = unsafe { std::fs::File::from_raw_fd(raw) };
    file.write_all(bytes).context("写入 keymap 失败")?;
    file.seek(SeekFrom::Start(0))
        .context("重置 keymap 读偏移失败")?;
    Ok(OwnedFd::from(file))
}

pub(crate) struct Keyboard {
    // Proxies first so they drop before the connection they were bound on.
    manager: manager::ZwpVirtualKeyboardManagerV1,
    seat: wl_seat::WlSeat,
    queue: EventQueue<State>,
    _globals: GlobalList,
    _connection: Connection,
}

impl Keyboard {
    pub(crate) fn connect() -> Result<Self> {
        let connection = Connection::connect_to_env().context("连接 Wayland 失败")?;
        let (globals, mut queue) =
            registry_queue_init::<State>(&connection).context("枚举 Wayland 全局对象失败")?;
        let (manager, seat) = {
            let qh = queue.handle();
            let manager: manager::ZwpVirtualKeyboardManagerV1 = globals
                .bind(&qh, 1..=1, ())
                .context("桌面没有提供 zwp_virtual_keyboard_manager_v1")?;
            let seat: wl_seat::WlSeat =
                globals.bind(&qh, 1..=8, ()).context("没有 Wayland seat")?;
            (manager, seat)
        };
        queue.roundtrip(&mut State).context("虚拟键盘握手失败")?;
        Ok(Self {
            manager,
            seat,
            queue,
            _globals: globals,
            _connection: connection,
        })
    }

    /// Bind when this is a Wayland session and the compositor offers the
    /// protocol. Failure leaves key and ASCII injection on `uinput`.
    pub(crate) fn connect_optional() -> Option<Self> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return None;
        }
        match Self::connect() {
            Ok(keyboard) => {
                tracing::info!("zwp_virtual_keyboard text input enabled");
                Some(keyboard)
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    "zwp_virtual_keyboard unavailable; non-ASCII text will use US-ASCII uinput"
                );
                None
            }
        }
    }

    pub(crate) fn commit(&mut self, text: &str, permitted: &dyn Fn() -> bool) -> Commit {
        let batches = plan(text);
        let mut committed = false;
        for batch in &batches {
            if !permitted() {
                return Commit::Done;
            }
            if let Err(error) = self.apply_batch(batch, permitted, &mut committed) {
                return Commit::Failed { committed, error };
            }
        }
        Commit::Done
    }

    fn apply_batch(
        &mut self,
        batch: &Batch,
        permitted: &dyn Fn() -> bool,
        committed: &mut bool,
    ) -> Result<()> {
        let keyboard = {
            let qh = self.queue.handle();
            self.manager.create_virtual_keyboard(&self.seat, &qh, ())
        };
        let typed = self.press(&keyboard, batch, permitted, committed);
        keyboard.destroy();
        let _ = self.queue.roundtrip(&mut State);
        typed
    }

    fn press(
        &mut self,
        keyboard: &keyboard::ZwpVirtualKeyboardV1,
        batch: &Batch,
        permitted: &dyn Fn() -> bool,
        committed: &mut bool,
    ) -> Result<()> {
        let fd = keymap_fd(&batch.keymap)?;
        keyboard.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), batch.keymap.len() as u32);
        self.queue
            .roundtrip(&mut State)
            .context("等待虚拟键盘 keymap 生效失败")?;
        // This object's own modifier state. It does not release keys held on
        // the uinput device; those stay on the ASCII/key path.
        keyboard.modifiers(0, 0, 0, 0);
        let _ = self.queue.roundtrip(&mut State);
        for stroke in &batch.strokes {
            if stroke.down && !permitted() {
                break;
            }
            let state = if stroke.down { KEY_DOWN } else { KEY_UP };
            keyboard.key(0, stroke.code, state);
            self.queue
                .roundtrip(&mut State)
                .context("提交虚拟键盘按键失败")?;
            if stroke.down {
                *committed = true;
            } else {
                std::thread::sleep(KEY_GAP);
            }
        }
        Ok(())
    }
}

/// One line for `host-selftest`. Never fails the rest of the probe.
pub(crate) fn status_line() -> String {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return "zwp_virtual_keyboard: 非 Wayland 会话（非 ASCII 文字仍走 US-ASCII uinput）"
            .to_owned();
    }
    match Keyboard::connect() {
        Ok(_) => "zwp_virtual_keyboard: 可用（非 ASCII / IME 文字可注入）".to_owned(),
        Err(error) => {
            format!("zwp_virtual_keyboard: 不可用（{error}）；非 ASCII 文字回退 US-ASCII uinput")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keymap_utf8(batch: &Batch) -> String {
        let end = batch.keymap.len() - 1;
        assert_eq!(batch.keymap[end], 0, "protocol size counts a trailing NUL");
        String::from_utf8(batch.keymap[..end].to_vec()).unwrap()
    }

    #[test]
    fn protocol_constants_match_the_virtual_keyboard_handshake() {
        assert_eq!(KEYMAP_FORMAT_XKB_V1, 1);
        assert_eq!(KEY_DOWN, 1);
        assert_eq!(KEY_UP, 0);
        assert_eq!(FIRST_CODE + KEYCODE_OFFSET, 9);
    }

    #[test]
    fn cjk_keymap_uses_a_unicode_keysym_and_an_evdev_keycode() {
        let batches = plan("你");
        assert_eq!(batches.len(), 1);
        let text = keymap_utf8(&batches[0]);
        let expected = "\
xkb_keymap {
  xkb_keycodes \"vk\" {
    minimum = 9;
    maximum = 9;
    <VK00> = 9;
  };
  xkb_types \"vk\" { include \"complete\" };
  xkb_compat \"vk\" { include \"complete\" };
  xkb_symbols \"vk\" {
    key <VK00> { [ 0x01004f60 ] };
  };
};
";
        assert_eq!(text, expected);
        assert_eq!(
            batches[0].strokes,
            vec![
                Stroke {
                    code: 1,
                    down: true
                },
                Stroke {
                    code: 1,
                    down: false
                },
            ]
        );
        assert_eq!(batches[0].strokes[0].code + KEYCODE_OFFSET, 9);
    }

    #[test]
    fn repeated_character_reuses_one_key() {
        let batches = plan("你你");
        assert_eq!(batches.len(), 1);
        let text = keymap_utf8(&batches[0]);
        assert!(text.contains("<VK00>"));
        assert!(!text.contains("<VK01>"));
        assert_eq!(batches[0].strokes.len(), 4);
        assert!(batches[0].strokes.iter().all(|stroke| stroke.code == 1));
    }

    #[test]
    fn windows_crlf_is_one_return_inside_a_unicode_commit() {
        let batches = plan("你\r\n好");
        let text = keymap_utf8(&batches[0]);
        assert!(text.contains("0x01004f60"));
        assert!(text.contains("0x0000ff0d"));
        assert!(text.contains("0x0100597d"));
        assert!(!text.contains("0x0000000a"));
        assert_eq!(
            batches[0]
                .strokes
                .iter()
                .filter(|stroke| stroke.down)
                .map(|stroke| stroke.code)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn long_commits_split_so_every_character_is_mapped() {
        let text: String = (0..57)
            .map(|index| char::from_u32(0x4E00 + index).unwrap())
            .collect();
        let batches = plan(&text);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].strokes.len(), MAX_UNIQUE * 2);
        assert_eq!(batches[1].strokes.len(), 2);
        assert_eq!(batches[1].strokes[0].code, FIRST_CODE);
        let second = keymap_utf8(&batches[1]);
        assert!(second.contains("<VK00> = 9;"));
        assert!(second.contains("0x01004e38"));
    }

    #[test]
    fn a_repeat_past_the_cap_stays_on_the_first_keymap() {
        let mut text: String = (0..MAX_UNIQUE)
            .map(|index| char::from_u32(0x4E00 + index as u32).unwrap())
            .collect();
        text.push('\u{4E00}');
        assert_eq!(plan(&text).len(), 1);
    }

    #[test]
    fn ascii_and_crlf_stay_on_uinput_but_ime_text_does_not() {
        assert!(!needs_layout_independent_text(""));
        assert!(!needs_layout_independent_text("Hello, world!\n"));
        assert!(!needs_layout_independent_text("Hello\r\n"));
        assert!(!needs_layout_independent_text("\r"));
        assert!(needs_layout_independent_text("你好"));
        assert!(needs_layout_independent_text("A你"));
        assert!(needs_layout_independent_text("é"));
        assert!(needs_layout_independent_text("Hello\r\n你"));
    }

    #[test]
    fn empty_text_uploads_nothing() {
        assert!(plan("").is_empty());
        assert!(plan("\r\n").len() == 1);
        assert_eq!(
            plan("\r\n")[0]
                .strokes
                .iter()
                .filter(|stroke| stroke.down)
                .count(),
            1
        );
    }
}
