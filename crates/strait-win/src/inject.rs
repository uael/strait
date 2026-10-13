//! Synthesized input on the interactive desktop, and the presses strait
//! holds there.

use std::mem::size_of;

use strait::keymap::scan_code;
use strait::wire::{Button, Keys};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, SendInput,
};

/// `XBUTTON1` and `XBUTTON2`, the back and forward buttons.
const XBUTTON1: u32 = 1;
const XBUTTON2: u32 = 2;
/// An unassigned virtual key. Tapping it while Windows or Alt is held keeps
/// their release from opening the Start menu or a menu bar.
const VK_MASK: u16 = 0xE8;

fn send(inputs: &[INPUT]) {
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        crate::log!("SendInput sent {sent} of {}: {}", inputs.len(), std::io::Error::last_os_error());
    }
}

fn mouse(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn keyboard(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

/// Relative motion: Windows applies its own pointer speed to the desktop
/// cursor, and games reading raw input get the Mac's device counts.
pub fn motion(dx: i32, dy: i32) {
    send(&[mouse(dx, dy, 0, MOUSEEVENTF_MOVE)]);
}

pub fn wheel(dx: i32, dy: i32) {
    if dy != 0 {
        send(&[mouse(0, 0, dy as u32, MOUSEEVENTF_WHEEL)]);
    }
    if dx != 0 {
        send(&[mouse(0, 0, dx as u32, MOUSEEVENTF_HWHEEL)]);
    }
}

fn button_input(button: Button, down: bool) -> INPUT {
    let (flags, data) = match (button, down) {
        (Button::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (Button::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (Button::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (Button::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (Button::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (Button::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (Button::Back, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
        (Button::Back, false) => (MOUSEEVENTF_XUP, XBUTTON1),
        (Button::Forward, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
        (Button::Forward, false) => (MOUSEEVENTF_XUP, XBUTTON2),
    };
    mouse(0, 0, data, flags)
}

/// Presses strait has injected and not yet released.
#[derive(Default)]
pub struct Held {
    keys: Keys,
    buttons: u8,
}

impl Held {
    /// Injects a key transition. A repeated press is injected again, since
    /// Windows does not auto-repeat synthesized keys.
    pub fn key(&mut self, usage: u8, down: bool) {
        let Some(sc) = scan_code(usage) else {
            crate::log!("no scan code for HID usage {usage:#04x}");
            return;
        };
        if !down && !self.keys.contains(usage) {
            return;
        }
        self.keys.set(usage, down);
        let mut flags = KEYEVENTF_SCANCODE;
        if sc.extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        send(&[keyboard(0, sc.code, flags)]);
    }

    pub fn button(&mut self, button: Button, down: bool) {
        if down == (self.buttons & button.bit() != 0) {
            return;
        }
        self.buttons ^= button.bit();
        send(&[button_input(button, down)]);
    }

    /// Converges on the Mac's view of what is held.
    pub fn reconcile(&mut self, keys: Keys, buttons: u8) {
        for usage in self.keys.iter().filter(|&u| !keys.contains(u)).collect::<Vec<_>>() {
            self.key(usage, false);
        }
        for usage in keys.iter().filter(|&u| !self.keys.contains(u)).collect::<Vec<_>>() {
            self.key(usage, true);
        }
        for button in Button::ALL {
            self.button(button, buttons & button.bit() != 0);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.keys == Keys::default() && self.buttons == 0
    }

    pub fn release_all(&mut self) {
        // Left and right Alt, left and right Windows keys.
        if [0xE2, 0xE6, 0xE3, 0xE7].iter().any(|&u| self.keys.contains(u)) {
            send(&[keyboard(VK_MASK, 0, 0), keyboard(VK_MASK, 0, KEYEVENTF_KEYUP)]);
        }
        self.reconcile(Keys::default(), 0);
    }
}
