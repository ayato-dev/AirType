//! The focused text field, asked directly through the accessibility API (AX on
//! macOS, UI Automation on Windows). That reads the text without touching the
//! clipboard, tells a selection from the whole field and spots password fields.
//! Apps that don't describe their fields this way come back as `Unknown`, and the
//! engine falls back to Ctrl/Cmd+C.

// Elsewhere the app doesn't run, and nothing builds a snapshot.
#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

pub enum Focused {
    /// A password field: its text never leaves it.
    Password,
    Text(Snapshot),
    /// The app doesn't expose the field; use the keyboard and the clipboard.
    Unknown,
}

pub enum Replaced {
    /// The field now holds the answer. Only macOS writes fields directly.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Done,
    /// The user edited the text while the model was thinking.
    Changed,
    /// The field can't be written this way; paste instead.
    Unsupported,
}

pub struct Snapshot {
    /// What goes to the model: the selection, or the whole field when nothing is selected.
    pub text: String,
    /// Nothing was selected, so the answer replaces the whole field.
    pub whole: bool,
    /// The text exactly as the app reported it, to notice edits made while waiting.
    raw: String,
    field: imp::Field,
}

pub fn focused() -> Focused {
    imp::focused()
}

impl Snapshot {
    fn new(raw: String, whole: bool, field: imp::Field) -> Self {
        let text = raw.replace("\r\n", "\n").replace('\r', "\n");
        Snapshot { text, whole, raw, field }
    }

    /// Puts the answer where the text came from, unless the user changed it meanwhile.
    pub fn replace(&self, answer: &str) -> Replaced {
        match imp::current(&self.field, self.whole) {
            Some(now) if now != self.raw => Replaced::Changed,
            Some(_) => imp::replace(&self.field, self.whole, answer),
            None => Replaced::Unsupported,
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{Focused, Replaced, Snapshot};
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::{CFString, CFStringRef};
    use std::{ffi::c_void, ptr};

    type AxError = i32;
    const AX_SUCCESS: AxError = 0;
    const AX_VALUE_CF_RANGE: u32 = 4;
    /// Seconds to wait for an app that has stopped answering.
    const TIMEOUT: f32 = 0.3;
    /// Enough to climb from a text field to the page it sits in.
    const MAX_DEPTH: usize = 40;

    #[repr(C)]
    #[derive(Default)]
    struct CfRange {
        location: isize,
        length: isize,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AxError;
        fn AXUIElementSetAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> AxError;
        fn AXUIElementIsAttributeSettable(
            element: CFTypeRef,
            attribute: CFStringRef,
            settable: *mut u8,
        ) -> AxError;
        fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> AxError;
        fn AXValueCreate(kind: u32, value: *const c_void) -> CFTypeRef;
        fn AXValueGetValue(value: CFTypeRef, kind: u32, out: *mut c_void) -> u8;
    }

    pub struct Field {
        element: CFType,
        /// Inside a web page. There a paste goes through the page's own input
        /// handling; a value written over its head can leave the page out of step.
        web: bool,
    }

    fn get(element: &CFType, name: &'static str) -> Option<CFType> {
        let name = CFString::from_static_string(name);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyAttributeValue(element.as_CFTypeRef(), name.as_concrete_TypeRef(), &mut value)
        };
        (error == AX_SUCCESS && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
    }

    fn set(element: &CFType, name: &'static str, value: &CFType) -> bool {
        let name = CFString::from_static_string(name);
        let error = unsafe {
            AXUIElementSetAttributeValue(
                element.as_CFTypeRef(),
                name.as_concrete_TypeRef(),
                value.as_CFTypeRef(),
            )
        };
        error == AX_SUCCESS
    }

    fn settable(element: &CFType, name: &'static str) -> bool {
        let name = CFString::from_static_string(name);
        let mut settable = 0u8;
        let error = unsafe {
            AXUIElementIsAttributeSettable(element.as_CFTypeRef(), name.as_concrete_TypeRef(), &mut settable)
        };
        error == AX_SUCCESS && settable != 0
    }

    fn string(element: &CFType, name: &'static str) -> Option<String> {
        get(element, name)?.downcast::<CFString>().map(|s| s.to_string())
    }

    fn has_range(element: &CFType, name: &'static str) -> bool {
        let Some(value) = get(element, name) else { return false };
        let mut range = CfRange::default();
        unsafe {
            AXValueGetValue(
                value.as_CFTypeRef(),
                AX_VALUE_CF_RANGE,
                &mut range as *mut CfRange as *mut c_void,
            ) != 0
        }
    }

    fn in_web_page(element: &CFType) -> bool {
        let mut current = element.clone();
        for _ in 0..MAX_DEPTH {
            if string(&current, "AXRole").as_deref() == Some("AXWebArea") {
                return true;
            }
            match get(&current, "AXParent") {
                Some(parent) => current = parent,
                None => return false,
            }
        }
        false
    }

    pub fn focused() -> Focused {
        let system = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) };
        unsafe { AXUIElementSetMessagingTimeout(system.as_CFTypeRef(), TIMEOUT) };

        // Electron apps build their accessibility tree only when asked to. The first
        // hotkey in such an app may still miss it and go through the clipboard.
        if let Some(app) = get(&system, "AXFocusedApplication") {
            set(&app, "AXManualAccessibility", &CFBoolean::true_value().as_CFType());
        }

        let Some(element) = get(&system, "AXFocusedUIElement") else { return Focused::Unknown };
        if string(&element, "AXSubrole").as_deref() == Some("AXSecureTextField") {
            return Focused::Password;
        }
        // Text you can edit has a selection range; buttons, lists and pages don't.
        let Some(value) = string(&element, "AXValue") else { return Focused::Unknown };
        if !has_range(&element, "AXSelectedTextRange") {
            return Focused::Unknown;
        }

        let selected = string(&element, "AXSelectedText").unwrap_or_default();
        if selected.is_empty() && !settable(&element, "AXValue") {
            return Focused::Unknown;
        }
        let web = in_web_page(&element);
        let field = Field { element, web };
        if selected.is_empty() {
            Focused::Text(Snapshot::new(value, true, field))
        } else {
            Focused::Text(Snapshot::new(selected, false, field))
        }
    }

    pub fn current(field: &Field, whole: bool) -> Option<String> {
        string(&field.element, if whole { "AXValue" } else { "AXSelectedText" })
    }

    pub fn replace(field: &Field, whole: bool, answer: &str) -> Replaced {
        if field.web {
            return Replaced::Unsupported;
        }
        let Some(before) = string(&field.element, "AXValue") else { return Replaced::Unsupported };
        if whole {
            // AX ranges count UTF-16 units, like NSString.
            let all = CfRange { location: 0, length: before.encode_utf16().count() as isize };
            let range = unsafe {
                CFType::wrap_under_create_rule(AXValueCreate(
                    AX_VALUE_CF_RANGE,
                    &all as *const CfRange as *const c_void,
                ))
            };
            if !set(&field.element, "AXSelectedTextRange", &range) {
                return Replaced::Unsupported;
            }
        }
        // Writing the selected text goes through the field's own editing, so it can be undone.
        if !set(&field.element, "AXSelectedText", &CFString::new(answer).as_CFType()) {
            return Replaced::Unsupported;
        }
        // Some apps accept the call and change nothing. Then a paste is still needed;
        // if anything did change, pasting on top would only duplicate it.
        match string(&field.element, "AXValue") {
            Some(after) if after != before || squash(&after).contains(&squash(answer)) => Replaced::Done,
            _ => Replaced::Unsupported,
        }
    }

    /// Fields rewrite line breaks and spaces their own way; compare without them.
    fn squash(text: &str) -> String {
        text.chars().filter(|c| !c.is_whitespace()).collect()
    }
}

#[cfg(windows)]
mod imp {
    use super::{Focused, Replaced, Snapshot};
    use windows::core::Interface;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern,
        IUIAutomationValuePattern, UIA_TextPatternId, UIA_ValuePatternId,
    };

    /// Milliseconds to wait for an app that has stopped answering.
    const TIMEOUT_MS: u32 = 400;

    /// Keeps COM initialised on this thread while a field is held.
    struct Com(bool);

    impl Com {
        fn init() -> Self {
            Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    // Fields drop in order, so COM outlives the objects that need it.
    pub struct Field {
        element: IUIAutomationElement,
        _automation: IUIAutomation,
        _com: Com,
    }

    pub fn focused() -> Focused {
        let com = Com::init();
        let Some(automation) = automation() else { return Focused::Unknown };
        let Ok(element) = (unsafe { automation.GetFocusedElement() }) else { return Focused::Unknown };
        if unsafe { element.CurrentIsPassword() }.is_ok_and(|password| password.as_bool()) {
            return Focused::Password;
        }

        let selected = selection(&element);
        let field = Field { element, _automation: automation, _com: com };
        match selected {
            Some(text) if !text.is_empty() => Focused::Text(Snapshot::new(text, false, field)),
            _ => match whole_text(&field.element) {
                Some(text) => Focused::Text(Snapshot::new(text, true, field)),
                None => Focused::Unknown,
            },
        }
    }

    fn automation() -> Option<IUIAutomation> {
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }.ok()?;
        if let Ok(timeouts) = automation.cast::<IUIAutomation2>() {
            unsafe {
                let _ = timeouts.SetConnectionTimeout(TIMEOUT_MS);
                let _ = timeouts.SetTransactionTimeout(TIMEOUT_MS);
            }
        }
        Some(automation)
    }

    /// The selected text; empty when there is only a caret.
    fn selection(element: &IUIAutomationElement) -> Option<String> {
        unsafe {
            let pattern: IUIAutomationTextPattern = element.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
            let ranges = pattern.GetSelection().ok()?;
            if ranges.Length().ok()? < 1 {
                return Some(String::new());
            }
            Some(ranges.GetElement(0).ok()?.GetText(-1).ok()?.to_string())
        }
    }

    /// The whole field, but only for something you can type into: a focused web
    /// page or document viewer would otherwise hand over all of its text.
    fn whole_text(element: &IUIAutomationElement) -> Option<String> {
        unsafe {
            let value: IUIAutomationValuePattern = element.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
            if value.CurrentIsReadOnly().ok()?.as_bool() {
                return None;
            }
            let document = element
                .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                .and_then(|pattern| pattern.DocumentRange())
                .and_then(|range| range.GetText(-1));
            match document {
                Ok(text) => Some(text.to_string()),
                Err(_) => Some(value.CurrentValue().ok()?.to_string()),
            }
        }
    }

    pub fn current(field: &Field, whole: bool) -> Option<String> {
        if whole {
            whole_text(&field.element)
        } else {
            selection(&field.element)
        }
    }

    /// UI Automation can only overwrite a whole value, which skips the app's undo and
    /// its own input handling. Ctrl+V keeps both, so writing always goes that way.
    pub fn replace(_field: &Field, _whole: bool, _answer: &str) -> Replaced {
        Replaced::Unsupported
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::{Focused, Replaced};

    pub struct Field;

    pub fn focused() -> Focused {
        Focused::Unknown
    }

    pub fn current(_field: &Field, _whole: bool) -> Option<String> {
        None
    }

    pub fn replace(_field: &Field, _whole: bool, _answer: &str) -> Replaced {
        Replaced::Unsupported
    }
}
