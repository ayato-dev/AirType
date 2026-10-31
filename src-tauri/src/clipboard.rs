//! A copy of what is on the system clipboard, so the user gets their clipboard
//! back as it was after a copy or a paste. On Windows and macOS that is every
//! format the app put there that holds plain data: a copied file, an RTF table
//! or an app's private format too. arboard alone only knows text, images and HTML.
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
    /// The paste keys couldn't be sent.
    NotSent,
}

/// Puts the text on the clipboard for one paste, runs `press`, and gives the
/// user's clipboard back. Err when the clipboard is held by another app.
pub fn paste(text: &str, press: impl FnOnce() -> bool) -> Result<Pasted, String> {
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
        GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
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

    /// Formats Windows makes from one another, best first. Asking for a made
    /// one forces the conversion (a large picture twice over), and Windows makes
    /// the lesser ones again from the best one we keep. An app may put several
    /// itself, so the best one offered is kept whatever the order.
    const DERIVED: [&[u16]; 2] = [&[CF_UNICODETEXT, CF_TEXT, CF_OEMTEXT], &[CF_DIBV5, CF_DIB]];

    fn derived(format: u32, offered: &[u32]) -> bool {
        let Ok(format) = u16::try_from(format) else { return false };
        DERIVED.iter().any(|group| {
            let Some(rank) = group.iter().position(|&f| f == format) else { return false };
            group[..rank].iter().any(|&better| offered.contains(&u32::from(better)))
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
        // Listing the formats renders nothing; only asking for the data does.
        let mut offered = Vec::new();
        let mut format = 0;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            offered.push(format);
        }
        let mut saved = Vec::new();
        for &format in &offered {
            if !copyable(format) || derived(format, &offered) {
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
        match open() {
            Some(clip) => fill(clip, &saved),
            None => restore_later(saved),
        }
    }

    /// Another app still holds the clipboard: a clipboard manager reading what was
    /// just copied, Office, Remote Desktop. Keeps trying for a while in the
    /// background, unless something new is copied meanwhile: that is newer than
    /// what we'd put back.
    fn restore_later(saved: Saved) {
        let copied = unsafe { GetClipboardSequenceNumber() };
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline && unsafe { GetClipboardSequenceNumber() } == copied {
                let soon = Instant::now() + Duration::from_millis(100);
                if let Some(clip) = open_until(soon, || thread::sleep(Duration::from_millis(15))) {
                    // Checked again while it is open, so nothing slips in between.
                    if unsafe { GetClipboardSequenceNumber() } == copied {
                        fill(clip, &saved);
                    }
                    return;
                }
            }
        });
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

    pub fn paste(text: &str, press: impl FnOnce() -> bool, saved: Saved) -> Result<super::Pasted, String> {
        use super::Pasted;
        // Nothing is touched yet on either failure, so nothing goes back: writing the
        // saved copy over the clipboard would lose the formats it can't hold.
        let Some(hwnd) = promise_window() else {
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
        if !opened {
            unsafe { DestroyWindow(hwnd) };
            PROMISED.with(|promised| promised.borrow_mut().clear());
            return Err(super::BUSY.to_string());
        }
        let asked = || ASKED.with(Cell::get) > 0;
        // A clipboard watcher that ignores the flags above may read it right away;
        // then the read no longer tells when the paste happened.
        pump(Instant::now() + Duration::from_millis(30), || false);
        let early = asked();
        let result = if !press() {
            Ok(Pasted::NotSent)
        } else if early {
            pump(Instant::now() + Duration::from_millis(600), || false);
            Ok(Pasted::Assumed)
        } else if pump(Instant::now() + Duration::from_millis(1500), asked) {
            // The app reads the text while it holds the clipboard open; putting the
            // user's clipboard back waits for it to close.
            pump(Instant::now() + Duration::from_millis(30), || false);
            Ok(Pasted::Taken)
        } else {
            Ok(Pasted::Missed)
        };
        // The app that pasted may still hold the clipboard. Wait longer than a plain
        // restore, and keep answering the clipboard meanwhile: if the user's
        // content doesn't go back before the window goes, the answer is put there
        // for good, and the user's content follows in the background.
        let deadline = Instant::now() + Duration::from_secs(2);
        let restored = match open_until(deadline, || {
            pump(Instant::now() + Duration::from_millis(15), || false);
        }) {
            Some(clip) => {
                fill(clip, &saved);
                true
            }
            None => false,
        };
        unsafe { DestroyWindow(hwnd) };
        PROMISED.with(|promised| promised.borrow_mut().clear());
        if !restored {
            restore_later(saved);
        }
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
    use objc2::{
        rc::{autoreleasepool, Retained},
        runtime::ProtocolObject,
    };
    use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSData, NSString};

    /// Each pasteboard item with every type it carries.
    pub type Saved = Vec<Vec<(Retained<NSString>, Retained<NSData>)>>;

    /// Tells clipboard managers (Maccy, Paste, Alfred) that this write is not a new copy.
    const TRANSIENT: &str = "org.nspasteboard.TransientType";

    // Called from the engine's threads, which have no autorelease pool of their own.
    pub fn capture() -> Option<Saved> {
        autoreleasepool(|_| {
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
        })
    }

    pub fn paste(text: &str, press: impl FnOnce() -> bool, saved: Saved) -> Result<super::Pasted, String> {
        let result = super::lend(text, press);
        restore(saved);
        result
    }

    pub fn restore(saved: Saved) {
        autoreleasepool(|_| restore_in_pool(saved));
    }

    fn restore_in_pool(saved: Saved) {
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

// Linux (X11): what arboard can put back, the richest kind on the clipboard: a
// list of files, a picture, formatted text or plain text. An app's private
// formats are lost. lib.rs keeps one arboard instance alive, so what is put back
// here is still served after this one is dropped.
#[cfg(target_os = "linux")]
mod imp {
    use arboard::{Clipboard, Error, ImageData, SetExtLinux as _};
    use std::{
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, WindowClass};
    use x11rb::protocol::Event;

    pub enum Saved {
        Empty,
        Files(Vec<PathBuf>),
        Image(ImageData<'static>),
        Html { html: String, text: Option<String> },
        Text(String),
    }

    const TEXT: &[&str] = &[
        "UTF8_STRING",
        "text/plain;charset=utf-8",
        "text/plain;charset=UTF-8",
        "STRING",
        "TEXT",
        "text/plain",
    ];

    /// None when the app that holds the clipboard doesn't answer: then nothing
    /// may be touched, or its content would be gone. Formats are read only when
    /// offered, since arboard can't tell a missing one from an owner that is slow.
    pub fn capture() -> Option<Saved> {
        let Some(offered) = targets()? else { return Some(Saved::Empty) };
        let has = |names: &[&str]| offered.iter().any(|offered| names.contains(&offered.as_str()));
        let mut clipboard = Clipboard::new().ok()?;
        if has(&["text/uri-list"]) {
            if let Some(files) = read(clipboard.get().file_list())? {
                return Some(Saved::Files(files));
            }
        }
        if has(&["image/png"]) {
            if let Some(image) = read(clipboard.get().image())? {
                return Some(Saved::Image(image));
            }
        }
        let text = if has(TEXT) { read(clipboard.get().text())? } else { None };
        if has(&["text/html"]) {
            if let Some(html) = read(clipboard.get().html())? {
                return Some(Saved::Html { html, text });
            }
        }
        Some(text.map_or(Saved::Empty, Saved::Text))
    }

    /// None when the owner stopped answering; a format that turns out unreadable
    /// (a broken picture) is Some(None) and gives way to the next one.
    fn read<T>(result: Result<T, Error>) -> Option<Option<T>> {
        match result {
            Ok(value) => Some(Some(value)),
            Err(Error::ConversionFailure) => Some(None),
            Err(_) => None,
        }
    }

    /// The clipboard's formats by name; Some(None) when nobody holds it, None
    /// when its owner doesn't answer.
    fn targets() -> Option<Option<Vec<String>>> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atom = |name: &[u8]| Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom);
        let (clipboard, targets, property) =
            (atom(b"CLIPBOARD")?, atom(b"TARGETS")?, atom(b"AIRTYPE_TARGETS")?);
        if conn.get_selection_owner(clipboard).ok()?.reply().ok()?.owner == x11rb::NONE {
            return Some(None);
        }
        let window = conn.generate_id().ok()?;
        let aux = CreateWindowAux::new();
        let class = WindowClass::INPUT_ONLY;
        conn.create_window(0, window, root, 0, 0, 1, 1, 0, class, x11rb::COPY_FROM_PARENT, &aux).ok()?;
        conn.convert_selection(window, clipboard, targets, property, x11rb::CURRENT_TIME).ok()?;
        conn.flush().ok()?;
        let deadline = Instant::now() + Duration::from_secs(1);
        let answered = loop {
            match conn.poll_for_event().ok()? {
                Some(Event::SelectionNotify(event)) if event.requestor == window => {
                    break event.property != x11rb::NONE;
                }
                Some(_) => {}
                None if Instant::now() >= deadline => return None,
                None => thread::sleep(Duration::from_millis(2)),
            }
        };
        // An owner that won't list its formats: try for text, as before.
        if !answered {
            return Some(Some(vec!["UTF8_STRING".into()]));
        }
        let reply = conn.get_property(true, window, property, AtomEnum::ATOM, 0, 4096).ok()?.reply().ok()?;
        let cookies: Vec<_> =
            reply.value32().into_iter().flatten().filter_map(|atom| conn.get_atom_name(atom).ok()).collect();
        let names = cookies
            .into_iter()
            .filter_map(|cookie| cookie.reply().ok())
            .map(|reply| String::from_utf8_lossy(&reply.name).into_owned())
            .collect();
        Some(Some(names))
    }

    pub fn paste(text: &str, press: impl FnOnce() -> bool, saved: Saved) -> Result<super::Pasted, String> {
        let result = super::lend(text, press);
        restore(saved);
        result
    }

    pub fn restore(saved: Saved) {
        let Ok(mut clipboard) = Clipboard::new() else { return };
        // Out of clipboard history: the user's content is there already.
        let _ = match saved {
            Saved::Empty => clipboard.clear(),
            Saved::Files(files) => clipboard.set().exclude_from_history().file_list(&files),
            Saved::Image(image) => clipboard.set().exclude_from_history().image(image),
            Saved::Html { html, text } => clipboard.set().exclude_from_history().html(html, text),
            Saved::Text(text) => clipboard.set().exclude_from_history().text(text),
        };
    }
}

// Airtype doesn't run on other systems yet (`input::SUPPORTED`); this keeps the build whole.
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    pub type Saved = ();

    pub fn capture() -> Option<Saved> {
        Some(())
    }

    pub fn paste(text: &str, press: impl FnOnce() -> bool, _saved: Saved) -> Result<super::Pasted, String> {
        super::lend(text, press)
    }

    pub fn restore(_saved: Saved) {}
}

/// Puts the text on the clipboard, out of clipboard history, presses paste and
/// waits long enough for the app to read it.
#[cfg(not(windows))]
fn lend(text: &str, press: impl FnOnce() -> bool) -> Result<Pasted, String> {
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
    if !press() {
        return Ok(Pasted::NotSent);
    }
    // Apps read the clipboard a moment after Cmd/Ctrl+V; swapping it back too early
    // would paste the user's old clipboard instead.
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(Pasted::Assumed)
}

/// Needs an X server: `xvfb-run cargo test -- --ignored clipboard`.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use arboard::{Clipboard, ImageData};
    use std::time::{Duration, Instant};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};

    /// What an action does to the clipboard: Ctrl+C puts the selection there,
    /// then the user's content goes back.
    fn round_trip(clipboard: &mut Clipboard) {
        let saved = Saved::capture().expect("clipboard readable");
        clipboard.set_text("the selection").unwrap();
        saved.restore();
    }

    #[test]
    #[ignore = "needs an X server"]
    fn clipboard_comes_back_as_it_was() {
        // Kept alive, as lib.rs does, so what is put back stays served.
        let mut clipboard = Clipboard::new().unwrap();

        let image = ImageData { width: 2, height: 1, bytes: vec![255, 0, 0, 255, 0, 0, 255, 255].into() };
        clipboard.set_image(image.clone()).unwrap();
        round_trip(&mut clipboard);
        assert_eq!(clipboard.get_image().unwrap().bytes, image.bytes);
        assert!(clipboard.get_text().is_err());

        // A copied file is a file that is there.
        let file = std::env::temp_dir().join("airtype clipboard test.txt");
        std::fs::write(&file, "x").unwrap();
        let files = [file.canonicalize().unwrap()];
        clipboard.set().file_list(&files).unwrap();
        round_trip(&mut clipboard);
        assert_eq!(clipboard.get().file_list().unwrap(), files);

        clipboard.set_html("<b>bold</b>", Some("bold")).unwrap();
        round_trip(&mut clipboard);
        assert_eq!(clipboard.get().html().unwrap(), "<b>bold</b>");
        assert_eq!(clipboard.get_text().unwrap(), "bold");

        clipboard.set_text("plain").unwrap();
        round_trip(&mut clipboard);
        assert_eq!(clipboard.get_text().unwrap(), "plain");

        clipboard.clear().unwrap();
        round_trip(&mut clipboard);
        assert!(clipboard.get_text().is_err());

        // An owner that never answers: hands off, and quickly.
        let (conn, screen) = x11rb::connect(None).unwrap();
        let root = conn.setup().roots[screen].root;
        let window = conn.generate_id().unwrap();
        let aux = CreateWindowAux::new();
        conn.create_window(0, window, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &aux).unwrap();
        let selection = conn.intern_atom(false, b"CLIPBOARD").unwrap().reply().unwrap().atom;
        conn.set_selection_owner(window, selection, x11rb::CURRENT_TIME).unwrap();
        conn.get_input_focus().unwrap().reply().unwrap();
        let start = Instant::now();
        assert!(Saved::capture().is_none());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
