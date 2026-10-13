//! The C API the menu bar app calls. Its declarations live in
//! `mac/Sources/StraitCore/include/strait.h`.

use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use strait::session::{Identity, fingerprint};
use strait::wire::{Clip, Control};

use crate::core::{Core, Shared, preset_named};
use crate::sys::*;
use crate::{capture, link, log, open_log};

pub type StatusFn = extern "C" fn(json: *const c_char, context: *mut c_void);
pub type ClipboardFn = extern "C" fn(kind: u32, bytes: *const u8, len: usize, context: *mut c_void);

const CLIP_TEXT: u32 = 0;
const CLIP_PNG: u32 = 1;

/// The app's context pointer; the app keeps it alive for the process.
#[derive(Clone, Copy)]
struct Context(*mut c_void);
unsafe impl Send for Context {}
unsafe impl Sync for Context {}

struct Engine {
    core: Shared,
    peer_path: PathBuf,
}

static ENGINE: OnceLock<Engine> = OnceLock::new();

/// Starts the engine with its files in `dir`. Returns false if `dir` is
/// unusable or the engine already runs.
#[unsafe(no_mangle)]
pub extern "C" fn strait_start(dir: *const c_char, on_status: StatusFn, on_clipboard: ClipboardFn, context: *mut c_void) -> bool {
    if dir.is_null() || ENGINE.get().is_some() {
        return false;
    }
    let dir = PathBuf::from(unsafe { CStr::from_ptr(dir) }.to_string_lossy().into_owned());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("strait: create {}: {e}", dir.display());
        return false;
    }
    open_log(&dir.join("strait.log"));
    let identity = match Identity::load_or_create(&dir.join("identity.key")) {
        Ok(id) => id,
        Err(e) => {
            log(&format!("identity: {e}"));
            return false;
        }
    };
    let peer_path = dir.join("peer.key");
    let ctx = Context(context);
    let notify = Box::new(move |status: &crate::core::Status| {
        let json = serde_json::to_string(status).expect("status serializes");
        let json = c_string(&json);
        let ctx = ctx;
        on_status(json.as_ptr(), ctx.0);
    });
    let clipboard = Box::new(move |clip: &Clip| {
        let ctx = ctx;
        match clip {
            Clip::Text(t) => on_clipboard(CLIP_TEXT, t.as_ptr(), t.len(), ctx.0),
            Clip::Png(p) => on_clipboard(CLIP_PNG, p.as_ptr(), p.len(), ctx.0),
        }
    });
    let paired = link::paired(&peer_path).map(|k| fingerprint(&k));
    let core: Shared = Arc::new(Mutex::new(Core::new(dir.join("settings.json"), fingerprint(&identity.public), paired, notify, clipboard)));
    let _ = ENGINE.set(Engine { core: core.clone(), peer_path: peer_path.clone() });
    log(&format!("strait {} started; this Mac is {}", env!("CARGO_PKG_VERSION"), fingerprint(&identity.public)));
    allow_background_cursor_hiding();
    link::start(core.clone(), identity, peer_path);
    std::thread::Builder::new().name("watch".into()).spawn(move || watch(core)).expect("spawn watch thread");
    true
}

/// Follows permissions and Secure Input, and starts capture once macOS
/// allows it.
fn watch(core: Shared) {
    let mut capturing = false;
    loop {
        let accessibility = unsafe { AXIsProcessTrusted() };
        let input_monitoring = unsafe { IOHIDCheckAccess(kIOHIDRequestTypeListenEvent) } == kIOHIDAccessTypeGranted;
        let secure_input = unsafe { IsSecureEventInputEnabled() };
        let mut problem = None;
        if !capturing && accessibility {
            match capture::start(core.clone()) {
                Ok(()) => capturing = true,
                Err(e) => problem = Some(e),
            }
        }
        core.lock().unwrap().set_status(|s| {
            s.accessibility = accessibility;
            s.input_monitoring = input_monitoring;
            s.secure_input = secure_input;
            if problem.is_some() {
                s.problem = problem;
            }
        });
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Command {
    Preset(String),
    SwapCommandControl(bool),
    /// "windows" gives input to Windows, locked; "mac" takes it back.
    Focus(String),
    Forget(bool),
    Host(Option<String>),
    /// Publishes the status now, for a view that just appeared.
    Refresh(bool),
}

/// Runs one command, given as a JSON object such as `{"preset":"split"}`.
#[unsafe(no_mangle)]
pub extern "C" fn strait_command(json: *const c_char) {
    let Some(engine) = ENGINE.get() else { return };
    if json.is_null() {
        return;
    }
    let text = unsafe { CStr::from_ptr(json) }.to_string_lossy();
    let command: Command = match serde_json::from_str(&text) {
        Ok(c) => c,
        Err(e) => {
            log(&format!("bad command {text}: {e}"));
            return;
        }
    };
    match command {
        Command::Preset(name) => match preset_named(&name) {
            Some(p) => Core::set_preset(&engine.core, p),
            None => log(&format!("unknown preset {name}")),
        },
        Command::SwapCommandControl(on) => engine.core.lock().unwrap().set_swap(on),
        Command::Focus(to) => engine.core.lock().unwrap().focus(to == "windows"),
        Command::Forget(true) => engine.core.lock().unwrap().forget(&engine.peer_path),
        Command::Host(host) => engine.core.lock().unwrap().set_host(host.filter(|h| !h.trim().is_empty())),
        Command::Refresh(true) => engine.core.lock().unwrap().publish(),
        Command::Forget(false) | Command::Refresh(false) => {}
    }
}

/// The Mac's clipboard changed: `kind` 0 is UTF-8 text, 1 is PNG.
#[unsafe(no_mangle)]
pub extern "C" fn strait_clipboard(kind: u32, bytes: *const u8, len: usize) {
    let Some(engine) = ENGINE.get() else { return };
    if bytes.is_null() {
        return;
    }
    let data = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
    let clip = match kind {
        CLIP_TEXT => match String::from_utf8(data) {
            Ok(t) => Clip::Text(t),
            Err(_) => return,
        },
        CLIP_PNG => Clip::Png(data),
        _ => return,
    };
    engine.core.lock().unwrap().send_control(Control::Clipboard(clip));
}
