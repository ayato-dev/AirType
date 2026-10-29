//! Synthetic keyboard input for the app that has focus: copy, select all, paste.
//!
//! Keys are sent by physical key code, not by character, so Ctrl/Cmd+C works
//! with any keyboard layout (Russian included).

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

pub use imp::{accessibility_granted, foreground, restore_focus, wayland, SUPPORTED};

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

#[cfg(windows)]
mod imp {
    use super::Chord;
    use std::mem::size_of;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

    pub const SUPPORTED: bool = true;

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

    pub fn foreground() -> Target {
        unsafe { GetForegroundWindow() as usize }
    }

    /// Puts focus back on the window the text came from, in case something stole it.
    pub fn restore_focus(target: Target) {
        if target != 0 && foreground() != target {
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

    pub const SUPPORTED: bool = true;

    /// macOS gives focus back to the previous app on its own, so nothing to remember.
    pub struct Target;

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

    pub fn foreground() -> Target {
        Target
    }

    pub fn restore_focus(_target: Target) {}

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
    use super::Chord;
    use std::cell::OnceCell;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        ConnectionExt as _, KeyButMask, Keycode, Keysym, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
    };
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    pub const SUPPORTED: bool = true;

    /// The hotkey's window keeps focus on X11; nothing to remember.
    pub struct Target;

    const XK_A: Keysym = 0x61;
    const XK_C: Keysym = 0x63;
    const XK_V: Keysym = 0x76;
    const XK_CONTROL_L: Keysym = 0xffe3;

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
            let (Some(ctrl), Some(key)) = (keycode(&d.conn, XK_CONTROL_L), keycode(&d.conn, keysym)) else {
                return;
            };
            let presses = [
                (KEY_PRESS_EVENT, ctrl),
                (KEY_PRESS_EVENT, key),
                (KEY_RELEASE_EVENT, key),
                (KEY_RELEASE_EVENT, ctrl),
            ];
            for (kind, code) in presses {
                let _ = d.conn.xtest_fake_input(kind, code, x11rb::CURRENT_TIME, d.root, 0, 0, 0);
            }
            // A round trip makes sure the server has taken the keys before we go on.
            let _ = d.conn.get_input_focus().map(|cookie| cookie.reply());
        });
    }

    pub fn foreground() -> Target {
        Target
    }

    pub fn restore_focus(_target: Target) {}

    pub fn accessibility_granted(_prompt: bool) -> bool {
        true
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
    pub struct Target;

    pub fn modifiers_down() -> bool {
        false
    }
    pub fn send(_chord: Chord) {}
    pub fn foreground() -> Target {
        Target
    }
    pub fn restore_focus(_target: Target) {}
    pub fn accessibility_granted(_prompt: bool) -> bool {
        true
    }
    pub fn wayland() -> bool {
        false
    }
}
