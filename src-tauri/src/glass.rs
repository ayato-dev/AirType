//! Window chrome Tauri doesn't set on its own: Windows 11 round corners, and the
//! acrylic under the HUD and the panel. Those two never become the active
//! window (the HUD) or not for long (the panel), and the Windows 11 system
//! backdrop goes flat grey on inactive windows, so they use the older acrylic
//! accent that stays lit. macOS gets its glass from the window config; Linux has
//! none and the page draws a solid sheet.
//!
//! The panel also fades in and out. Windows has no fade for a borderless popup,
//! so its opacity is stepped by hand; macOS plays its own once asked to; on
//! Linux the page fades the sheet it draws.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::WebviewWindow;

static ROUNDED: AtomicBool = AtomicBool::new(false);

/// Windows 11 rounds the window corners. False on Windows 10, where windows stay square.
pub fn rounded() -> bool {
    ROUNDED.load(Ordering::Relaxed)
}

/// Asks for round corners and drops the system border; the page draws its own rim.
pub fn round(window: &WebviewWindow) {
    let ok = imp::round(window);
    ROUNDED.store(ok, Ordering::Relaxed);
}

/// Acrylic behind a small floating window.
pub fn acrylic(window: &WebviewWindow) {
    let dark = matches!(window.theme(), Ok(tauri::Theme::Dark));
    imp::acrylic(window, dark);
}

/// Lets the window fade as it comes and goes. Once, when it is made.
pub fn fading(window: &WebviewWindow) {
    imp::fading(window);
}

/// Shows the window with a fade, or brings it back from fading out. Only one
/// window fades: the panel.
pub fn show(window: &WebviewWindow) {
    imp::show(window);
}

/// Fades the window out and hides it.
pub fn hide(window: &WebviewWindow) {
    imp::hide(window);
}

#[cfg(windows)]
mod imp {
    use std::{
        ffi::c_void,
        sync::atomic::{AtomicU64, AtomicU8, Ordering},
        thread,
        time::Duration,
    };
    use tauri::WebviewWindow;
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CLOAK, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA,
        WS_EX_LAYERED,
    };

    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
    const WCA_ACCENT_POLICY: u32 = 19;

    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        /// ABGR.
        color: u32,
        animation: u32,
    }

    #[repr(C)]
    struct CompositionData {
        attribute: u32,
        data: *mut c_void,
        size: usize,
    }

    type SetWindowCompositionAttribute = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;

    fn hwnd(window: &WebviewWindow) -> Option<HWND> {
        window.hwnd().ok().map(|h| h.0 as HWND)
    }

    pub fn round(window: &WebviewWindow) -> bool {
        let Some(hwnd) = hwnd(window) else { return false };
        let corner = DWMWCP_ROUND;
        let none = DWMWA_COLOR_NONE;
        unsafe {
            // Windows 10 doesn't know the attribute and says so.
            let rounded = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                &corner as *const _ as *const c_void,
                size_of_val(&corner) as u32,
            ) >= 0;
            if rounded {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_BORDER_COLOR as u32,
                    &none as *const _ as *const c_void,
                    size_of_val(&none) as u32,
                );
            }
            rounded
        }
    }

    pub fn acrylic(window: &WebviewWindow, dark: bool) {
        let Some(hwnd) = hwnd(window) else { return };
        round(window);
        // Tao's shadow keeps a hidden frame around the page, and the acrylic fills it
        // once Windows stops drawing that frame: a chin under the text. With no frame
        // the glass is exactly the page, and the system still draws its shadow.
        let _ = window.set_shadow(false);
        let set: Option<SetWindowCompositionAttribute> = unsafe {
            let user32 = GetModuleHandleW(windows_sys::w!("user32.dll"));
            if user32.is_null() {
                return;
            }
            GetProcAddress(user32, c"SetWindowCompositionAttribute".as_ptr().cast()).map(|f| {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, SetWindowCompositionAttribute>(f)
            })
        };
        let Some(set) = set else { return };
        // A tint over the blur, as on the system flyouts.
        let color = if dark { 0xC020_2020 } else { 0xB8F1_F3F3 };
        let mut policy =
            AccentPolicy { state: ACCENT_ENABLE_ACRYLICBLURBEHIND, flags: 0, color, animation: 0 };
        let mut data = CompositionData {
            attribute: WCA_ACCENT_POLICY,
            data: &mut policy as *mut _ as *mut c_void,
            size: size_of_val(&policy),
        };
        unsafe { set(hwnd, &mut data) };
    }

    /// Bumped by every fade, so the one before stops where it is.
    static FADE: AtomicU64 = AtomicU64::new(0);
    /// The opacity right now, and the one the running fade goes to.
    static ALPHA: AtomicU8 = AtomicU8::new(255);
    static GOAL: AtomicU8 = AtomicU8::new(255);

    const FRAME: Duration = Duration::from_millis(15);

    pub fn fading(_window: &WebviewWindow) {}

    /// Opacity works on layered windows only. Tao rewrites the styles whenever it
    /// shows or hides the window, so the flag is put back each time.
    fn set_alpha(hwnd: HWND, alpha: u8) {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if style & WS_EX_LAYERED as isize == 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED as isize);
            }
            SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
        }
        ALPHA.store(alpha, Ordering::SeqCst);
    }

    fn cloak(hwnd: HWND, on: bool) {
        let value: BOOL = on.into();
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_CLOAK as u32,
                &value as *const _ as *const c_void,
                size_of_val(&value) as u32,
            );
        }
    }

    /// Steps the opacity to `goal` on its own thread, easing out; `then` runs at the end
    /// unless another fade has started meanwhile.
    fn fade(
        window: &WebviewWindow,
        goal: u8,
        frames: u32,
        hold: u32,
        then: impl FnOnce(&WebviewWindow) + Send + 'static,
    ) {
        let Some(hwnd) = hwnd(window) else { return };
        let hwnd = hwnd as usize;
        let generation = FADE.fetch_add(1, Ordering::SeqCst) + 1;
        GOAL.store(goal, Ordering::SeqCst);
        let from = f64::from(ALPHA.load(Ordering::SeqCst));
        let window = window.clone();
        thread::spawn(move || {
            // A moment at the start for the page to draw itself in the shown window.
            thread::sleep(FRAME * hold);
            for frame in 1..=frames {
                if FADE.load(Ordering::SeqCst) != generation {
                    return;
                }
                let t = f64::from(frame) / f64::from(frames);
                let eased = 1.0 - (1.0 - t) * (1.0 - t);
                set_alpha(hwnd as HWND, (from + (f64::from(goal) - from) * eased).round() as u8);
                thread::sleep(FRAME);
            }
            if FADE.load(Ordering::SeqCst) == generation {
                then(&window);
            }
        });
    }

    pub fn show(window: &WebviewWindow) {
        let Some(hwnd) = hwnd(window) else {
            let _ = window.show();
            return;
        };
        if window.is_visible().unwrap_or(false) {
            // Up already, or fading in: nothing to do. Fading out: come back from where it is.
            if GOAL.load(Ordering::SeqCst) != 255 {
                fade(window, 255, 6, 0, |_| {});
            }
            return;
        }
        // Shown while cloaked, so no frame of it slips out before it is see-through.
        cloak(hwnd, true);
        let _ = window.show();
        set_alpha(hwnd, 0);
        cloak(hwnd, false);
        fade(window, 255, 9, 2, |_| {});
    }

    pub fn hide(window: &WebviewWindow) {
        if hwnd(window).is_none() || !window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            return;
        }
        if GOAL.load(Ordering::SeqCst) == 0 {
            return;
        }
        fade(window, 0, 6, 0, |window| {
            // `show` runs on the main thread: checked there, a show that came in
            // after the last frame can't be hidden by this fade.
            let shown = window.clone();
            let _ = window.run_on_main_thread(move || {
                if GOAL.load(Ordering::SeqCst) == 0 {
                    let _ = shown.hide();
                }
            });
        });
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use tauri::WebviewWindow;

    pub fn round(_window: &WebviewWindow) -> bool {
        false
    }

    pub fn acrylic(_window: &WebviewWindow, _dark: bool) {}

    /// AppKit infers no animation for a borderless window; a utility window's is a short fade.
    pub fn fading(window: &WebviewWindow) {
        const UTILITY_WINDOW: isize = 4;
        let Ok(ns_window) = window.ns_window() else { return };
        let ns_window = ns_window as usize;
        let _ = window.run_on_main_thread(move || unsafe {
            let ns_window = &*(ns_window as *const objc2::runtime::AnyObject);
            let _: () = objc2::msg_send![ns_window, setAnimationBehavior: UTILITY_WINDOW];
        });
    }

    pub fn show(window: &WebviewWindow) {
        if !window.is_visible().unwrap_or(false) {
            let _ = window.show();
        }
    }

    pub fn hide(window: &WebviewWindow) {
        let _ = window.hide();
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use tauri::WebviewWindow;

    pub fn round(_window: &WebviewWindow) -> bool {
        false
    }

    pub fn acrylic(_window: &WebviewWindow, _dark: bool) {}

    pub fn fading(_window: &WebviewWindow) {}

    pub fn show(window: &WebviewWindow) {
        if !window.is_visible().unwrap_or(false) {
            let _ = window.show();
        }
    }

    pub fn hide(window: &WebviewWindow) {
        let _ = window.hide();
    }
}
