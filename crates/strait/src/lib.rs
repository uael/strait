//! Shared core of strait: what the Mac and Windows say to each other, how
//! they protect it, and the decisions that do not depend on either OS.

pub mod keymap;
pub mod layout;
pub mod route;
pub mod session;
pub mod wire;

/// TCP port of the session and UDP port of input datagrams, on Windows.
pub const PORT: u16 = 47800;
/// UDP port where Windows broadcasts its beacon.
pub const BEACON_PORT: u16 = 47801;

const BEACON_MAGIC: &[u8; 8] = b"strait/1";

/// The beacon Windows broadcasts: a magic tag and its public key.
pub fn beacon(public: &[u8; 32]) -> [u8; 40] {
    let mut b = [0; 40];
    b[..8].copy_from_slice(BEACON_MAGIC);
    b[8..].copy_from_slice(public);
    b
}

/// The public key a beacon announces, if `bytes` is one.
pub fn parse_beacon(bytes: &[u8]) -> Option<[u8; 32]> {
    let rest = bytes.strip_prefix(BEACON_MAGIC)?;
    rest.try_into().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn beacon_round_trips() {
        let key = [7; 32];
        assert_eq!(super::parse_beacon(&super::beacon(&key)), Some(key));
        assert_eq!(super::parse_beacon(b"strait/2"), None);
        assert_eq!(super::parse_beacon(&super::beacon(&key)[..39]), None);
    }
}
