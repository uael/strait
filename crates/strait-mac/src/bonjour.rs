//! Finds the PC on the local network through Bonjour. The system's
//! mDNSResponder does the networking, so the Mac's firewall stays out of it.

use std::ffi::{CStr, c_char, c_void};
use std::time::Duration;

use crate::link::Heard;
use crate::log;

type DNSServiceRef = *mut c_void;

type BrowseReply = extern "C" fn(
    sd: DNSServiceRef,
    flags: u32,
    interface: u32,
    error: i32,
    name: *const c_char,
    regtype: *const c_char,
    domain: *const c_char,
    context: *mut c_void,
);

type ResolveReply = extern "C" fn(
    sd: DNSServiceRef,
    flags: u32,
    interface: u32,
    error: i32,
    fullname: *const c_char,
    host: *const c_char,
    port: u16,
    txt_len: u16,
    txt: *const u8,
    context: *mut c_void,
);

const FLAGS_ADD: u32 = 0x2;

unsafe extern "C" {
    fn DNSServiceBrowse(
        sd: *mut DNSServiceRef,
        flags: u32,
        interface: u32,
        regtype: *const c_char,
        domain: *const c_char,
        reply: BrowseReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceResolve(
        sd: *mut DNSServiceRef,
        flags: u32,
        interface: u32,
        name: *const c_char,
        regtype: *const c_char,
        domain: *const c_char,
        reply: ResolveReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceProcessResult(sd: DNSServiceRef) -> i32;
    fn DNSServiceRefSockFD(sd: DNSServiceRef) -> i32;
    fn DNSServiceRefDeallocate(sd: DNSServiceRef);
}

/// A service instance seen by the browser.
struct Found {
    interface: u32,
    name: Vec<u8>,
    regtype: Vec<u8>,
    domain: Vec<u8>,
}

fn owned(s: *const c_char) -> Vec<u8> {
    unsafe { CStr::from_ptr(s) }.to_bytes_with_nul().to_vec()
}

extern "C" fn on_browse(
    _sd: DNSServiceRef,
    flags: u32,
    interface: u32,
    error: i32,
    name: *const c_char,
    regtype: *const c_char,
    domain: *const c_char,
    context: *mut c_void,
) {
    if error != 0 {
        log(&format!("Bonjour browse reported error {error}"));
        return;
    }
    if flags & FLAGS_ADD == 0 {
        return;
    }
    let found = unsafe { &mut *(context as *mut Vec<Found>) };
    found.push(Found { interface, name: owned(name), regtype: owned(regtype), domain: owned(domain) });
}

extern "C" fn on_resolve(
    _sd: DNSServiceRef,
    _flags: u32,
    _interface: u32,
    error: i32,
    _fullname: *const c_char,
    host: *const c_char,
    _port: u16,
    txt_len: u16,
    txt: *const u8,
    context: *mut c_void,
) {
    if error != 0 {
        log(&format!("Bonjour resolve reported error {error}"));
        return;
    }
    let out = unsafe { &mut *(context as *mut Option<(String, [u8; 32])>) };
    let txt = unsafe { std::slice::from_raw_parts(txt, usize::from(txt_len)) };
    let host = unsafe { CStr::from_ptr(host) }.to_string_lossy().into_owned();
    if let Some(key) = txt_value(txt, strait::SERVICE_KEY).and_then(|v| strait::key_from_hex(&v)) {
        *out = Some((host, key));
    }
}

/// Value of `key` in a DNS TXT record: length-prefixed `key=value` strings.
fn txt_value(mut txt: &[u8], key: &str) -> Option<String> {
    while let Some((&len, rest)) = txt.split_first() {
        let (entry, next) = rest.split_at(usize::from(len).min(rest.len()));
        if let Some(value) = entry.strip_prefix(key.as_bytes()).and_then(|v| v.strip_prefix(b"=")) {
            return String::from_utf8(value.to_vec()).ok();
        }
        txt = next;
    }
    None
}

/// Waits until the service has a reply to process, or `timeout` passes.
fn readable(sd: DNSServiceRef, timeout: Duration) -> bool {
    let mut fd = libc::pollfd { fd: unsafe { DNSServiceRefSockFD(sd) }, events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut fd, 1, timeout.as_millis() as i32) > 0 }
}

fn resolve(found: &Found) -> Option<(String, [u8; 32])> {
    let mut out: Option<(String, [u8; 32])> = None;
    let mut sd: DNSServiceRef = std::ptr::null_mut();
    let err = unsafe {
        DNSServiceResolve(
            &mut sd,
            0,
            found.interface,
            found.name.as_ptr().cast(),
            found.regtype.as_ptr().cast(),
            found.domain.as_ptr().cast(),
            on_resolve,
            (&mut out as *mut Option<(String, [u8; 32])>).cast(),
        )
    };
    if err != 0 {
        return None;
    }
    if readable(sd, Duration::from_secs(3)) {
        unsafe { DNSServiceProcessResult(sd) };
    }
    unsafe { DNSServiceRefDeallocate(sd) };
    out
}

/// Browses for the PC for as long as the app runs.
pub fn browse(heard: Heard) {
    let mut found: Vec<Found> = Vec::new();
    let mut sd: DNSServiceRef = std::ptr::null_mut();
    let regtype = crate::sys::c_string(strait::SERVICE_TYPE);
    let err = unsafe {
        DNSServiceBrowse(&mut sd, 0, 0, regtype.as_ptr(), c"local.".as_ptr(), on_browse, (&mut found as *mut Vec<Found>).cast())
    };
    if err != 0 {
        log(&format!("Bonjour browse failed: error {err}"));
        return;
    }
    loop {
        if readable(sd, Duration::from_secs(60)) && unsafe { DNSServiceProcessResult(sd) } != 0 {
            log("Bonjour browse ended");
            return;
        }
        for f in found.drain(..) {
            let name = String::from_utf8_lossy(&f.name[..f.name.len() - 1]).into_owned();
            let Some(pc) = resolve(&f) else {
                log(&format!("found {name}, but it did not resolve to a strait PC"));
                continue;
            };
            log(&format!("found PC {} at {}", strait::session::fingerprint(&pc.1), pc.0));
            *heard.lock().unwrap() = Some(pc);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_txt_values() {
        let txt = b"\x05a=b=c\x06key=ab\x00";
        assert_eq!(super::txt_value(txt, "key"), Some("ab".into()));
        assert_eq!(super::txt_value(txt, "a"), Some("b=c".into()));
        assert_eq!(super::txt_value(txt, "k"), None);
    }
}
