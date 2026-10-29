//! The small floating status pill. It never takes focus, so the paste
//! still lands in the app the text came from.

use serde::Serialize;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
enum HudEvent<'a> {
    Working { action: &'a str, label: &'a str },
    Done,
    Error { message: &'a str },
}

/// Bumped on every show so a stale delayed hide doesn't close a newer message.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("hud")
}

pub fn working(app: &AppHandle, action: &str, label: &str) {
    show(app);
    let _ = app.emit_to("hud", "hud", HudEvent::Working { action, label });
}

pub fn done(app: &AppHandle) {
    let _ = app.emit_to("hud", "hud", HudEvent::Done);
    hide_after(app, Duration::from_millis(900));
}

pub fn error(app: &AppHandle, message: &str) {
    show(app);
    let _ = app.emit_to("hud", "hud", HudEvent::Error { message });
    hide_after(app, Duration::from_millis(3800));
}

fn show(app: &AppHandle) {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    let Some(hud) = window(app) else { return };
    place(app, &hud);
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
            if let Some(hud) = window(&app) {
                platform_hide(&hud);
            }
        }
    });
}

/// Bottom center of the screen the mouse is on.
fn place(app: &AppHandle, hud: &WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(size)) = (monitor, hud.outer_size()) else { return };
    let area = monitor.work_area();
    let margin = (56.0 * monitor.scale_factor()) as i32;
    let x = area.position.x + (area.size.width as i32 - size.width as i32) / 2;
    let y = area.position.y + area.size.height as i32 - size.height as i32 - margin;
    let _ = hud.set_position(PhysicalPosition::new(x, y));
}

// Tauri's show() activates the window on Windows, which would steal focus from the
// text field, so the pill is shown with SW_SHOWNOACTIVATE instead.
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
