//! A copy of everything on the system clipboard, every format the app put there,
//! so the user gets their clipboard back exactly as it was after a paste. arboard
//! alone only knows text, images and HTML: a copied file, an RTF table or an
//! app's private format would be lost.
//!
//! Pasting lends the clipboard for one Ctrl/Cmd+V. On Windows the text is only
//! promised, not put: Windows asks for it the moment an app pastes, so the user's
//! clipboard comes back right then, and if nobody asks we know the paste missed.

pub struct Saved(imp::Saved);

impl Saved {
    /// `None` when the clipboard can't be read right now (another app holds it).
    pub fn capture() -> Option<Self> {
        imp::capture().map(Saved)
    }

    pub fn restore(self) {
        imp::restore(self.0);
    }
}

// Only Windows can tell whether an app took the text.
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Pasted {
    /// An app took the text.
    Taken,
    /// No app asked for it: nothing there to paste into.
    Missed,
    /// The system can't tell; the text was given time to be read.
    Assumed,
}

/// Puts the text on the clipboard for one paste, runs `press`, and gives the
/// user's clipboard back. Err when the clipboard is held by another app.
pub fn paste(text: &str, press: impl FnOnce()) -> Result<Pasted, String> {
    let saved = Saved::capture().ok_or(BUSY)?;
    imp::paste(text, press, saved.0)
}

pub const BUSY: &str = "Буфер обмена занят другой программой, попробуй ещё раз";

#[cfg(windows)]
mod imp {
    use std::{
        cell::{Cell, RefCell},
        ptr, thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::Foundation::{GlobalFree, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::System::Ole::{
        CF_BITMAP, CF_DIB, CF_DIBV5, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT, CF_ENHMETAFILE,
        CF_METAFILEPICT, CF_OEMTEXT, CF_OWNERDISPLAY, CF_PALETTE, CF_TEXT, CF_UNICODETEXT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MsgWaitForMultipleObjects,
        PeekMessageW, RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, PM_REMOVE, QS_ALLINPUT,
        WM_RENDERALLFORMATS, WM_RENDERFORMAT, WNDCLASSW,
    };

    /// Format id and the bytes behind its memory handle.
    pub type Saved = Vec<(u32, Vec<u8>)>;

    /// Private and GDI formats hold handles, not memory; a copy of the handle
    /// would point at nothing once the clipboard is emptied. Bitmaps survive as
    /// `CF_DIB`/`CF_DIBV5`, which Windows offers alongside `CF_BITMAP`.
    fn copyable(format: u32) -> bool {
        let handle_formats = [
            CF_BITMAP,
            CF_METAFILEPICT,
            CF_PALETTE,
            CF_ENHMETAFILE,
            CF_OWNERDISPLAY,
            CF_DSPBITMAP,
            CF_DSPMETAFILEPICT,
            CF_DSPENHMETAFILE,
        ];
        !u16::try_from(format).is_ok_and(|f| handle_formats.contains(&f))
            && !(0x200..=0x3FF).contains(&format)
    }

    /// Formats Windows makes from one another. Asking for a made one forces the
    /// conversion (a large picture twice over), and Windows makes it again from
    /// the one we keep. The app's own format comes first in the enumeration.
    const DERIVED: [&[u16]; 2] = [&[CF_UNICODETEXT, CF_TEXT, CF_OEMTEXT], &[CF_DIBV5, CF_DIB]];

    fn derived(format: u32, kept: &[(u32, Vec<u8>)]) -> bool {
        let Ok(format) = u16::try_from(format) else { return false };
        DERIVED.iter().any(|group| {
            group.contains(&format)
                && kept
                    .iter()
                    .any(|&(f, _)| u16::try_from(f).is_ok_and(|f| f != format && group.contains(&f)))
        })
    }

    /// Opens the clipboard, retrying while another app has it open.
    fn open() -> Option<Clip> {
        open_until(Instant::now() + Duration::from_millis(600), || thread::sleep(Duration::from_millis(15)))
    }

    fn open_until(deadline: Instant, mut wait: impl FnMut()) -> Option<Clip> {
        loop {
            if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
                return Some(Clip);
            }
            if Instant::now() >= deadline {
                return None;
            }
            wait();
        }
    }

    struct Clip;

    impl Drop for Clip {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    pub fn capture() -> Option<Saved> {
        let _clip = open()?;
        let mut saved = Vec::new();
        let mut format = 0;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            if !copyable(format) || derived(format, &saved) {
                continue;
            }
            let handle = unsafe { GetClipboardData(format) };
            if handle.is_null() {
                continue;
            }
            unsafe {
                let size = GlobalSize(handle);
                let data = GlobalLock(handle);
                if data.is_null() {
                    continue;
                }
                let bytes = std::slice::from_raw_parts(data.cast::<u8>(), size).to_vec();
                GlobalUnlock(handle);
                saved.push((format, bytes));
            }
        }
        Some(saved)
    }

    pub fn restore(saved: Saved) {
        if let Some(clip) = open() {
            fill(clip, &saved);
        }
    }

    fn fill(_clip: Clip, saved: &Saved) {
        unsafe { EmptyClipboard() };
        for (format, bytes) in saved {
            put(*format, bytes);
        }
        // The user already has this content in Win+V history; don't add it twice.
        let no_history: Vec<u16> = "CanIncludeInClipboardHistory\0".encode_utf16().collect();
        let format = unsafe { RegisterClipboardFormatW(no_history.as_ptr()) };
        if format != 0 && !saved.iter().any(|(f, _)| *f == format) {
            put(format, &0u32.to_ne_bytes());
        }
    }

    thread_local! {
        /// The text promised to the clipboard, as UTF-16 with the closing zero.
        static PROMISED: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
        /// How many times an app has asked for it.
        static ASKED: Cell<u32> = const { Cell::new(0) };
    }

    unsafe extern "system" fn promise_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            // An app pastes: hand over the text. The clipboard is already open for us.
            WM_RENDERFORMAT if wparam == CF_UNICODETEXT as usize => {
                PROMISED.with(|text| put(CF_UNICODETEXT as u32, &utf16_bytes(&text.borrow())));
                ASKED.with(|asked| asked.set(asked.get() + 1));
                0
            }
            // The window goes away while the promise still stands: keep it.
            WM_RENDERALLFORMATS => {
                if unsafe { OpenClipboard(hwnd) } != 0 {
                    if unsafe { GetClipboardOwner() } == hwnd {
                        PROMISED.with(|text| put(CF_UNICODETEXT as u32, &utf16_bytes(&text.borrow())));
                    }
                    unsafe { CloseClipboard() };
                }
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    fn utf16_bytes(units: &[u16]) -> Vec<u8> {
        units.iter().flat_map(|unit| unit.to_ne_bytes()).collect()
    }

    /// An invisible window that owns the clipboard while the text is promised.
    fn promise_window() -> Option<HWND> {
        let class = windows_sys::w!("AirtypeClipboardPromise");
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(promise_proc);
            wc.hInstance = instance;
            wc.lpszClassName = class;
            // Fails harmlessly when the class is already there from an earlier paste.
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class,
                ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                ptr::null_mut(),
                instance,
                ptr::null(),
            );
            (!hwnd.is_null()).then_some(hwnd)
        }
    }

    /// Handles window messages, the clipboard's requests among them, until `done` or the deadline.
    fn pump(deadline: Instant, done: impl Fn() -> bool) -> bool {
        loop {
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if done() {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let wait = (deadline - now).as_millis().min(50) as u32;
            unsafe { MsgWaitForMultipleObjects(0, ptr::null(), 0, wait, QS_ALLINPUT) };
        }
    }

    pub fn paste(text: &str, press: impl FnOnce(), saved: Saved) -> Result<super::Pasted, String> {
        use super::Pasted;
        let Some(hwnd) = promise_window() else {
            restore(saved);
            return Err("Не смог положить текст в буфер обмена".into());
        };
        PROMISED.with(|promised| *promised.borrow_mut() = text.encode_utf16().chain([0]).collect());
        ASKED.with(|asked| asked.set(0));
        let opened = unsafe { OpenClipboard(hwnd) } != 0;
        if opened {
            unsafe {
                EmptyClipboard();
                // No data yet: Windows comes back for it when an app pastes.
                SetClipboardData(CF_UNICODETEXT as u32, ptr::null_mut());
                // Keep the text out of Win+V history, the cloud clipboard and clipboard managers.
                for name in [
                    windows_sys::w!("ExcludeClipboardContentFromMonitorProcessing"),
                    windows_sys::w!("CanIncludeInClipboardHistory"),
                    windows_sys::w!("CanUploadToCloudClipboard"),
                ] {
                    let format = RegisterClipboardFormatW(name);
                    if format != 0 {
                        put(format, &0u32.to_ne_bytes());
                    }
                }
                CloseClipboard();
            }
        }
        let asked = || ASKED.with(Cell::get) > 0;
        let result = if !opened {
            Err(super::BUSY.to_string())
        } else {
            // A clipboard watcher that ignores the flags above may read it right away;
            // then the read no longer tells when the paste happened.
            pump(Instant::now() + Duration::from_millis(30), || false);
            let early = asked();
            press();
            if early {
                pump(Instant::now() + Duration::from_millis(600), || false);
                Ok(Pasted::Assumed)
            } else if pump(Instant::now() + Duration::from_millis(1500), asked) {
                // The app reads the text while it holds the clipboard open; putting the
                // user's clipboard back waits for it to close.
                pump(Instant::now() + Duration::from_millis(30), || false);
                Ok(Pasted::Taken)
            } else {
                Ok(Pasted::Missed)
            }
        };
        // The app that pasted may still hold the clipboard. Wait longer than a plain
        // restore, and keep answering the clipboard meanwhile: if the user's
        // content doesn't go back before the window goes, the answer stays there.
        let deadline = Instant::now() + Duration::from_secs(2);
        if let Some(clip) = open_until(deadline, || {
            pump(Instant::now() + Duration::from_millis(15), || false);
        }) {
            fill(clip, &saved);
        }
        unsafe { DestroyWindow(hwnd) };
        PROMISED.with(|promised| promised.borrow_mut().clear());
        result
    }

    fn put(format: u32, bytes: &[u8]) {
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
            if handle.is_null() {
                return;
            }
            let data = GlobalLock(handle);
            if data.is_null() {
                GlobalFree(handle);
                return;
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), data.cast::<u8>(), bytes.len());
            GlobalUnlock(handle);
            // On success the clipboard owns the memory; on failure it is still ours.
            if SetClipboardData(format, handle).is_null() {
                GlobalFree(handle);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::{rc::Retained, runtime::ProtocolObject};
    use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSData, NSString};

    /// Each pasteboard item with every type it carries.
    pub type Saved = Vec<Vec<(Retained<NSString>, Retained<NSData>)>>;

    /// Tells clipboard managers (Maccy, Paste, Alfred) that this write is not a new copy.
    const TRANSIENT: &str = "org.nspasteboard.TransientType";

    pub fn capture() -> Option<Saved> {
        let pasteboard = NSPasteboard::generalPasteboard();
        let Some(items) = pasteboard.pasteboardItems() else { return Some(Vec::new()) };
        let saved = items
            .iter()
            .map(|item| {
                item.types()
                    .iter()
                    .filter_map(|kind| item.dataForType(&kind).map(|data| (kind, data)))
                    .collect()
            })
            .collect();
        Some(saved)
    }

    pub fn paste(text: &str, press: impl FnOnce(), saved: Saved) -> Result<super::Pasted, String> {
        let result = super::lend(text, press);
        restore(saved);
        result
    }

    pub fn restore(saved: Saved) {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if saved.is_empty() {
            return;
        }
        let transient = NSString::from_str(TRANSIENT);
        let mut items = Vec::with_capacity(saved.len());
        for (index, types) in saved.iter().enumerate() {
            let item = NSPasteboardItem::new();
            for (kind, data) in types {
                item.setData_forType(data, kind);
            }
            if index == 0 && !types.iter().any(|(kind, _)| **kind == *transient) {
                item.setData_forType(&NSData::new(), &transient);
            }
            items.push(ProtocolObject::<dyn NSPasteboardWriting>::from_retained(item));
        }
        pasteboard.writeObjects(&NSArray::from_retained_slice(&items));
    }
}

// Linux (X11): plain text only. lib.rs keeps one arboard instance alive, so the
// text put back here is still served after this one is dropped.
#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub type Saved = Option<String>;

    pub fn capture() -> Option<Saved> {
        Some(arboard::Clipboard::new().ok()?.get_text().ok())
    }

    pub fn paste(text: &str, press: impl FnOnce(), saved: Saved) -> Result<super::Pasted, String> {
        let result = super::lend(text, press);
        restore(saved);
        result
    }

    pub fn restore(saved: Saved) {
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return };
        let _ = match saved {
            Some(text) => clipboard.set_text(text),
            None => clipboard.clear(),
        };
    }
}

/// Puts the text on the clipboard, out of clipboard history, presses paste and
/// waits long enough for the app to read it.
#[cfg(not(windows))]
fn lend(text: &str, press: impl FnOnce()) -> Result<Pasted, String> {
    #[cfg(target_os = "macos")]
    use arboard::SetExtApple as _;
    #[cfg(all(unix, not(target_os = "macos")))]
    use arboard::SetExtLinux as _;
    let mut clipboard = arboard::Clipboard::new().map_err(|_| BUSY.to_string())?;
    clipboard
        .set()
        .exclude_from_history()
        .text(text)
        .map_err(|e| format!("Не смог положить текст в буфер обмена: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(40));
    press();
    // Apps read the clipboard a moment after Cmd/Ctrl+V; swapping it back too early
    // would paste the user's old clipboard instead.
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(Pasted::Assumed)
}
