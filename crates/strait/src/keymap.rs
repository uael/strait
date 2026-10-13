//! Key names on both machines: macOS virtual key codes to HID keyboard
//! usages on the Mac, HID usages to PC scan codes (set 1) on Windows.
//!
//! The HID keyboard page leaves 0xE8 to 0xFF reserved; strait uses three of
//! them for media keys, which macOS reports outside the keyboard.

pub const PLAY_PAUSE: u8 = 0xE8;
pub const NEXT_TRACK: u8 = 0xE9;
pub const PREVIOUS_TRACK: u8 = 0xEA;

/// HID usage of a macOS virtual key code (`kVK_*`).
pub fn usage_of_mac_key(code: u16) -> Option<u8> {
    Some(match code {
        0x00 => 0x04, // A
        0x01 => 0x16, // S
        0x02 => 0x07, // D
        0x03 => 0x09, // F
        0x04 => 0x0B, // H
        0x05 => 0x0A, // G
        0x06 => 0x1D, // Z
        0x07 => 0x1B, // X
        0x08 => 0x06, // C
        0x09 => 0x19, // V
        0x0A => 0x64, // ISO section, the extra ISO key
        0x0B => 0x05, // B
        0x0C => 0x14, // Q
        0x0D => 0x1A, // W
        0x0E => 0x08, // E
        0x0F => 0x15, // R
        0x10 => 0x1C, // Y
        0x11 => 0x17, // T
        0x12 => 0x1E, // 1
        0x13 => 0x1F, // 2
        0x14 => 0x20, // 3
        0x15 => 0x21, // 4
        0x16 => 0x23, // 6
        0x17 => 0x22, // 5
        0x18 => 0x2E, // =
        0x19 => 0x26, // 9
        0x1A => 0x24, // 7
        0x1B => 0x2D, // -
        0x1C => 0x25, // 8
        0x1D => 0x27, // 0
        0x1E => 0x30, // ]
        0x1F => 0x12, // O
        0x20 => 0x18, // U
        0x21 => 0x2F, // [
        0x22 => 0x0C, // I
        0x23 => 0x13, // P
        0x24 => 0x28, // Return
        0x25 => 0x0F, // L
        0x26 => 0x0D, // J
        0x27 => 0x34, // '
        0x28 => 0x0E, // K
        0x29 => 0x33, // ;
        0x2A => 0x31, // backslash
        0x2B => 0x36, // ,
        0x2C => 0x38, // /
        0x2D => 0x11, // N
        0x2E => 0x10, // M
        0x2F => 0x37, // .
        0x30 => 0x2B, // Tab
        0x31 => 0x2C, // Space
        0x32 => 0x35, // `
        0x33 => 0x2A, // Delete (backspace)
        0x35 => 0x29, // Escape
        0x36 => 0xE7, // right Command
        0x37 => 0xE3, // left Command
        0x38 => 0xE1, // left Shift
        0x39 => 0x39, // Caps Lock
        0x3A => 0xE2, // left Option
        0x3B => 0xE0, // left Control
        0x3C => 0xE5, // right Shift
        0x3D => 0xE6, // right Option
        0x3E => 0xE4, // right Control
        0x40 => 0x6C, // F17
        0x41 => 0x63, // keypad .
        0x43 => 0x55, // keypad *
        0x45 => 0x57, // keypad +
        0x47 => 0x53, // keypad Clear, at Num Lock's place
        0x48 => 0x80, // volume up
        0x49 => 0x81, // volume down
        0x4A => 0x7F, // mute
        0x4B => 0x54, // keypad /
        0x4C => 0x58, // keypad Enter
        0x4E => 0x56, // keypad -
        0x4F => 0x6D, // F18
        0x50 => 0x6E, // F19
        0x51 => 0x67, // keypad =
        0x52 => 0x62, // keypad 0
        0x53 => 0x59, // keypad 1
        0x54 => 0x5A, // keypad 2
        0x55 => 0x5B, // keypad 3
        0x56 => 0x5C, // keypad 4
        0x57 => 0x5D, // keypad 5
        0x58 => 0x5E, // keypad 6
        0x59 => 0x5F, // keypad 7
        0x5A => 0x6F, // F20
        0x5B => 0x60, // keypad 8
        0x5C => 0x61, // keypad 9
        0x5D => 0x89, // JIS yen
        0x5E => 0x87, // JIS underscore (ro)
        0x5F => 0x85, // JIS keypad comma
        0x60 => 0x3E, // F5
        0x61 => 0x3F, // F6
        0x62 => 0x40, // F7
        0x63 => 0x3C, // F3
        0x64 => 0x41, // F8
        0x65 => 0x42, // F9
        0x66 => 0x91, // JIS eisu
        0x67 => 0x44, // F11
        0x68 => 0x90, // JIS kana
        0x69 => 0x68, // F13
        0x6A => 0x6B, // F16
        0x6B => 0x69, // F14
        0x6D => 0x43, // F10
        0x6E => 0x65, // context menu
        0x6F => 0x45, // F12
        0x71 => 0x6A, // F15
        0x72 => 0x49, // Help, at Insert's place
        0x73 => 0x4A, // Home
        0x74 => 0x4B, // Page Up
        0x75 => 0x4C, // forward delete
        0x76 => 0x3D, // F4
        0x77 => 0x4D, // End
        0x78 => 0x3B, // F2
        0x79 => 0x4E, // Page Down
        0x7A => 0x3A, // F1
        0x7B => 0x50, // left arrow
        0x7C => 0x4F, // right arrow
        0x7D => 0x51, // down arrow
        0x7E => 0x52, // up arrow
        _ => return None,
    })
}

/// A PC scan code (set 1); `extended` keys carry the E0 prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanCode {
    pub code: u16,
    pub extended: bool,
}

const fn plain(code: u16) -> Option<ScanCode> {
    Some(ScanCode { code, extended: false })
}

const fn ext(code: u16) -> Option<ScanCode> {
    Some(ScanCode { code, extended: true })
}

/// Scan code that types the key with HID usage `usage` on a PC keyboard.
pub fn scan_code(usage: u8) -> Option<ScanCode> {
    const LETTERS: [u16; 26] = [
        0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19, 0x10, 0x13,
        0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
    ];
    match usage {
        0x04..=0x1D => plain(LETTERS[usize::from(usage - 0x04)]),
        0x1E..=0x27 => plain(u16::from(usage - 0x1E) + 0x02), // 1..9, 0
        0x28 => plain(0x1C),
        0x29 => plain(0x01),
        0x2A => plain(0x0E),
        0x2B => plain(0x0F),
        0x2C => plain(0x39),
        0x2D => plain(0x0C),
        0x2E => plain(0x0D),
        0x2F => plain(0x1A),
        0x30 => plain(0x1B),
        0x31 | 0x32 => plain(0x2B),
        0x33 => plain(0x27),
        0x34 => plain(0x28),
        0x35 => plain(0x29),
        0x36 => plain(0x33),
        0x37 => plain(0x34),
        0x38 => plain(0x35),
        0x39 => plain(0x3A),
        0x3A..=0x43 => plain(u16::from(usage - 0x3A) + 0x3B), // F1..F10
        0x44 => plain(0x57),
        0x45 => plain(0x58),
        0x46 => ext(0x37),
        0x47 => plain(0x46),
        0x49 => ext(0x52),
        0x4A => ext(0x47),
        0x4B => ext(0x49),
        0x4C => ext(0x53),
        0x4D => ext(0x4F),
        0x4E => ext(0x51),
        0x4F => ext(0x4D),
        0x50 => ext(0x4B),
        0x51 => ext(0x50),
        0x52 => ext(0x48),
        0x53 => plain(0x45),
        0x54 => ext(0x35),
        0x55 => plain(0x37),
        0x56 => plain(0x4A),
        0x57 => plain(0x4E),
        0x58 => ext(0x1C),
        0x59 => plain(0x4F),
        0x5A => plain(0x50),
        0x5B => plain(0x51),
        0x5C => plain(0x4B),
        0x5D => plain(0x4C),
        0x5E => plain(0x4D),
        0x5F => plain(0x47),
        0x60 => plain(0x48),
        0x61 => plain(0x49),
        0x62 => plain(0x52),
        0x63 => plain(0x53),
        0x64 => plain(0x56),
        0x65 => ext(0x5D),
        0x67 => plain(0x59),
        0x68..=0x6F => plain(u16::from(usage - 0x68) + 0x64), // F13..F20
        0x70 => plain(0x6C),
        0x71 => plain(0x6D),
        0x72 => plain(0x6E),
        0x73 => plain(0x76),
        0x7F => ext(0x20),
        0x80 => ext(0x30),
        0x81 => ext(0x2E),
        0x85 => plain(0x7E),
        0x87 => plain(0x73),
        0x89 => plain(0x7D),
        0x90 => plain(0x72),
        0x91 => plain(0x71),
        0xE0 => plain(0x1D),
        0xE1 => plain(0x2A),
        0xE2 => plain(0x38),
        0xE3 => ext(0x5B),
        0xE4 => ext(0x1D),
        0xE5 => plain(0x36),
        0xE6 => ext(0x38),
        0xE7 => ext(0x5C),
        PLAY_PAUSE => ext(0x22),
        NEXT_TRACK => ext(0x19),
        PREVIOUS_TRACK => ext(0x10),
        _ => None,
    }
}

/// Exchanges Command and Control, for typing Windows shortcuts with ⌘.
pub fn swap_command_control(usage: u8) -> u8 {
    match usage {
        0xE0 => 0xE3,
        0xE3 => 0xE0,
        0xE4 => 0xE7,
        0xE7 => 0xE4,
        u => u,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mac_key_reaches_a_scan_code() {
        for code in 0..=0x7Fu16 {
            if let Some(usage) = usage_of_mac_key(code) {
                assert!(scan_code(usage).is_some(), "mac key {code:#x} maps to usage {usage:#x} with no scan code");
            }
        }
    }

    #[test]
    fn mac_keys_map_to_distinct_usages() {
        let mut seen = std::collections::HashMap::new();
        for code in 0..=0x7Fu16 {
            if let Some(usage) = usage_of_mac_key(code) {
                assert_eq!(seen.insert(usage, code), None, "usage {usage:#x} used twice");
            }
        }
    }

    #[test]
    fn known_keys() {
        assert_eq!(scan_code(usage_of_mac_key(0x00).unwrap()), plain(0x1E)); // A
        assert_eq!(scan_code(usage_of_mac_key(0x0C).unwrap()), plain(0x10)); // Q
        assert_eq!(scan_code(usage_of_mac_key(0x1D).unwrap()), plain(0x0B)); // 0
        assert_eq!(scan_code(usage_of_mac_key(0x7E).unwrap()), ext(0x48)); // up
        assert_eq!(scan_code(usage_of_mac_key(0x37).unwrap()), ext(0x5B)); // ⌘ to left Windows key
        assert_eq!(scan_code(usage_of_mac_key(0x6F).unwrap()), plain(0x58)); // F12
        assert_eq!(scan_code(usage_of_mac_key(0x69).unwrap()), plain(0x64)); // F13
    }

    #[test]
    fn swap_is_an_involution() {
        for u in 0..=255u8 {
            assert_eq!(swap_command_control(swap_command_control(u)), u);
        }
    }
}
