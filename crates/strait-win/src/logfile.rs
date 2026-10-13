//! A plain log file, since the app has no console.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

pub fn init(path: &Path) {
    if let Ok(file) = File::create(path) {
        let _ = LOG.set(Mutex::new(file));
    }
}

pub fn write(args: std::fmt::Arguments) {
    let Some(file) = LOG.get() else { return };
    let _ = writeln!(file.lock().unwrap(), "{} {args}", strait::timestamp());
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::logfile::write(format_args!($($arg)*)) };
}
