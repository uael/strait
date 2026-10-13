//! Messages exchanged between the Mac and Windows, and their byte encoding.
//!
//! `Datagram` travels over UDP. From the Mac it carries input: losing one
//! only loses a motion delta or a transition that the next `State` repairs.
//! From Windows it carries pongs and sound, where a lost one is a short gap.
//! `Control` travels over the TCP session, which keeps order and delivery.
//! All integers are little-endian.

/// Pressed keys, as a bitmap indexed by HID keyboard usage (page 0x07).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys([u8; 32]);

impl Keys {
    pub fn set(&mut self, usage: u8, down: bool) {
        let (byte, bit) = (usize::from(usage / 8), usage % 8);
        if down {
            self.0[byte] |= 1 << bit;
        } else {
            self.0[byte] &= !(1 << bit);
        }
    }

    pub fn contains(&self, usage: u8) -> bool {
        self.0[usize::from(usage / 8)] & (1 << (usage % 8)) != 0
    }

    pub fn iter(&self) -> impl Iterator<Item = u8> + '_ {
        (0..=255u8).filter(|&u| self.contains(u))
    }
}

/// Mouse buttons, numbered as macOS numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Button {
    Left = 0,
    Right = 1,
    Middle = 2,
    Back = 3,
    Forward = 4,
}

impl Button {
    pub const ALL: [Button; 5] = [Button::Left, Button::Right, Button::Middle, Button::Back, Button::Forward];

    pub fn from_index(i: u8) -> Option<Button> {
        Button::ALL.get(usize::from(i)).copied()
    }

    pub fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Stereo frames in one `Sound` datagram: 5 ms at 48 kHz, which keeps the
/// datagram under a typical 1500-byte MTU.
pub const SOUND_FRAMES: usize = 240;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Datagram {
    /// Pointer motion in device counts, before any acceleration.
    Motion { dx: i32, dy: i32 },
    Button { button: Button, down: bool },
    /// Scroll in 1/120 notch units (Windows `WHEEL_DELTA`). Positive `dy`
    /// scrolls content up, positive `dx` scrolls right.
    Wheel { dx: i32, dy: i32 },
    /// A key changed, named by its HID keyboard usage.
    Key { usage: u8, down: bool },
    /// Everything held right now; sent periodically so lost transitions heal.
    State { keys: Keys, buttons: u8 },
    /// Round-trip probe; Windows echoes `t` back in a `Pong` datagram.
    Ping { t: u64 },
    Pong { t: u64 },
    /// What Windows plays, as interleaved 16-bit stereo samples at `rate` Hz,
    /// at most `SOUND_FRAMES` frames.
    Sound { rate: u32, samples: Vec<i16> },
}

/// Screen edges of the Windows desktop, as a bit set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Edge {
    Left = 0,
    Right = 1,
    Top = 2,
    Bottom = 3,
}

impl Edge {
    pub const ALL: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
}

impl Edges {
    pub fn with(self, edge: Edge) -> Edges {
        Edges(self.0 | 1 << edge as u8)
    }

    pub fn contains(self, edge: Edge) -> bool {
        self.0 & (1 << edge as u8) != 0
    }
}

/// Display layouts of the monitor, chosen from the Mac.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Preset {
    /// Both halves show the Mac.
    MacFull = 0,
    /// Left half shows the Mac, right half shows Windows.
    Split = 1,
    /// The whole monitor shows Windows.
    WindowsFull = 2,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::MacFull, Preset::Split, Preset::WindowsFull];
}

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    /// Windows now receives input. The pointer appears at (`x`, `y`), as
    /// fractions of the desktop. Reaching an edge in `exits` hands input back,
    /// unless `locked`.
    Enter { x: f32, y: f32, exits: Edges, locked: bool },
    /// The Mac took input back.
    Leave,
    /// The Windows pointer reached `edge`, `along` the way from its start
    /// (left or top) as a fraction.
    Exit { edge: Edge, along: f32 },
    Clipboard(Clip),
    /// Windows asks the Mac to switch the monitor to `preset`.
    Show(Preset),
    /// The Mac asks Windows to set the monitor's input source over DDC.
    /// The monitor changes the half that shows the input the command
    /// arrives on, so only Windows can give its own half back to the Mac.
    Select(u16),
    /// The Mac asks Windows to send, or stop sending, what it plays.
    Sound(bool),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clip {
    Text(String),
    /// PNG-encoded image.
    Png(Vec<u8>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    Trailing,
    Tag(u8),
    Value(u8),
    Utf8,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Truncated => write!(f, "message is truncated"),
            DecodeError::Trailing => write!(f, "message has trailing bytes"),
            DecodeError::Tag(t) => write!(f, "unknown message tag {t}"),
            DecodeError::Value(v) => write!(f, "invalid field value {v}"),
            DecodeError::Utf8 => write!(f, "text is not UTF-8"),
        }
    }
}

impl std::error::Error for DecodeError {}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.0.len() < n {
            return Err(DecodeError::Truncated);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        Ok(self.take(N)?.try_into().expect("take returns N bytes"))
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.array::<1>()?[0])
    }

    fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => Err(DecodeError::Value(v)),
        }
    }

    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.0)
    }

    fn end(self) -> Result<(), DecodeError> {
        if self.0.is_empty() { Ok(()) } else { Err(DecodeError::Trailing) }
    }
}

fn enum_from<T: Copy>(all: &[T], v: u8) -> Result<T, DecodeError> {
    all.get(usize::from(v)).copied().ok_or(DecodeError::Value(v))
}

impl Datagram {
    pub fn encode(&self, out: &mut Vec<u8>) {
        match *self {
            Datagram::Motion { dx, dy } => {
                out.push(0);
                out.extend_from_slice(&dx.to_le_bytes());
                out.extend_from_slice(&dy.to_le_bytes());
            }
            Datagram::Button { button, down } => out.extend_from_slice(&[1, button as u8, down as u8]),
            Datagram::Wheel { dx, dy } => {
                out.push(2);
                out.extend_from_slice(&dx.to_le_bytes());
                out.extend_from_slice(&dy.to_le_bytes());
            }
            Datagram::Key { usage, down } => out.extend_from_slice(&[3, usage, down as u8]),
            Datagram::State { keys, buttons } => {
                out.push(4);
                out.extend_from_slice(&keys.0);
                out.push(buttons);
            }
            Datagram::Ping { t } => {
                out.push(5);
                out.extend_from_slice(&t.to_le_bytes());
            }
            Datagram::Pong { t } => {
                out.push(6);
                out.extend_from_slice(&t.to_le_bytes());
            }
            Datagram::Sound { rate, ref samples } => {
                out.push(7);
                out.extend_from_slice(&rate.to_le_bytes());
                out.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Datagram, DecodeError> {
        let mut r = Reader(bytes);
        let datagram = match r.u8()? {
            0 => Datagram::Motion { dx: r.i32()?, dy: r.i32()? },
            1 => Datagram::Button { button: enum_from(&Button::ALL, r.u8()?)?, down: r.bool()? },
            2 => Datagram::Wheel { dx: r.i32()?, dy: r.i32()? },
            3 => Datagram::Key { usage: r.u8()?, down: r.bool()? },
            4 => Datagram::State { keys: Keys(r.array()?), buttons: r.u8()? },
            5 => Datagram::Ping { t: r.u64()? },
            6 => Datagram::Pong { t: r.u64()? },
            7 => {
                let rate = r.u32()?;
                let bytes = r.rest();
                // Whole stereo frames only.
                if bytes.len() % 4 != 0 {
                    return Err(DecodeError::Truncated);
                }
                Datagram::Sound { rate, samples: bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect() }
            }
            t => return Err(DecodeError::Tag(t)),
        };
        r.end()?;
        Ok(datagram)
    }
}

impl Control {
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Control::Enter { x, y, exits, locked } => {
                out.push(0);
                out.extend_from_slice(&x.to_le_bytes());
                out.extend_from_slice(&y.to_le_bytes());
                out.extend_from_slice(&[exits.0, *locked as u8]);
            }
            Control::Leave => out.push(1),
            Control::Exit { edge, along } => {
                out.extend_from_slice(&[2, *edge as u8]);
                out.extend_from_slice(&along.to_le_bytes());
            }
            Control::Clipboard(Clip::Text(text)) => {
                out.push(3);
                out.extend_from_slice(text.as_bytes());
            }
            Control::Clipboard(Clip::Png(png)) => {
                out.push(4);
                out.extend_from_slice(png);
            }
            Control::Show(preset) => out.extend_from_slice(&[5, *preset as u8]),
            Control::Select(source) => {
                out.push(6);
                out.extend_from_slice(&source.to_le_bytes());
            }
            Control::Sound(on) => out.extend_from_slice(&[7, *on as u8]),
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Control, DecodeError> {
        let mut r = Reader(bytes);
        let control = match r.u8()? {
            0 => Control::Enter { x: r.f32()?, y: r.f32()?, exits: Edges(r.u8()?), locked: r.bool()? },
            1 => Control::Leave,
            2 => Control::Exit { edge: enum_from(&Edge::ALL, r.u8()?)?, along: r.f32()? },
            3 => {
                let text = std::str::from_utf8(r.rest()).map_err(|_| DecodeError::Utf8)?;
                Control::Clipboard(Clip::Text(text.to_owned()))
            }
            4 => Control::Clipboard(Clip::Png(r.rest().to_vec())),
            5 => Control::Show(enum_from(&Preset::ALL, r.u8()?)?),
            6 => Control::Select(u16::from_le_bytes(r.array()?)),
            7 => Control::Sound(r.bool()?),
            t => return Err(DecodeError::Tag(t)),
        };
        r.end()?;
        Ok(control)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datagram_round_trip(input: Datagram) {
        let mut buf = Vec::new();
        input.encode(&mut buf);
        assert_eq!(Datagram::decode(&buf), Ok(input.clone()));
        assert_eq!(Datagram::decode(&buf[..buf.len() - 1]), Err(DecodeError::Truncated));
        if !matches!(input, Datagram::Sound { .. }) {
            buf.push(0);
            assert_eq!(Datagram::decode(&buf), Err(DecodeError::Trailing));
        }
    }

    fn control_round_trip(control: Control) {
        let mut buf = Vec::new();
        control.encode(&mut buf);
        assert_eq!(Control::decode(&buf), Ok(control));
    }

    #[test]
    fn inputs_round_trip() {
        let mut keys = Keys::default();
        keys.set(0x04, true);
        keys.set(0xE7, true);
        for input in [
            Datagram::Motion { dx: -3, dy: 70000 },
            Datagram::Button { button: Button::Forward, down: true },
            Datagram::Wheel { dx: 120, dy: -240 },
            Datagram::Key { usage: 0x2C, down: false },
            Datagram::State { keys, buttons: Button::Left.bit() | Button::Back.bit() },
            Datagram::Ping { t: u64::MAX - 1 },
            Datagram::Pong { t: 7 },
            Datagram::Sound { rate: 48_000, samples: vec![i16::MIN, -1, 0, i16::MAX] },
        ] {
            datagram_round_trip(input);
        }
    }

    #[test]
    fn controls_round_trip() {
        for control in [
            Control::Enter { x: 0.25, y: 1.0, exits: Edges::default().with(Edge::Left), locked: true },
            Control::Leave,
            Control::Exit { edge: Edge::Bottom, along: 0.5 },
            Control::Clipboard(Clip::Text("héllo\n".into())),
            Control::Clipboard(Clip::Png(vec![0x89, b'P', b'N', b'G'])),
            Control::Show(Preset::WindowsFull),
            Control::Select(0x0F),
            Control::Sound(true),
        ] {
            control_round_trip(control);
        }
    }

    #[test]
    fn rejects_bad_fields() {
        assert_eq!(Datagram::decode(&[9]), Err(DecodeError::Tag(9)));
        assert_eq!(Datagram::decode(&[1, 5, 0]), Err(DecodeError::Value(5)));
        assert_eq!(Datagram::decode(&[3, 4, 2]), Err(DecodeError::Value(2)));
        assert_eq!(Control::decode(&[3, 0xFF]), Err(DecodeError::Utf8));
        assert_eq!(Datagram::decode(&[]), Err(DecodeError::Truncated));
    }

    #[test]
    fn keys_track_usages() {
        let mut keys = Keys::default();
        keys.set(0xE0, true);
        keys.set(0x04, true);
        keys.set(0x04, false);
        assert_eq!(keys.iter().collect::<Vec<_>>(), vec![0xE0]);
    }
}
