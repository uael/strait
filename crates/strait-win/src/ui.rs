//! The tray icon, its menu, and the window that receives clipboard and
//! display changes.

use std::collections::VecDeque;
use std::mem::size_of;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use strait::wire::{Control, Preset};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows_sys::Win32::System::DataExchange::{AddClipboardFormatListener, GetClipboardOwner};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    ShellExecuteW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, HICON, ICONINFO, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SW_SHOWNORMAL, SetForegroundWindow,
    TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CLIPBOARDUPDATE,
    WM_COMMAND, WM_DISPLAYCHANGE, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
};

use crate::net::{Event, Net, Status};
use crate::{clip, display, log};

const WM_TRAY: u32 = WM_APP + 1;
const WM_NET: u32 = WM_APP + 2;

const CMD_SHOW_MAC: usize = 10;
const CMD_SHOW_SPLIT: usize = 11;
const CMD_SHOW_WINDOWS: usize = 12;
const CMD_FORGET: usize = 20;
const CMD_LOG: usize = 30;
const CMD_QUIT: usize = 99;

struct Ui {
    net: OnceLock<Arc<Net>>,
    hwnd: AtomicIsize,
    events: Mutex<VecDeque<Event>>,
    status: Mutex<Status>,
    taskbar_created: u32,
    log_path: std::path::PathBuf,
}

static UI: OnceLock<Ui> = OnceLock::new();

fn ui() -> &'static Ui {
    UI.get().expect("UI initialized before its window exists")
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn copy_into(dst: &mut [u16], s: &str) {
    let units: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..units.len()].copy_from_slice(&units);
    dst[units.len()] = 0;
}

/// Hands net events to the UI thread. Safe from any thread, and before
/// the window exists: events wait in the queue until it drains them.
pub fn notifier() -> impl Fn(Event) + Send + Sync + 'static {
    |event| {
        let ui = ui();
        ui.events.lock().unwrap().push_back(event);
        let hwnd = ui.hwnd.load(Ordering::Acquire);
        if hwnd != 0 {
            unsafe { PostMessageW(hwnd as HWND, WM_NET, 0, 0) };
        }
    }
}

pub fn init(log_path: std::path::PathBuf) {
    let taskbar_created = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
    let _ = UI.set(Ui {
        net: OnceLock::new(),
        hwnd: AtomicIsize::new(0),
        events: Mutex::new(VecDeque::new()),
        status: Mutex::new(Status::Starting),
        taskbar_created,
        log_path,
    });
}

/// Runs the tray until the user quits.
pub fn run(net: Arc<Net>) {
    let ui = ui();
    let _ = ui.net.set(net);
    let class = wide("strait");
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        assert!(!hwnd.is_null(), "create window: {}", std::io::Error::last_os_error());
        ui.hwnd.store(hwnd as isize, Ordering::Release);
        AddClipboardFormatListener(hwnd);
        tray(NIM_ADD);
        PostMessageW(hwnd, WM_NET, 0, 0);
        display::fastest_refresh();
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        tray(NIM_DELETE);
    }
}

fn hwnd() -> HWND {
    ui().hwnd.load(Ordering::Acquire) as HWND
}

fn net() -> &'static Arc<Net> {
    ui().net.get().expect("net started before the window")
}

fn status_line(status: &Status, paired: bool) -> String {
    match status {
        Status::Starting => "Starting…".into(),
        Status::Waiting if paired => "Waiting for your Mac".into(),
        Status::Waiting => "Waiting for a Mac to pair".into(),
        Status::Connected { fingerprint, .. } => format!("Connected to your Mac ({fingerprint})"),
        Status::Rejected { fingerprint } => format!("Refused an unknown Mac ({fingerprint})"),
        Status::Failed(e) => e.clone(),
    }
}

/// Background color of the icon: teal when connected, amber on a problem,
/// slate while waiting.
fn tint(status: &Status) -> [u8; 3] {
    match status {
        Status::Connected { .. } => [0x0E, 0xA5, 0xA4],
        Status::Rejected { .. } | Status::Failed(_) => [0xD9, 0x77, 0x06],
        Status::Starting | Status::Waiting => [0x64, 0x74, 0x8B],
    }
}

/// Signed distance from `(x, y)` to a rounded rectangle.
fn rounded(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (x - cx).abs() - (hw - r);
    let qy = (y - cy).abs() - (hh - r);
    (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - r
}

/// Draws the icon: a tinted tile holding two screens side by side.
fn icon(status: &Status) -> HICON {
    const N: usize = 32;
    let [r, g, b] = tint(status);
    let mut bgra = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let tile = (0.5 - rounded(px, py, 16.0, 16.0, 15.0, 15.0, 7.0)).clamp(0.0, 1.0);
            let screens = [9.5f32, 22.5]
                .iter()
                .map(|&cx| (0.5 - rounded(px, py, cx, 15.0, 5.5, 7.0, 1.5)).clamp(0.0, 1.0))
                .fold(0.0f32, f32::max);
            let mix = |c: u8| (f32::from(c) * (1.0 - screens) + 255.0 * screens) as u8;
            let i = (y * N + x) * 4;
            bgra[i..i + 4].copy_from_slice(&[mix(b), mix(g), mix(r), (tile * 255.0) as u8]);
        }
    }
    unsafe {
        let color = CreateBitmap(N as i32, N as i32, 1, 32, bgra.as_ptr().cast());
        let mask_bits = vec![0u8; N * N / 8];
        let mask = CreateBitmap(N as i32, N as i32, 1, 1, mask_bits.as_ptr().cast());
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

fn tray_data() -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd();
    data.uID = 1;
    data
}

fn tray(action: u32) {
    let status = ui().status.lock().unwrap().clone();
    let mut data = tray_data();
    if action != NIM_DELETE {
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = icon(&status);
        copy_into(&mut data.szTip, &format!("strait: {}", status_line(&status, net().paired().is_some())));
    }
    unsafe {
        Shell_NotifyIconW(action, &data);
        if !data.hIcon.is_null() {
            DestroyIcon(data.hIcon);
        }
    }
}

fn balloon(title: &str, text: &str) {
    let mut data = tray_data();
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = NIIF_INFO;
    copy_into(&mut data.szInfoTitle, title);
    copy_into(&mut data.szInfo, text);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
}

fn drain_events() {
    let events: Vec<Event> = ui().events.lock().unwrap().drain(..).collect();
    for event in events {
        match event {
            Event::Status(status) => {
                let was = std::mem::replace(&mut *ui().status.lock().unwrap(), status.clone());
                tray(NIM_MODIFY);
                if let Status::Connected { fingerprint, .. } = &status
                    && !matches!(was, Status::Connected { .. })
                {
                    balloon("strait", &format!("Your Mac is connected ({fingerprint})"));
                }
            }
            Event::Clipboard(c) => {
                if let Err(e) = clip::write(hwnd(), &c) {
                    log!("set clipboard: {e}");
                }
            }
        }
    }
}

fn menu() {
    let ui = ui();
    let status = ui.status.lock().unwrap().clone();
    let connected = net().connected();
    let paired = net().paired().is_some();
    let enabled = |on: bool| if on { MF_STRING } else { MF_STRING | MF_GRAYED };
    unsafe {
        let m = CreatePopupMenu();
        AppendMenuW(m, MF_STRING | MF_GRAYED, 1, wide(&status_line(&status, paired)).as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
        for (cmd, label) in [
            (CMD_SHOW_MAC, "Monitor: Mac"),
            (CMD_SHOW_SPLIT, "Monitor: Mac + Windows"),
            (CMD_SHOW_WINDOWS, "Monitor: Windows"),
        ] {
            AppendMenuW(m, enabled(connected), cmd, wide(label).as_ptr());
        }
        AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(m, enabled(paired), CMD_FORGET, wide("Forget paired Mac").as_ptr());
        AppendMenuW(m, MF_STRING, CMD_LOG, wide("Open log").as_ptr());
        AppendMenuW(m, MF_STRING, CMD_QUIT, wide("Quit strait").as_ptr());
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        SetForegroundWindow(hwnd());
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, p.x, p.y, 0, hwnd(), std::ptr::null());
        DestroyMenu(m);
        command(cmd as usize);
    }
}

fn command(cmd: usize) {
    match cmd {
        CMD_SHOW_MAC => net().send(Control::Show(Preset::MacFull)),
        CMD_SHOW_SPLIT => net().send(Control::Show(Preset::Split)),
        CMD_SHOW_WINDOWS => net().send(Control::Show(Preset::WindowsFull)),
        CMD_FORGET => net().forget(),
        CMD_LOG => unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide("open").as_ptr(),
                wide(&ui().log_path.to_string_lossy()).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
        },
        CMD_QUIT => unsafe { PostQuitMessage(0) },
        _ => {}
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            if matches!(lparam as u32, WM_LBUTTONUP | WM_RBUTTONUP) {
                menu();
            }
            0
        }
        WM_NET => {
            drain_events();
            0
        }
        WM_CLIPBOARDUPDATE => {
            if unsafe { GetClipboardOwner() } != hwnd {
                match clip::read(hwnd) {
                    Ok(Some(c)) => net().send(Control::Clipboard(c)),
                    Ok(None) => {}
                    Err(e) => log!("read clipboard: {e}"),
                }
            }
            0
        }
        WM_DISPLAYCHANGE => {
            display::fastest_refresh();
            0
        }
        WM_COMMAND => {
            command(wparam & 0xFFFF);
            0
        }
        m if m == ui().taskbar_created => {
            tray(NIM_ADD);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
