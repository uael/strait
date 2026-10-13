//! A plain log file, since the app has no console.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

pub fn init(path: &Path) {
    if let Ok(file) = File::create(path) {
        let _ = LOG.set(Mutex::new(file));
    }
}

pub fn write(args: std::fmt::Arguments) {
    let Some(file) = LOG.get() else { return };
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let (h, m, s, ms) = (ms / 3_600_000 % 24, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    let mut file = file.lock().unwrap();
    let _ = writeln!(file, "{h:02}:{m:02}:{s:02}.{ms:03}Z {args}");
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::logfile::write(format_args!($($arg)*)) };
}
