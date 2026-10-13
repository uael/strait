//! The Windows clipboard, in the forms strait carries: text and images.

use std::io;

use strait::wire::Clip;
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};

use crate::dib;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn png_format() -> u32 {
    unsafe { RegisterClipboardFormatW(wide("PNG").as_ptr()) }
}

/// Holds the clipboard open; another program may hold it briefly.
struct Open;

impl Open {
    fn new(owner: HWND) -> io::Result<Open> {
        for _ in 0..10 {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(Open);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Err(io::Error::last_os_error())
    }

    fn bytes(&self, format: u32) -> Option<Vec<u8>> {
        if unsafe { IsClipboardFormatAvailable(format) } == 0 {
            return None;
        }
        let handle = unsafe { GetClipboardData(format) };
        if handle.is_null() {
            return None;
        }
        unsafe {
            let ptr = GlobalLock(handle) as *const u8;
            if ptr.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(ptr, GlobalSize(handle)).to_vec();
            GlobalUnlock(handle);
            Some(bytes)
        }
    }

    fn put(&self, format: u32, bytes: &[u8]) -> io::Result<()> {
        unsafe {
            let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if mem.is_null() {
                return Err(io::Error::last_os_error());
            }
            let ptr = GlobalLock(mem) as *mut u8;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            GlobalUnlock(mem);
            if SetClipboardData(format, mem as HANDLE).is_null() {
                let e = io::Error::last_os_error();
                GlobalFree(mem);
                return Err(e);
            }
        }
        Ok(())
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

/// What the clipboard holds, preferring text when both text and an image
/// are offered (as spreadsheets do).
pub fn read(owner: HWND) -> io::Result<Option<Clip>> {
    let open = Open::new(owner)?;
    if let Some(bytes) = open.bytes(u32::from(CF_UNICODETEXT)) {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
        return Ok(Some(Clip::Text(String::from_utf16_lossy(&units))));
    }
    if let Some(png) = open.bytes(png_format()) {
        return Ok(Some(Clip::Png(png)));
    }
    if let Some(dib) = open.bytes(u32::from(CF_DIB)) {
        return dib::dib_to_png(&dib).map(|png| Some(Clip::Png(png)));
    }
    Ok(None)
}

/// Replaces the clipboard; `owner` becomes its owner, which tells the
/// change listener that this change came from the Mac.
pub fn write(owner: HWND, clip: &Clip) -> io::Result<()> {
    let open = Open::new(owner)?;
    unsafe { EmptyClipboard() };
    match clip {
        Clip::Text(text) => {
            let units = wide(&text.replace("\r\n", "\n").replace('\n', "\r\n"));
            let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
            open.put(u32::from(CF_UNICODETEXT), &bytes)
        }
        Clip::Png(png) => {
            open.put(png_format(), png)?;
            open.put(u32::from(CF_DIB), &dib::png_to_dib(png)?)
        }
    }
}
