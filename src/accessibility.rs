use std::ffi::{CStr, c_char, c_void};
use std::ptr;

type CfTypeRef = *const c_void;
type CfStringRef = *const c_void;
type AxUiElementRef = *const c_void;

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const MESSAGING_TIMEOUT_SECS: f32 = 0.25;
/// Each call while finding the caret for Labs › Words to cursor, which runs
/// just before a paste: a slow app loses its flight, never the paste's time.
const CARET_TIMEOUT_SECS: f32 = 0.05;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
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
    fn AXUIElementSetAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: CfTypeRef,
    ) -> i32;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
}

/// `kAXValueTypeCGPoint`, `kAXValueTypeCGSize`, `kAXValueTypeCGRect`.
const AX_VALUE_CG_POINT: u32 = 1;
const AX_VALUE_CG_SIZE: u32 = 2;
const AX_VALUE_CG_RECT: u32 = 3;

/// `kAXValueTypeCFRange`.
const AX_VALUE_CF_RANGE: u32 = 4;

/// Core Foundation's `CFRange`, in UTF-16 code units.
#[repr(C)]
#[derive(Clone, Copy, Default)]
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
    static kCFBooleanTrue: CfTypeRef;
}

/// A rectangle in Quartz global points: from the primary display's
/// top-left corner, y down (Rune's global space).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenRect {
    pub fn center(self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    fn finite(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
    }

    /// Whether `inner` lies inside, give or take `slack` points.
    fn holds(self, inner: Self, slack: f64) -> bool {
        inner.x >= self.x - slack
            && inner.y >= self.y - slack
            && inner.x + inner.width <= self.x + self.width + slack
            && inner.y + inner.height <= self.y + self.height + slack
    }
}

/// Where a paste into the frontmost app went, for Labs › Words to cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CaretTarget {
    /// The insertion point itself.
    Caret(ScreenRect),
    /// Only the focused text field's frame.
    Field(ScreenRect),
}

/// Whether the frontmost app's text is off limits: secure input is on
/// (a password field has focus somewhere).
pub fn secure_input() -> bool {
    // SAFETY: a plain Carbon query with no arguments.
    unsafe { IsSecureEventInputEnabled() }
}

/// Where the frontmost app's insertion point is. `None` in a password
/// field, when nothing editable has focus, or when what the app reports
/// isn't believable (a collapsed Chromium rect, or one outside the focused
/// window): better no flight than one to the wrong place. Each
/// Accessibility call is bounded by 50 ms.
pub fn caret_target(pid: i32) -> Option<CaretTarget> {
    let (focused, window) = focused_text(pid)?;
    let by_range = copy_range(&focused, c"AXSelectedTextRange").and_then(|selection| {
        copy_bounds_for_range(
            &focused,
            CfRange {
                location: selection.location + selection.length,
                length: 0,
            },
        )
    });
    let rect = by_range
        .filter(|rect| plausible_caret(*rect, window))
        .or_else(|| {
            // Chromium and Electron only answer with text markers.
            let markers = copy_attribute(&focused, c"AXSelectedTextMarkerRange")?;
            copy_parameterized_rect(&focused, c"AXBoundsForTextMarkerRange", &markers)
                .filter(|rect| plausible_caret(*rect, window))
        });
    if let Some(rect) = rect {
        return Some(CaretTarget::Caret(rect));
    }
    let role = copy_string(&focused, c"AXRole");
    if !matches!(
        role.as_deref(),
        Some("AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField")
    ) {
        return None;
    }
    element_frame(&focused)
        .filter(|frame| plausible_field(*frame, window))
        .map(CaretTarget::Field)
}

/// Asks Chromium and Electron apps to build their accessibility tree, which
/// they keep asleep until an assistive app asks, so it's ready by the time a
/// dictation pastes. Other apps ignore it.
pub fn wake_text_markers(pid: i32) {
    // SAFETY: AXUIElementCreateApplication returns a retained Core Foundation object.
    let Some(application) = bounded_element(
        unsafe { AXUIElementCreateApplication(pid) },
        CARET_TIMEOUT_SECS,
    ) else {
        return;
    };
    let Some(attribute) = cf_string_literal(c"AXManualAccessibility") else {
        return;
    };
    let attribute = CfObject(attribute);
    // SAFETY: both are valid Core Foundation objects; kCFBooleanTrue is a constant.
    unsafe { AXUIElementSetAttributeValue(application.0, attribute.0, kCFBooleanTrue) };
}

/// Where each of `text`'s first `limit` words sits on screen, once the
/// frontmost app shows it pasted: `None` until the text just before the
/// cursor reads as `text`, word for word, and for each word whose rect
/// isn't believable.
pub fn pasted_words(pid: i32, text: &str, limit: usize) -> Option<Vec<Option<ScreenRect>>> {
    let body = text.trim_start();
    let units = body.encode_utf16().count() as isize;
    if units == 0 {
        return None;
    }
    let (focused, window) = focused_text(pid)?;
    let selection = copy_range(&focused, c"AXSelectedTextRange")?;
    let start = selection.location - units;
    if selection.length != 0 || start < 0 {
        return None;
    }
    let found = copy_string_for_range(
        &focused,
        CfRange {
            location: start,
            length: units,
        },
    )?;
    if !found.split_whitespace().eq(body.split_whitespace()) {
        return None;
    }
    Some(
        utf16_words(&found)
            .take(limit)
            .map(|(offset, units)| {
                copy_bounds_for_range(
                    &focused,
                    CfRange {
                        location: start + offset,
                        length: units,
                    },
                )
                .filter(|rect| plausible_word(*rect, window))
            })
            .collect(),
    )
}

/// The frontmost app's focused element, unless it's a password field, and
/// its focused window's frame.
fn focused_text(pid: i32) -> Option<(CfObject, ScreenRect)> {
    // SAFETY: AXUIElementCreateApplication returns a retained Core Foundation object.
    let application = bounded_element(
        unsafe { AXUIElementCreateApplication(pid) },
        CARET_TIMEOUT_SECS,
    )?;
    let focused = copy_element(&application, c"AXFocusedUIElement", CARET_TIMEOUT_SECS)?;
    if copy_string(&focused, c"AXSubrole").as_deref() == Some("AXSecureTextField") {
        return None;
    }
    let window = copy_element(&application, c"AXFocusedWindow", CARET_TIMEOUT_SECS)
        .and_then(|window| element_frame(&window))?;
    Some((focused, window))
}

/// Each whitespace-separated word's offset and length in UTF-16 units.
fn utf16_words(text: &str) -> impl Iterator<Item = (isize, isize)> + '_ {
    let mut offset = 0_isize;
    let mut start = None;
    let mut chars = text.chars().peekable();
    std::iter::from_fn(move || {
        loop {
            let Some(ch) = chars.next() else {
                return start.take().map(|from| (from, offset - from));
            };
            let at = offset;
            offset += ch.len_utf16() as isize;
            match (ch.is_whitespace(), start) {
                (false, None) => start = Some(at),
                (true, Some(from)) => {
                    start = None;
                    return Some((from, at - from));
                }
                _ => {}
            }
        }
    })
}

/// A collapsed caret: line-height tall, a few points wide at most, inside
/// the focused window. Chromium reports an unanswered query as
/// `(0, primary height, 0, 0)`, which this refuses.
fn plausible_caret(rect: ScreenRect, window: ScreenRect) -> bool {
    rect.finite()
        && (4.0..=200.0).contains(&rect.height)
        && (0.0..=40.0).contains(&rect.width)
        && window.holds(rect, 8.0)
}

/// One pasted word's rect: on screen, inside the window, no taller than a
/// few lines (a word wrapped across lines reports their union).
fn plausible_word(rect: ScreenRect, window: ScreenRect) -> bool {
    rect.finite()
        && (4.0..=200.0).contains(&rect.height)
        && rect.width > 0.0
        && window.holds(rect, 8.0)
}

fn plausible_field(rect: ScreenRect, window: ScreenRect) -> bool {
    rect.finite() && rect.width >= 8.0 && rect.height >= 8.0 && window.holds(rect, 8.0)
}

pub fn focused_window_title(pid: i32) -> Option<String> {
    // SAFETY: AXUIElementCreateApplication returns a retained Core Foundation object.
    let application = bounded_element(
        unsafe { AXUIElementCreateApplication(pid) },
        MESSAGING_TIMEOUT_SECS,
    )?;
    let window = copy_element(&application, c"AXFocusedWindow", MESSAGING_TIMEOUT_SECS)?;
    copy_string(&window, c"AXTitle").filter(|title| !title.is_empty())
}

/// The page address of the web area holding the application's focused
/// element, for browsers without a scripting dictionary (Firefox, Zen).
/// The outermost web area wins, so focus inside an iframe still reports
/// the page itself.
pub fn focused_web_url(pid: i32) -> Option<String> {
    const MAX_DEPTH: usize = 64;
    // SAFETY: AXUIElementCreateApplication returns a retained Core Foundation object.
    let application = bounded_element(
        unsafe { AXUIElementCreateApplication(pid) },
        MESSAGING_TIMEOUT_SECS,
    )?;
    let mut element = copy_element(&application, c"AXFocusedUIElement", MESSAGING_TIMEOUT_SECS)?;
    let mut url = None;
    for _ in 0..MAX_DEPTH {
        if copy_string(&element, c"AXRole").as_deref() == Some("AXWebArea") {
            url = copy_url(&element, c"AXURL").or(url);
        }
        match copy_element(&element, c"AXParent", MESSAGING_TIMEOUT_SECS) {
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
    let application = bounded_element(
        unsafe { AXUIElementCreateApplication(pid) },
        MESSAGING_TIMEOUT_SECS,
    )?;
    let focused = copy_element(&application, c"AXFocusedUIElement", MESSAGING_TIMEOUT_SECS)?;
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

fn bounded_element(element: AxUiElementRef, timeout: f32) -> Option<CfObject> {
    if element.is_null() {
        return None;
    }
    let element = CfObject(element);
    (unsafe { AXUIElementSetMessagingTimeout(element.0, timeout) } == 0).then_some(element)
}

fn copy_attribute(element: &CfObject, name: &CStr) -> Option<CfObject> {
    let attribute = CfObject(cf_string_literal(name)?);
    let mut value = ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element.0, attribute.0, &mut value) };
    // `then`, not `then_some`: dropping a `CfObject(null)` would CFRelease(NULL).
    (status == 0 && !value.is_null()).then(|| CfObject(value))
}

fn copy_element(element: &CfObject, name: &CStr, timeout: f32) -> Option<CfObject> {
    let child = copy_attribute(element, name)?;
    (unsafe { AXUIElementSetMessagingTimeout(child.0, timeout) } == 0).then_some(child)
}

/// An element's `AXPosition` and `AXSize`.
fn element_frame(element: &CfObject) -> Option<ScreenRect> {
    let mut origin = [0.0_f64; 2];
    let mut size = [0.0_f64; 2];
    copy_ax_value(element, c"AXPosition", AX_VALUE_CG_POINT, &mut origin)?;
    copy_ax_value(element, c"AXSize", AX_VALUE_CG_SIZE, &mut size)?;
    Some(ScreenRect {
        x: origin[0],
        y: origin[1],
        width: size[0],
        height: size[1],
    })
}

/// Reads an `AXValue` attribute of `value_type` into `out`, which has its
/// layout (two `f64`s for a point or a size).
fn copy_ax_value(
    element: &CfObject,
    name: &CStr,
    value_type: u32,
    out: &mut [f64; 2],
) -> Option<()> {
    let value = copy_attribute(element, name)?;
    ax_value_into(&value, value_type, out.as_mut_ptr().cast())
}

fn ax_value_into(value: &CfObject, value_type: u32, out: *mut c_void) -> Option<()> {
    if unsafe { CFGetTypeID(value.0) } != unsafe { AXValueGetTypeID() } {
        return None;
    }
    // SAFETY: `value` is an AXValue and the caller's `out` has the layout of `value_type`.
    unsafe { AXValueGetValue(value.0, value_type, out) }.then_some(())
}

fn copy_bounds_for_range(element: &CfObject, range: CfRange) -> Option<ScreenRect> {
    // SAFETY: AXValueCreate copies the CFRange and returns a retained object.
    let parameter = unsafe { AXValueCreate(AX_VALUE_CF_RANGE, (&range as *const CfRange).cast()) };
    if parameter.is_null() {
        return None;
    }
    let parameter = CfObject(parameter);
    copy_parameterized_rect(element, c"AXBoundsForRange", &parameter)
}

fn copy_parameterized_rect(
    element: &CfObject,
    name: &CStr,
    parameter: &CfObject,
) -> Option<ScreenRect> {
    let attribute = CfObject(cf_string_literal(name)?);
    let mut value = ptr::null();
    let status = unsafe {
        AXUIElementCopyParameterizedAttributeValue(element.0, attribute.0, parameter.0, &mut value)
    };
    if status != 0 || value.is_null() {
        return None;
    }
    let value = CfObject(value);
    let mut rect = [0.0_f64; 4];
    ax_value_into(&value, AX_VALUE_CG_RECT, rect.as_mut_ptr().cast())?;
    Some(ScreenRect {
        x: rect[0],
        y: rect[1],
        width: rect[2],
        height: rect[3],
    })
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
        assert_eq!(caret_target(i32::MAX), None);
        assert_eq!(pasted_words(i32::MAX, "so the plan", 14), None);
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> ScreenRect {
        ScreenRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn only_a_believable_caret_inside_the_window_counts() {
        let window = rect(100.0, 100.0, 800.0, 600.0);
        assert!(plausible_caret(rect(284.4, 366.0, 0.0, 14.0), window));
        assert!(plausible_caret(rect(300.0, 400.0, 2.0, 22.0), window));
        // Chromium's unanswered query on a 1440-point-tall primary display.
        assert!(!plausible_caret(rect(0.0, 1440.0, 0.0, 0.0), window));
        assert!(!plausible_caret(rect(0.0, 0.0, 0.0, 0.0), window));
        // A selection's rect, or a whole field, is no caret.
        assert!(!plausible_caret(rect(200.0, 200.0, 300.0, 18.0), window));
        // Elsewhere on screen.
        assert!(!plausible_caret(rect(1200.0, 300.0, 0.0, 14.0), window));
        assert!(!plausible_caret(rect(f64::NAN, 300.0, 0.0, 14.0), window));
    }

    #[test]
    fn words_are_counted_in_utf16_units() {
        let words: Vec<_> = utf16_words(" so 👋 the  plan\n").collect();
        assert_eq!(words, vec![(1, 2), (4, 2), (7, 3), (12, 4)]);
        assert_eq!(utf16_words("   ").count(), 0);
        assert_eq!(utf16_words("one").collect::<Vec<_>>(), vec![(0, 3)]);
    }
}
