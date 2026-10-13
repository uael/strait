//! Shared core of strait: what the Mac and Windows say to each other, how
//! they protect it, and the decisions that do not depend on either OS.

pub mod keymap;
pub mod layout;
pub mod route;
pub mod session;
pub mod wire;

/// TCP port of the session and UDP port of input datagrams, on Windows.
pub const PORT: u16 = 47800;
