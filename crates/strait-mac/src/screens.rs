//! The Mac's displays, and where each monitor preset puts Windows among them.

use strait::layout::{Layout, Point, Rect};
use strait::wire::Preset;

use crate::sys::*;

#[derive(Clone, Copy, Debug)]
pub struct Display {
    pub bounds: Rect,
    pub builtin: bool,
}

fn active() -> Vec<CGDirectDisplayID> {
    let mut ids = [0u32; 16];
    let mut count = 0;
    if unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) } != 0 {
        return Vec::new();
    }
    ids[..count as usize].to_vec()
}

pub fn displays() -> Vec<Display> {
    active()
        .into_iter()
        .map(|id| {
            let b = unsafe { CGDisplayBounds(id) };
            Display {
                bounds: Rect { x: b.origin.x, y: b.origin.y, w: b.size.width, h: b.size.height },
                builtin: unsafe { CGDisplayIsBuiltin(id) } != 0,
            }
        })
        .collect()
}

/// Where Windows appears for `preset`, given the Mac's displays.
///
/// - Mac: Windows is not on screen.
/// - Windows: the monitor shows Windows in place of every external display.
/// - Mac + Windows: the monitor splits in two halves. With one external
///   display, the Mac owns the left half and Windows the empty space to its
///   right. With two (the Mac on both monitor inputs), the rightmost one is
///   hidden behind Windows.
pub fn layout(preset: Preset, displays: &[Display]) -> Layout {
    let all: Vec<Rect> = displays.iter().map(|d| d.bounds).collect();
    let mut external: Vec<Rect> = displays.iter().filter(|d| !d.builtin).map(|d| d.bounds).collect();
    external.sort_by(|a, b| a.x.total_cmp(&b.x));
    let hide = |hidden: &[Rect]| all.iter().copied().filter(|r| !hidden.contains(r)).collect::<Vec<_>>();
    match preset {
        Preset::MacFull => Layout { visible: all.clone(), windows: None },
        Preset::WindowsFull => match bounding(&external) {
            Some(w) => Layout { visible: hide(&external), windows: Some(w) },
            None => Layout { visible: all.clone(), windows: None },
        },
        Preset::Split => match external.as_slice() {
            [] => Layout { visible: all.clone(), windows: None },
            [only] => Layout { visible: all.clone(), windows: Some(Rect { x: only.right(), ..*only }) },
            [.., right] => Layout { visible: hide(&[*right]), windows: Some(*right) },
        },
    }
}

/// Whether the monitor shows two inputs side by side. The monitor is 32:9
/// and gives each of its inputs a 16:9 picture in PBP, so the Mac's external
/// displays are all 16:9 then; without PBP the Mac's HDMI display is 32:9.
pub fn is_pbp(displays: &[Display]) -> bool {
    let mut external = displays.iter().filter(|d| !d.builtin).peekable();
    external.peek().is_some() && external.all(|d| d.bounds.w / d.bounds.h < 2.5)
}

fn bounding(rects: &[Rect]) -> Option<Rect> {
    let first = rects.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.right(), first.bottom());
    for r in rects {
        x0 = x0.min(r.x);
        y0 = y0.min(r.y);
        x1 = x1.max(r.right());
        y1 = y1.max(r.bottom());
    }
    Some(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

/// The external displays that can show HDR.
fn hdr_capable(hdr: &Hdr) -> Vec<CGDirectDisplayID> {
    active().into_iter().filter(|&id| unsafe { CGDisplayIsBuiltin(id) } == 0 && (hdr.supported)(id)).collect()
}

/// Whether the external displays show HDR; None when none can.
pub fn hdr_on() -> Option<bool> {
    let hdr = hdr()?;
    let capable = hdr_capable(hdr);
    (!capable.is_empty()).then(|| capable.iter().all(|&id| (hdr.enabled)(id)))
}

/// Turns HDR on or off on every external display that can show it.
pub fn set_hdr(on: bool) {
    let Some(hdr) = hdr() else { return };
    for id in hdr_capable(hdr) {
        let error = (hdr.set)(id, on, 0, 0);
        crate::log(&format!("HDR {} on display {id}{}", if on { "on" } else { "off" }, if error == 0 { String::new() } else { format!(": error {error}") }));
    }
}

/// The Mac pointer's position now.
pub fn pointer() -> Point {
    unsafe {
        let event = CGEventCreate(std::ptr::null_mut());
        let p = CGEventGetLocation(event);
        core_foundation_sys::base::CFRelease(event.cast());
        Point { x: p.x, y: p.y }
    }
}

/// Middle of the main display, where the pointer returns when its former
/// place is not visible.
pub fn main_center() -> Point {
    let b = unsafe { CGDisplayBounds(CGMainDisplayID()) };
    Point { x: b.origin.x + b.size.width / 2.0, y: b.origin.y + b.size.height / 2.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(x: f64, y: f64, w: f64, h: f64, builtin: bool) -> Display {
        Display { bounds: Rect { x, y, w, h }, builtin }
    }

    #[test]
    fn presets_place_windows() {
        let builtin = d(500.0, 1440.0, 1512.0, 982.0, true);
        let wide = d(0.0, 0.0, 5120.0, 1440.0, false);
        assert_eq!(layout(Preset::MacFull, &[builtin, wide]).windows, None);
        let full = layout(Preset::WindowsFull, &[builtin, wide]);
        assert_eq!(full.windows, Some(wide.bounds));
        assert_eq!(full.visible, vec![builtin.bounds]);

        let half = d(0.0, 0.0, 2560.0, 1440.0, false);
        let split = layout(Preset::Split, &[builtin, half]);
        assert_eq!(split.windows, Some(Rect { x: 2560.0, y: 0.0, w: 2560.0, h: 1440.0 }));
        assert_eq!(split.visible.len(), 2);

        assert!(!is_pbp(&[builtin, wide]));
        assert!(is_pbp(&[builtin, half]));
        assert!(!is_pbp(&[builtin]));
        assert!(!is_pbp(&[builtin, wide, half]));

        let right = d(2560.0, 0.0, 2560.0, 1440.0, false);
        let two = layout(Preset::Split, &[right, builtin, half]);
        assert_eq!(two.windows, Some(right.bounds));
        assert_eq!(two.visible, vec![builtin.bounds, half.bounds]);
    }
}
