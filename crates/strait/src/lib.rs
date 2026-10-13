//! Shared core of strait: what the Mac and Windows say to each other, how
//! they protect it, and the decisions that do not depend on either OS.

pub mod keymap;
pub mod layout;
pub mod route;
pub mod session;
pub mod wire;

/// TCP port of the session and UDP port of input datagrams, on Windows.
pub const PORT: u16 = 47800;
/// Bonjour service type Windows publishes, so the Mac finds it with no
/// configuration. Its TXT record `key` holds the PC's public key in hex,
/// which lets the Mac recognize its paired PC.
pub const SERVICE_TYPE: &str = "_strait._tcp";
pub const SERVICE_KEY: &str = "key";

/// Time of day in UTC to the millisecond, the prefix of each log line on
/// both machines so their logs line up.
pub fn timestamp() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let (h, m, s, ms) = (ms / 3_600_000 % 24, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}Z")
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn key_from_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut key = [0; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys_round_trip_through_hex() {
        let key: [u8; 32] = std::array::from_fn(|i| i as u8 * 7);
        assert_eq!(super::key_from_hex(&super::to_hex(&key)), Some(key));
        assert_eq!(super::key_from_hex("zz"), None);
        assert_eq!(super::key_from_hex(&"g".repeat(64)), None);
    }
}
