//! strait for the Mac: the engine of the menu bar app, behind a C API.
//!
//! The app calls `strait_start` once, then `strait_command` and
//! `strait_clipboard`; the engine calls back with its status as JSON and
//! with clipboard contents from Windows.

#[cfg(target_os = "macos")]
mod capture;
#[cfg(target_os = "macos")]
mod core;
#[cfg(target_os = "macos")]
mod ddc;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod link;
#[cfg(target_os = "macos")]
mod screens;
#[cfg(target_os = "macos")]
mod settings;
#[cfg(target_os = "macos")]
mod sys;

use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

/// Appends a line to the app's log, and to stderr for a terminal launch.
pub fn log(line: &str) {
    eprintln!("strait: {line}");
    if let Some(file) = LOG.get() {
        let _ = writeln!(file.lock().unwrap(), "{line}");
    }
}

fn open_log(path: &std::path::Path) {
    if let Ok(file) = File::create(path) {
        let _ = LOG.set(Mutex::new(file));
    }
}
