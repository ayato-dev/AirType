//! Where the small windows go: right under the text the user works on, or above
//! it when the screen ends below.
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
}

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

/// Puts a window of the given logical size next to the anchor.
pub fn place(
    app: &AppHandle,
    window: &WebviewWindow,
    width: f64,
    height: f64,
    anchor: Option<Rect>,
    fallback: Fallback,
) {
    let areas = work_areas(app);
    let point = match anchor {
        Some(a) => Some((a.x + a.w / 2.0, a.y + a.h / 2.0)),
        None => pointer(app),
    };
    let Some((area, scale)) = point
        .and_then(|(x, y)| areas.iter().find(|(r, _)| r.contains(x, y)).copied())
        .or_else(|| areas.first().copied())
    else {
        return;
    };
    // Sizes and gaps are logical; on Windows and Linux they become pixels of this screen.
    let unit = if POINTS { 1.0 } else { scale };
    let (w, h) = (width * unit, height * unit);
    let gap = 8.0 * unit;
    let margin = 12.0 * unit;

    let (x, y) = match (anchor, fallback) {
        (Some(a), _) => {
            let below = a.bottom() + gap;
            let above = a.y - gap - h;
            let y = if below + h <= area.bottom() - margin {
                below
            } else if above >= area.y + margin {
                above
            } else {
                area.bottom() - margin - h
            };
            (a.x, y)
        }
        (None, Fallback::BottomCenter) => (area.x + (area.w - w) / 2.0, area.bottom() - h - 56.0 * unit),
        (None, Fallback::Pointer) => {
            let (px, py) = point.unwrap_or((area.x + area.w / 2.0, area.y + area.h / 2.0));
            let y = if py + gap * 2.0 + h <= area.bottom() - margin { py + gap * 2.0 } else { py - gap - h };
            (px - w / 3.0, y)
        }
    };
    let x = x.clamp(area.x + margin, (area.right() - margin - w).max(area.x + margin));
    let y = y.clamp(area.y + margin, (area.bottom() - margin - h).max(area.y + margin));
    let _ = if POINTS {
        window.set_position(LogicalPosition::new(x, y))
    } else {
        window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
    };
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
    }
}
