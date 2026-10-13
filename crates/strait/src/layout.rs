//! Where Windows sits among the Mac's displays.
//!
//! Coordinates are macOS global display points: origin at the top-left of
//! the main display, y growing downward. The Windows region is the
//! rectangle of Mac space where the monitor shows Windows: a Mac display
//! that the monitor hides behind Windows, or empty space beside a display
//! when the monitor shows both side by side.

use crate::wire::{Edge, Edges};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Displays touch when their edges are this close, in points.
const TOUCH: f64 = 1.0;
/// How far past an edge the Mac pointer lands when Windows hands it back.
const LANDING_INSET: f64 = 2.0;

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    fn clamp(&self, p: Point) -> Point {
        Point {
            x: p.x.clamp(self.x + LANDING_INSET, self.right() - LANDING_INSET),
            y: p.y.clamp(self.y + LANDING_INSET, self.bottom() - LANDING_INSET),
        }
    }

    /// Length shared by two spans, positive when they overlap.
    fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
        a1.min(b1) - a0.max(b0)
    }

    /// Whether `other` sits against this rectangle's `edge`.
    fn touches(&self, edge: Edge, other: &Rect) -> bool {
        let (gap, shared) = match edge {
            Edge::Left => (self.x - other.right(), Rect::overlap(self.y, self.bottom(), other.y, other.bottom())),
            Edge::Right => (other.x - self.right(), Rect::overlap(self.y, self.bottom(), other.y, other.bottom())),
            Edge::Top => (self.y - other.bottom(), Rect::overlap(self.x, self.right(), other.x, other.right())),
            Edge::Bottom => (other.y - self.bottom(), Rect::overlap(self.x, self.right(), other.x, other.right())),
        };
        gap.abs() < TOUCH && shared > 0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Mac displays the user can see.
    pub visible: Vec<Rect>,
    /// Where Windows appears, if the monitor shows it.
    pub windows: Option<Rect>,
}

impl Layout {
    /// Where the Windows pointer starts when the Mac pointer at `p`, moving
    /// by (`dx`, `dy`), reaches the Windows region; `None` if it does not.
    /// Testing `p` plus the motion catches a pointer that macOS holds at a
    /// display edge while the user keeps pushing toward Windows.
    pub fn entry(&self, p: Point, dx: f64, dy: f64) -> Option<(f32, f32)> {
        let w = self.windows?;
        let next = Point { x: p.x + dx, y: p.y + dy };
        if !w.contains(p) && !w.contains(next) {
            return None;
        }
        let q = if w.contains(p) { p } else { next };
        Some((((q.x - w.x) / w.w) as f32, ((q.y - w.y) / w.h) as f32))
    }

    /// Edges of the Windows region that lead back to a visible Mac display.
    pub fn exits(&self) -> Edges {
        let Some(w) = self.windows else { return Edges::default() };
        Edge::ALL
            .into_iter()
            .filter(|&e| self.visible.iter().any(|v| w.touches(e, v)))
            .fold(Edges::default(), Edges::with)
    }

    /// Mac point where the pointer lands when Windows reports it reached
    /// `edge`, `along` that edge from its left or top end.
    pub fn landing(&self, edge: Edge, along: f32) -> Option<Point> {
        let w = self.windows?;
        let along = f64::from(along.clamp(0.0, 1.0));
        let p = match edge {
            Edge::Left => Point { x: w.x - LANDING_INSET, y: w.y + along * w.h },
            Edge::Right => Point { x: w.right() + LANDING_INSET, y: w.y + along * w.h },
            Edge::Top => Point { x: w.x + along * w.w, y: w.y - LANDING_INSET },
            Edge::Bottom => Point { x: w.x + along * w.w, y: w.bottom() + LANDING_INSET },
        };
        if self.visible.iter().any(|v| v.contains(p)) {
            return Some(p);
        }
        self.visible.iter().filter(|v| w.touches(edge, v)).map(|v| v.clamp(p)).min_by(|a, b| {
            let d = |q: &Point| (q.x - p.x).powi(2) + (q.y - p.y).powi(2);
            d(a).total_cmp(&d(b))
        })
    }

    /// Whether `p` shows on a Mac display rather than behind Windows.
    pub fn is_visible(&self, p: Point) -> bool {
        self.visible.iter().any(|v| v.contains(p)) && !self.windows.is_some_and(|w| w.contains(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    /// The monitor above the MacBook's built-in display.
    fn desk() -> (Rect, Rect) {
        (r(0.0, 0.0, 2560.0, 1440.0), r(500.0, 1440.0, 1512.0, 982.0))
    }

    #[test]
    fn side_by_side_enters_at_the_right_edge() {
        let (monitor, builtin) = desk();
        let l = Layout { visible: vec![monitor, builtin], windows: Some(r(2560.0, 0.0, 2560.0, 1440.0)) };
        assert_eq!(l.exits(), Edges::default().with(Edge::Left));
        // Held at the edge by macOS, pushing right.
        assert_eq!(l.entry(Point { x: 2559.0, y: 720.0 }, 3.0, 0.0), Some((0.00078125, 0.5)));
        assert_eq!(l.entry(Point { x: 2559.0, y: 720.0 }, 0.0, 3.0), None);
        assert_eq!(l.landing(Edge::Left, 0.25), Some(Point { x: 2558.0, y: 360.0 }));
    }

    #[test]
    fn hidden_display_enters_anywhere_and_exits_to_the_builtin() {
        let (monitor, builtin) = desk();
        let l = Layout { visible: vec![builtin], windows: Some(monitor) };
        assert_eq!(l.exits(), Edges::default().with(Edge::Bottom));
        assert_eq!(l.entry(Point { x: 1280.0, y: 1439.0 }, 0.0, -1.0), Some((0.5, 1439.0 / 1440.0)));
        // Leaving the bottom edge beside the built-in display clamps onto it.
        assert_eq!(l.landing(Edge::Bottom, 0.0), Some(Point { x: 502.0, y: 1442.0 }));
        assert_eq!(l.landing(Edge::Bottom, 0.5), Some(Point { x: 1280.0, y: 1442.0 }));
        assert_eq!(l.landing(Edge::Top, 0.5), None);
    }

    #[test]
    fn no_windows_region() {
        let (monitor, builtin) = desk();
        let l = Layout { visible: vec![monitor, builtin], windows: None };
        assert_eq!(l.exits(), Edges::default());
        assert_eq!(l.entry(Point { x: 10.0, y: 10.0 }, 5.0, 5.0), None);
        assert!(l.is_visible(Point { x: 10.0, y: 10.0 }));
    }
}
