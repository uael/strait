//! Finds the PC, keeps a session with it, and measures the round trip.

use std::io::{BufReader, BufWriter};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use strait::session::{Identity, Session, fingerprint};
use strait::wire::{Control, Datagram, Preset};

use crate::core::{Link, Shared, Uplink};
use crate::sound::Player;
use crate::log;

/// How often Windows learns what is held while it has input.
const STATE_EVERY: Duration = Duration::from_millis(100);
/// Pings go out every this many state ticks.
const PING_TICKS: u32 = 10;
/// Windows sends no sound while it plays none; after this long the Mac
/// lets go of its sound output.
const SOUND_IDLE: Duration = Duration::from_secs(1);
/// After the sound output fails to open, the Mac tries again this late.
const SOUND_RETRY: Duration = Duration::from_secs(5);

fn now_ns() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// The PC most recently found on the network: its host name and key.
pub type Heard = Arc<Mutex<Option<(String, [u8; 32])>>>;

pub fn start(core: Shared, identity: Identity, peer_path: PathBuf) {
    let heard: Heard = Arc::default();
    let h = heard.clone();
    std::thread::Builder::new().name("discovery".into()).spawn(move || crate::bonjour::browse(h)).expect("spawn discovery thread");
    std::thread::Builder::new()
        .name("link".into())
        .spawn(move || loop {
            if let Err(e) = connect(&core, &identity, &peer_path, &heard) {
                log(&format!("session: {e}"));
            }
            std::thread::sleep(Duration::from_secs(1));
        })
        .expect("spawn link thread");
}

pub fn paired(peer_path: &PathBuf) -> Option<[u8; 32]> {
    std::fs::read(peer_path).ok().and_then(|b| b.try_into().ok())
}

/// Where to dial: the configured host, else the paired PC's beacon (or,
/// before pairing, any PC's).
fn target(core: &Shared, peer_path: &PathBuf, heard: &Heard) -> Option<(String, SocketAddr)> {
    let host = core.lock().unwrap().host();
    if let Some(host) = host {
        let addr = (host.as_str(), strait::PORT).to_socket_addrs().ok()?.next()?;
        return Some((host, addr));
    }
    let (host, key) = heard.lock().unwrap().clone()?;
    if paired(peer_path).is_some_and(|p| p != key) {
        return None;
    }
    // Resolved for each attempt, so a new address from DHCP is picked up.
    let addr = match (host.as_str(), strait::PORT).to_socket_addrs().map(|mut a| a.find(SocketAddr::is_ipv4)) {
        Ok(Some(addr)) => addr,
        Ok(None) => {
            log(&format!("{host} has no IPv4 address"));
            return None;
        }
        Err(e) => {
            log(&format!("resolve {host}: {e}"));
            return None;
        }
    };
    Some((host.trim_end_matches('.').to_owned(), addr))
}

fn connect(core: &Shared, identity: &Identity, peer_path: &PathBuf, heard: &Heard) -> std::io::Result<()> {
    let Some((host, addr)) = target(core, peer_path, heard) else {
        core.lock().unwrap().set_status(|s| s.link = Link::Searching);
        return Ok(());
    };
    core.lock().unwrap().set_status(|s| s.link = Link::Connecting { host: host.clone() });
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let session = Session::handshake(&mut stream, identity, true)?;
    let fp = fingerprint(&session.peer);
    match paired(peer_path) {
        Some(key) if key != session.peer => {
            core.lock().unwrap().set_status(|s| s.link = Link::Refused { fingerprint: fp.clone() });
            std::thread::sleep(Duration::from_secs(2));
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("PC {fp} is not the paired one")));
        }
        Some(_) => {}
        None => {
            std::fs::write(peer_path, session.peer)?;
            log(&format!("paired with PC {fp}"));
        }
    }
    stream.set_read_timeout(None)?;

    let socket = Arc::new(UdpSocket::bind(("0.0.0.0", 0))?);
    socket.connect(SocketAddr::new(addr.ip(), strait::PORT))?;
    let (tx, rx) = mpsc::channel::<Control>();
    let mut writer = session.writer(BufWriter::new(stream.try_clone()?));
    std::thread::spawn(move || {
        for control in rx {
            if let Err(e) = writer.send(&control) {
                log(&format!("send to PC: {e}"));
                return;
            }
        }
    });
    let uplink = Arc::new(Uplink::new(session.clone(), socket.clone(), tx, stream.try_clone()?));
    {
        let mut c = core.lock().unwrap();
        c.link_up(uplink.clone(), host, fp.clone());
        c.set_status(|s| s.paired_pc = Some(fp));
    }
    spawn_heartbeat(core.clone(), uplink.clone());
    spawn_receiver(core.clone(), uplink.clone());

    let mut reader = session.reader(BufReader::new(stream));
    let end = loop {
        match reader.recv() {
            Ok(Control::Exit { edge, along }) => core.lock().unwrap().exit(edge, along),
            Ok(Control::Show(preset)) => {
                // Windows does not know whether PBP is on; "Windows" means
                // side by side then.
                let mut c = core.lock().unwrap();
                let preset = if c.status.pbp && preset == Preset::WindowsFull { Preset::Split } else { preset };
                c.show(preset);
            }
            Ok(Control::Clipboard(clip)) => core.lock().unwrap().clipboard_from_windows(&clip),
            Ok(other) => log(&format!("unexpected control from the PC: {other:?}")),
            Err(e) => break e,
        }
    };
    core.lock().unwrap().link_down(Link::Searching);
    Err(end)
}

fn is_current(core: &Shared, uplink: &Arc<Uplink>) -> bool {
    core.lock().unwrap().uplink().is_some_and(|u| Arc::ptr_eq(&u, uplink))
}

/// Sends what is held while Windows has input, and a ping every second.
fn spawn_heartbeat(core: Shared, uplink: Arc<Uplink>) {
    std::thread::spawn(move || {
        let mut tick = 0u32;
        while is_current(&core, &uplink) {
            let state = core.lock().unwrap().windows_state();
            if let Some((keys, buttons)) = state {
                uplink.send(Datagram::State { keys, buttons });
            }
            if tick % PING_TICKS == 0 {
                uplink.send(Datagram::Ping { t: now_ns() });
            }
            tick = tick.wrapping_add(1);
            std::thread::sleep(STATE_EVERY);
        }
    });
}

/// Reads what Windows sends: pongs, which measure the round trip, and the
/// sound it plays, which goes to the Mac's sound output.
fn spawn_receiver(core: Shared, uplink: Arc<Uplink>) {
    std::thread::spawn(move || {
        let socket = uplink.socket.clone();
        socket.set_read_timeout(Some(SOUND_IDLE / 4)).expect("non-zero timeout");
        let mut buf = [0u8; 2048];
        let mut last_seq = None;
        let mut player: Option<Player> = None;
        let mut sound_at = Instant::now();
        let mut failed_at: Option<Instant> = None;
        while is_current(&core, &uplink) {
            if player.is_some() && sound_at.elapsed() > SOUND_IDLE {
                player = None;
            }
            let Ok(n) = socket.recv(&mut buf) else { continue };
            let Ok((seq, datagram)) = uplink.session.open_datagram(&buf[..n]) else { continue };
            if last_seq.is_some_and(|l| seq <= l) {
                continue;
            }
            last_seq = Some(seq);
            match datagram {
                Datagram::Pong { t } => {
                    let rtt = (now_ns().saturating_sub(t)) as f64 / 1e6;
                    core.lock().unwrap().set_status(|s| s.rtt_ms = Some((rtt * 10.0).round() / 10.0));
                }
                Datagram::Sound { rate, samples } => {
                    sound_at = Instant::now();
                    if player.as_ref().is_none_or(|p| p.rate != rate) && failed_at.is_none_or(|t| t.elapsed() > SOUND_RETRY) {
                        player = match Player::new(rate) {
                            Ok(p) => {
                                failed_at = None;
                                Some(p)
                            }
                            Err(e) => {
                                log(&format!("play Windows sound: {e}"));
                                failed_at = Some(Instant::now());
                                None
                            }
                        };
                    }
                    if let Some(p) = &mut player {
                        p.push(&samples);
                    }
                }
                other => log(&format!("unexpected datagram from the PC: {other:?}")),
            }
        }
    });
}
