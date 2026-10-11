//! Windows virtual-key codes (what UU controllers send) to Linux evdev codes.

/// Map a Windows VK code to an evdev `KEY_*` code.
pub(crate) fn evdev(vk: u16) -> Option<u16> {
    Some(match vk {
        0x08 => 14,  // Backspace
        0x09 => 15,  // Tab
        0x0D => 28,  // Enter
        0x10 | 0xA0 => 42, // Shift / LShift
        0xA1 => 54,  // RShift
        0x11 | 0xA2 => 29, // Ctrl / LCtrl
        0xA3 => 97,  // RCtrl
        0x12 | 0xA4 => 56, // Alt / LAlt
        0xA5 => 100, // RAlt
        0x13 => 119, // Pause
        0x14 => 58,  // CapsLock
        0x1B => 1,   // Esc
        0x20 => 57,  // Space
        0x21 => 104, // PageUp
        0x22 => 109, // PageDown
        0x23 => 107, // End
        0x24 => 102, // Home
        0x25 => 105, // Left
        0x26 => 103, // Up
        0x27 => 106, // Right
        0x28 => 108, // Down
        0x2C => 99,  // PrintScreen
        0x2D => 110, // Insert
        0x2E => 111, // Delete
        0x30 => 11,
        0x31..=0x39 => 2 + (vk - 0x31),
        0x41 => 30, 0x42 => 48, 0x43 => 46, 0x44 => 32, 0x45 => 18, 0x46 => 33,
        0x47 => 34, 0x48 => 35, 0x49 => 23, 0x4A => 36, 0x4B => 37, 0x4C => 38,
        0x4D => 50, 0x4E => 49, 0x4F => 24, 0x50 => 25, 0x51 => 16, 0x52 => 19,
        0x53 => 31, 0x54 => 20, 0x55 => 22, 0x56 => 47, 0x57 => 17, 0x58 => 45,
        0x59 => 21, 0x5A => 44,
        0x5B => 125, // LWin -> LeftMeta
        0x5C => 126, // RWin -> RightMeta
        0x5D => 127, // Apps -> Compose
        0x60 => 82, 0x61 => 79, 0x62 => 80, 0x63 => 81, 0x64 => 75,
        0x65 => 76, 0x66 => 77, 0x67 => 71, 0x68 => 72, 0x69 => 73,
        0x6A => 55,  // KP *
        0x6B => 78,  // KP +
        0x6C => 96,  // Separator -> KP Enter
        0x6D => 74,  // KP -
        0x6E => 83,  // KP .
        0x6F => 98,  // KP /
        0x70..=0x79 => 59 + (vk - 0x70), // F1-F10
        0x7A => 87,  // F11
        0x7B => 88,  // F12
        0x7C..=0x87 => 183 + (vk - 0x7C), // F13-F24
        0x90 => 69,  // NumLock
        0x91 => 70,  // ScrollLock
        0xAD => 113, // Mute
        0xAE => 114, // Volume down
        0xAF => 115, // Volume up
        0xB0 => 163, // Next track
        0xB1 => 165, // Previous track
        0xB2 => 166, // Stop
        0xB3 => 164, // Play/Pause
        0xBA => 39,  // ;
        0xBB => 13,  // =
        0xBC => 51,  // ,
        0xBD => 12,  // -
        0xBE => 52,  // .
        0xBF => 53,  // /
        0xC0 => 41,  // `
        0xDB => 26,  // [
        0xDC => 43,  // backslash
        0xDD => 27,  // ]
        0xDE => 40,  // '
        0xE2 => 86,  // OEM 102 (<>)
        0x15 => 122, // Hangul / Kana
        0x19 => 123, // Hanja
        _ => return None,
    })
}

/// US-layout evdev key and Shift requirement for printable ASCII.
pub(crate) fn ascii(c: char) -> Option<(u16, bool)> {
    let lower = |c: char| -> Option<u16> {
        Some(match c {
            'a'..='z' => evdev(c.to_ascii_uppercase() as u16)?,
            '0'..='9' => evdev(c as u16)?,
            ' ' => 57,
            '\n' => 28,
            '\t' => 15,
            '-' => 12, '=' => 13, '[' => 26, ']' => 27, '\\' => 43, ';' => 39,
            '\'' => 40, '`' => 41, ',' => 51, '.' => 52, '/' => 53,
            _ => return None,
        })
    };
    if let Some(code) = lower(c) {
        return Some((code, false));
    }
    let base = match c {
        'A'..='Z' => c.to_ascii_lowercase(),
        '!' => '1', '@' => '2', '#' => '3', '$' => '4', '%' => '5', '^' => '6',
        '&' => '7', '*' => '8', '(' => '9', ')' => '0', '_' => '-', '+' => '=',
        '{' => '[', '}' => ']', '|' => '\\', ':' => ';', '"' => '\'', '~' => '`',
        '<' => ',', '>' => '.', '?' => '/',
        _ => return None,
    };
    lower(base).map(|code| (code, true))
}

/// Linux button code for a UU button bit (1 left, 2 right, 16 middle, 32/64 X).
pub(crate) fn button(button: u8) -> Option<u16> {
    Some(match button {
        1 => 0x110,
        2 => 0x111,
        16 => 0x112,
        32 => 0x113,
        64 => 0x114,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn letters_digits_and_shifted() {
        assert_eq!(super::evdev(0x41), Some(30));
        assert_eq!(super::evdev(0x31), Some(2));
        assert_eq!(super::evdev(0x30), Some(11));
        assert_eq!(super::evdev(0x7B), Some(88));
        assert_eq!(super::ascii('A'), Some((30, true)));
        assert_eq!(super::ascii('?'), Some((53, true)));
        assert_eq!(super::ascii('z'), Some((44, false)));
    }
}
