//! A copy of everything on the system clipboard, every format the app put there,
//! so the user gets their clipboard back exactly as it was after a paste. arboard
//! alone only knows text, images and HTML: a copied file, an RTF table or an
//! app's private format would be lost.

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

#[cfg(windows)]
mod imp {
    use std::{ptr, thread, time::Duration};
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
        RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::System::Ole::{
        CF_BITMAP, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT, CF_ENHMETAFILE, CF_METAFILEPICT,
        CF_OWNERDISPLAY, CF_PALETTE,
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

    /// Opens the clipboard, retrying while another app has it open.
    fn open() -> Option<Clip> {
        for _ in 0..10 {
            if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
                return Some(Clip);
            }
            thread::sleep(Duration::from_millis(15));
        }
        None
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
            if !copyable(format) {
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
        let Some(_clip) = open() else { return };
        unsafe { EmptyClipboard() };
        for (format, bytes) in &saved {
            put(*format, bytes);
        }
        // The user already has this content in Win+V history; don't add it twice.
        let no_history: Vec<u16> = "CanIncludeInClipboardHistory\0".encode_utf16().collect();
        let format = unsafe { RegisterClipboardFormatW(no_history.as_ptr()) };
        if format != 0 && !saved.iter().any(|(f, _)| *f == format) {
            put(format, &0u32.to_ne_bytes());
        }
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

// The app doesn't run elsewhere; keep plain text so the build and tests work.
#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub type Saved = Option<String>;

    pub fn capture() -> Option<Saved> {
        Some(arboard::Clipboard::new().ok()?.get_text().ok())
    }

    pub fn restore(saved: Saved) {
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return };
        let _ = match saved {
            Some(text) => clipboard.set_text(text),
            None => clipboard.clear(),
        };
    }
}
