//! The Mac's decisions: which machine has input, where presses go, and
//! what the user sees in the menu bar.

use std::net::{TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, Weak};
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
    /// The monitor shows two inputs side by side (PBP), which only its own
    /// menu can turn on or off.
    pub pbp: bool,
    pub monitor: bool,
    pub rtt_ms: Option<f64>,
    pub accessibility: bool,
    pub input_monitoring: bool,
    pub secure_input: bool,
    pub swap_command_control: bool,
    pub pointer_speed: f64,
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
    me: Weak<Mutex<Core>>,
    pub status: Status,
    router: Router,
    layout: Layout,
    uplink: Option<Arc<Uplink>>,
    /// Where the Mac pointer was when Windows took input.
    parked_at: Option<Point>,
    raw_motion_at: Option<Instant>,
    /// Scaled motion not yet sent, below one count.
    motion_rest: (f64, f64),
    settings: Settings,
    settings_path: PathBuf,
    notify: Box<dyn Fn(&Status) + Send>,
    clipboard: Box<dyn Fn(&Clip) + Send>,
}

pub type Shared = Arc<Mutex<Core>>;

impl Core {
    pub fn shared(
        settings_path: PathBuf,
        this_mac: String,
        paired_pc: Option<String>,
        notify: Box<dyn Fn(&Status) + Send>,
        clipboard: Box<dyn Fn(&Clip) + Send>,
    ) -> Shared {
        let settings = Settings::load(&settings_path);
        let status = Status {
            link: Link::Searching,
            focus: Focus::Mac,
            preset: settings.preset,
            pbp: false,
            monitor: false,
            rtt_ms: None,
            accessibility: false,
            input_monitoring: false,
            secure_input: false,
            swap_command_control: settings.swap_command_control,
            pointer_speed: settings.pointer_speed,
            this_mac,
            paired_pc,
            problem: None,
        };
        Arc::new_cyclic(|me| {
            let mut core = Core {
                me: me.clone(),
                status,
                router: Router::default(),
                layout: Layout { visible: Vec::new(), windows: None },
            uplink: None,
            parked_at: None,
            raw_motion_at: None,
                motion_rest: (0.0, 0.0),
                settings,
                settings_path,
                notify,
                clipboard,
            };
            core.displays_changed();
            Mutex::new(core)
        })
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
        let Some(uplink) = self.uplink.clone() else {
            crate::log("input stays on the Mac: no PC connected");
            return;
        };
        crate::log(&format!("input to Windows at ({x:.2}, {y:.2}){}", if locked { ", locked" } else { "" }));
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
        crate::log(&format!("input back to the Mac{}", if landing.is_some() { " through an edge" } else { "" }));
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
            CGAssociateMouseAndMouseCursorPosition(true);
            CGDisplayShowCursor(0);
        }
        self.set_status(|s| s.focus = Focus::Mac);
    }

    /// The switch hotkey. Toward Windows it first puts Windows on screen
    /// if it is not; back toward the Mac it also brings the Mac back when
    /// Windows filled the screen, like a hardware KVM. Windows' edges stay
    /// open, so the pointer can still reach the Mac's displays.
    fn toggle(&mut self) {
        self.focus(!self.windows_has_input(), false);
    }

    /// Gives input to Windows or takes it back. `locked` keeps Windows'
    /// edges from handing input back, for games that leave the pointer free.
    pub fn focus(&mut self, windows: bool, locked: bool) {
        if windows {
            if self.status.preset == Preset::MacFull {
                self.show(if self.status.pbp { Preset::Split } else { Preset::WindowsFull });
            }
            if !self.windows_has_input() {
                self.give_input(0.5, 0.5, locked);
            }
        } else {
            self.take_input_back(None, true);
            if self.status.preset == Preset::WindowsFull {
                self.show(Preset::MacFull);
            }
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
        if !raw_is_fresh {
            self.send_motion(dx, dy);
        }
        true
    }

    /// Sends motion scaled by the user's speed for Windows. The fraction a
    /// datagram cannot carry waits for the next one, so slow motion is not lost.
    fn send_motion(&mut self, dx: f64, dy: f64) {
        let Some(u) = &self.uplink else { return };
        let speed = self.settings.pointer_speed;
        let (rx, ry) = (self.motion_rest.0 + dx * speed, self.motion_rest.1 + dy * speed);
        let (ix, iy) = (rx.trunc(), ry.trunc());
        self.motion_rest = (rx - ix, ry - iy);
        if ix != 0.0 || iy != 0.0 {
            u.send(Input::Motion { dx: ix as i32, dy: iy as i32 });
        }
    }

    /// Raw mouse motion, in device counts, before macOS acceleration.
    pub fn raw_motion(&mut self, dx: i32, dy: i32) {
        self.raw_motion_at = Some(Instant::now());
        if self.windows_has_input() {
            self.send_motion(f64::from(dx), f64::from(dy));
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

    /// Re-reads the displays: whether the monitor is side by side, and
    /// where Windows sits. The monitor reports neither PBP nor what each
    /// half shows, so strait keeps what it last showed, and corrects it to
    /// what PBP allows when the user switches PBP from the monitor's menu.
    pub fn displays_changed(&mut self) {
        let displays = screens::displays();
        let pbp = screens::is_pbp(&displays);
        let preset = match (pbp, self.settings.preset) {
            (true, Preset::WindowsFull) => Preset::Split,
            (false, Preset::Split) => Preset::MacFull,
            (_, p) => p,
        };
        self.settings.preset = preset;
        self.layout = screens::layout(preset, &displays);
        self.set_status(|s| {
            s.pbp = pbp;
            s.preset = preset;
        });
    }

    /// Switches the monitor to `preset` over DDC.
    ///
    /// The monitor changes the half that shows the input a command arrives
    /// on (the whole picture without PBP). The Mac sits on HDMI and Windows
    /// on DisplayPort, so the Mac gives its HDMI half to Windows, and only
    /// Windows can give its half back to the Mac.
    pub fn show(&mut self, preset: Preset) {
        use crate::settings::{MAC_INPUT, WINDOWS_INPUT};
        enum By {
            Mac(u16),
            Windows(u16),
        }
        let by = match (self.status.pbp, preset) {
            (true, Preset::WindowsFull) => Err("PBP is on: turn it off in the monitor's menu to show Windows full screen"),
            (false, Preset::Split) => Err("Turn on PBP in the monitor's menu to see both side by side"),
            (true, Preset::Split) | (false, Preset::WindowsFull) => Ok(By::Mac(WINDOWS_INPUT)),
            (true, Preset::MacFull) => Ok(By::Windows(MAC_INPUT)),
            (false, Preset::MacFull) => Ok(By::Mac(MAC_INPUT)),
        };
        let by = match by {
            Ok(by) => by,
            Err(problem) => {
                self.set_status(|s| s.problem = Some(problem.to_owned()));
                return;
            }
        };
        if preset == self.status.preset {
            return;
        }
        // A second command from the same side would change the other half.
        self.settings.preset = preset;
        self.settings.save(&self.settings_path);
        self.displays_changed();
        if preset == Preset::MacFull {
            self.take_input_back(None, true);
        }
        match by {
            By::Windows(source) => {
                crate::log(&format!("asking Windows to select monitor input {source:#04x}"));
                self.send_control(Control::Select(source));
                self.set_status(|s| s.problem = None);
            }
            By::Mac(source) => {
                let me = self.me.clone();
                std::thread::spawn(move || {
                    let ok = ddc::Monitor::find().is_some_and(|mut m| m.write(ddc::INPUT_SOURCE, source));
                    crate::log(&format!("monitor input {source:#04x}: {}", if ok { "switched" } else { "no DDC answer" }));
                    if let Some(core) = me.upgrade() {
                        core.lock().unwrap().set_status(|s| {
                            s.monitor = ok;
                            s.problem = (!ok).then(|| "The monitor did not answer DDC".to_owned());
                        });
                    }
                });
            }
        }
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

    pub fn set_pointer_speed(&mut self, speed: f64) {
        let speed = speed.clamp(crate::settings::MIN_SPEED, crate::settings::MAX_SPEED);
        self.settings.pointer_speed = speed;
        self.settings.save(&self.settings_path);
        self.set_status(|s| s.pointer_speed = speed);
    }

    pub fn set_host(&mut self, host: Option<String>) {
        self.settings.host = host;
        self.settings.save(&self.settings_path);
    }

    pub fn clipboard_from_windows(&self, clip: &Clip) {
        (self.clipboard)(clip);
    }
}
