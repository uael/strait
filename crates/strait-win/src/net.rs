//! Serves the Mac: accepts its session, injects its input, and hands input
//! back when the pointer leaves through an exit edge.

use std::io::{self, BufReader, BufWriter};
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use strait::session::{Identity, Session, fingerprint};
use strait::wire::{Clip, Control, Edge, Edges, Input};
use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSOR_SUPPRESSED, CURSORINFO, GetClipCursor, GetCursorInfo, GetCursorPos, GetSystemMetrics,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetCursorPos,
};

use crate::inject::{self, Held};
use crate::log;

/// Without a datagram for this long, the Mac is presumed gone and every
/// held press is released. The Mac sends its state ten times a second.
const SILENCE: Duration = Duration::from_millis(500);

/// What the tray shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Starting,
    Waiting,
    Connected { mac: String, fingerprint: String },
    Rejected { fingerprint: String },
    Failed(String),
}

/// News for the UI thread.
pub enum Event {
    Status(Status),
    Clipboard(Clip),
}

/// The established session with the paired Mac.
struct Link {
    id: u64,
    session: Session,
    tx: Sender<Control>,
    mac: IpAddr,
    stream: TcpStream,
}

/// Whether Windows has input, and what it holds.
struct Focus {
    active: bool,
    exits: Edges,
    locked: bool,
    held: Held,
    link: u64,
    last_seq: Option<u64>,
    last_input: Instant,
}

pub struct Net {
    identity: Identity,
    peer_path: PathBuf,
    link: Mutex<Option<Link>>,
    focus: Mutex<Focus>,
    notify: Box<dyn Fn(Event) + Send + Sync>,
}

impl Net {
    pub fn start(identity: Identity, peer_path: PathBuf, notify: impl Fn(Event) + Send + Sync + 'static) -> Arc<Net> {
        let net = Arc::new(Net {
            identity,
            peer_path,
            link: Mutex::new(None),
            focus: Mutex::new(Focus {
                active: false,
                exits: Edges::default(),
                locked: false,
                held: Held::default(),
                link: 0,
                last_seq: None,
                last_input: Instant::now(),
            }),
            notify: Box::new(notify),
        });
        let n = net.clone();
        std::thread::spawn(move || n.listen());
        let n = net.clone();
        std::thread::spawn(move || n.datagrams());
        announce(net.identity.public);
        net
    }

    pub fn paired(&self) -> Option<[u8; 32]> {
        std::fs::read(&self.peer_path).ok().and_then(|b| b.try_into().ok())
    }

    /// Forgets the paired Mac; the next Mac to connect pairs.
    pub fn forget(&self) {
        if let Err(e) = std::fs::remove_file(&self.peer_path) {
            log!("forget paired Mac: {e}");
        }
        self.drop_link(None);
        (self.notify)(Event::Status(Status::Waiting));
    }

    /// Sends to the Mac, if one is connected.
    pub fn send(&self, control: Control) {
        if let Some(link) = &*self.link.lock().unwrap() {
            let _ = link.tx.send(control);
        }
    }

    pub fn connected(&self) -> bool {
        self.link.lock().unwrap().is_some()
    }

    fn listen(self: Arc<Self>) {
        (self.notify)(Event::Status(Status::Starting));
        let listener = match TcpListener::bind(("0.0.0.0", strait::PORT)) {
            Ok(l) => l,
            Err(e) => {
                log!("listen on port {}: {e}", strait::PORT);
                (self.notify)(Event::Status(Status::Failed(format!("Port {} is busy: {e}", strait::PORT))));
                return;
            }
        };
        (self.notify)(Event::Status(Status::Waiting));
        let mut next_id = 1;
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    log!("accept: {e}");
                    continue;
                }
            };
            match self.accept(stream, next_id) {
                Ok(()) => next_id += 1,
                Err(e) => log!("session refused: {e}"),
            }
        }
    }

    fn accept(self: &Arc<Self>, mut stream: TcpStream, id: u64) -> io::Result<()> {
        let mac = stream.peer_addr()?.ip();
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let session = Session::handshake(&mut stream, &self.identity, false)?;
        let fp = fingerprint(&session.peer);
        match self.paired() {
            Some(key) if key != session.peer => {
                (self.notify)(Event::Status(Status::Rejected { fingerprint: fp.clone() }));
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("Mac {fp} is not the paired one")));
            }
            Some(_) => {}
            None => {
                std::fs::write(&self.peer_path, session.peer)?;
                log!("paired with Mac {fp}");
            }
        }
        stream.set_read_timeout(None)?;
        let (tx, rx) = mpsc::channel::<Control>();
        let mut writer = session.writer(BufWriter::new(stream.try_clone()?));
        std::thread::spawn(move || {
            for control in rx {
                if let Err(e) = writer.send(&control) {
                    log!("send to Mac: {e}");
                    return;
                }
            }
        });
        let mut reader = session.reader(BufReader::new(stream.try_clone()?));
        self.drop_link(Some(Link { id, session, tx, mac, stream }));
        log!("Mac {fp} connected from {mac}");
        (self.notify)(Event::Status(Status::Connected { mac: mac.to_string(), fingerprint: fp }));
        let net = self.clone();
        std::thread::spawn(move || {
            let end = loop {
                match reader.recv() {
                    Ok(control) => net.control(control),
                    Err(e) => break e,
                }
            };
            log!("session {id} ended: {end}");
            let mut link = net.link.lock().unwrap();
            if link.as_ref().is_some_and(|l| l.id == id) {
                *link = None;
                drop(link);
                net.leave();
                (net.notify)(Event::Status(Status::Waiting));
            }
        });
        Ok(())
    }

    /// Replaces the current link, closing the old one.
    fn drop_link(&self, next: Option<Link>) {
        let old = std::mem::replace(&mut *self.link.lock().unwrap(), next);
        if let Some(old) = old {
            let _ = old.stream.shutdown(Shutdown::Both);
        }
        self.leave();
    }

    fn leave(&self) {
        let mut focus = self.focus.lock().unwrap();
        focus.active = false;
        focus.held.release_all();
    }

    fn control(&self, control: Control) {
        match control {
            Control::Enter { x, y, exits, locked } => {
                let screen = virtual_screen();
                let px = screen.left + (x.clamp(0.0, 1.0) * (screen.right - screen.left - 1) as f32) as i32;
                let py = screen.top + (y.clamp(0.0, 1.0) * (screen.bottom - screen.top - 1) as f32) as i32;
                unsafe { SetCursorPos(px, py) };
                let mut focus = self.focus.lock().unwrap();
                focus.active = true;
                focus.exits = exits;
                focus.locked = locked;
                focus.last_input = Instant::now();
            }
            Control::Leave => self.leave(),
            Control::Clipboard(clip) => (self.notify)(Event::Clipboard(clip)),
            // DDC takes a few hundred milliseconds; the session keeps reading meanwhile.
            Control::Select(source) => {
                std::thread::spawn(move || {
                    if !crate::ddc::write(crate::ddc::INPUT_SOURCE, u32::from(source)) {
                        log!("no monitor took input source {source:#04x}");
                    }
                });
            }
            Control::Exit { .. } | Control::Show(_) => log!("unexpected control from the Mac: {control:?}"),
        }
    }

    fn datagrams(self: Arc<Self>) {
        let socket = match UdpSocket::bind(("0.0.0.0", strait::PORT)) {
            Ok(s) => s,
            Err(e) => {
                log!("bind UDP port {}: {e}", strait::PORT);
                (self.notify)(Event::Status(Status::Failed(format!("UDP port {} is busy: {e}", strait::PORT))));
                return;
            }
        };
        socket.set_read_timeout(Some(Duration::from_millis(100))).expect("non-zero timeout");
        let mut buf = [0u8; 2048];
        let mut pong_seq = (0u64, 0u64); // (link id, next sequence)
        loop {
            let received = socket.recv_from(&mut buf);
            let mut focus = self.focus.lock().unwrap();
            if focus.active && focus.last_input.elapsed() > SILENCE && !focus.held.is_empty() {
                log!("no input from the Mac for {SILENCE:?}; releasing held presses");
                focus.held.release_all();
            }
            let (n, from) = match received {
                Ok(r) => r,
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => continue,
                Err(e) => {
                    log!("receive datagram: {e}");
                    continue;
                }
            };
            let Some((id, session, mac, tx)) =
                self.link.lock().unwrap().as_ref().map(|l| (l.id, l.session.clone(), l.mac, l.tx.clone()))
            else {
                continue;
            };
            if from.ip() != mac {
                continue;
            }
            let (seq, input) = match session.open_input(&buf[..n]) {
                Ok(d) => d,
                Err(e) => {
                    log!("bad datagram from {from}: {e}");
                    continue;
                }
            };
            if focus.link != id {
                focus.link = id;
                focus.last_seq = None;
            }
            // Late or replayed datagrams are stale: newer ones already won.
            if focus.last_seq.is_some_and(|last| seq <= last) {
                continue;
            }
            focus.last_seq = Some(seq);
            focus.last_input = Instant::now();
            if let Input::Ping { t } = input {
                if pong_seq.0 != id {
                    pong_seq = (id, 0);
                }
                let reply = session.seal_input(pong_seq.1, &Input::Pong { t });
                pong_seq.1 += 1;
                if let Err(e) = socket.send_to(&reply, from) {
                    log!("send pong: {e}");
                }
                continue;
            }
            if !focus.active {
                continue;
            }
            match input {
                Input::Motion { dx, dy } => {
                    inject::motion(dx, dy);
                    if let Some((edge, along)) = exit_reached(&focus, dx, dy) {
                        focus.active = false;
                        focus.held.release_all();
                        let _ = tx.send(Control::Exit { edge, along });
                    }
                }
                Input::Button { button, down } => focus.held.button(button, down),
                Input::Wheel { dx, dy } => inject::wheel(dx, dy),
                Input::Key { usage, down } => focus.held.key(usage, down),
                Input::State { keys, buttons } => focus.held.reconcile(keys, buttons),
                Input::Ping { .. } | Input::Pong { .. } => {}
            }
        }
    }
}

/// Publishes this PC as a Bonjour service so the Mac finds it without
/// configuration. Windows keeps answering for it while the process lives.
fn announce(public: [u8; 32]) {
    use windows_sys::Win32::NetworkManagement::Dns::{
        DNS_QUERY_REQUEST_VERSION1, DNS_SERVICE_CANCEL, DNS_SERVICE_INSTANCE, DNS_SERVICE_REGISTER_REQUEST,
        DnsServiceConstructInstance, DnsServiceRegister,
    };

    unsafe extern "system" fn registered(status: u32, _context: *const std::ffi::c_void, _instance: *const DNS_SERVICE_INSTANCE) {
        if status == 0 {
            log!("published {} on the local network", strait::SERVICE_TYPE);
        } else {
            log!("publishing {} failed: error {status}", strait::SERVICE_TYPE);
        }
    }

    let wide = |s: &str| -> &'static [u16] { Box::leak(s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>().into_boxed_slice()) };
    let computer = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows".to_owned());
    let service = wide(&format!("{computer}.{}.local", strait::SERVICE_TYPE));
    let host = wide(&format!("{computer}.local"));
    let keys = [wide(strait::SERVICE_KEY).as_ptr()];
    let values = [wide(&strait::to_hex(&public)).as_ptr()];
    // The request and what it points to must outlive the registration,
    // which lasts as long as the process.
    let instance = unsafe {
        DnsServiceConstructInstance(service.as_ptr(), host.as_ptr(), std::ptr::null(), std::ptr::null(), strait::PORT, 0, 0, 1, keys.as_ptr(), values.as_ptr())
    };
    if instance.is_null() {
        log!("could not describe the {} service", strait::SERVICE_TYPE);
        return;
    }
    let request: &'static DNS_SERVICE_REGISTER_REQUEST = Box::leak(Box::new(DNS_SERVICE_REGISTER_REQUEST {
        Version: DNS_QUERY_REQUEST_VERSION1,
        InterfaceIndex: 0,
        pServiceInstance: instance,
        pRegisterCompletionCallback: Some(registered),
        pQueryContext: std::ptr::null_mut(),
        hCredentials: std::ptr::null_mut(),
        unicastEnabled: 0,
    }));
    let cancel: &'static mut DNS_SERVICE_CANCEL = Box::leak(Box::new(DNS_SERVICE_CANCEL { reserved: std::ptr::null_mut() }));
    let status = unsafe { DnsServiceRegister(request, cancel) };
    if status != windows_sys::Win32::Foundation::DNS_REQUEST_PENDING as u32 {
        log!("DnsServiceRegister failed: error {status}");
    }
}

fn virtual_screen() -> RECT {
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        RECT {
            left,
            top,
            right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// A game owns the pointer when it confines or hides it; edges never
/// hand input back then. A cursor merely suppressed for lack of a physical
/// mouse does not count.
fn pointer_captured(screen: &RECT) -> bool {
    let mut clip = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let clipped = unsafe { GetClipCursor(&mut clip) } != 0
        && (clip.left > screen.left || clip.top > screen.top || clip.right < screen.right || clip.bottom < screen.bottom);
    let mut info = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, flags: 0, hCursor: std::ptr::null_mut(), ptScreenPos: POINT { x: 0, y: 0 } };
    let hidden = unsafe { GetCursorInfo(&mut info) } != 0 && info.flags & (CURSOR_SHOWING | CURSOR_SUPPRESSED) == 0;
    clipped || hidden
}

/// The exit edge the pointer is pushing against, and where along it.
fn exit_reached(focus: &Focus, dx: i32, dy: i32) -> Option<(Edge, f32)> {
    if focus.locked || focus.exits == Edges::default() {
        return None;
    }
    let screen = virtual_screen();
    if pointer_captured(&screen) {
        return None;
    }
    let mut p = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut p) } == 0 {
        return None;
    }
    let (w, h) = ((screen.right - screen.left) as f32, (screen.bottom - screen.top) as f32);
    let along_y = (p.y - screen.top) as f32 / h;
    let along_x = (p.x - screen.left) as f32 / w;
    let hit = [
        (Edge::Left, dx < 0 && p.x <= screen.left, along_y),
        (Edge::Right, dx > 0 && p.x >= screen.right - 1, along_y),
        (Edge::Top, dy < 0 && p.y <= screen.top, along_x),
        (Edge::Bottom, dy > 0 && p.y >= screen.bottom - 1, along_x),
    ];
    hit.into_iter().find(|&(edge, pushing, _)| pushing && focus.exits.contains(edge)).map(|(edge, _, along)| (edge, along))
}
