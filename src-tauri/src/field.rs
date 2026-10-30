//! The focused text field, asked directly through the accessibility API (AX on
//! macOS, UI Automation on Windows). That reads the text without touching the
//! clipboard, tells a selection from the whole field and spots password fields.
//! Apps that don't describe their fields this way come back as `Unknown`, and the
//! engine falls back to Ctrl/Cmd+C.

// Elsewhere the app doesn't run, and nothing builds a snapshot.
#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

use crate::popup::Anchor;

pub enum Focused {
    /// A password field: its text never leaves it.
    Password,
    Text(Snapshot),
    /// The app doesn't expose the field; use the keyboard and the clipboard.
    /// `select_all` is false when the focus is plainly not a text field (a web
    /// page, a list, a button): select-all there would grab the whole page.
    Unknown {
        select_all: bool,
        editable: Option<bool>,
    },
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
    /// Whether the answer can go back in: false for a page or a document opened
    /// for reading, `None` when the app doesn't say.
    pub editable: Option<bool>,
    /// The text exactly as the app reported it, to notice edits made while waiting.
    raw: String,
    field: imp::Field,
}

pub fn focused() -> Focused {
    imp::focused()
}

/// Where the focused text is on screen: the selection, else the caret in a large
/// field, else the field. `None` when the app doesn't say.
pub fn anchor() -> Option<Anchor> {
    imp::anchor().filter(|a| a.rect.w >= 0.0 && a.rect.h > 0.0)
}

impl Snapshot {
    fn new(raw: String, whole: bool, editable: Option<bool>, field: imp::Field) -> Self {
        let text = raw.replace("\r\n", "\n").replace('\r', "\n");
        Snapshot { text, whole, editable, raw, field }
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
    use crate::popup::{Anchor, Kind, Rect};
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::{CFString, CFStringRef};
    use std::{ffi::c_void, ptr};

    type AxError = i32;
    const AX_SUCCESS: AxError = 0;
    const AX_VALUE_CG_POINT: u32 = 1;
    const AX_VALUE_CG_SIZE: u32 = 2;
    const AX_VALUE_CG_RECT: u32 = 3;
    const AX_VALUE_CF_RANGE: u32 = 4;
    /// Seconds to wait for an app that has stopped answering.
    const TIMEOUT: f32 = 0.3;
    /// Enough to climb from a text field to the page it sits in.
    const MAX_DEPTH: usize = 40;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct CfRange {
        location: isize,
        length: isize,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct CgPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct CgSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct CgRect {
        origin: CgPoint,
        size: CgSize,
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
        fn AXUIElementCopyParameterizedAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            parameter: CFTypeRef,
            value: *mut CFTypeRef,
        ) -> AxError;
        fn AXValueCreate(kind: u32, value: *const c_void) -> CFTypeRef;
        fn AXValueGetValue(value: CFTypeRef, kind: u32, out: *mut c_void) -> u8;
    }

    pub struct Field {
        element: CFType,
        /// Inside a web page. There a paste goes through the page's own input
        /// handling; a value written over its head can leave the page out of step.
        web: bool,
        /// Keeps an Electron app's tree on until the answer is written back.
        _manual: ManualAccessibility,
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

    /// Unpacks an AXValue holding a range, point, size or rect.
    fn unpack<T: Default>(value: &CFType, kind: u32) -> Option<T> {
        let mut out = T::default();
        let ok = unsafe { AXValueGetValue(value.as_CFTypeRef(), kind, &mut out as *mut T as *mut c_void) };
        (ok != 0).then_some(out)
    }

    fn range(element: &CFType, name: &'static str) -> Option<CfRange> {
        unpack(&get(element, name)?, AX_VALUE_CF_RANGE)
    }

    fn frame(element: &CFType) -> Option<Rect> {
        let origin: CgPoint = unpack(&get(element, "AXPosition")?, AX_VALUE_CG_POINT)?;
        let size: CgSize = unpack(&get(element, "AXSize")?, AX_VALUE_CG_SIZE)?;
        Some(Rect { x: origin.x, y: origin.y, w: size.width, h: size.height })
    }

    fn bounds_for(element: &CFType, range: CfRange) -> Option<Rect> {
        let name = CFString::from_static_string("AXBoundsForRange");
        let param = unsafe {
            CFType::wrap_under_create_rule(AXValueCreate(
                AX_VALUE_CF_RANGE,
                &range as *const CfRange as *const c_void,
            ))
        };
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyParameterizedAttributeValue(
                element.as_CFTypeRef(),
                name.as_concrete_TypeRef(),
                param.as_CFTypeRef(),
                &mut value,
            )
        };
        if error != AX_SUCCESS || value.is_null() {
            return None;
        }
        let value = unsafe { CFType::wrap_under_create_rule(value) };
        let rect: CgRect = unpack(&value, AX_VALUE_CG_RECT)?;
        (rect.size.height > 0.0).then_some(Rect {
            x: rect.origin.x,
            y: rect.origin.y,
            w: rect.size.width,
            h: rect.size.height,
        })
    }

    pub fn anchor() -> Option<Anchor> {
        let text = |rect| Some(Anchor { rect, kind: Kind::Text });
        let system = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) };
        unsafe { AXUIElementSetMessagingTimeout(system.as_CFTypeRef(), TIMEOUT) };
        let element = get(&system, "AXFocusedUIElement")?;
        let field = frame(&element);
        // A one- or few-line field is the place to stand under; in a large one the
        // selection or the caret line is.
        let large = field.map_or(true, |f| f.h > 120.0);
        if let Some(selection) = range(&element, "AXSelectedTextRange") {
            if selection.length > 0 {
                if let Some(rect) = bounds_for(&element, selection) {
                    return text(rect);
                }
            } else if large {
                let caret = CfRange { location: (selection.location - 1).max(0), length: 1 };
                if let Some(rect) = bounds_for(&element, caret) {
                    return text(rect);
                }
            }
        }
        field.filter(|f| f.h <= 400.0).map(|rect| Anchor { rect, kind: Kind::Field })
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

    /// Roles that are never a text field. Select-all on them takes a whole page or list.
    const NOT_TEXT: &[&str] = &[
        "AXWebArea",
        "AXGroup",
        "AXList",
        "AXTable",
        "AXOutline",
        "AXBrowser",
        "AXScrollArea",
        "AXButton",
        "AXStaticText",
        "AXImage",
        "AXLink",
        "AXRow",
        "AXCell",
        "AXMenu",
        "AXMenuItem",
    ];

    /// Text you read, not write: a web page, a label.
    const READ_ONLY: &[&str] = &["AXWebArea", "AXStaticText"];

    /// Electron apps build their accessibility tree only when asked to, and a full
    /// tree slows them down. So the switch is flipped only when the app shows no
    /// focused element, and flipped back once the action is over.
    struct ManualAccessibility(Option<CFType>);

    impl ManualAccessibility {
        fn enable(app: &CFType) -> Self {
            let on = get(app, "AXManualAccessibility")
                .and_then(|v| v.downcast::<CFBoolean>())
                .is_some_and(bool::from);
            if on || !set(app, "AXManualAccessibility", &CFBoolean::true_value().as_CFType()) {
                return ManualAccessibility(None);
            }
            ManualAccessibility(Some(app.clone()))
        }
    }

    impl Drop for ManualAccessibility {
        fn drop(&mut self) {
            if let Some(app) = &self.0 {
                set(app, "AXManualAccessibility", &CFBoolean::false_value().as_CFType());
            }
        }
    }

    pub fn focused() -> Focused {
        let system = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) };
        unsafe { AXUIElementSetMessagingTimeout(system.as_CFTypeRef(), TIMEOUT) };

        let mut manual = ManualAccessibility(None);
        let mut element = get(&system, "AXFocusedUIElement");
        if element.is_none() {
            if let Some(app) = get(&system, "AXFocusedApplication") {
                manual = ManualAccessibility::enable(&app);
                if manual.0.is_some() {
                    // Chromium builds the tree asynchronously.
                    std::thread::sleep(std::time::Duration::from_millis(80));
                    element = get(&system, "AXFocusedUIElement");
                }
            }
        }

        let Some(element) = element else { return Focused::Unknown { select_all: true, editable: None } };
        if string(&element, "AXSubrole").as_deref() == Some("AXSecureTextField") {
            return Focused::Password;
        }
        let role = string(&element, "AXRole");
        let editable = if settable(&element, "AXValue") {
            Some(true)
        } else if role.as_deref().is_some_and(|r| READ_ONLY.contains(&r)) {
            Some(false)
        } else {
            None
        };
        let unknown = Focused::Unknown {
            select_all: !role.as_deref().is_some_and(|r| NOT_TEXT.contains(&r)),
            editable,
        };
        // Text you can edit has a selection range; buttons, lists and pages don't.
        let Some(value) = string(&element, "AXValue") else { return unknown };
        let Some(selection) = range(&element, "AXSelectedTextRange") else { return unknown };

        let selected = string(&element, "AXSelectedText").unwrap_or_default();
        // Something is selected but the app won't hand it over: Cmd+C will, and
        // the whole field must not be taken instead.
        if selection.length > 0 && selected.is_empty() {
            return unknown;
        }
        if selected.is_empty() && !settable(&element, "AXValue") {
            return unknown;
        }
        let web = in_web_page(&element);
        let field = Field { element, web, _manual: manual };
        if selected.is_empty() {
            Focused::Text(Snapshot::new(value, true, editable, field))
        } else {
            Focused::Text(Snapshot::new(selected, false, editable, field))
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
    use crate::popup::{Anchor, Kind, Rect};
    use windows::core::Interface;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern,
        IUIAutomationTextRange, IUIAutomationValuePattern, TextUnit_Character, UIA_ButtonControlTypeId,
        UIA_CalendarControlTypeId, UIA_CheckBoxControlTypeId, UIA_DataGridControlTypeId,
        UIA_DataItemControlTypeId, UIA_HeaderControlTypeId, UIA_HeaderItemControlTypeId,
        UIA_HyperlinkControlTypeId, UIA_ImageControlTypeId, UIA_ListControlTypeId, UIA_ListItemControlTypeId,
        UIA_MenuBarControlTypeId, UIA_MenuControlTypeId, UIA_MenuItemControlTypeId,
        UIA_ProgressBarControlTypeId, UIA_RadioButtonControlTypeId, UIA_ScrollBarControlTypeId,
        UIA_SliderControlTypeId, UIA_SplitButtonControlTypeId, UIA_StatusBarControlTypeId,
        UIA_TabControlTypeId, UIA_TabItemControlTypeId, UIA_TableControlTypeId, UIA_TextControlTypeId,
        UIA_TextPatternId, UIA_TitleBarControlTypeId, UIA_ToolBarControlTypeId, UIA_TreeControlTypeId,
        UIA_TreeItemControlTypeId, UIA_ValuePatternId, UIA_CONTROLTYPE_ID,
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
        let Some(automation) = automation() else {
            return Focused::Unknown { select_all: true, editable: None };
        };
        let Ok(element) = (unsafe { automation.GetFocusedElement() }) else {
            return Focused::Unknown { select_all: true, editable: None };
        };
        if unsafe { element.CurrentIsPassword() }.is_ok_and(|password| password.as_bool()) {
            return Focused::Password;
        }

        let selected = selection(&element);
        let editable = editable(&element);
        let field = Field { element, _automation: automation, _com: com };
        let unknown = |field: &Field| Focused::Unknown { select_all: may_be_text(&field.element), editable };
        match selected {
            Some(text) if !text.is_empty() => Focused::Text(Snapshot::new(text, false, editable, field)),
            // No text pattern, so no telling whether part of it is selected. Taking the
            // whole value could swallow a selection; Ctrl+C copies just what is selected.
            None => unknown(&field),
            Some(_) => match whole_text(&field.element) {
                Some(text) => Focused::Text(Snapshot::new(text, true, editable, field)),
                None => unknown(&field),
            },
        }
    }

    /// What the value pattern says; a browser page says read-only (its value is the URL).
    fn editable(element: &IUIAutomationElement) -> Option<bool> {
        unsafe {
            let value: IUIAutomationValuePattern = element.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
            Some(!value.CurrentIsReadOnly().ok()?.as_bool())
        }
    }

    /// False for controls that are plainly not text, and for read-only documents
    /// such as a web page (browsers expose the page's URL as a read-only value).
    fn may_be_text(element: &IUIAutomationElement) -> bool {
        const NOT_TEXT: &[UIA_CONTROLTYPE_ID] = &[
            UIA_ButtonControlTypeId,
            UIA_CalendarControlTypeId,
            UIA_CheckBoxControlTypeId,
            UIA_DataGridControlTypeId,
            UIA_DataItemControlTypeId,
            UIA_HeaderControlTypeId,
            UIA_HeaderItemControlTypeId,
            UIA_HyperlinkControlTypeId,
            UIA_ImageControlTypeId,
            UIA_ListControlTypeId,
            UIA_ListItemControlTypeId,
            UIA_MenuBarControlTypeId,
            UIA_MenuControlTypeId,
            UIA_MenuItemControlTypeId,
            UIA_ProgressBarControlTypeId,
            UIA_RadioButtonControlTypeId,
            UIA_ScrollBarControlTypeId,
            UIA_SliderControlTypeId,
            UIA_SplitButtonControlTypeId,
            UIA_StatusBarControlTypeId,
            UIA_TabControlTypeId,
            UIA_TabItemControlTypeId,
            UIA_TableControlTypeId,
            UIA_TextControlTypeId,
            UIA_TitleBarControlTypeId,
            UIA_ToolBarControlTypeId,
            UIA_TreeControlTypeId,
            UIA_TreeItemControlTypeId,
        ];
        unsafe {
            if element.CurrentControlType().is_ok_and(|kind| NOT_TEXT.contains(&kind)) {
                return false;
            }
            let read_only = element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .and_then(|value| value.CurrentIsReadOnly())
                .is_ok_and(|read_only| read_only.as_bool());
            !read_only
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

    pub fn anchor() -> Option<Anchor> {
        let _com = Com::init();
        let automation = automation()?;
        let element = unsafe { automation.GetFocusedElement() }.ok()?;
        let field = unsafe { element.CurrentBoundingRectangle() }.ok().map(|r| Rect {
            x: r.left as f64,
            y: r.top as f64,
            w: (r.right - r.left) as f64,
            h: (r.bottom - r.top) as f64,
        });
        // A one- or few-line field is the place to stand under; in a large one the
        // selection or the caret line is.
        let large = field.map_or(true, |f| f.h > 180.0);
        if let Some(rect) = text_bounds(&element, large) {
            return Some(Anchor { rect, kind: Kind::Text });
        }
        field.filter(|f| f.w > 0.0 && f.h <= 600.0).map(|rect| Anchor { rect, kind: Kind::Field })
    }

    /// The selected text on screen; with `caret`, the character at the caret when nothing is selected.
    fn text_bounds(element: &IUIAutomationElement, caret: bool) -> Option<Rect> {
        unsafe {
            let pattern: IUIAutomationTextPattern = element.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
            let ranges = pattern.GetSelection().ok()?;
            if ranges.Length().ok()? < 1 {
                return None;
            }
            let range = ranges.GetElement(0).ok()?;
            if range.GetText(1).ok()?.is_empty() {
                if !caret {
                    return None;
                }
                range.ExpandToEnclosingUnit(TextUnit_Character).ok()?;
            }
            bounding(&range)
        }
    }

    /// One rectangle around all lines of a range. UI Automation gives them as a
    /// flat array of doubles: left, top, width, height for each line.
    fn bounding(range: &IUIAutomationTextRange) -> Option<Rect> {
        use windows::Win32::System::Ole::{
            SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
            SafeArrayUnaccessData,
        };
        unsafe {
            let array = range.GetBoundingRectangles().ok()?;
            if array.is_null() {
                return None;
            }
            let count = match (SafeArrayGetLBound(array, 1), SafeArrayGetUBound(array, 1)) {
                (Ok(low), Ok(high)) if high >= low => (high - low + 1) as usize,
                _ => 0,
            };
            let mut data = std::ptr::null_mut();
            let mut rect: Option<Rect> = None;
            if count >= 4 && SafeArrayAccessData(array, &mut data).is_ok() {
                let values = std::slice::from_raw_parts(data as *const f64, count);
                for line in values.chunks_exact(4) {
                    let (x, y, w, h) = (line[0], line[1], line[2], line[3]);
                    if h <= 0.0 {
                        continue;
                    }
                    rect = Some(match rect {
                        None => Rect { x, y, w, h },
                        Some(r) => {
                            let left = r.x.min(x);
                            let top = r.y.min(y);
                            let right = (r.x + r.w).max(x + w);
                            let bottom = (r.y + r.h).max(y + h);
                            Rect { x: left, y: top, w: right - left, h: bottom - top }
                        }
                    });
                }
                let _ = SafeArrayUnaccessData(array);
            }
            let _ = SafeArrayDestroy(array);
            rect
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
    use super::{Anchor, Focused, Replaced};

    pub struct Field;

    pub fn anchor() -> Option<Anchor> {
        None
    }

    pub fn focused() -> Focused {
        Focused::Unknown { select_all: true, editable: None }
    }

    pub fn current(_field: &Field, _whole: bool) -> Option<String> {
        None
    }

    pub fn replace(_field: &Field, _whole: bool, _answer: &str) -> Replaced {
        Replaced::Unsupported
    }
}
