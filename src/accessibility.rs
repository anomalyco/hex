use std::ffi::{CStr, c_char, c_void};
use std::ptr;

type CfTypeRef = *const c_void;
type CfStringRef = *const c_void;
type AxUiElementRef = *const c_void;

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const MESSAGING_TIMEOUT_SECS: f32 = 0.25;
const CARET_TIMEOUT_SECS: f32 = 0.10;
/// Taller than two ordinary lines: collapse multi-line selections to their
/// active end so the picker sits beside the line in view.
const MULTI_LINE_SELECTION_HEIGHT: f64 = 48.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CgRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
    fn AXUIElementCreateSystemWide() -> AxUiElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: *mut CfTypeRef,
    ) -> i32;
    fn AXUIElementSetMessagingTimeout(element: AxUiElementRef, timeout_in_seconds: f32) -> i32;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: AxUiElementRef,
        parameterized_attribute: CfStringRef,
        parameter: CfTypeRef,
        result: *mut CfTypeRef,
    ) -> i32;
    fn AXValueGetTypeID() -> usize;
    fn AXValueCreate(value_type: u32, value: *const c_void) -> CfTypeRef;
    fn AXValueGetValue(value: CfTypeRef, value_type: u32, value_out: *mut c_void) -> bool;
}

/// `kAXValueTypeCFRange`.
const AX_VALUE_CF_RANGE: u32 = 4;
/// `kAXValueTypeCGRect`.
const AX_VALUE_CGRECT: u32 = 3;

/// Core Foundation's `CFRange`, in UTF-16 code units.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CfRange {
    location: isize,
    length: isize,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFGetTypeID(value: CfTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetLength(value: CfStringRef) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(
        value: CfStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        string: *const c_char,
        encoding: u32,
    ) -> CfStringRef;
    fn CFURLGetTypeID() -> usize;
    fn CFURLGetString(url: CfTypeRef) -> CfStringRef;
    fn CFRelease(value: CfTypeRef);
}

/// Bounds of the focused text caret or selection in Quartz global coordinates
/// (points from the primary display's top-left, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Reads the focused text caret's global bounds via Accessibility, bounded by
/// [`CARET_TIMEOUT_SECS`] so a hung foreground app never stalls Paste Last.
/// Returns `None` when focus has no text caret or reports an unpopulated rect,
/// letting the caller fall back to the pointer location.
pub fn focused_caret_rect(pid: Option<i32>) -> Option<CaretRect> {
    let element = pid
        .and_then(focused_element_for_pid)
        .or_else(system_focused_element)?;
    caret_rect_from_range(&element).or_else(|| caret_rect_from_marker_range(&element))
}

pub fn focused_window_title(pid: i32) -> Option<String> {
    // SAFETY: AXUIElementCreateApplication returns a retained Core Foundation object.
    let application = bounded_element(unsafe { AXUIElementCreateApplication(pid) })?;
    let window = copy_element(&application, c"AXFocusedWindow")?;
    copy_string(&window, c"AXTitle").filter(|title| !title.is_empty())
}

/// The page address of the web area holding the application's focused
/// element, for browsers without a scripting dictionary (Firefox, Zen).
/// The outermost web area wins, so focus inside an iframe still reports
/// the page itself.
pub fn focused_web_url(pid: i32) -> Option<String> {
    const MAX_DEPTH: usize = 64;
    // SAFETY: AXUIElementCreateApplication returns a retained Core Foundation object.
    let application = bounded_element(unsafe { AXUIElementCreateApplication(pid) })?;
    let mut element = copy_element(&application, c"AXFocusedUIElement")?;
    let mut url = None;
    for _ in 0..MAX_DEPTH {
        if copy_string(&element, c"AXRole").as_deref() == Some("AXWebArea") {
            url = copy_url(&element, c"AXURL").or(url);
        }
        match copy_element(&element, c"AXParent") {
            Some(parent) => element = parent,
            None => break,
        }
    }
    url
}

/// The character just before the insertion point of the application's
/// focused text element, for Labs › Smart spacing. `None` when text is
/// selected, the cursor is at the start, or the app doesn't say.
pub fn char_before_caret(pid: i32) -> Option<char> {
    // SAFETY: AXUIElementCreateApplication returns a retained Core Foundation object.
    let application = bounded_element(unsafe { AXUIElementCreateApplication(pid) })?;
    let focused = copy_element(&application, c"AXFocusedUIElement")?;
    let selection = copy_range(&focused, c"AXSelectedTextRange")?;
    if selection.length != 0 || selection.location <= 0 {
        return None;
    }
    // One UTF-16 unit, or two when it is half of a surrogate pair.
    (1..=2.min(selection.location)).find_map(|units| {
        let range = CfRange {
            location: selection.location - units,
            length: units,
        };
        copy_string_for_range(&focused, range)?.chars().next_back()
    })
}

fn copy_range(element: &CfObject, name: &CStr) -> Option<CfRange> {
    let value = copy_attribute(element, name)?;
    if unsafe { CFGetTypeID(value.0) } != unsafe { AXValueGetTypeID() } {
        return None;
    }
    let mut range = CfRange::default();
    // SAFETY: `value` is an AXValue and `range` has CFRange's layout.
    unsafe {
        AXValueGetValue(
            value.0,
            AX_VALUE_CF_RANGE,
            (&mut range as *mut CfRange).cast(),
        )
    }
    .then_some(range)
}

fn copy_string_for_range(element: &CfObject, range: CfRange) -> Option<String> {
    // SAFETY: AXValueCreate copies the CFRange and returns a retained object.
    let parameter = unsafe { AXValueCreate(AX_VALUE_CF_RANGE, (&range as *const CfRange).cast()) };
    if parameter.is_null() {
        return None;
    }
    let parameter = CfObject(parameter);
    let attribute = CfObject(cf_string_literal(c"AXStringForRange")?);
    let mut value = ptr::null();
    let status = unsafe {
        AXUIElementCopyParameterizedAttributeValue(element.0, attribute.0, parameter.0, &mut value)
    };
    if status != 0 || value.is_null() {
        return None;
    }
    let value = CfObject(value);
    cf_string(value.0).filter(|text| !text.is_empty())
}

struct CfObject(CfTypeRef);

impl Drop for CfObject {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

fn bounded_element(element: AxUiElementRef) -> Option<CfObject> {
    bounded_element_with_timeout(element, MESSAGING_TIMEOUT_SECS)
}

fn bounded_element_with_timeout(element: AxUiElementRef, timeout_secs: f32) -> Option<CfObject> {
    if element.is_null() {
        return None;
    }
    let element = CfObject(element);
    (unsafe { AXUIElementSetMessagingTimeout(element.0, timeout_secs) } == 0).then_some(element)
}

fn focused_element_for_pid(pid: i32) -> Option<CfObject> {
    // SAFETY: AXUIElementCreateApplication returns a retained Core Foundation object.
    let app = bounded_element_with_timeout(
        unsafe { AXUIElementCreateApplication(pid) },
        CARET_TIMEOUT_SECS,
    )?;
    copy_element_with_timeout(&app, c"AXFocusedUIElement", CARET_TIMEOUT_SECS)
}

fn system_focused_element() -> Option<CfObject> {
    // SAFETY: AXUIElementCreateSystemWide returns a retained Core Foundation object.
    let system =
        bounded_element_with_timeout(unsafe { AXUIElementCreateSystemWide() }, CARET_TIMEOUT_SECS)?;
    copy_element_with_timeout(&system, c"AXFocusedUIElement", CARET_TIMEOUT_SECS)
}

fn caret_rect_from_range(element: &CfObject) -> Option<CaretRect> {
    let range = copy_range(element, c"AXSelectedTextRange")?;
    let rect = bounds_for_cf_range(element, range)?;
    if range.length > 0 && rect.height > MULTI_LINE_SELECTION_HEIGHT {
        let collapsed = CfRange {
            location: range.location.saturating_add(range.length),
            length: 0,
        };
        if let Some(end_rect) = bounds_for_cf_range(element, collapsed) {
            return Some(end_rect);
        }
    }
    Some(rect)
}

fn bounds_for_cf_range(element: &CfObject, range: CfRange) -> Option<CaretRect> {
    // SAFETY: `range` is a valid `CFRange`-layout value for the duration of the call.
    let param = unsafe { AXValueCreate(AX_VALUE_CF_RANGE, (&raw const range).cast()) };
    if param.is_null() {
        return None;
    }
    let param = CfObject(param);
    let bounds = copy_parameterized_attribute(element, c"AXBoundsForRange", param.0)?;
    ax_cg_rect(bounds.0).and_then(valid_caret_rect)
}

fn caret_rect_from_marker_range(element: &CfObject) -> Option<CaretRect> {
    let marker_range = copy_attribute(element, c"AXSelectedTextMarkerRange")?;
    let bounds =
        copy_parameterized_attribute(element, c"AXBoundsForTextMarkerRange", marker_range.0)?;
    ax_cg_rect(bounds.0).and_then(valid_caret_rect)
}

fn copy_parameterized_attribute(
    element: &CfObject,
    name: &CStr,
    parameter: CfTypeRef,
) -> Option<CfObject> {
    let attribute = CfObject(cf_string_literal(name)?);
    let mut value = ptr::null();
    let status = unsafe {
        AXUIElementCopyParameterizedAttributeValue(element.0, attribute.0, parameter, &mut value)
    };
    (status == 0 && !value.is_null()).then(|| CfObject(value))
}

fn ax_cg_rect(value: CfTypeRef) -> Option<CgRect> {
    if unsafe { CFGetTypeID(value) } != unsafe { AXValueGetTypeID() } {
        return None;
    }
    let mut rect = CgRect::default();
    unsafe { AXValueGetValue(value, AX_VALUE_CGRECT, (&raw mut rect).cast()) }.then_some(rect)
}

fn valid_caret_rect(rect: CgRect) -> Option<CaretRect> {
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width < 0.0
        || rect.height < 0.0
    {
        return None;
    }
    // Apps without layout bounds often answer (0, 0, 0, 0); macOS's menu bar
    // occupies y = 0, so an all-zero rect is never a live text caret.
    if rect.x == 0.0 && rect.y == 0.0 && rect.width == 0.0 && rect.height == 0.0 {
        return None;
    }
    Some(CaretRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height.max(16.0),
    })
}

fn copy_attribute(element: &CfObject, name: &CStr) -> Option<CfObject> {
    let attribute = CfObject(cf_string_literal(name)?);
    let mut value = ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element.0, attribute.0, &mut value) };
    // `then`, not `then_some`: dropping a `CfObject(null)` would CFRelease(NULL).
    (status == 0 && !value.is_null()).then(|| CfObject(value))
}

fn copy_element(element: &CfObject, name: &CStr) -> Option<CfObject> {
    copy_element_with_timeout(element, name, MESSAGING_TIMEOUT_SECS)
}

fn copy_element_with_timeout(
    element: &CfObject,
    name: &CStr,
    timeout_secs: f32,
) -> Option<CfObject> {
    let child = copy_attribute(element, name)?;
    (unsafe { AXUIElementSetMessagingTimeout(child.0, timeout_secs) } == 0).then_some(child)
}

fn copy_string(element: &CfObject, name: &CStr) -> Option<String> {
    let value = copy_attribute(element, name)?;
    cf_string(value.0)
}

fn copy_url(element: &CfObject, name: &CStr) -> Option<String> {
    let value = copy_attribute(element, name)?;
    if unsafe { CFGetTypeID(value.0) } != unsafe { CFURLGetTypeID() } {
        return cf_string(value.0);
    }
    // SAFETY: CFURLGetString follows the Get rule; `value` keeps it alive.
    let string = unsafe { CFURLGetString(value.0) };
    (!string.is_null()).then(|| cf_string(string)).flatten()
}

fn cf_string_literal(value: &CStr) -> Option<CfStringRef> {
    let string =
        unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), CF_STRING_ENCODING_UTF8) };
    (!string.is_null()).then_some(string)
}

fn cf_string(value: CfTypeRef) -> Option<String> {
    if unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
        return None;
    }
    let value = value.cast();
    let length = unsafe { CFStringGetLength(value) };
    let capacity = unsafe { CFStringGetMaximumSizeForEncoding(length, CF_STRING_ENCODING_UTF8) }
        .saturating_add(1);
    let mut bytes = vec![0_u8; capacity as usize];
    if !unsafe {
        CFStringGetCString(
            value,
            bytes.as_mut_ptr().cast(),
            capacity,
            CF_STRING_ENCODING_UTF8,
        )
    } {
        return None;
    }
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    bytes.truncate(length);
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_attribute_is_none() {
        assert_eq!(focused_window_title(i32::MAX), None);
        assert_eq!(focused_web_url(i32::MAX), None);
        assert_eq!(char_before_caret(i32::MAX), None);
    }

    #[test]
    fn ax_value_reads_rects_and_rejects_ranges() {
        let range = CfRange {
            location: 42,
            length: 7,
        };
        let boxed =
            CfObject(unsafe { AXValueCreate(AX_VALUE_CF_RANGE, (&raw const range).cast()) });
        assert_eq!(ax_cg_rect(boxed.0), None);

        let rect = CgRect {
            x: 312.0,
            y: 480.0,
            width: 1.0,
            height: 18.0,
        };
        let boxed = CfObject(unsafe { AXValueCreate(AX_VALUE_CGRECT, (&raw const rect).cast()) });
        assert_eq!(ax_cg_rect(boxed.0), Some(rect));
    }

    #[test]
    fn valid_caret_rect_rejects_unpopulated_or_non_finite_bounds() {
        assert_eq!(valid_caret_rect(CgRect::default()), None);
        assert_eq!(
            valid_caret_rect(CgRect {
                x: f64::NAN,
                y: 120.0,
                width: 1.0,
                height: 18.0,
            }),
            None
        );
        assert_eq!(
            valid_caret_rect(CgRect {
                x: 100.0,
                y: 200.0,
                width: -1.0,
                height: 18.0,
            }),
            None
        );
        assert_eq!(
            valid_caret_rect(CgRect {
                x: 240.0,
                y: 512.0,
                width: 0.0,
                height: 0.0,
            }),
            Some(CaretRect {
                x: 240.0,
                y: 512.0,
                width: 0.0,
                height: 16.0,
            })
        );
    }
}
