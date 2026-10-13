//! Which machine each key and button press belongs to.
//!
//! A press goes to the machine that has input when it happens, and its
//! release always follows it there, even if input moved in between. So
//! neither machine ever sees a release without its press, or keeps a key
//! down forever.

use crate::wire::{Button, Keys};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Mac,
    Windows,
    /// Nobody: the press started the switch hotkey, or its Windows side was
    /// already released when input came back.
    Dropped,
}

#[derive(Debug, Default)]
pub struct Router {
    windows_keys: Keys,
    windows_buttons: u8,
    dropped_keys: Keys,
    dropped_buttons: u8,
}

impl Router {
    pub fn press_key(&mut self, usage: u8, to: Target) -> Target {
        match to {
            Target::Windows => self.windows_keys.set(usage, true),
            Target::Dropped => self.dropped_keys.set(usage, true),
            Target::Mac => {}
        }
        to
    }

    pub fn release_key(&mut self, usage: u8) -> Target {
        if self.windows_keys.contains(usage) {
            self.windows_keys.set(usage, false);
            Target::Windows
        } else if self.dropped_keys.contains(usage) {
            self.dropped_keys.set(usage, false);
            Target::Dropped
        } else {
            Target::Mac
        }
    }

    pub fn press_button(&mut self, button: Button, to: Target) -> Target {
        match to {
            Target::Windows => self.windows_buttons |= button.bit(),
            Target::Dropped => self.dropped_buttons |= button.bit(),
            Target::Mac => {}
        }
        to
    }

    pub fn release_button(&mut self, button: Button) -> Target {
        if self.windows_buttons & button.bit() != 0 {
            self.windows_buttons &= !button.bit();
            Target::Windows
        } else if self.dropped_buttons & button.bit() != 0 {
            self.dropped_buttons &= !button.bit();
            Target::Dropped
        } else {
            Target::Mac
        }
    }

    /// What Windows should hold right now.
    pub fn windows_state(&self) -> (Keys, u8) {
        (self.windows_keys, self.windows_buttons)
    }

    /// Input left Windows, which releases everything it held on its own.
    /// The later physical releases of those presses go nowhere.
    pub fn leave_windows(&mut self) {
        for usage in self.windows_keys.iter() {
            self.dropped_keys.set(usage, true);
        }
        self.dropped_buttons |= self.windows_buttons;
        self.windows_keys = Keys::default();
        self.windows_buttons = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_follows_press_across_a_switch() {
        let mut r = Router::default();
        assert_eq!(r.press_key(0xE0, Target::Mac), Target::Mac);
        assert_eq!(r.press_key(0x04, Target::Windows), Target::Windows);
        assert_eq!(r.release_key(0xE0), Target::Mac);
        assert_eq!(r.release_key(0x04), Target::Windows);
        assert_eq!(r.windows_state().0, Keys::default());
    }

    #[test]
    fn leaving_windows_drops_held_presses() {
        let mut r = Router::default();
        r.press_key(0xE2, Target::Windows);
        r.press_button(Button::Left, Target::Windows);
        r.leave_windows();
        assert_eq!(r.windows_state(), (Keys::default(), 0));
        assert_eq!(r.release_key(0xE2), Target::Dropped);
        assert_eq!(r.release_button(Button::Left), Target::Dropped);
        // Once consumed, the next release of the same key is the Mac's.
        assert_eq!(r.release_key(0xE2), Target::Mac);
    }
}
