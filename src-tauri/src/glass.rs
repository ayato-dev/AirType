//! Window chrome Tauri doesn't set on its own: Windows 11 round corners, and the
//! acrylic under the HUD and the panel. Those two never become the active
//! window (the HUD) or not for long (the panel), and the Windows 11 system
//! backdrop goes flat grey on inactive windows, so they use the older acrylic
//! accent that stays lit. macOS gets its glass from the window config; Linux has
//! none and the page draws a solid sheet.

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

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use tauri::WebviewWindow;
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

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
}

#[cfg(not(windows))]
mod imp {
    use tauri::WebviewWindow;

    pub fn round(_window: &WebviewWindow) -> bool {
        false
    }

    pub fn acrylic(_window: &WebviewWindow, _dark: bool) {}
}
