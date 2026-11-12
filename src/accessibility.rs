use std::collections::VecDeque;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::time::{Duration, Instant};

type CfTypeRef = *const c_void;
type CfStringRef = *const c_void;
type AxUiElementRef = *const c_void;

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const MESSAGING_TIMEOUT_SECS: f32 = 0.25;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: *mut CfTypeRef,
    ) -> i32;
    fn AXUIElementSetMessagingTimeout(element: AxUiElementRef, timeout_in_seconds: f32) -> i32;
    fn AXUIElementSetAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: CfTypeRef,
    ) -> i32;
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

/// `kCFNumberSInt64Type`.
const CF_NUMBER_SINT64_TYPE: isize = 4;

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
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: CfTypeRef, kind: isize, value: *mut c_void) -> u8;
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
    fn CFRetain(value: CfTypeRef) -> CfTypeRef;
    fn CFEqual(left: CfTypeRef, right: CfTypeRef) -> u8;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetCount(array: CfTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CfTypeRef, index: isize) -> CfTypeRef;
    static kCFBooleanTrue: CfTypeRef;
}

pub fn focused_window_title(pid: i32) -> Option<String> {
    let application = foreign_application(pid)?;
    let window = copy_element(&application, c"AXFocusedWindow")?;
    copy_string(&window, c"AXTitle").filter(|title| !title.is_empty())
}

/// How much of a focused window Labs › Screen vocabulary reads: elements
/// walked, depth below the window, bytes collected, characters per value,
/// and wall-clock time.
const MAX_TEXT_NODES: usize = 1_500;
const MAX_TEXT_DEPTH: usize = 40;
const MAX_TEXT_BYTES: usize = 32 * 1024;
const MAX_VALUE_CHARS: usize = 4 * 1024;
const TEXT_DEADLINE: Duration = Duration::from_millis(150);

/// The visible text of another process's focused window, for Labs › Screen
/// vocabulary: each element's visible value, title, and description,
/// breadth-first, bounded by elements, depth, bytes, and per-value size. The
/// 150 ms deadline is soft, checked between elements. A text value over 4 KB
/// contributes only its `AXVisibleCharacterRange`, and is skipped when the app
/// names none, so a document or a terminal's scrollback never fills the
/// budget. Secure text fields and their subtrees are skipped, so a password
/// never leaves the app. `None` for Hex's own window or when the tree has no
/// text.
pub fn focused_window_text(pid: i32) -> Option<String> {
    let application = foreign_application(pid)?;
    let window = copy_element(&application, c"AXFocusedWindow")?;
    let started = Instant::now();
    let mut text = String::new();
    let mut queue = VecDeque::from([(window, 0_usize)]);
    let mut visited = 0_usize;
    while let Some((element, depth)) = queue.pop_front() {
        if visited >= MAX_TEXT_NODES
            || started.elapsed() >= TEXT_DEADLINE
            || text.len() >= MAX_TEXT_BYTES
        {
            break;
        }
        visited += 1;
        if is_secure_text_field(&element) {
            continue;
        }
        let mut full = false;
        if let Some(value) = element_value(&element) {
            full = !append_text(&mut text, &value);
        }
        if !full {
            for name in [c"AXTitle", c"AXDescription"] {
                let Some(value) = copy_string_capped(&element, name, MAX_VALUE_CHARS) else {
                    continue;
                };
                if !append_text(&mut text, &value) {
                    full = true;
                    break;
                }
            }
        }
        if full {
            break;
        }
        if depth >= MAX_TEXT_DEPTH || visited + queue.len() >= MAX_TEXT_NODES {
            continue;
        }
        let room = MAX_TEXT_NODES - visited - queue.len();
        for child in copy_elements_up_to(&element, c"AXChildren", room) {
            queue.push_back((child, depth + 1));
        }
    }
    (!text.trim().is_empty()).then_some(text)
}

/// Whether an element holds a password. macOS marks most secure fields with
/// the `AXSecureTextField` subrole, and some apps use it as the role.
fn is_secure_text_field(element: &CfObject) -> bool {
    [c"AXRole", c"AXSubrole"]
        .into_iter()
        .any(|name| copy_string(element, name).as_deref() == Some("AXSecureTextField"))
}

/// An element's `AXValue`. A text area can expose its whole document or a
/// terminal's scrollback there, so a value over [`MAX_VALUE_CHARS`] reads only
/// the visible character range, and is skipped when the app names none.
fn element_value(element: &CfObject) -> Option<String> {
    if let Some(characters) = copy_number(element, c"AXNumberOfCharacters")
        && characters > MAX_VALUE_CHARS as i64
    {
        let range = copy_range(element, c"AXVisibleCharacterRange")?;
        let length = range.length.clamp(0, MAX_VALUE_CHARS as isize);
        if range.location < 0 || length == 0 {
            return None;
        }
        return copy_string_for_range(
            element,
            CfRange {
                location: range.location,
                length,
            },
        );
    }
    copy_string_capped(element, c"AXValue", MAX_VALUE_CHARS)
}

/// Appends one element's text within the total byte budget. Returns whether
/// there is room for more after it.
fn append_text(text: &mut String, value: &str) -> bool {
    let room = MAX_TEXT_BYTES.saturating_sub(text.len() + 1);
    if room == 0 {
        return false;
    }
    let end = char_boundary(value, value.len().min(room));
    text.push_str(&value[..end]);
    text.push('\n');
    end == value.len() && text.len() < MAX_TEXT_BYTES
}

/// An integer attribute such as `AXNumberOfCharacters`.
fn copy_number(element: &CfObject, name: &CStr) -> Option<i64> {
    let value = copy_attribute(element, name)?;
    if unsafe { CFGetTypeID(value.0) } != unsafe { CFNumberGetTypeID() } {
        return None;
    }
    let mut number = 0_i64;
    // SAFETY: `value` is a CFNumber, and `number` is an SInt64 for the type
    // asked.
    let read = unsafe {
        CFNumberGetValue(
            value.0,
            CF_NUMBER_SINT64_TYPE,
            (&mut number as *mut i64).cast(),
        )
    };
    (read != 0).then_some(number)
}

/// A string attribute no longer than `max_chars`, checked before converting
/// it so a large document is never copied.
fn copy_string_capped(element: &CfObject, name: &CStr, max_chars: usize) -> Option<String> {
    let value = copy_attribute(element, name)?;
    if unsafe { CFGetTypeID(value.0) } != unsafe { CFStringGetTypeID() } {
        return None;
    }
    let length = unsafe { CFStringGetLength(value.0) };
    if length < 0 || length as usize > max_chars {
        return None;
    }
    cf_string(value.0)
}

/// The largest boundary of `text` at or below `at`.
fn char_boundary(text: &str, mut at: usize) -> usize {
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The page address of the web area holding the application's focused
/// element, for browsers without a scripting dictionary (Firefox, Zen).
/// The outermost web area wins, so focus inside an iframe still reports
/// the page itself.
pub fn focused_web_url(pid: i32) -> Option<String> {
    const MAX_DEPTH: usize = 64;
    let application = foreign_application(pid)?;
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

/// Every page a browser's windows show, for a call's address or tab title
/// when its scripting dictionary can't be asked (#831). Chromium (Arc,
/// Chrome, Brave, Edge) builds its web accessibility tree only for an
/// assistive app that asks, which `AXManualAccessibility` does, once per
/// process; without it a browser behind another app shows windows but no
/// web areas.
pub fn browser_pages(pid: i32) -> crate::dictation::browser_page::BrowserPages {
    let Some(application) = foreign_application(pid) else {
        return Default::default();
    };
    request_web_accessibility(pid, &application);
    let mut windows: Vec<CfObject> = Vec::new();
    let candidates = [
        copy_element(&application, c"AXFocusedWindow"),
        copy_element(&application, c"AXMainWindow"),
    ]
    .into_iter()
    .flatten()
    .chain(copy_elements(&application, c"AXWindows"));
    for window in candidates {
        // SAFETY: Both are live AXUIElements.
        if !windows
            .iter()
            .any(|known| unsafe { CFEqual(known.0, window.0) } != 0)
        {
            windows.push(window);
        }
    }
    let mut pages = crate::dictation::browser_page::pages(windows.into_iter().map(Element));
    // The focused web area, when there is one, is the likeliest page.
    if let Some(url) = focused_web_url(pid) {
        pages.urls.retain(|known| known != &url);
        pages.urls.insert(0, url);
    }
    pages
}

/// Asks a Chromium browser for its web accessibility tree, once per
/// process. Other apps ignore the attribute.
fn request_web_accessibility(pid: i32, application: &CfObject) {
    static ASKED: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());
    let mut asked = ASKED.lock().unwrap_or_else(|error| error.into_inner());
    if asked.contains(&pid) {
        return;
    }
    asked.push(pid);
    let Some(attribute) = cf_string_literal(c"AXManualAccessibility").map(CfObject) else {
        return;
    };
    // SAFETY: A live application element, a CFString name, and the shared
    // kCFBooleanTrue constant.
    let status =
        unsafe { AXUIElementSetAttributeValue(application.0, attribute.0, kCFBooleanTrue) };
    if status != 0 {
        tracing::debug!(
            status,
            pid,
            "the browser has no manual accessibility switch"
        );
    }
}

/// An element as `browser_page` walks it.
struct Element(CfObject);

impl crate::dictation::browser_page::AxNode for Element {
    fn role(&self) -> Option<String> {
        copy_string(&self.0, c"AXRole")
    }

    fn title(&self) -> Option<String> {
        copy_string(&self.0, c"AXTitle")
    }

    fn url(&self) -> Option<String> {
        copy_url(&self.0, c"AXURL")
    }

    fn children(&self) -> Vec<Self> {
        copy_elements(&self.0, c"AXChildren")
            .into_iter()
            .map(Element)
            .collect()
    }
}

/// An attribute holding elements (`AXWindows`, `AXChildren`), each bounded.
fn copy_elements(element: &CfObject, name: &CStr) -> Vec<CfObject> {
    copy_elements_up_to(element, name, usize::MAX)
}

/// The first `limit` elements of an attribute, each bounded like
/// [`copy_elements`], so a window with a huge child array cannot cost more
/// than the caller's node budget.
fn copy_elements_up_to(element: &CfObject, name: &CStr, limit: usize) -> Vec<CfObject> {
    let Some(array) = copy_attribute(element, name) else {
        return Vec::new();
    };
    if unsafe { CFGetTypeID(array.0) } != unsafe { CFArrayGetTypeID() } {
        return Vec::new();
    }
    let limit = isize::try_from(limit).unwrap_or(isize::MAX);
    let count = unsafe { CFArrayGetCount(array.0) }.min(limit);
    (0..count)
        .filter_map(|index| {
            // SAFETY: Get rule: the array owns the item, so it is retained
            // before the array goes.
            let item = unsafe { CFArrayGetValueAtIndex(array.0, index) };
            if item.is_null() {
                return None;
            }
            let item = CfObject(unsafe { CFRetain(item) });
            (unsafe { AXUIElementSetMessagingTimeout(item.0, MESSAGING_TIMEOUT_SECS) } == 0)
                .then_some(item)
        })
        .collect()
}

/// The character just before the insertion point of the application's
/// focused text element, for Labs › Smart spacing. `None` when text is
/// selected, the cursor is at the start, or the app doesn't say.
pub fn char_before_caret(pid: i32) -> Option<char> {
    let application = foreign_application(pid)?;
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

/// The Accessibility application element for another process, never for
/// Hex's own: its window serves that tree from the same runtime its main
/// thread updates, so a read from the foreground-context thread (or Smart
/// spacing) while an update runs can panic and take Hex down (#1402).
fn foreign_application(pid: i32) -> Option<CfObject> {
    if pid == std::process::id() as i32 {
        return None;
    }
    // SAFETY: AXUIElementCreateApplication returns a retained Core Foundation object.
    bounded_element(unsafe { AXUIElementCreateApplication(pid) })
}

fn bounded_element(element: AxUiElementRef) -> Option<CfObject> {
    if element.is_null() {
        return None;
    }
    let element = CfObject(element);
    (unsafe { AXUIElementSetMessagingTimeout(element.0, MESSAGING_TIMEOUT_SECS) } == 0)
        .then_some(element)
}

fn copy_attribute(element: &CfObject, name: &CStr) -> Option<CfObject> {
    let attribute = CfObject(cf_string_literal(name)?);
    let mut value = ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element.0, attribute.0, &mut value) };
    // `then`, not `then_some`: dropping a `CfObject(null)` would CFRelease(NULL).
    (status == 0 && !value.is_null()).then(|| CfObject(value))
}

fn copy_element(element: &CfObject, name: &CStr) -> Option<CfObject> {
    let child = copy_attribute(element, name)?;
    (unsafe { AXUIElementSetMessagingTimeout(child.0, MESSAGING_TIMEOUT_SECS) } == 0)
        .then_some(child)
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
        assert_eq!(focused_window_text(i32::MAX), None);
        assert_eq!(focused_web_url(i32::MAX), None);
        assert_eq!(char_before_caret(i32::MAX), None);
    }

    /// Hex's own tree belongs to the main thread that updates it: the
    /// foreground-context poller and Smart spacing must never ask for it
    /// (#1402).
    #[test]
    fn hex_never_reads_its_own_accessibility_tree() {
        let own_pid = std::process::id() as i32;
        assert!(foreign_application(own_pid).is_none());
        assert_eq!(focused_window_title(own_pid), None);
        assert_eq!(focused_window_text(own_pid), None);
        assert_eq!(focused_web_url(own_pid), None);
        assert_eq!(char_before_caret(own_pid), None);
        assert!(browser_pages(own_pid).urls.is_empty());
    }
}
