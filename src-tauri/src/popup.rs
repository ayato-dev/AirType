//! Where the small windows go: right under the text the user works on, or above
//! it when the screen ends below. When the app only says which field has focus,
//! the mouse is often the better guess: it just selected the text.
//!
//! Rectangles are in the units the accessibility API reports: physical pixels on
//! Windows and Linux, points on macOS. Monitors are compared in the same units.

use tauri::{AppHandle, LogicalPosition, PhysicalPosition, WebviewWindow};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.w
    }

    fn bottom(&self) -> f64 {
        self.y + self.h
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// The part of the rectangle inside `area`; `None` when they don't meet.
    fn clip(&self, area: &Rect) -> Option<Rect> {
        let (x, y) = (self.x.max(area.x), self.y.max(area.y));
        let (right, bottom) = (self.right().min(area.right()), self.bottom().min(area.bottom()));
        (right > x && bottom > y).then_some(Rect { x, y, w: right - x, h: bottom - y })
    }
}

/// What the app told us about where the text is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub rect: Rect,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The selection or the caret itself.
    Text,
    /// The focused field the text is in.
    Field,
    /// The focused field, but the text was copied from somewhere else in the
    /// window: a message in the chat above the box that has focus.
    Elsewhere,
}

impl Anchor {
    /// The text came by Ctrl+C, not from the field: a field rectangle no longer says where it is.
    pub fn copied(self) -> Self {
        match self.kind {
            Kind::Field => Anchor { kind: Kind::Elsewhere, ..self },
            _ => self,
        }
    }
}

/// Taller than this (logical pixels) a field holds many lines, and standing under
/// all of it is far from the line the user means.
const TALL: f64 = 100.0;

/// Where to put the window when there is no text to stand next to.
#[derive(Clone, Copy)]
pub enum Fallback {
    /// Bottom center of the screen with the mouse: a status that shouldn't get in the way.
    BottomCenter,
    /// Under the mouse pointer: the user probably just selected something with it.
    Pointer,
}

/// macOS positions windows in points, the others in physical pixels.
const POINTS: bool = cfg!(target_os = "macos");

/// A monitor's work area and scale, in the anchor's units.
fn work_areas(app: &AppHandle) -> Vec<(Rect, f64)> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let area = m.work_area();
            let scale = m.scale_factor();
            let div = if POINTS { scale } else { 1.0 };
            let rect = Rect {
                x: area.position.x as f64 / div,
                y: area.position.y as f64 / div,
                w: area.size.width as f64 / div,
                h: area.size.height as f64 / div,
            };
            (rect, scale)
        })
        .collect()
}

fn pointer(app: &AppHandle) -> Option<(f64, f64)> {
    let p = app.cursor_position().ok()?;
    if POINTS {
        // The cursor comes in physical pixels of the screen it is on.
        let scale = app.monitor_from_point(p.x, p.y).ok().flatten().map_or(1.0, |m| m.scale_factor());
        Some((p.x / scale, p.y / scale))
    } else {
        Some((p.x, p.y))
    }
}

/// Where the window stands.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spot {
    /// Under this rectangle, or above it when the screen ends below.
    Under(Rect),
    /// Under the mouse pointer.
    Pointer((f64, f64)),
    BottomCenter,
}

/// Picks what to stand next to. `areas` are work areas with their scales.
fn spot(
    areas: &[(Rect, f64)],
    anchor: Option<Anchor>,
    pointer: Option<(f64, f64)>,
    fallback: Fallback,
) -> Spot {
    let area_of = |(x, y): (f64, f64)| areas.iter().find(|(r, _)| r.contains(x, y)).copied();
    // Coordinates off every screen are the app's mistake, not a place to go.
    let anchor = anchor.and_then(|a| {
        let (area, scale) = area_of(a.rect.center())?;
        Some((Anchor { rect: a.rect.clip(&area)?, ..a }, area, scale))
    });
    match (anchor, pointer) {
        (Some((a, _, _)), _) if a.kind == Kind::Text => Spot::Under(a.rect),
        (Some((a, _, scale)), Some(p)) if a.kind == Kind::Field => {
            let unit = if POINTS { 1.0 } else { scale };
            if a.rect.h > TALL * unit && a.rect.contains(p.0, p.1) {
                Spot::Pointer(p)
            } else {
                Spot::Under(a.rect)
            }
        }
        // The text is somewhere in this window: under the mouse if it is on the same screen.
        (Some((a, area, _)), Some(p)) if a.kind == Kind::Elsewhere && area.contains(p.0, p.1) => {
            Spot::Pointer(p)
        }
        (Some((a, _, _)), _) => Spot::Under(a.rect),
        (None, Some(p)) if matches!(fallback, Fallback::Pointer) && area_of(p).is_some() => Spot::Pointer(p),
        (None, _) => Spot::BottomCenter,
    }
}

/// Puts a window of the given logical size next to the anchor.
pub fn place(
    app: &AppHandle,
    window: &WebviewWindow,
    width: f64,
    height: f64,
    anchor: Option<Anchor>,
    fallback: Fallback,
) {
    let areas = work_areas(app);
    let pointer = pointer(app);
    let spot = spot(&areas, anchor, pointer, fallback);
    let point = match spot {
        Spot::Under(r) => Some(r.center()),
        Spot::Pointer(p) => Some(p),
        Spot::BottomCenter => pointer,
    };
    let Some((area, scale)) = point
        .and_then(|(x, y)| areas.iter().find(|(r, _)| r.contains(x, y)).copied())
        .or_else(|| areas.first().copied())
    else {
        return;
    };
    let (x, y) = position(spot, area, scale, width, height);
    let _ = if POINTS {
        window.set_position(LogicalPosition::new(x, y))
    } else {
        window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
    };
}

/// The window's top left corner for a spot on this screen.
fn position(spot: Spot, area: Rect, scale: f64, width: f64, height: f64) -> (f64, f64) {
    // Sizes and gaps are logical; on Windows and Linux they become pixels of this screen.
    let unit = if POINTS { 1.0 } else { scale };
    let (w, h) = (width * unit, height * unit);
    let gap = 8.0 * unit;
    let margin = 12.0 * unit;
    let fits_below = |top: f64| top + h <= area.bottom() - margin;
    let fits_above = |bottom: f64| bottom - h >= area.y + margin;

    let (x, y) = match spot {
        Spot::Under(a) if fits_below(a.bottom() + gap) => (a.x, a.bottom() + gap),
        Spot::Under(a) if fits_above(a.y - gap) => (a.x, a.y - gap - h),
        // Neither below nor above: a selection as tall as the screen. Keep the
        // bottom of it, where the selecting ended.
        Spot::Under(a) => (a.x, area.bottom() - margin - h),
        Spot::Pointer((px, py)) => {
            // Clear of the pointer and of the line it stands on.
            let below = py + gap * 2.5;
            let y = if fits_below(below) { below } else { py - gap * 2.0 - h };
            (px - w / 3.0, y)
        }
        Spot::BottomCenter => (area.x + (area.w - w) / 2.0, area.bottom() - h - 56.0 * unit),
    };
    let x = x.clamp(area.x + margin, (area.right() - margin - w).max(area.x + margin));
    let y = y.clamp(area.y + margin, (area.bottom() - margin - h).max(area.y + margin));
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_edges() {
        let r = Rect { x: 10.0, y: 20.0, w: 100.0, h: 30.0 };
        assert_eq!(r.right(), 110.0);
        assert_eq!(r.bottom(), 50.0);
        assert!(r.contains(10.0, 20.0));
        assert!(!r.contains(110.0, 20.0));
        let screen = Rect { x: 0.0, y: 0.0, w: 50.0, h: 40.0 };
        assert_eq!(r.clip(&screen), Some(Rect { x: 10.0, y: 20.0, w: 40.0, h: 20.0 }));
        assert_eq!(Rect { x: 60.0, ..r }.clip(&screen), None);
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn spots() {
        let areas = [(rect(0.0, 0.0, 1920.0, 1040.0), 1.0), (rect(1920.0, 0.0, 1920.0, 1040.0), 1.0)];
        let at = |rect, kind| Some(Anchor { rect, kind });
        let input = rect(400.0, 960.0, 800.0, 40.0);
        let history = rect(400.0, 80.0, 800.0, 860.0);
        let mouse = Some((700.0, 300.0));
        let spot = |anchor, pointer| spot(&areas, anchor, pointer, Fallback::Pointer);

        // The selection itself is the best there is.
        let selection = rect(600.0, 290.0, 120.0, 20.0);
        assert_eq!(spot(at(selection, Kind::Text), mouse), Spot::Under(selection));
        // A one-line field: right under it, wherever the mouse is.
        assert_eq!(spot(at(input, Kind::Field), mouse), Spot::Under(input));
        // A tall field with the mouse over it: the mouse is nearer the line.
        assert_eq!(spot(at(history, Kind::Field), mouse), Spot::Pointer((700.0, 300.0)));
        assert_eq!(spot(at(history, Kind::Field), Some((1500.0, 300.0))), Spot::Under(history));
        // Copied from a chat message while the box below has focus: the mouse selected it.
        assert_eq!(spot(at(input, Kind::Elsewhere), mouse), Spot::Pointer((700.0, 300.0)));
        // ...unless the mouse is on another screen.
        assert_eq!(spot(at(input, Kind::Elsewhere), Some((2500.0, 300.0))), Spot::Under(input));
        // An anchor off every screen is ignored.
        assert_eq!(
            spot(at(rect(-5000.0, 0.0, 10.0, 10.0), Kind::Text), mouse),
            Spot::Pointer((700.0, 300.0))
        );
        assert_eq!(spot(None, None), Spot::BottomCenter);
    }

    #[test]
    fn positions() {
        let area = rect(0.0, 0.0, 1920.0, 1040.0);
        let selection = rect(600.0, 290.0, 120.0, 20.0);
        assert_eq!(position(Spot::Under(selection), area, 1.0, 420.0, 200.0), (600.0, 318.0));
        // No room below: above it.
        let low = rect(600.0, 950.0, 120.0, 20.0);
        assert_eq!(position(Spot::Under(low), area, 1.0, 420.0, 200.0), (600.0, 742.0));
        // At 150% a logical gap is 12 pixels; macOS works in points, where it stays 8.
        let gap = if POINTS { 8.0 } else { 12.0 };
        assert_eq!(position(Spot::Under(selection), area, 1.5, 420.0, 200.0), (600.0, 310.0 + gap));
        assert_eq!(position(Spot::Pointer((700.0, 300.0)), area, 1.0, 420.0, 200.0), (560.0, 320.0));
    }
}
