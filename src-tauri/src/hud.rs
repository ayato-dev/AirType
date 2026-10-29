//! The small status slip on glass, right under the text being worked on. It
//! never takes focus, so the answer still lands in the app the text came from.
//!
//! The page lays out the slip and reports its size; only then is the window
//! sized to it, put next to the text and shown.

use crate::popup::{self, Fallback, Rect};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, WebviewWindow};

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
enum HudEvent<'a> {
    Working { action: &'a str, label: &'a str },
    Done,
    Error { message: &'a str },
}

/// Bumped on every show so a stale delayed hide doesn't close a newer message.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// A message is up; the page's size report may show the window.
static WANTED: AtomicBool = AtomicBool::new(false);
static ANCHOR: Mutex<Option<Rect>> = Mutex::new(None);

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("hud")
}

/// Sets up the glass once the app starts.
pub fn init(app: &AppHandle) {
    if let Some(hud) = window(app) {
        crate::glass::acrylic(&hud);
    }
}

/// The text the next messages are about; `None` puts them at the bottom of the screen.
pub fn set_anchor(anchor: Option<Rect>) {
    *ANCHOR.lock().unwrap() = anchor;
}

pub fn working(app: &AppHandle, action: &str, label: &str) {
    show();
    let _ = app.emit_to("hud", "hud", HudEvent::Working { action, label });
}

pub fn done(app: &AppHandle) {
    let _ = app.emit_to("hud", "hud", HudEvent::Done);
    hide_after(app, Duration::from_millis(900));
}

pub fn error(app: &AppHandle, message: &str) {
    show();
    let _ = app.emit_to("hud", "hud", HudEvent::Error { message });
    hide_after(app, Duration::from_millis(3800));
}

/// Takes the slip away now: the panel has the answer.
pub fn hide(app: &AppHandle) {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    WANTED.store(false, Ordering::SeqCst);
    if let Some(hud) = window(app) {
        platform_hide(&hud);
    }
}

fn show() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    WANTED.store(true, Ordering::SeqCst);
}

/// The page has laid out the slip at this size (logical pixels).
pub fn fit(app: &AppHandle, width: f64, height: f64) {
    if !WANTED.load(Ordering::SeqCst) {
        return;
    }
    let Some(hud) = window(app) else { return };
    let _ = hud.set_size(LogicalSize::new(width, height));
    let anchor = *ANCHOR.lock().unwrap();
    popup::place(app, &hud, width, height, anchor, Fallback::BottomCenter);
    platform_show(&hud);
    // Clicks go through to whatever is underneath. Set after showing: GTK can't
    // change input regions of a window that was never mapped.
    let _ = hud.set_ignore_cursor_events(true);
}

fn hide_after(app: &AppHandle, delay: Duration) {
    let generation = GENERATION.load(Ordering::SeqCst);
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(delay);
        if GENERATION.load(Ordering::SeqCst) == generation {
            WANTED.store(false, Ordering::SeqCst);
            if let Some(hud) = window(&app) {
                platform_hide(&hud);
            }
        }
    });
}

// Tauri's show() activates the window on Windows, which would steal focus from the
// text field, so the slip is shown with SW_SHOWNOACTIVATE instead.
#[cfg(windows)]
fn platform_show(hud: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindowAsync, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SW_SHOWNOACTIVATE,
    };
    if let Ok(hwnd) = hud.hwnd() {
        let hwnd = hwnd.0 as _;
        unsafe {
            ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
        }
    }
}

#[cfg(windows)]
fn platform_hide(hud: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_HIDE};
    if let Ok(hwnd) = hud.hwnd() {
        unsafe {
            ShowWindowAsync(hwnd.0 as _, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
fn platform_show(hud: &WebviewWindow) {
    let _ = hud.show();
}

#[cfg(not(windows))]
fn platform_hide(hud: &WebviewWindow) {
    let _ = hud.hide();
}
