//! macOS functions strait calls that have no Rust crate it depends on:
//! Core Graphics events and displays, IOKit HID, DDC over IOAVService,
//! display HDR through SkyLight, and two Objective-C entry points.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{CString, c_char, c_void};

use core_foundation_sys::base::{CFAllocatorRef, CFIndex, CFRelease, CFTypeRef, kCFAllocatorDefault};
use core_foundation_sys::dictionary::CFDictionaryRef;
use core_foundation_sys::mach_port::CFMachPortRef;
use core_foundation_sys::runloop::CFRunLoopRef;
use core_foundation_sys::string::{CFStringCreateWithBytes, CFStringRef, kCFStringEncodingUTF8};

pub type CGEventRef = *mut c_void;
pub type CGDirectDisplayID = u32;
pub type IOReturn = i32;
pub type IOHIDManagerRef = *mut c_void;
pub type IOHIDValueRef = *mut c_void;
pub type IOHIDElementRef = *mut c_void;
pub type IOAVServiceRef = *mut c_void;
pub type io_object_t = u32;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

pub type CGEventTapCallBack = extern "C" fn(proxy: *mut c_void, ty: u32, event: CGEventRef, info: *mut c_void) -> CGEventRef;
pub type CGDisplayReconfigurationCallBack = extern "C" fn(display: CGDirectDisplayID, flags: u32, info: *mut c_void);
pub type IOHIDValueCallback = extern "C" fn(context: *mut c_void, result: IOReturn, sender: *mut c_void, value: IOHIDValueRef);

// Event tap placement and options.
pub const kCGHIDEventTap: u32 = 0;
pub const kCGHeadInsertEventTap: u32 = 0;
pub const kCGEventTapOptionDefault: u32 = 0;

// Event types.
pub const kCGEventLeftMouseDown: u32 = 1;
pub const kCGEventLeftMouseUp: u32 = 2;
pub const kCGEventRightMouseDown: u32 = 3;
pub const kCGEventRightMouseUp: u32 = 4;
pub const kCGEventMouseMoved: u32 = 5;
pub const kCGEventLeftMouseDragged: u32 = 6;
pub const kCGEventRightMouseDragged: u32 = 7;
pub const kCGEventKeyDown: u32 = 10;
pub const kCGEventKeyUp: u32 = 11;
pub const kCGEventFlagsChanged: u32 = 12;
pub const NX_SYSDEFINED: u32 = 14;
pub const kCGEventScrollWheel: u32 = 22;
pub const kCGEventOtherMouseDown: u32 = 25;
pub const kCGEventOtherMouseUp: u32 = 26;
pub const kCGEventOtherMouseDragged: u32 = 27;
pub const kCGEventTapDisabledByTimeout: u32 = 0xFFFF_FFFE;
pub const kCGEventTapDisabledByUserInput: u32 = 0xFFFF_FFFF;

// Event fields.
pub const kCGMouseEventButtonNumber: u32 = 3;
pub const kCGMouseEventDeltaX: u32 = 4;
pub const kCGMouseEventDeltaY: u32 = 5;
pub const kCGKeyboardEventAutorepeat: u32 = 8;
pub const kCGKeyboardEventKeycode: u32 = 9;
pub const kCGScrollWheelEventDeltaAxis1: u32 = 11;
pub const kCGScrollWheelEventDeltaAxis2: u32 = 12;
pub const kCGScrollWheelEventIsContinuous: u32 = 88;
pub const kCGScrollWheelEventPointDeltaAxis1: u32 = 96;
pub const kCGScrollWheelEventPointDeltaAxis2: u32 = 97;

// Modifier flags, device independent and per side.
pub const kCGEventFlagMaskAlphaShift: u64 = 1 << 16;
pub const kCGEventFlagMaskControl: u64 = 1 << 18;
pub const kCGEventFlagMaskAlternate: u64 = 1 << 19;
pub const kCGEventFlagMaskCommand: u64 = 1 << 20;
pub const NX_DEVICELCTLKEYMASK: u64 = 0x0001;
pub const NX_DEVICELSHIFTKEYMASK: u64 = 0x0002;
pub const NX_DEVICERSHIFTKEYMASK: u64 = 0x0004;
pub const NX_DEVICELCMDKEYMASK: u64 = 0x0008;
pub const NX_DEVICERCMDKEYMASK: u64 = 0x0010;
pub const NX_DEVICELALTKEYMASK: u64 = 0x0020;
pub const NX_DEVICERALTKEYMASK: u64 = 0x0040;
pub const NX_DEVICERCTLKEYMASK: u64 = 0x2000;

// HID.
pub const kIOHIDOptionsTypeNone: u32 = 0;
pub const kIOHIDRequestTypeListenEvent: u32 = 1;
pub const kIOHIDAccessTypeGranted: u32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        info: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    pub fn CGEventGetFlags(event: CGEventRef) -> u64;
    pub fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    pub fn CGEventCreate(source: *mut c_void) -> CGEventRef;
    pub fn CGWarpMouseCursorPosition(p: CGPoint) -> i32;
    pub fn CGEventSourceCreate(state: i32) -> *mut c_void;
    pub fn CGEventSourceSetLocalEventsSuppressionInterval(source: *mut c_void, seconds: f64);
    pub fn CGAssociateMouseAndMouseCursorPosition(connected: bool) -> i32;
    pub fn CGDisplayHideCursor(display: CGDirectDisplayID) -> i32;
    pub fn CGDisplayShowCursor(display: CGDirectDisplayID) -> i32;
    pub fn CGMainDisplayID() -> CGDirectDisplayID;
    pub fn CGGetActiveDisplayList(max: u32, displays: *mut CGDirectDisplayID, count: *mut u32) -> i32;
    pub fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
    pub fn CGDisplayIsBuiltin(display: CGDirectDisplayID) -> u32;
    pub fn CGDisplayRegisterReconfigurationCallback(cb: CGDisplayReconfigurationCallBack, info: *mut c_void) -> i32;
    pub fn AXIsProcessTrusted() -> bool;
    // Private: lets a background app hide the cursor.
    pub fn _CGSDefaultConnection() -> i32;
    pub fn CGSSetConnectionProperty(cid: i32, target: i32, key: CFStringRef, value: CFTypeRef) -> i32;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    pub fn IsSecureEventInputEnabled() -> bool;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    pub fn IOHIDManagerCreate(allocator: CFAllocatorRef, options: u32) -> IOHIDManagerRef;
    pub fn IOHIDManagerSetDeviceMatchingMultiple(manager: IOHIDManagerRef, multiple: *const c_void);
    pub fn IOHIDManagerRegisterInputValueCallback(manager: IOHIDManagerRef, callback: IOHIDValueCallback, context: *mut c_void);
    pub fn IOHIDManagerScheduleWithRunLoop(manager: IOHIDManagerRef, run_loop: CFRunLoopRef, mode: CFStringRef);
    pub fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: u32) -> IOReturn;
    pub fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
    pub fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> CFIndex;
    pub fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
    pub fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
    pub fn IOHIDElementIsRelative(element: IOHIDElementRef) -> bool;
    pub fn IOHIDCheckAccess(request: u32) -> u32;

    pub fn IOServiceMatching(name: *const c_char) -> CFDictionaryRef;
    pub fn IOServiceGetMatchingServices(main_port: u32, matching: CFDictionaryRef, iterator: *mut io_object_t) -> IOReturn;
    pub fn IOIteratorNext(iterator: io_object_t) -> io_object_t;
    pub fn IOObjectRelease(object: io_object_t) -> IOReturn;
    pub fn IORegistryEntryCreateCFProperty(entry: io_object_t, key: CFStringRef, allocator: CFAllocatorRef, options: u32) -> CFTypeRef;

    // Private: DDC/CI over the display's I2C bus on Apple silicon.
    pub fn IOAVServiceCreateWithService(allocator: CFAllocatorRef, service: io_object_t) -> IOAVServiceRef;
    pub fn IOAVServiceReadI2C(service: IOAVServiceRef, chip: u32, offset: u32, out: *mut u8, len: u32) -> IOReturn;
    pub fn IOAVServiceWriteI2C(service: IOAVServiceRef, chip: u32, offset: u32, data: *const u8, len: u32) -> IOReturn;
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

/// A CFString that releases itself.
pub struct CfString(pub CFStringRef);

impl CfString {
    pub fn new(s: &str) -> CfString {
        CfString(unsafe { CFStringCreateWithBytes(kCFAllocatorDefault, s.as_ptr(), s.len() as CFIndex, kCFStringEncodingUTF8, 0) })
    }
}

impl Drop for CfString {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0.cast()) };
    }
}

/// The `subtype` and `data1` of a system-defined event, which is how
/// macOS reports media keys.
pub fn system_event_data(event: CGEventRef) -> (i16, i64) {
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let class = objc_getClass(c"NSEvent".as_ptr());
        let with_cg: extern "C" fn(*mut c_void, *mut c_void, CGEventRef) -> *mut c_void = std::mem::transmute(objc_msgSend as *const ());
        let ns_event = with_cg(class, sel_registerName(c"eventWithCGEvent:".as_ptr()), event);
        let result = if ns_event.is_null() {
            (0, 0)
        } else {
            let get_i16: extern "C" fn(*mut c_void, *mut c_void) -> i16 = std::mem::transmute(objc_msgSend as *const ());
            let get_i64: extern "C" fn(*mut c_void, *mut c_void) -> i64 = std::mem::transmute(objc_msgSend as *const ());
            (get_i16(ns_event, sel_registerName(c"subtype".as_ptr())), get_i64(ns_event, sel_registerName(c"data1".as_ptr())))
        };
        objc_autoreleasePoolPop(pool);
        result
    }
}

/// Removes the pause macOS puts on local mouse input after a warp, which
/// otherwise freezes the pointer for a quarter second each time it comes
/// back from Windows. The setting belongs to the combined session state,
/// so the source is kept for the life of the process.
pub fn no_pause_after_warp() {
    const COMBINED_SESSION_STATE: i32 = 0;
    unsafe {
        let source = CGEventSourceCreate(COMBINED_SESSION_STATE);
        if !source.is_null() {
            CGEventSourceSetLocalEventsSuppressionInterval(source, 0.0);
        }
    }
}

/// Lets this background app hide the cursor while Windows has input.
pub fn allow_background_cursor_hiding() {
    let key = CfString::new("SetsCursorInBackground");
    unsafe {
        let cid = _CGSDefaultConnection();
        CGSSetConnectionProperty(cid, cid, key.0, core_foundation_sys::number::kCFBooleanTrue.cast());
    }
}

/// SkyLight's private HDR calls, found at run time: a macOS that lacks them
/// leaves HDR to System Settings instead of failing to load strait.
pub struct Hdr {
    pub supported: DisplayQuery,
    pub enabled: DisplayQuery,
    pub set: SetHdr,
}

pub type DisplayQuery = extern "C" fn(CGDirectDisplayID) -> bool;
/// The two trailing arguments are undocumented; 0 and 0 switch HDR on macOS 26.
pub type SetHdr = extern "C" fn(CGDirectDisplayID, bool, i32, i32) -> i32;

pub fn hdr() -> Option<&'static Hdr> {
    static HDR: std::sync::OnceLock<Option<Hdr>> = std::sync::OnceLock::new();
    HDR.get_or_init(|| unsafe {
        let sky = libc::dlopen(c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(), libc::RTLD_NOW);
        if sky.is_null() {
            return None;
        }
        let find = |name: &std::ffi::CStr| Some(libc::dlsym(sky, name.as_ptr())).filter(|f| !f.is_null());
        Some(Hdr {
            supported: std::mem::transmute::<*mut c_void, DisplayQuery>(find(c"SLSDisplaySupportsHDRMode")?),
            enabled: std::mem::transmute::<*mut c_void, DisplayQuery>(find(c"SLSDisplayIsHDRModeEnabled")?),
            set: std::mem::transmute::<*mut c_void, SetHdr>(find(c"SLSDisplaySetHDRModeEnabled")?),
        })
    })
    .as_ref()
}

pub fn c_string(s: &str) -> CString {
    CString::new(s).expect("no interior NUL")
}
