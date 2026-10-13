//! Keeps the display at the fastest refresh rate its current resolution
//! offers. Windows picks 59 Hz when the monitor switches to side by side,
//! though the monitor runs that resolution at 120 Hz.

use std::mem::size_of;

use windows_sys::Win32::Graphics::Gdi::{
    CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DEVMODEW, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY,
    ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW,
};

use crate::log;

fn mode(index: u32) -> Option<DEVMODEW> {
    let mut m: DEVMODEW = unsafe { std::mem::zeroed() };
    m.dmSize = size_of::<DEVMODEW>() as u16;
    (unsafe { EnumDisplaySettingsW(std::ptr::null(), index, &mut m) } != 0).then_some(m)
}

pub fn fastest_refresh() {
    let Some(current) = mode(ENUM_CURRENT_SETTINGS) else { return };
    let same_size = |m: &DEVMODEW| {
        m.dmPelsWidth == current.dmPelsWidth && m.dmPelsHeight == current.dmPelsHeight && m.dmBitsPerPel == current.dmBitsPerPel
    };
    let Some(best) = (0..).map_while(mode).filter(same_size).max_by_key(|m| m.dmDisplayFrequency) else { return };
    if best.dmDisplayFrequency <= current.dmDisplayFrequency {
        return;
    }
    let mut next = current;
    next.dmDisplayFrequency = best.dmDisplayFrequency;
    next.dmFields = DM_DISPLAYFREQUENCY;
    let r = unsafe {
        ChangeDisplaySettingsExW(std::ptr::null(), &next, std::ptr::null_mut(), CDS_UPDATEREGISTRY, std::ptr::null())
    };
    if r == DISP_CHANGE_SUCCESSFUL {
        log!(
            "refresh rate {} Hz -> {} Hz at {}x{}",
            current.dmDisplayFrequency,
            best.dmDisplayFrequency,
            current.dmPelsWidth,
            current.dmPelsHeight
        );
    } else {
        log!("could not raise the refresh rate to {} Hz: code {r}", best.dmDisplayFrequency);
    }
}
