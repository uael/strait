//! Authenticated, encrypted session between the Mac and Windows.
//!
//! The Mac dials Windows over TCP and both run a Noise XX handshake, which
//! proves each side's static key. Callers then decide whether they trust the
//! peer's key (pairing). One cipher pair protects both channels: control
//! frames on the TCP stream use nonces 0, 1, 2... per direction, and input
//! datagrams use their sequence number with the top bit set, so the two
//! channels never reuse a nonce.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;

use crate::wire::{Control, Input};

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"strait/1";
const TAG: usize = 16;
const MAX_NOISE: usize = 65535;
const MAX_CHUNK: usize = MAX_NOISE - TAG;
const DATAGRAM_NONCE: u64 = 1 << 63;
/// Largest control message accepted; bounds memory spent on one clipboard.
pub const MAX_CONTROL: usize = 64 << 20;

/// A long-lived static key pair, created once per machine.
#[derive(Clone)]
pub struct Identity {
    private: [u8; 32],
    pub public: [u8; 32],
}

impl Identity {
    pub fn generate() -> Identity {
        let pair = builder().generate_keypair().expect("default resolver provides X25519");
        Identity {
            private: pair.private.try_into().expect("X25519 private key is 32 bytes"),
            public: pair.public.try_into().expect("X25519 public key is 32 bytes"),
        }
    }

    /// Reads the key pair stored at `path`, creating it on first use.
    pub fn load_or_create(path: &Path) -> io::Result<Identity> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let bytes: [u8; 64] = bytes
                    .try_into()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, format!("{} is not 64 bytes", path.display())))?;
                Ok(Identity {
                    private: bytes[..32].try_into().expect("32-byte half"),
                    public: bytes[32..].try_into().expect("32-byte half"),
                })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let id = Identity::generate();
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(path, [id.private, id.public].concat())?;
                Ok(id)
            }
            Err(e) => Err(e),
        }
    }
}

/// Short human-readable form of a public key, for comparing on both screens.
pub fn fingerprint(public: &[u8; 32]) -> String {
    public[..6].chunks(2).map(|c| format!("{:02x}{:02x}", c[0], c[1])).collect::<Vec<_>>().join("-")
}

fn builder() -> snow::Builder<'static> {
    snow::Builder::new(PATTERN.parse().expect("valid Noise pattern"))
}

fn crypto_err(e: snow::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e)
}

fn write_frame(stream: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    let len = u16::try_from(bytes.len()).expect("handshake message fits u16");
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(bytes)
}

fn read_frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0; 2];
    stream.read_exact(&mut len)?;
    let mut buf = vec![0; usize::from(u16::from_le_bytes(len))];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

/// Keys of one established session. Cheap to clone and shared by the
/// threads that send and receive on its channels.
#[derive(Clone)]
pub struct Session {
    transport: Arc<snow::StatelessTransportState>,
    /// The peer's static public key, proven by the handshake.
    pub peer: [u8; 32],
}

impl Session {
    /// Runs the handshake as the Mac (`initiator`) or Windows over `stream`.
    pub fn handshake(stream: &mut (impl Read + Write), me: &Identity, initiator: bool) -> io::Result<Session> {
        let b = builder().local_private_key(&me.private).prologue(PROLOGUE);
        let mut hs = if initiator { b.build_initiator() } else { b.build_responder() }.map_err(crypto_err)?;
        let mut buf = vec![0; MAX_NOISE];
        let mut sink = vec![0; MAX_NOISE];
        let mut ours = initiator;
        while !hs.is_handshake_finished() {
            if ours {
                let n = hs.write_message(&[], &mut buf).map_err(crypto_err)?;
                write_frame(stream, &buf[..n])?;
            } else {
                let frame = read_frame(stream)?;
                hs.read_message(&frame, &mut sink).map_err(crypto_err)?;
            }
            ours = !ours;
        }
        let peer = hs
            .get_remote_static()
            .and_then(|k| k.try_into().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "peer sent no static key"))?;
        let transport = hs.into_stateless_transport_mode().map_err(crypto_err)?;
        Ok(Session { transport: Arc::new(transport), peer })
    }

    /// Encrypts one input datagram. `seq` must grow with every datagram.
    pub fn seal_input(&self, seq: u64, input: &Input) -> Vec<u8> {
        assert!(seq < DATAGRAM_NONCE, "datagram sequence exhausted");
        let mut plain = Vec::with_capacity(48);
        input.encode(&mut plain);
        let mut out = vec![0; 8 + plain.len() + TAG];
        out[..8].copy_from_slice(&seq.to_le_bytes());
        let n = self
            .transport
            .write_message(seq | DATAGRAM_NONCE, &plain, &mut out[8..])
            .expect("datagram fits one Noise message");
        out.truncate(8 + n);
        out
    }

    /// Authenticates and decodes one datagram, returning its sequence number.
    pub fn open_input(&self, datagram: &[u8]) -> io::Result<(u64, Input)> {
        if datagram.len() < 8 + TAG {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "datagram too short"));
        }
        let seq = u64::from_le_bytes(datagram[..8].try_into().expect("8 bytes"));
        if seq >= DATAGRAM_NONCE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "datagram sequence out of range"));
        }
        let mut plain = vec![0; datagram.len()];
        let n = self.transport.read_message(seq | DATAGRAM_NONCE, &datagram[8..], &mut plain).map_err(crypto_err)?;
        let input = Input::decode(&plain[..n]).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok((seq, input))
    }

    pub fn writer<W: Write>(&self, stream: W) -> ControlWriter<W> {
        ControlWriter { session: self.clone(), stream, nonce: 0 }
    }

    pub fn reader<R: Read>(&self, stream: R) -> ControlReader<R> {
        ControlReader { session: self.clone(), stream, nonce: 0 }
    }
}

/// Sends control messages: a cleartext u32 length, then the message sealed
/// in chunks of at most one Noise message each.
pub struct ControlWriter<W> {
    session: Session,
    stream: W,
    nonce: u64,
}

impl<W: Write> ControlWriter<W> {
    pub fn send(&mut self, control: &Control) -> io::Result<()> {
        let mut plain = Vec::new();
        control.encode(&mut plain);
        if plain.len() > MAX_CONTROL {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "control message too large"));
        }
        let mut out = Vec::with_capacity(4 + plain.len() + TAG * (plain.len() / MAX_CHUNK + 1));
        out.extend_from_slice(&(plain.len() as u32).to_le_bytes());
        let mut sealed = vec![0; MAX_NOISE];
        for chunk in plain.chunks(MAX_CHUNK) {
            let n = self.session.transport.write_message(self.nonce, chunk, &mut sealed).map_err(crypto_err)?;
            self.nonce += 1;
            out.extend_from_slice(&sealed[..n]);
        }
        self.stream.write_all(&out)?;
        self.stream.flush()
    }
}

pub struct ControlReader<R> {
    session: Session,
    stream: R,
    nonce: u64,
}

impl<R: Read> ControlReader<R> {
    pub fn recv(&mut self) -> io::Result<Control> {
        let mut len = [0; 4];
        self.stream.read_exact(&mut len)?;
        let len = u32::from_le_bytes(len) as usize;
        if len > MAX_CONTROL {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "control message too large"));
        }
        let mut plain = Vec::with_capacity(len);
        let mut sealed = vec![0; MAX_NOISE];
        let mut chunk = vec![0; MAX_NOISE];
        let mut left = len;
        // Every encoded message starts with a tag byte, so `len` is never 0
        // and the writer always sends at least one chunk.
        while left > 0 {
            let take = left.min(MAX_CHUNK);
            self.stream.read_exact(&mut sealed[..take + TAG])?;
            let n = self.session.transport.read_message(self.nonce, &sealed[..take + TAG], &mut chunk).map_err(crypto_err)?;
            self.nonce += 1;
            plain.extend_from_slice(&chunk[..n]);
            left -= take;
        }
        Control::decode(&plain).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Clip;
    use std::net::{TcpListener, TcpStream};

    fn pair() -> (Session, Session, TcpStream, TcpStream, Identity, Identity) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (mac, win) = (Identity::generate(), Identity::generate());
        let win_id = win.clone();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let session = Session::handshake(&mut s, &win_id, false).unwrap();
            (session, s)
        });
        let mut c = TcpStream::connect(addr).unwrap();
        let mac_session = Session::handshake(&mut c, &mac, true).unwrap();
        let (win_session, s) = server.join().unwrap();
        (mac_session, win_session, c, s, mac, win)
    }

    #[test]
    fn handshake_proves_static_keys() {
        let (m, w, _, _, mac, win) = pair();
        assert_eq!(m.peer, win.public);
        assert_eq!(w.peer, mac.public);
    }

    #[test]
    fn datagrams_authenticate_and_reject_tampering() {
        let (m, w, ..) = pair();
        let d = m.seal_input(41, &Input::Motion { dx: 1, dy: -2 });
        assert_eq!(w.open_input(&d).unwrap(), (41, Input::Motion { dx: 1, dy: -2 }));
        let mut bad = d.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(w.open_input(&bad).is_err());
        let mut renumbered = d;
        renumbered[0] ^= 1;
        assert!(w.open_input(&renumbered).is_err());
        // Windows' own datagrams use the other direction's key.
        let pong = w.seal_input(0, &Input::Pong { t: 9 });
        assert_eq!(m.open_input(&pong).unwrap().1, Input::Pong { t: 9 });
        assert!(w.open_input(&pong).is_err());
    }

    #[test]
    fn control_messages_cross_chunk_boundaries() {
        let (m, w, c, s, ..) = pair();
        let mut tx = m.writer(c);
        let mut rx = w.reader(s);
        let big = Control::Clipboard(Clip::Png((0..200_000u32).map(|i| i as u8).collect()));
        tx.send(&Control::Leave).unwrap();
        tx.send(&big).unwrap();
        tx.send(&Control::Clipboard(Clip::Text("ok".into()))).unwrap();
        assert_eq!(rx.recv().unwrap(), Control::Leave);
        assert_eq!(rx.recv().unwrap(), big);
        assert_eq!(rx.recv().unwrap(), Control::Clipboard(Clip::Text("ok".into())));
    }

    #[test]
    fn identity_persists() {
        let dir = std::env::temp_dir().join(format!("strait-id-{}", std::process::id()));
        let path = dir.join("identity.key");
        let a = Identity::load_or_create(&path).unwrap();
        let b = Identity::load_or_create(&path).unwrap();
        assert_eq!(a.public, b.public);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fingerprint_is_short_hex() {
        assert_eq!(fingerprint(&[0xAB; 32]), "abab-abab-abab");
    }
}
