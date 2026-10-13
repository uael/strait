//! DDC/CI from Windows, for when the PC, not the Mac, sits on the monitor's
//! HDMI input (the monitor answers DDC only there).

use windows_sys::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    GetVCPFeatureAndVCPFeatureReply, PHYSICAL_MONITOR, SetVCPFeature,
};
use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

/// Every physical monitor Windows drives, released when dropped.
struct Physical(Vec<PHYSICAL_MONITOR>);

impl Physical {
    fn all() -> Physical {
        unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
            let out = unsafe { &mut *(out as *mut Vec<PHYSICAL_MONITOR>) };
            let mut n = 0;
            if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut n) } != 0 && n > 0 {
                let mut found: Vec<PHYSICAL_MONITOR> = (0..n).map(|_| unsafe { std::mem::zeroed() }).collect();
                if unsafe { GetPhysicalMonitorsFromHMONITOR(monitor, n, found.as_mut_ptr()) } != 0 {
                    out.extend(found);
                }
            }
            1
        }
        let mut all: Vec<PHYSICAL_MONITOR> = Vec::new();
        unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &mut all as *mut _ as LPARAM) };
        Physical(all)
    }
}

impl Drop for Physical {
    fn drop(&mut self) {
        if !self.0.is_empty() {
            unsafe { DestroyPhysicalMonitors(self.0.len() as u32, self.0.as_ptr()) };
        }
    }
}

fn name(m: &PHYSICAL_MONITOR) -> String {
    // The struct is packed; copy the field out before borrowing it.
    let text = m.szPhysicalMonitorDescription;
    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
    String::from_utf16_lossy(&text[..end])
}

/// Each monitor's description and current value of `feature`, where it answers.
pub fn read(feature: u8) -> Vec<(String, Option<u32>)> {
    Physical::all()
        .0
        .iter()
        .map(|m| {
            let (mut kind, mut current, mut max) = (0, 0, 0);
            let ok = unsafe { GetVCPFeatureAndVCPFeatureReply(m.hPhysicalMonitor, feature, &mut kind, &mut current, &mut max) } != 0;
            (name(m), ok.then_some(current))
        })
        .collect()
}

/// Sets `feature` on every monitor that answers DDC; true if one took it.
pub fn write(feature: u8, value: u32) -> bool {
    let monitors = Physical::all();
    let mut any = false;
    for m in &monitors.0 {
        let (mut kind, mut current, mut max) = (0, 0, 0);
        if unsafe { GetVCPFeatureAndVCPFeatureReply(m.hPhysicalMonitor, feature, &mut kind, &mut current, &mut max) } == 0 {
            continue;
        }
        let ok = unsafe { SetVCPFeature(m.hPhysicalMonitor, feature, value) } != 0;
        crate::log!("DDC {feature:#04x} := {value:#x} on {}: {}", name(m), if ok { "sent" } else { "refused" });
        any |= ok;
    }
    any
}
