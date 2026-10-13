//! The Mac's decisions: which machine has input, where presses go, and
//! what the user sees in the menu bar.

use std::net::{TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use strait::keymap::swap_command_control;
use strait::layout::{Layout, Point};
use strait::route::{Router, Target};
use strait::session::Session;
use strait::wire::{Button, Clip, Control, Edge, Input, Keys, Preset};

use crate::sys::*;
use crate::{ddc, screens, settings::Settings};

/// The established session with Windows, as the capture thread uses it.
pub struct Uplink {
    pub session: Session,
    pub socket: Arc<UdpSocket>,
    pub control: Sender<Control>,
    stream: TcpStream,
    seq: AtomicU64,
}

impl Uplink {
    pub fn new(session: Session, socket: Arc<UdpSocket>, control: Sender<Control>, stream: TcpStream) -> Uplink {
        Uplink { session, socket, control, stream, seq: AtomicU64::new(0) }
    }

    /// Ends the session; the link thread then dials again.
    pub fn close(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    pub fn send(&self, input: Input) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        // A full socket buffer drops the datagram, like a lost packet.
        let _ = self.socket.send(&self.session.seal_input(seq, &input));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Focus {
    Mac,
    Windows,
    /// Windows has input until the hotkey; edges do not hand it back.
    WindowsLocked,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Link {
    Searching,
    Connecting { host: String },
    Connected { host: String, fingerprint: String },
    Refused { fingerprint: String },
}

/// Everything the menu bar shows, sent whole on every change.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub link: Link,
    pub focus: Focus,
    #[serde(serialize_with = "serialize_preset")]
    pub preset: Preset,
    pub monitor: bool,
    pub rtt_ms: Option<f64>,
    pub accessibility: bool,
    pub input_monitoring: bool,
    pub secure_input: bool,
    pub swap_command_control: bool,
    pub this_mac: String,
    pub paired_pc: Option<String>,
    pub problem: Option<String>,
}

fn serialize_preset<S: serde::Serializer>(p: &Preset, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(preset_name(*p))
}

pub fn preset_name(p: Preset) -> &'static str {
    match p {
        Preset::MacFull => "mac",
        Preset::Split => "split",
        Preset::WindowsFull => "windows",
    }
}

pub fn preset_named(name: &str) -> Option<Preset> {
    Preset::ALL.into_iter().find(|&p| preset_name(p) == name)
}

/// Raw mouse motion is trusted for this long after its last report; past
/// it, motion comes from the event tap (the trackpad has no raw report).
const RAW_MOTION_FRESH: std::time::Duration = std::time::Duration::from_millis(50);

pub struct Core {
    pub status: Status,
    router: Router,
    layout: Layout,
    uplink: Option<Arc<Uplink>>,
    /// Where the Mac pointer was when Windows took input.
    parked_at: Option<Point>,
    raw_motion_at: Option<Instant>,
    settings: Settings,
    settings_path: PathBuf,
    notify: Box<dyn Fn(&Status) + Send>,
    clipboard: Box<dyn Fn(&Clip) + Send>,
}

pub type Shared = Arc<Mutex<Core>>;

impl Core {
    pub fn new(
        settings_path: PathBuf,
        this_mac: String,
        paired_pc: Option<String>,
        notify: Box<dyn Fn(&Status) + Send>,
        clipboard: Box<dyn Fn(&Clip) + Send>,
    ) -> Core {
        let settings = Settings::load(&settings_path);
        let status = Status {
            link: Link::Searching,
            focus: Focus::Mac,
            preset: settings.preset,
            monitor: false,
            rtt_ms: None,
            accessibility: false,
            input_monitoring: false,
            secure_input: false,
            swap_command_control: settings.swap_command_control,
            this_mac,
            paired_pc,
            problem: None,
        };
        let layout = screens::layout(settings.preset, &screens::displays());
        Core {
            status,
            router: Router::default(),
            layout,
            uplink: None,
            parked_at: None,
            raw_motion_at: None,
            settings,
            settings_path,
            notify,
            clipboard,
        }
    }

    pub fn publish(&self) {
        (self.notify)(&self.status);
    }

    pub fn set_status(&mut self, change: impl FnOnce(&mut Status)) {
        let before = self.status.clone();
        change(&mut self.status);
        if self.status != before {
            self.publish();
        }
    }

    pub fn host(&self) -> Option<String> {
        self.settings.host.clone()
    }

    pub fn windows_has_input(&self) -> bool {
        self.status.focus != Focus::Mac
    }

    fn out(&self, usage: u8) -> u8 {
        if self.settings.swap_command_control { swap_command_control(usage) } else { usage }
    }

    // ---- link ----

    pub fn link_up(&mut self, uplink: Arc<Uplink>, host: String, fingerprint: String) {
        self.uplink = Some(uplink);
        self.set_status(|s| s.link = Link::Connected { host, fingerprint });
    }

    pub fn link_down(&mut self, next: Link) {
        self.uplink = None;
        if self.windows_has_input() {
            self.take_input_back(None, false);
        }
        self.set_status(|s| {
            s.link = next;
            s.rtt_ms = None;
        });
    }

    pub fn uplink(&self) -> Option<Arc<Uplink>> {
        self.uplink.clone()
    }

    pub fn send_control(&self, control: Control) {
        if let Some(u) = &self.uplink {
            let _ = u.control.send(control);
        }
    }

    // ---- focus ----

    /// Gives input to Windows; the pointer appears at (`x`, `y`) of its desktop.
    fn give_input(&mut self, x: f32, y: f32, locked: bool) {
        let Some(uplink) = self.uplink.clone() else { return };
        self.parked_at = Some(screens::pointer());
        let _ = uplink.control.send(Control::Enter { x, y, exits: self.layout.exits(), locked });
        unsafe {
            CGAssociateMouseAndMouseCursorPosition(false);
            CGDisplayHideCursor(0);
        }
        self.set_status(|s| s.focus = if locked { Focus::WindowsLocked } else { Focus::Windows });
    }

    /// Takes input back, putting the pointer at `landing` or where it was.
    pub fn take_input_back(&mut self, landing: Option<Point>, tell_windows: bool) {
        if !self.windows_has_input() {
            return;
        }
        if tell_windows {
            self.send_control(Control::Leave);
        }
        self.router.leave_windows();
        let target = landing
            .or(self.parked_at.take())
            .filter(|p| self.layout.is_visible(*p))
            .unwrap_or_else(screens::main_center);
        unsafe {
            CGWarpMouseCursorPosition(CGPoint { x: target.x, y: target.y });
            // Reconnecting also cancels the pause macOS imposes after a warp.
            CGAssociateMouseAndMouseCursorPosition(true);
            CGDisplayShowCursor(0);
        }
        self.set_status(|s| s.focus = Focus::Mac);
    }

    /// The switch hotkey: Windows takes input, locked; or the Mac takes it back.
    fn toggle(&mut self) {
        if self.windows_has_input() {
            self.take_input_back(None, true);
        } else {
            self.give_input(0.5, 0.5, true);
        }
    }

    /// Gives input to Windows, locked, or takes it back, from the menu.
    pub fn focus(&mut self, windows: bool) {
        if windows && !self.windows_has_input() {
            self.give_input(0.5, 0.5, true);
        } else if !windows {
            self.take_input_back(None, true);
        }
    }

    /// Windows reports its pointer left through an exit edge.
    pub fn exit(&mut self, edge: Edge, along: f32) {
        let landing = self.layout.landing(edge, along);
        self.take_input_back(landing, false);
    }

    // ---- capture: each returns whether the Mac must not see the event ----

    pub fn key(&mut self, usage: u8, down: bool, hotkey: bool) -> bool {
        if down && hotkey {
            self.router.press_key(usage, Target::Dropped);
            self.toggle();
            return true;
        }
        let target = if down {
            let to = if self.windows_has_input() { Target::Windows } else { Target::Mac };
            self.router.press_key(usage, to)
        } else {
            self.router.release_key(usage)
        };
        if target == Target::Windows
            && let Some(u) = &self.uplink
        {
            u.send(Input::Key { usage: self.out(usage), down });
        }
        target != Target::Mac
    }

    pub fn button(&mut self, button: Button, down: bool) -> bool {
        let target = if down {
            let to = if self.windows_has_input() { Target::Windows } else { Target::Mac };
            self.router.press_button(button, to)
        } else {
            self.router.release_button(button)
        };
        if target == Target::Windows
            && let Some(u) = &self.uplink
        {
            u.send(Input::Button { button, down });
        }
        target != Target::Mac
    }

    pub fn wheel(&mut self, dx: i32, dy: i32) -> bool {
        if !self.windows_has_input() {
            return false;
        }
        if let Some(u) = &self.uplink
            && (dx != 0 || dy != 0)
        {
            u.send(Input::Wheel { dx, dy });
        }
        true
    }

    /// Pointer motion seen by the event tap, at `p`, by (`dx`, `dy`) points.
    pub fn pointer(&mut self, p: Point, dx: f64, dy: f64) -> bool {
        if !self.windows_has_input() {
            if self.uplink.is_some()
                && let Some((x, y)) = self.layout.entry(p, dx, dy)
            {
                self.give_input(x, y, false);
                return true;
            }
            return false;
        }
        let raw_is_fresh = self.raw_motion_at.is_some_and(|t| t.elapsed() < RAW_MOTION_FRESH);
        if !raw_is_fresh
            && let Some(u) = &self.uplink
        {
            u.send(Input::Motion { dx: dx.round() as i32, dy: dy.round() as i32 });
        }
        true
    }

    /// Raw mouse motion, in device counts, before macOS acceleration.
    pub fn raw_motion(&mut self, dx: i32, dy: i32) {
        self.raw_motion_at = Some(Instant::now());
        if self.windows_has_input()
            && let Some(u) = &self.uplink
        {
            u.send(Input::Motion { dx, dy });
        }
    }

    /// What Windows should hold, for the periodic state datagram.
    pub fn windows_state(&self) -> Option<(Keys, u8)> {
        if !self.windows_has_input() {
            return None;
        }
        let (keys, buttons) = self.router.windows_state();
        let mut out = Keys::default();
        for usage in keys.iter() {
            out.set(self.out(usage), true);
        }
        Some((out, buttons))
    }

    // ---- monitor and settings ----

    pub fn displays_changed(&mut self) {
        self.layout = screens::layout(self.status.preset, &screens::displays());
    }

    /// Switches the monitor, then places Windows for that preset.
    pub fn set_preset(core: &Shared, preset: Preset) {
        {
            let mut c = core.lock().unwrap();
            c.settings.preset = preset;
            c.settings.save(&c.settings_path);
            c.layout = screens::layout(preset, &screens::displays());
            c.set_status(|s| s.preset = preset);
        }
        let core = core.clone();
        std::thread::spawn(move || {
            let source = match preset {
                Preset::MacFull => Some(crate::settings::MAC_INPUT),
                Preset::WindowsFull => Some(crate::settings::WINDOWS_INPUT),
                // Side by side is the monitor's PBP mode, which DDC cannot
                // switch; the user turns it on from the monitor's menu.
                Preset::Split => None,
            };
            let Some(source) = source else { return };
            let ok = ddc::Monitor::find().is_some_and(|mut m| m.write(ddc::INPUT_SOURCE, source));
            core.lock().unwrap().set_status(|s| {
                s.monitor = ok;
                s.problem = (!ok).then(|| "The monitor did not answer DDC".to_owned());
            });
        });
    }

    pub fn set_swap(&mut self, on: bool) {
        self.settings.swap_command_control = on;
        self.settings.save(&self.settings_path);
        self.set_status(|s| s.swap_command_control = on);
    }

    /// Forgets the paired PC and drops the session; the next PC heard pairs.
    pub fn forget(&mut self, peer_path: &std::path::Path) {
        if let Err(e) = std::fs::remove_file(peer_path) {
            crate::log(&format!("forget paired PC: {e}"));
        }
        if let Some(u) = &self.uplink {
            u.close();
        }
        self.set_status(|s| s.paired_pc = None);
    }

    pub fn set_host(&mut self, host: Option<String>) {
        self.settings.host = host;
        self.settings.save(&self.settings_path);
    }

    pub fn clipboard_from_windows(&self, clip: &Clip) {
        (self.clipboard)(clip);
    }
}
