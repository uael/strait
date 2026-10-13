//! Plays the Mac's side of a session from a terminal, to test the PC end to
//! end without the menu bar app: pairs with the Mac's identity, measures the
//! round trip, moves the Windows pointer and types.
//!
//!     cargo run -p strait-mac --example drive -- <pc-host> [text to type]

use std::io::{BufReader, BufWriter};
use std::net::{TcpStream, UdpSocket};
use std::time::{Duration, Instant};

use strait::keymap::usage_of_mac_key;
use strait::session::{Identity, Session, fingerprint};
use strait::wire::{Control, Edges, Datagram};

/// macOS key code of each lowercase letter, digit and space, in ASCII order.
fn mac_key(c: char) -> Option<u16> {
    const LETTERS: [u16; 26] = [
        0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E, 0x2D, 0x1F, 0x23, 0x0C, 0x0F,
        0x01, 0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06,
    ];
    match c {
        'a'..='z' => Some(LETTERS[c as usize - 'a' as usize]),
        ' ' => Some(0x31),
        _ => None,
    }
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let host = args.next().expect("usage: drive <pc-host> [text]");
    let text = args.next();
    let dir = std::path::PathBuf::from(std::env::var("HOME").expect("HOME")).join("Library/Application Support/strait");
    let me = Identity::load_or_create(&dir.join("identity.key"))?;
    let mut stream = TcpStream::connect((host.as_str(), strait::PORT))?;
    stream.set_nodelay(true)?;
    let session = Session::handshake(&mut stream, &me, true)?;
    println!("session with PC {} as Mac {}", fingerprint(&session.peer), fingerprint(&me.public));
    let mut control = session.writer(BufWriter::new(stream.try_clone()?));
    let _reader = session.reader(BufReader::new(stream));
    let udp = UdpSocket::bind(("0.0.0.0", 0))?;
    udp.connect((host.as_str(), strait::PORT))?;
    udp.set_read_timeout(Some(Duration::from_secs(1)))?;
    let mut seq = 0u64;
    let mut send = |input: Datagram| {
        udp.send(&session.seal_datagram(seq, &input)).expect("send datagram");
        seq += 1;
    };

    let start = Instant::now();
    let mut rtts = Vec::new();
    for _ in 0..20 {
        send(Datagram::Ping { t: start.elapsed().as_nanos() as u64 });
        let mut buf = [0u8; 128];
        if let Ok(n) = udp.recv(&mut buf)
            && let Ok((_, Datagram::Pong { t })) = session.open_datagram(&buf[..n])
        {
            rtts.push((start.elapsed().as_nanos() as u64 - t) as f64 / 1e6);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    rtts.sort_by(f64::total_cmp);
    if rtts.is_empty() {
        println!("no pong received");
    } else {
        println!("round trip over {} pings: min {:.2} ms, median {:.2} ms, max {:.2} ms", rtts.len(), rtts[0], rtts[rtts.len() / 2], rtts[rtts.len() - 1]);
    }

    control.send(&Control::Enter { x: 0.5, y: 0.5, exits: Edges::default(), locked: true })?;
    std::thread::sleep(Duration::from_millis(300));
    for _ in 0..10 {
        send(Datagram::Motion { dx: 10, dy: 0 });
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("entered at the center, then moved right by 10 x 10 counts");
    if let Some(text) = text {
        for c in text.chars() {
            let Some(usage) = mac_key(c).and_then(usage_of_mac_key) else { continue };
            send(Datagram::Key { usage, down: true });
            send(Datagram::Key { usage, down: false });
            std::thread::sleep(Duration::from_millis(15));
        }
        println!("typed {text:?}");
    }
    std::thread::sleep(Duration::from_millis(300));
    control.send(&Control::Leave)?;
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}
