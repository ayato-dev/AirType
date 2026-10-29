//! Synthetic keyboard input for the app that has focus: copy, select all, paste,
//! and typing an answer in.
//!
//! Shortcuts are sent by physical key code, not by character, so Ctrl/Cmd+C works
//! with any keyboard layout (Russian included). Typed text goes as Unicode
//! characters, which no layout can change either.

use std::{
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub enum Chord {
    Copy,
    Paste,
    SelectAll,
}

pub use imp::{
    accessibility_granted, foreground, restore_focus, terminal_focused, wayland, Target, SUPPORTED,
};

/// Gives focus back to the app the text came from and waits until it has it.
/// False when another window took over meanwhile: then typing or pasting would
/// land in the wrong place.
pub fn return_to(target: Target) -> bool {
    imp::restore_focus(target);
    let start = Instant::now();
    while !imp::is_foreground(target) {
        if start.elapsed() >= Duration::from_millis(600) {
            return false;
        }
        thread::sleep(Duration::from_millis(15));
    }
    true
}

/// The hotkey's own modifiers are still held when it fires; pressing Ctrl+C on top
/// of them would send e.g. Ctrl+Alt+C, so wait until the user lets go.
pub fn wait_modifiers_released(timeout: Duration) {
    let start = Instant::now();
    while imp::modifiers_down() && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(10));
    }
}

pub fn press(chord: Chord) {
    imp::send(chord);
}

/// Types the text into the focused field as keystrokes, so the clipboard is never
/// touched. Only one line: Enter would send a chat message, Tab would leave the
/// field. False when the text or the system doesn't allow it; then paste instead.
pub fn type_text(text: &str) -> bool {
    typeable(text) && imp::type_text(text)
}

fn typeable(text: &str) -> bool {
    imp::CAN_TYPE && !text.is_empty() && text.chars().count() <= 1000 && !text.contains(['\n', '\r', '\t'])
}

/// Window classes of Windows consoles.
#[cfg_attr(not(windows), allow(dead_code))]
const TERMINAL_CLASSES: &[&str] =
    &["ConsoleWindowClass", "CASCADIA_HOSTING_WINDOW_CLASS", "mintty", "PuTTY", "VirtualConsoleClass"];

/// Executables of Windows terminals and shells, lowercase, without `.exe`.
#[cfg_attr(not(windows), allow(dead_code))]
const TERMINAL_EXES: &[&str] = &[
    "windowsterminal",
    "wt",
    "openconsole",
    "conhost",
    "cmd",
    "powershell",
    "pwsh",
    "wsl",
    "bash",
    "wezterm-gui",
    "alacritty",
    "mintty",
    "putty",
    "kitty",
    "tabby",
    "hyper",
    "conemu",
    "conemu64",
    "warp",
    "ghostty",
];

/// Bundle ids of macOS terminals.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const TERMINAL_BUNDLES: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "io.alacritty",
    "com.github.wez.wezterm",
    "dev.warp.Warp-Stable",
    "dev.warp.Warp",
    "com.mitchellh.ghostty",
    "co.zeit.hyper",
    "org.tabby",
    "com.raphaelamorim.rio",
];

/// WM_CLASS names of X11 terminals, lowercase. They copy and paste with
/// Ctrl+Shift+C/V; plain Ctrl+C would stop the running program.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const TERMINAL_WM_CLASSES: &[&str] = &[
    "gnome-terminal",
    "gnome-terminal-server",
    "kgx",
    "org.gnome.console",
    "org.gnome.ptyxis",
    "ptyxis",
    "konsole",
    "alacritty",
    "kitty",
    "tilix",
    "terminator",
    "xfce4-terminal",
    "mate-terminal",
    "lxterminal",
    "qterminal",
    "terminology",
    "st-256color",
    "org.wezfurlong.wezterm",
    "com.mitchellh.ghostty",
    "guake",
    "yakuake",
    "tilda",
    "warp",
];

/// Old X terminals with no copy shortcut at all: Ctrl+Shift+C still sends ^C there.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const KEYLESS_TERMINAL_WM_CLASSES: &[&str] = &["xterm", "uxterm", "urxvt", "rxvt"];

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Terminal {
    /// Copies and pastes with Ctrl+Shift+C/V.
    Shifted,
    /// Has no key that copies without interrupting the program.
    Keyless,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn terminal_kind(class: &str) -> Option<Terminal> {
    let class = class.to_ascii_lowercase();
    if TERMINAL_WM_CLASSES.contains(&class.as_str()) {
        Some(Terminal::Shifted)
    } else if KEYLESS_TERMINAL_WM_CLASSES.contains(&class.as_str()) {
        Some(Terminal::Keyless)
    } else {
        None
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_terminal_window(class: &str, exe_path: Option<&str>) -> bool {
    if TERMINAL_CLASSES.contains(&class) {
        return true;
    }
    let Some(path) = exe_path else { return false };
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase();
    let stem = file.strip_suffix(".exe").unwrap_or(&file);
    TERMINAL_EXES.contains(&stem)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn is_terminal_bundle(bundle_id: &str) -> bool {
    TERMINAL_BUNDLES.contains(&bundle_id)
}

#[cfg(windows)]
mod imp {
    use super::Chord;
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };

    pub const SUPPORTED: bool = true;
    pub const CAN_TYPE: bool = true;

    /// Foreground window handle, stored as an integer so it can cross threads.
    pub type Target = usize;

    const VK_A: VIRTUAL_KEY = 0x41;
    const VK_C: VIRTUAL_KEY = 0x43;
    const VK_V: VIRTUAL_KEY = 0x56;

    pub fn modifiers_down() -> bool {
        [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
            .iter()
            .any(|&vk| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0)
    }

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    pub fn send(chord: Chord) {
        let vk = match chord {
            Chord::Copy => VK_C,
            Chord::Paste => VK_V,
            Chord::SelectAll => VK_A,
        };
        let inputs = [key(VK_CONTROL, false), key(vk, false), key(vk, true), key(VK_CONTROL, true)];
        unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
        }
    }

    /// One UTF-16 unit as a key press; an emoji goes as its two surrogates.
    fn unicode(unit: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    pub fn type_text(text: &str) -> bool {
        let inputs: Vec<INPUT> =
            text.encode_utf16().flat_map(|unit| [unicode(unit, false), unicode(unit, true)]).collect();
        // A window of an elevated app refuses input from us; then nothing goes in at all.
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        sent as usize == inputs.len()
    }

    pub fn foreground() -> Target {
        unsafe { GetForegroundWindow() as usize }
    }

    pub fn is_foreground(target: Target) -> bool {
        target == 0 || foreground() == target
    }

    /// Puts focus back on the window the text came from. Allowed while one of our
    /// windows is in front, or while we handle the hotkey.
    pub fn restore_focus(target: Target) {
        if !is_foreground(target) {
            unsafe {
                SetForegroundWindow(target as _);
            }
        }
    }

    pub fn accessibility_granted(_prompt: bool) -> bool {
        true
    }

    pub fn wayland() -> bool {
        false
    }

    /// A console or terminal has focus. There Ctrl+C without a selection stops
    /// the running program instead of copying.
    pub fn terminal_focused() -> bool {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return false;
        }
        let mut class = [0u16; 128];
        let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
        let class = String::from_utf16_lossy(&class[..usize::try_from(len).unwrap_or(0)]);
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        super::is_terminal_window(&class, process_path(pid).as_deref())
    }

    fn process_path(pid: u32) -> Option<String> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut path = [0u16; 1024];
            let mut len = path.len() as u32;
            let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut len);
            CloseHandle(process);
            (ok != 0).then(|| String::from_utf16_lossy(&path[..len as usize]))
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Chord;
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::CFString;
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

    pub const SUPPORTED: bool = true;
    pub const CAN_TYPE: bool = true;

    /// The frontmost app's process id; 0 when there is none.
    pub type Target = i32;

    const KEY_A: CGKeyCode = 0x00;
    const KEY_C: CGKeyCode = 0x08;
    const KEY_V: CGKeyCode = 0x09;
    /// Shift, Control, Option, Command.
    const MODIFIER_MASK: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000;
    const HID_SYSTEM_STATE: i32 = 1;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    pub fn modifiers_down() -> bool {
        unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) & MODIFIER_MASK != 0 }
    }

    pub fn send(chord: Chord) {
        let code = match chord {
            Chord::Copy => KEY_C,
            Chord::Paste => KEY_V,
            Chord::SelectAll => KEY_A,
        };
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            return;
        };
        for down in [true, false] {
            if let Ok(event) = CGEvent::new_keyboard_event(source.clone(), code, down) {
                event.set_flags(CGEventFlags::CGEventFlagCommand);
                event.post(CGEventTapLocation::HID);
            }
        }
    }

    /// Key events that carry their text: the app inserts the characters as typed,
    /// whatever the key code says. macOS takes up to 20 UTF-16 units per event.
    pub fn type_text(text: &str) -> bool {
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            return false;
        };
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut start = 0;
        while start < units.len() {
            let mut end = (start + 20).min(units.len());
            // Keep a surrogate pair in one event.
            if end < units.len() && (0xD800..0xDC00).contains(&units[end - 1]) {
                end -= 1;
            }
            for down in [true, false] {
                let Ok(event) = CGEvent::new_keyboard_event(source.clone(), KEY_A, down) else {
                    return false;
                };
                event.set_flags(CGEventFlags::empty());
                event.set_string_from_utf16_unchecked(&units[start..end]);
                event.post(CGEventTapLocation::HID);
            }
            start = end;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        true
    }

    pub fn foreground() -> Target {
        NSWorkspace::sharedWorkspace().frontmostApplication().map_or(0, |app| app.processIdentifier())
    }

    pub fn is_foreground(target: Target) -> bool {
        target == 0 || foreground() == target
    }

    /// Our panel made Airtype the active app; hand activation back to the app the
    /// text came from. An active app may do that without asking.
    pub fn restore_focus(target: Target) {
        if is_foreground(target) {
            return;
        }
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(target) {
            app.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    }

    pub fn terminal_focused() -> bool {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .and_then(|app| app.bundleIdentifier())
            .is_some_and(|id| super::is_terminal_bundle(&id.to_string()))
    }

    /// Sending keystrokes needs the Accessibility permission. With `prompt`,
    /// macOS shows its own dialog that leads the user to the right switch.
    pub fn accessibility_granted(prompt: bool) -> bool {
        let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
        let value = if prompt { CFBoolean::true_value() } else { CFBoolean::false_value() };
        let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
    }

    pub fn wayland() -> bool {
        false
    }
}

/// X11 only: XTest presses the keys. Wayland lets no app grab global hotkeys or
/// type into other windows, so there it works just for apps running under XWayland.
#[cfg(target_os = "linux")]
mod imp {
    use super::{terminal_kind, Chord, Terminal};
    use std::cell::OnceCell;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, KeyButMask, Keycode, Keysym,
        KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
    };
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    pub const SUPPORTED: bool = true;

    /// The active X11 window; 0 when the window manager doesn't say.
    pub type Target = u32;

    /// XTest sends key codes, and a character missing from the layout has none.
    pub const CAN_TYPE: bool = false;

    pub fn type_text(_text: &str) -> bool {
        false
    }

    const XK_A: Keysym = 0x61;
    const XK_C: Keysym = 0x63;
    const XK_V: Keysym = 0x76;
    const XK_CONTROL_L: Keysym = 0xffe3;
    const XK_SHIFT_L: Keysym = 0xffe1;

    struct Display {
        conn: RustConnection,
        root: u32,
    }

    thread_local! {
        // One connection per engine run; each run has its own thread.
        static DISPLAY: OnceCell<Option<Display>> = const { OnceCell::new() };
    }

    fn with_display(f: impl FnOnce(&Display)) {
        DISPLAY.with(|cell| {
            let display = cell.get_or_init(|| {
                let (conn, screen) = x11rb::connect(None).ok()?;
                let root = conn.setup().roots.get(screen)?.root;
                Some(Display { conn, root })
            });
            if let Some(display) = display {
                f(display);
            }
        });
    }

    /// Finds the key that carries `keysym` in any layout group, so Ctrl+C is
    /// the same physical key with a Russian layout active.
    fn keycode(conn: &RustConnection, keysym: Keysym) -> Option<Keycode> {
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
        let per_key = usize::from(map.keysyms_per_keycode).max(1);
        let index = map.keysyms.chunks(per_key).position(|syms| syms.contains(&keysym))?;
        Some(min + index as Keycode)
    }

    pub fn modifiers_down() -> bool {
        let held = KeyButMask::SHIFT | KeyButMask::CONTROL | KeyButMask::MOD1 | KeyButMask::MOD4;
        let mut down = false;
        with_display(|d| {
            if let Ok(Ok(reply)) = d.conn.query_pointer(d.root).map(|cookie| cookie.reply()) {
                down = u16::from(reply.mask) & u16::from(held) != 0;
            }
        });
        down
    }

    pub fn send(chord: Chord) {
        let keysym = match chord {
            Chord::Copy => XK_C,
            Chord::Paste => XK_V,
            Chord::SelectAll => XK_A,
        };
        with_display(|d| {
            let shift = match focused_terminal(d) {
                None => false,
                Some(Terminal::Shifted) => !matches!(chord, Chord::SelectAll),
                // Any copy key there would stop the running program.
                Some(Terminal::Keyless) => return,
            };
            let (Some(ctrl), Some(key)) = (keycode(&d.conn, XK_CONTROL_L), keycode(&d.conn, keysym)) else {
                return;
            };
            let mut held = vec![ctrl];
            if shift {
                held.extend(keycode(&d.conn, XK_SHIFT_L));
            }
            let presses = held
                .iter()
                .map(|&code| (KEY_PRESS_EVENT, code))
                .chain([(KEY_PRESS_EVENT, key), (KEY_RELEASE_EVENT, key)])
                .chain(held.iter().rev().map(|&code| (KEY_RELEASE_EVENT, code)));
            for (kind, code) in presses {
                let _ = d.conn.xtest_fake_input(kind, code, x11rb::CURRENT_TIME, d.root, 0, 0, 0);
            }
            // A round trip makes sure the server has taken the keys before we go on.
            let _ = d.conn.get_input_focus().map(|cookie| cookie.reply());
        });
    }

    pub fn foreground() -> Target {
        let mut window = 0;
        with_display(|d| window = active_window(d).unwrap_or(0));
        window
    }

    pub fn is_foreground(target: Target) -> bool {
        target == 0 || foreground() == target
    }

    /// Asks the window manager to activate the window, as a taskbar would.
    pub fn restore_focus(target: Target) {
        if is_foreground(target) {
            return;
        }
        with_display(|d| {
            let Ok(Ok(reply)) = d.conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").map(|cookie| cookie.reply())
            else {
                return;
            };
            // Source 2: a pager or taskbar, which window managers obey without focus-stealing checks.
            let event = ClientMessageEvent::new(32, target, reply.atom, [2, x11rb::CURRENT_TIME, 0, 0, 0]);
            let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
            let _ = d.conn.send_event(false, d.root, mask, event);
            let _ = d.conn.flush();
        });
    }

    pub fn accessibility_granted(_prompt: bool) -> bool {
        true
    }

    /// A terminal has focus: there Ctrl+C stops the running program.
    pub fn terminal_focused() -> bool {
        let mut terminal = false;
        with_display(|d| terminal = focused_terminal(d).is_some());
        terminal
    }

    fn focused_terminal(d: &Display) -> Option<Terminal> {
        let window = active_window(d)?;
        let class = d
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        // WM_CLASS is "instance\0class\0".
        class.value.split(|&b| b == 0).find_map(|name| std::str::from_utf8(name).ok().and_then(terminal_kind))
    }

    fn active_window(d: &Display) -> Option<u32> {
        let atom = d.conn.intern_atom(true, b"_NET_ACTIVE_WINDOW").ok()?.reply().ok()?.atom;
        let reply = d.conn.get_property(false, d.root, atom, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
        let window = reply.value32()?.next();
        window.filter(|&window| window != 0)
    }

    pub fn wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|session| session == "wayland")
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    use super::Chord;

    pub const SUPPORTED: bool = false;
    pub const CAN_TYPE: bool = false;
    pub type Target = u32;

    pub fn is_foreground(_target: Target) -> bool {
        true
    }

    pub fn type_text(_text: &str) -> bool {
        false
    }

    pub fn modifiers_down() -> bool {
        false
    }
    pub fn send(_chord: Chord) {}
    pub fn foreground() -> Target {
        0
    }
    pub fn restore_focus(_target: Target) {}
    pub fn accessibility_granted(_prompt: bool) -> bool {
        true
    }
    pub fn wayland() -> bool {
        false
    }
    pub fn terminal_focused() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_terminals() {
        assert!(is_terminal_window("CASCADIA_HOSTING_WINDOW_CLASS", None));
        assert!(is_terminal_window("Chrome_WidgetWin_1", Some(r"C:\Program Files\PowerShell\7\pwsh.exe")));
        assert!(is_terminal_window("x", Some(r"C:\Users\me\AppData\Local\WezTerm\WEZTERM-GUI.EXE")));
        assert!(!is_terminal_window(
            "Chrome_WidgetWin_1",
            Some(r"C:\Program Files\Google\Chrome\chrome.exe")
        ));
        assert!(!is_terminal_window("Notepad", None));
        assert!(is_terminal_bundle("com.googlecode.iterm2"));
        assert!(!is_terminal_bundle("com.apple.TextEdit"));
        assert_eq!(terminal_kind("gnome-terminal-server"), Some(Terminal::Shifted));
        assert_eq!(terminal_kind("Alacritty"), Some(Terminal::Shifted));
        assert_eq!(terminal_kind("XTerm"), Some(Terminal::Keyless));
        assert_eq!(terminal_kind("firefox"), None);
    }
}
