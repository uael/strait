//! Watches the Mac's keyboard, mouse and trackpad.
//!
//! An event tap sees every event before applications do and swallows the
//! ones that belong to Windows. Raw mouse motion comes from IOKit HID
//! instead, before macOS acceleration, so games on Windows get the mouse's
//! own counts. Both run on one thread, which accumulates the raw motion of
//! a HID report and sends it as one datagram before the thread sleeps.

use std::ffi::c_void;
use std::ptr::null_mut;

use core_foundation_sys::array::{CFArrayCreate, kCFTypeArrayCallBacks};
use core_foundation_sys::base::{CFRelease, kCFAllocatorDefault};
use core_foundation_sys::dictionary::{CFDictionaryCreate, kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks};
use core_foundation_sys::mach_port::{CFMachPortCreateRunLoopSource, CFMachPortRef};
use core_foundation_sys::number::{CFNumberCreate, kCFNumberSInt32Type};
use core_foundation_sys::runloop::{
    CFRunLoopActivity, CFRunLoopAddObserver, CFRunLoopAddSource, CFRunLoopGetCurrent, CFRunLoopObserverCreate,
    CFRunLoopObserverRef, CFRunLoopRun, kCFRunLoopBeforeWaiting, kCFRunLoopCommonModes, kCFRunLoopDefaultMode,
};
use strait::keymap::{NEXT_TRACK, PLAY_PAUSE, PREVIOUS_TRACK, usage_of_mac_key};
use strait::layout::Point;
use strait::wire::Button;

use crate::core::Shared;
use crate::sys::*;

const SPACE: i64 = 0x31;
const HOTKEY_FLAGS: u64 = kCGEventFlagMaskControl | kCGEventFlagMaskAlternate | kCGEventFlagMaskCommand;

struct Capture {
    core: Shared,
    tap: CFMachPortRef,
    /// Raw motion of the HID report being delivered.
    pending: (i32, i32),
}

fn mask(types: &[u32]) -> u64 {
    types.iter().fold(0, |m, t| m | 1 << t)
}

/// Starts capture on its own thread. Fails when macOS has not granted
/// Accessibility, which the event tap needs.
pub fn start(core: Shared) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("capture".into())
        .spawn(move || {
            let capture = Box::into_raw(Box::new(Capture { core, tap: null_mut(), pending: (0, 0) }));
            let events = mask(&[
                kCGEventLeftMouseDown,
                kCGEventLeftMouseUp,
                kCGEventRightMouseDown,
                kCGEventRightMouseUp,
                kCGEventMouseMoved,
                kCGEventLeftMouseDragged,
                kCGEventRightMouseDragged,
                kCGEventKeyDown,
                kCGEventKeyUp,
                kCGEventFlagsChanged,
                NX_SYSDEFINED,
                kCGEventScrollWheel,
                kCGEventOtherMouseDown,
                kCGEventOtherMouseUp,
                kCGEventOtherMouseDragged,
            ]);
            unsafe {
                let tap = CGEventTapCreate(kCGHIDEventTap, kCGHeadInsertEventTap, kCGEventTapOptionDefault, events, on_event, capture.cast());
                if tap.is_null() {
                    let _ = tx.send(Err("macOS refused the event tap: allow strait in Accessibility".to_owned()));
                    drop(Box::from_raw(capture));
                    return;
                }
                (*capture).tap = tap;
                let run_loop = CFRunLoopGetCurrent();
                let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0);
                CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                start_raw_motion(capture);
                let mut context = core_foundation_sys::runloop::CFRunLoopObserverContext {
                    version: 0,
                    info: capture.cast(),
                    retain: None,
                    release: None,
                    copyDescription: None,
                };
                let observer = CFRunLoopObserverCreate(kCFAllocatorDefault, kCFRunLoopBeforeWaiting, 1, 0, flush, &mut context);
                CFRunLoopAddObserver(run_loop, observer, kCFRunLoopCommonModes);
                CGDisplayRegisterReconfigurationCallback(displays_changed, capture.cast());
                let _ = tx.send(Ok(()));
                CFRunLoopRun();
            }
        })
        .expect("spawn capture thread");
    rx.recv().unwrap_or_else(|_| Err("capture thread ended".to_owned()))
}

fn number(n: i32) -> *const c_void {
    unsafe { CFNumberCreate(kCFAllocatorDefault, kCFNumberSInt32Type, (&n as *const i32).cast()).cast() }
}

/// Matches mice and other pointing devices for raw motion. Without Input
/// Monitoring the manager opens empty and motion comes from the tap.
unsafe fn start_raw_motion(capture: *mut Capture) {
    unsafe {
        let page_key = CfString::new("DeviceUsagePage");
        let usage_key = CfString::new("DeviceUsage");
        let keys = [page_key.0.cast(), usage_key.0.cast()];
        // Generic desktop page (1): pointer (1) and mouse (2).
        let matches: Vec<*const c_void> = [1, 2]
            .iter()
            .map(|&usage| {
                let values = [number(1), number(usage)];
                let dict = CFDictionaryCreate(
                    kCFAllocatorDefault,
                    keys.as_ptr(),
                    values.as_ptr(),
                    2,
                    &kCFTypeDictionaryKeyCallBacks,
                    &kCFTypeDictionaryValueCallBacks,
                );
                values.iter().for_each(|v| CFRelease(*v));
                dict.cast()
            })
            .collect();
        let array = CFArrayCreate(kCFAllocatorDefault, matches.as_ptr(), matches.len() as isize, &kCFTypeArrayCallBacks);
        matches.iter().for_each(|d| CFRelease(*d));
        let manager = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
        IOHIDManagerSetDeviceMatchingMultiple(manager, array.cast());
        CFRelease(array.cast());
        IOHIDManagerRegisterInputValueCallback(manager, on_hid_value, capture.cast());
        IOHIDManagerScheduleWithRunLoop(manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
        let r = IOHIDManagerOpen(manager, kIOHIDOptionsTypeNone);
        if r != 0 {
            crate::log(&format!("raw mouse input unavailable (IOHIDManagerOpen {r:#x}); using the event tap's motion"));
        }
    }
}

extern "C" fn on_hid_value(context: *mut c_void, _result: IOReturn, _sender: *mut c_void, value: IOHIDValueRef) {
    let capture = unsafe { &mut *(context as *mut Capture) };
    unsafe {
        let element = IOHIDValueGetElement(value);
        if IOHIDElementGetUsagePage(element) != 1 || !IOHIDElementIsRelative(element) {
            return;
        }
        let v = IOHIDValueGetIntegerValue(value) as i32;
        match IOHIDElementGetUsage(element) {
            0x30 => capture.pending.0 += v,
            0x31 => capture.pending.1 += v,
            _ => {}
        }
    }
}

extern "C" fn flush(_observer: CFRunLoopObserverRef, _activity: CFRunLoopActivity, info: *mut c_void) {
    let capture = unsafe { &mut *(info as *mut Capture) };
    let (dx, dy) = std::mem::take(&mut capture.pending);
    if dx != 0 || dy != 0 {
        capture.core.lock().unwrap().raw_motion(dx, dy);
    }
}

extern "C" fn displays_changed(_display: CGDirectDisplayID, _flags: u32, info: *mut c_void) {
    let capture = unsafe { &*(info as *const Capture) };
    capture.core.lock().unwrap().displays_changed();
}

/// Whether the modifier key `code` is down, per the event's flags.
fn modifier_down(code: i64, flags: u64) -> Option<bool> {
    let bit = match code {
        0x38 => NX_DEVICELSHIFTKEYMASK,
        0x3C => NX_DEVICERSHIFTKEYMASK,
        0x3B => NX_DEVICELCTLKEYMASK,
        0x3E => NX_DEVICERCTLKEYMASK,
        0x3A => NX_DEVICELALTKEYMASK,
        0x3D => NX_DEVICERALTKEYMASK,
        0x37 => NX_DEVICELCMDKEYMASK,
        0x36 => NX_DEVICERCMDKEYMASK,
        0x39 => return Some(flags & kCGEventFlagMaskAlphaShift != 0),
        _ => return None,
    };
    Some(flags & bit != 0)
}

/// HID usage of a media key reported as a system-defined event.
fn media_usage(key: i64) -> Option<u8> {
    match key {
        0 => Some(0x80),  // volume up
        1 => Some(0x81),  // volume down
        7 => Some(0x7F),  // mute
        16 => Some(PLAY_PAUSE),
        17 => Some(NEXT_TRACK),
        18 => Some(PREVIOUS_TRACK),
        _ => None,
    }
}

#[allow(non_upper_case_globals)]
extern "C" fn on_event(_proxy: *mut c_void, ty: u32, event: CGEventRef, info: *mut c_void) -> CGEventRef {
    let capture = unsafe { &mut *(info as *mut Capture) };
    if ty == kCGEventTapDisabledByTimeout || ty == kCGEventTapDisabledByUserInput {
        unsafe { CGEventTapEnable(capture.tap, true) };
        return event;
    }
    let field = |f| unsafe { CGEventGetIntegerValueField(event, f) };
    let mut core = capture.core.lock().unwrap();
    let swallow = match ty {
        kCGEventKeyDown | kCGEventKeyUp => {
            let code = field(kCGKeyboardEventKeycode);
            let down = ty == kCGEventKeyDown;
            let flags = unsafe { CGEventGetFlags(event) };
            let hotkey = code == SPACE && flags & HOTKEY_FLAGS == HOTKEY_FLAGS && field(kCGKeyboardEventAutorepeat) == 0;
            match usage_of_mac_key(code as u16) {
                Some(usage) => core.key(usage, down, hotkey),
                None => core.windows_has_input(),
            }
        }
        kCGEventFlagsChanged => {
            let code = field(kCGKeyboardEventKeycode);
            let flags = unsafe { CGEventGetFlags(event) };
            match (usage_of_mac_key(code as u16), modifier_down(code, flags)) {
                // Caps Lock reports each press as a change of its lock state;
                // Windows wants a tap per press.
                (Some(0x39), Some(_)) => core.key(0x39, true, false) | core.key(0x39, false, false),
                (Some(usage), Some(down)) => core.key(usage, down, false),
                _ => false,
            }
        }
        kCGEventLeftMouseDown | kCGEventLeftMouseUp => core.button(Button::Left, ty == kCGEventLeftMouseDown),
        kCGEventRightMouseDown | kCGEventRightMouseUp => core.button(Button::Right, ty == kCGEventRightMouseDown),
        kCGEventOtherMouseDown | kCGEventOtherMouseUp => match Button::from_index(field(kCGMouseEventButtonNumber) as u8) {
            Some(b) => core.button(b, ty == kCGEventOtherMouseDown),
            None => false,
        },
        kCGEventMouseMoved | kCGEventLeftMouseDragged | kCGEventRightMouseDragged | kCGEventOtherMouseDragged => {
            let p = unsafe { CGEventGetLocation(event) };
            core.pointer(Point { x: p.x, y: p.y }, field(kCGMouseEventDeltaX) as f64, field(kCGMouseEventDeltaY) as f64)
        }
        kCGEventScrollWheel => {
            let (dx, dy) = if field(kCGScrollWheelEventIsContinuous) != 0 {
                // Trackpads scroll in points. Windows counts 120 per wheel
                // notch, which scrolls about 40 points.
                (field(kCGScrollWheelEventPointDeltaAxis2) * 3, field(kCGScrollWheelEventPointDeltaAxis1) * 3)
            } else {
                (field(kCGScrollWheelEventDeltaAxis2) * 120, field(kCGScrollWheelEventDeltaAxis1) * 120)
            };
            // macOS counts horizontal scroll toward the left as positive.
            core.wheel(-dx as i32, dy as i32)
        }
        NX_SYSDEFINED => {
            let (subtype, data) = system_event_data(event);
            // Subtype 8 carries the auxiliary keys: key code, then a state byte (0x0A down, 0x0B up).
            match (subtype, media_usage((data >> 16) & 0xFFFF)) {
                (8, Some(usage)) => core.key(usage, (data >> 8) & 0xFF == 0x0A, false),
                _ => false,
            }
        }
        _ => false,
    };
    if swallow { null_mut() } else { event }
}
