//! Windows VK → macOS virtual keycode (CGKeyCode) for CONTROL `kbd_press`.
//!
//! UU's Android `libinputmanager` maps labels through `WindowsToMacTransformer`
//! before sending HID JSON. Linux/Windows controllers historically emit Windows
//! VKs; macOS hosts expect Carbon/CGEvent virtual key codes (not Win VK, not
//! USB HID usage IDs). Values match HIToolbox `Events.h` / Apple `kVK_*`.

#![cfg_attr(not(test), allow(dead_code))]

/// Map a Windows virtual-key code to a macOS CGKeyCode.
/// Returns `None` when there is no stable Mac equivalent (skip the event).
pub fn win_vk_to_mac(vk: u16) -> Option<u16> {
    Some(match vk {
        // Letters (VK_A..VK_Z → kVK_ANSI_*)
        0x41 => 0x00, // A
        0x53 => 0x01, // S
        0x44 => 0x02, // D
        0x46 => 0x03, // F
        0x48 => 0x04, // H
        0x47 => 0x05, // G
        0x5A => 0x06, // Z
        0x58 => 0x07, // X
        0x43 => 0x08, // C
        0x56 => 0x09, // V
        0x42 => 0x0B, // B
        0x51 => 0x0C, // Q
        0x57 => 0x0D, // W
        0x45 => 0x0E, // E
        0x52 => 0x0F, // R
        0x59 => 0x10, // Y
        0x54 => 0x11, // T
        0x31 => 0x12, // 1
        0x32 => 0x13, // 2
        0x33 => 0x14, // 3
        0x34 => 0x15, // 4
        0x36 => 0x16, // 6
        0x35 => 0x17, // 5
        0xBB => 0x18, // OEM_PLUS (=)
        0x39 => 0x19, // 9
        0x37 => 0x1A, // 7
        0xBD => 0x1B, // OEM_MINUS
        0x38 => 0x1C, // 8
        0x30 => 0x1D, // 0
        0xDD => 0x1E, // OEM_6 ]
        0x4F => 0x1F, // O
        0x55 => 0x20, // U
        0xDB => 0x21, // OEM_4 [
        0x49 => 0x22, // I
        0x50 => 0x23, // P
        0x0D => 0x24, // Return
        0x4C => 0x25, // L
        0x4A => 0x26, // J
        0xDE => 0x27, // OEM_7 '
        0x4B => 0x28, // K
        0xBA => 0x29, // OEM_1 ;
        0xDC => 0x2A, // OEM_5 \
        0xBC => 0x2B, // OEM_COMMA
        0xBF => 0x2C, // OEM_2 /
        0x4E => 0x2D, // N
        0x4D => 0x2E, // M
        0xBE => 0x2F, // OEM_PERIOD
        0x09 => 0x30, // Tab
        0x20 => 0x31, // Space
        0xC0 => 0x32, // OEM_3 `
        0x08 => 0x33, // Delete (Backspace)
        0x1B => 0x35, // Escape
        // Modifiers
        0x5B => 0x37, // LWin → Command
        0x5C => 0x36, // RWin → Right Command
        0x10 | 0xA0 => 0x38, // Shift / LShift
        0xA1 => 0x3C, // RShift
        0x14 => 0x39, // Caps Lock
        0x12 | 0xA4 => 0x3A, // Alt / LAlt → Option
        0xA5 => 0x3D, // RAlt → Right Option
        0x11 | 0xA2 => 0x3B, // Ctrl / LCtrl
        0xA3 => 0x3E, // RCtrl
        // Function keys
        0x70 => 0x7A, // F1
        0x71 => 0x78, // F2
        0x72 => 0x63, // F3
        0x73 => 0x76, // F4
        0x74 => 0x60, // F5
        0x75 => 0x61, // F6
        0x76 => 0x62, // F7
        0x77 => 0x64, // F8
        0x78 => 0x65, // F9
        0x79 => 0x6D, // F10
        0x7A => 0x67, // F11
        0x7B => 0x6F, // F12
        // Navigation / editing
        0x2D => 0x72, // Insert → Help
        0x24 => 0x73, // Home
        0x21 => 0x74, // Page Up
        0x2E => 0x75, // Delete forward
        0x23 => 0x77, // End
        0x22 => 0x79, // Page Down
        0x25 => 0x7B, // Left
        0x27 => 0x7C, // Right
        0x28 => 0x7D, // Down
        0x26 => 0x7E, // Up
        // Numpad
        0x60 => 0x52, // Numpad 0
        0x61 => 0x53, // 1
        0x62 => 0x54, // 2
        0x63 => 0x55, // 3
        0x64 => 0x56, // 4
        0x65 => 0x57, // 5
        0x66 => 0x58, // 6
        0x67 => 0x59, // 7
        0x68 => 0x5B, // 8
        0x69 => 0x5C, // 9
        0x6E => 0x41, // Decimal
        0x6A => 0x43, // Multiply
        0x6B => 0x45, // Add
        0x6D => 0x4E, // Subtract
        0x6F => 0x4B, // Divide
        0x90 => 0x47, // Num Lock → Clear
        // Misc
        0x5D => 0x6E, // Apps → contextual menu (kVK_ContextualMenu)
        0x2C => 0x69, // Print Screen → F13 (common remote mapping)
        0x91 => 0x6B, // Scroll Lock → F14
        0x13 => 0x71, // Pause → F15
        _ => return None,
    })
}

/// macOS modifier CGKeyCodes (for release ordering).
pub fn is_mac_modifier(key: u16) -> bool {
    matches!(
        key,
        0x36 | 0x37 | // Right/Left Command
        0x38 | 0x3C | // Left/Right Shift
        0x39 | // Caps Lock
        0x3A | 0x3D | // Left/Right Option
        0x3B | 0x3E | // Left/Right Control
        0x3F // Function
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_password_keys() {
        assert_eq!(win_vk_to_mac(0x41), Some(0x00)); // A
        assert_eq!(win_vk_to_mac(0x4E), Some(0x2D)); // N
        assert_eq!(win_vk_to_mac(0x08), Some(0x33)); // Backspace
        assert_eq!(win_vk_to_mac(0x0D), Some(0x24)); // Return
        assert_eq!(win_vk_to_mac(0x20), Some(0x31)); // Space
        assert_eq!(win_vk_to_mac(0xA0), Some(0x38)); // LShift
        assert_eq!(win_vk_to_mac(0xA2), Some(0x3B)); // LCtrl
    }
}
