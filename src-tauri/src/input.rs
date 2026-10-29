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

pub use imp::{accessibility_granted, foreground, restore_focus, terminal_focused, SUPPORTED};

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
        VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };

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

    pub fn terminal_focused() -> bool {
        objc2_app_kit::NSWorkspace::sharedWorkspace()
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
}

#[cfg(not(any(windows, target_os = "macos")))]
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
    }
}
