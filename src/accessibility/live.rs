//! The live tree behind [`super::tree`]: one app's windows as the walk's
//! elements. Every element costs two messages to its app, one for its
//! attributes and children together and one for its value, each under the
//! 0.25 s messaging timeout. It only reads; the one write is
//! [`request_web_accessibility`], which a caller makes only when asked to.

use std::cell::Cell;
use std::ffi::{CStr, c_void};
use std::ptr;

use super::tree::{Element, Node, Read};
use super::{
    AXUIElementSetAttributeValue, AXUIElementSetMessagingTimeout, AxUiElementRef,
    CF_NUMBER_SINT64_TYPE, CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFEqual,
    CFGetTypeID, CFNumberGetTypeID, CFNumberGetValue, CFRetain, CFStringGetTypeID, CfObject,
    CfTypeRef, Keep, MESSAGING_TIMEOUT_SECS, cf_string_literal, cf_string_lossy, copy_attribute,
    copy_element, copy_elements, kCFBooleanTrue,
};

/// The longest title, description, value or DOM id kept, in UTF-16 units.
/// A longer one keeps its head.
const MAX_STRING_UNITS: usize = 512;
/// A value longer than this many characters is not asked for at all, so a
/// document or a terminal's scrollback never crosses into Hex.
const MAX_VALUE_CHARS: i64 = 4 * 1024;
/// The most DOM classes kept for one element.
const MAX_CLASSES: isize = 64;

/// `kCFNumberFloat64Type`.
const CF_NUMBER_FLOAT64_TYPE: isize = 6;

/// What one attribute message asks for, in this order.
const ATTRIBUTES: [&CStr; 8] = [
    c"AXRole",
    c"AXSubrole",
    c"AXTitle",
    c"AXDescription",
    c"AXDOMIdentifier",
    c"AXDOMClassList",
    c"AXNumberOfCharacters",
    c"AXChildren",
];

#[repr(C)]
struct CfArrayCallBacks {
    _opaque: [u8; 0],
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCopyMultipleAttributeValues(
        element: AxUiElementRef,
        attributes: CfTypeRef,
        options: u32,
        values: *mut CfTypeRef,
    ) -> i32;
    safe fn AXIsProcessTrusted() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: CfArrayCallBacks;
    fn CFArrayCreate(
        allocator: *const c_void,
        values: *const CfTypeRef,
        count: isize,
        callbacks: *const CfArrayCallBacks,
    ) -> CfTypeRef;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(value: CfTypeRef) -> u8;
    fn CFNumberIsFloatType(number: CfTypeRef) -> u8;
}

/// Whether macOS lets this process read other apps' trees.
pub(super) fn trusted() -> bool {
    AXIsProcessTrusted()
}

/// The names every element is asked for, built once per read.
pub(super) struct Attributes(CfObject);

impl Attributes {
    pub(super) fn new() -> Option<Self> {
        let names = ATTRIBUTES
            .iter()
            .map(|name| cf_string_literal(name).map(CfObject))
            .collect::<Option<Vec<_>>>()?;
        let pointers: Vec<CfTypeRef> = names.iter().map(|name| name.0).collect();
        // SAFETY: `pointers` holds `pointers.len()` live CFStrings, which the
        // array retains through the CFType callbacks before `names` drops.
        let array = unsafe {
            CFArrayCreate(
                ptr::null(),
                pointers.as_ptr(),
                pointers.len() as isize,
                &raw const kCFTypeArrayCallBacks,
            )
        };
        (!array.is_null()).then(|| Self(CfObject(array)))
    }
}

/// One live element.
pub(super) struct Live<'a> {
    element: CfObject,
    attributes: &'a Attributes,
    /// Set by [`Element::read`] when the element says its value is longer
    /// than [`MAX_VALUE_CHARS`].
    long_value: Cell<bool>,
}

impl<'a> Live<'a> {
    fn new(element: CfObject, attributes: &'a Attributes) -> Self {
        Self {
            element,
            attributes,
            long_value: Cell::new(false),
        }
    }
}

impl Element for Live<'_> {
    fn read(&self, room: usize) -> Option<Read<Self>> {
        let mut values = ptr::null();
        // SAFETY: a live element, a CFArray of CFString names, and an out
        // pointer that receives a retained CFArray. Option 0 keeps going
        // past an attribute the element lacks, leaving an error value in
        // its place.
        let status = unsafe {
            AXUIElementCopyMultipleAttributeValues(
                self.element.0,
                self.attributes.0.0,
                0,
                &mut values,
            )
        };
        if status != 0 || values.is_null() {
            return None;
        }
        let values = CfObject(values);
        // SAFETY: `values` is the CFArray the call returned.
        if unsafe { CFArrayGetCount(values.0) } != ATTRIBUTES.len() as isize {
            return None;
        }
        // SAFETY: the index is below the count checked above; the array owns
        // the item for as long as `values` lives.
        let at = |index: usize| unsafe { CFArrayGetValueAtIndex(values.0, index as isize) };
        self.long_value
            .set(number(at(6)).is_some_and(|characters| characters > MAX_VALUE_CHARS));
        let (children, dropped) = self.children(at(7), room);
        Some(Read {
            node: Node {
                role: text(at(0)),
                subrole: text(at(1)),
                title: text(at(2)),
                description: text(at(3)),
                value: None,
                dom_id: text(at(4)),
                dom_classes: classes(at(5)),
                children: Vec::new(),
            },
            children,
            dropped,
        })
    }

    fn value(&self) -> Option<String> {
        if self.long_value.get() {
            return None;
        }
        let value = copy_attribute(&self.element, c"AXValue")?;
        scalar(value.0)
    }
}

impl<'a> Live<'a> {
    /// Up to `room` of the element's children, each bounded by the messaging
    /// timeout, and how many were left.
    fn children(&self, array: CfTypeRef, room: usize) -> (Vec<Live<'a>>, usize) {
        if !is(array, unsafe { CFArrayGetTypeID() }) {
            return (Vec::new(), 0);
        }
        // SAFETY: `array` is a CFArray.
        let count = unsafe { CFArrayGetCount(array) }.max(0);
        let taken = count.min(isize::try_from(room).unwrap_or(isize::MAX));
        let children = (0..taken)
            .filter_map(|index| {
                // SAFETY: Get rule: the array owns the item, so it is
                // retained before the array goes.
                let item = unsafe { CFArrayGetValueAtIndex(array, index) };
                if item.is_null() {
                    return None;
                }
                let item = CfObject(unsafe { CFRetain(item) });
                (unsafe { AXUIElementSetMessagingTimeout(item.0, MESSAGING_TIMEOUT_SECS) } == 0)
                    .then(|| Live::new(item, self.attributes))
            })
            .collect();
        (children, (count - taken) as usize)
    }
}

/// Every window of an app: the focused and main ones first, then the rest.
/// A window the app lists is read whether or not it is in front.
pub(super) fn windows<'a>(application: &CfObject, attributes: &'a Attributes) -> Vec<Live<'a>> {
    let mut windows: Vec<CfObject> = Vec::new();
    let candidates = [
        copy_element(application, c"AXFocusedWindow"),
        copy_element(application, c"AXMainWindow"),
    ]
    .into_iter()
    .flatten()
    .chain(copy_elements(application, c"AXWindows"));
    for window in candidates {
        // SAFETY: both are live AXUIElements.
        if !windows
            .iter()
            .any(|known| unsafe { CFEqual(known.0, window.0) } != 0)
        {
            windows.push(window);
        }
    }
    windows
        .into_iter()
        .map(|window| Live::new(window, attributes))
        .collect()
}

/// Whether the app is the one in front, when it says.
pub(super) fn frontmost(application: &CfObject) -> Option<bool> {
    let value = copy_attribute(application, c"AXFrontmost")?;
    boolean(value.0)
}

/// Asks a Chromium or Electron app to build its web accessibility tree
/// (`AXManualAccessibility`). The app keeps it on until it quits. Returns
/// whether the app took the request.
pub(super) fn request_web_accessibility(application: &CfObject) -> bool {
    let Some(attribute) = cf_string_literal(c"AXManualAccessibility").map(CfObject) else {
        return false;
    };
    // SAFETY: a live application element, a CFString name, and the shared
    // kCFBooleanTrue constant.
    unsafe { AXUIElementSetAttributeValue(application.0, attribute.0, kCFBooleanTrue) == 0 }
}

/// Whether `value` is a Core Foundation object of type `kind`. An attribute
/// the element lacks comes back as an error value, which is none of the
/// types asked about here.
fn is(value: CfTypeRef, kind: usize) -> bool {
    // SAFETY: a non-null item of a CFArray of CFTypes.
    !value.is_null() && unsafe { CFGetTypeID(value) } == kind
}

/// A string's head, at most [`MAX_STRING_UNITS`]; `None` for anything else
/// or an empty one.
fn text(value: CfTypeRef) -> Option<String> {
    if !is(value, unsafe { CFStringGetTypeID() }) {
        return None;
    }
    cf_string_lossy(value, MAX_STRING_UNITS, Keep::Head).filter(|text| !text.is_empty())
}

fn number(value: CfTypeRef) -> Option<i64> {
    if !is(value, unsafe { CFNumberGetTypeID() }) {
        return None;
    }
    let mut number = 0_i64;
    // SAFETY: `value` is a CFNumber and `number` an SInt64 for the type asked.
    let read = unsafe { CFNumberGetValue(value, CF_NUMBER_SINT64_TYPE, (&raw mut number).cast()) };
    (read != 0).then_some(number)
}

fn boolean(value: CfTypeRef) -> Option<bool> {
    // SAFETY: `value` is a CFBoolean once its type is checked.
    is(value, unsafe { CFBooleanGetTypeID() }).then(|| unsafe { CFBooleanGetValue(value) } != 0)
}

/// A value as text: a string's head, a number, or a switch's `true` or
/// `false`. A toggle's state (a mute button, a checkbox) is a number or a
/// boolean, not a string.
fn scalar(value: CfTypeRef) -> Option<String> {
    if let Some(flag) = boolean(value) {
        return Some(flag.to_string());
    }
    if !is(value, unsafe { CFNumberGetTypeID() }) {
        return text(value);
    }
    // SAFETY: `value` is a CFNumber.
    if unsafe { CFNumberIsFloatType(value) } == 0 {
        return number(value).map(|number| number.to_string());
    }
    let mut float = 0_f64;
    // SAFETY: `value` is a CFNumber and `float` a Float64 for the type asked.
    let read = unsafe { CFNumberGetValue(value, CF_NUMBER_FLOAT64_TYPE, (&raw mut float).cast()) };
    (read != 0).then(|| float.to_string())
}

/// An element's DOM classes: the strings of `AXDOMClassList`.
fn classes(value: CfTypeRef) -> Vec<String> {
    if !is(value, unsafe { CFArrayGetTypeID() }) {
        return Vec::new();
    }
    // SAFETY: `value` is a CFArray; each item is borrowed while it lives.
    let count = unsafe { CFArrayGetCount(value) }.clamp(0, MAX_CLASSES);
    (0..count)
        .filter_map(|index| text(unsafe { CFArrayGetValueAtIndex(value, index) }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFNumberCreate(allocator: *const c_void, kind: isize, value: *const c_void)
        -> CfTypeRef;
        static kCFBooleanFalse: CfTypeRef;
        static kCFNull: CfTypeRef;
    }

    fn string(text: &str) -> CfObject {
        let text = std::ffi::CString::new(text).unwrap();
        CfObject(cf_string_literal(&text).unwrap())
    }

    fn integer(value: i64) -> CfObject {
        // SAFETY: an SInt64 for the type named; the number copies it.
        CfObject(unsafe {
            CFNumberCreate(
                ptr::null(),
                CF_NUMBER_SINT64_TYPE,
                (&raw const value).cast(),
            )
        })
    }

    fn array(items: &[&CfObject]) -> CfObject {
        let pointers: Vec<CfTypeRef> = items.iter().map(|item| item.0).collect();
        // SAFETY: live CFTypes, retained by the array.
        CfObject(unsafe {
            CFArrayCreate(
                ptr::null(),
                pointers.as_ptr(),
                pointers.len() as isize,
                &raw const kCFTypeArrayCallBacks,
            )
        })
    }

    #[test]
    fn a_value_reads_as_text_by_its_type() {
        let name = string("View Mira Okonkwo's profile");
        assert_eq!(text(name.0).as_deref(), Some("View Mira Okonkwo's profile"));
        assert_eq!(
            scalar(name.0).as_deref(),
            Some("View Mira Okonkwo's profile")
        );
        assert_eq!(text(string("").0), None);
        // What an element without the attribute leaves in its place.
        assert_eq!(text(ptr::null()), None);
        assert_eq!(text(unsafe { kCFNull }), None);
        assert_eq!(scalar(unsafe { kCFNull }), None);
        assert_eq!(text(unsafe { kCFBooleanTrue }), None);

        // A toggle's state: a number or a boolean.
        assert_eq!(scalar(integer(1).0).as_deref(), Some("1"));
        assert_eq!(number(integer(4_097).0), Some(4_097));
        assert_eq!(number(name.0), None);
        let half = 0.5_f64;
        // SAFETY: a Float64 for the type named.
        let half = CfObject(unsafe {
            CFNumberCreate(
                ptr::null(),
                CF_NUMBER_FLOAT64_TYPE,
                (&raw const half).cast(),
            )
        });
        assert_eq!(scalar(half.0).as_deref(), Some("0.5"));
        assert_eq!(scalar(unsafe { kCFBooleanTrue }).as_deref(), Some("true"));
        assert_eq!(scalar(unsafe { kCFBooleanFalse }).as_deref(), Some("false"));
        assert_eq!(boolean(integer(1).0), None);
    }

    #[test]
    fn a_long_string_keeps_its_head() {
        let long = string(&"é".repeat(MAX_STRING_UNITS + 90));
        assert_eq!(text(long.0), Some("é".repeat(MAX_STRING_UNITS)));
    }

    #[test]
    fn dom_classes_are_the_strings_of_the_list() {
        let (tile, speaking, count) = (
            string("p-huddle_peer_tile__overlay"),
            string("p-huddle_peer_tile__overlay--active_speaker"),
            integer(3),
        );
        assert_eq!(
            classes(array(&[&tile, &count, &speaking]).0),
            [
                "p-huddle_peer_tile__overlay",
                "p-huddle_peer_tile__overlay--active_speaker"
            ]
        );
        assert!(classes(tile.0).is_empty());
        assert!(classes(ptr::null()).is_empty());
        let many: Vec<&CfObject> = std::iter::repeat_n(&tile, 100).collect();
        assert_eq!(classes(array(&many).0).len(), MAX_CLASSES as usize);
    }

    #[test]
    fn one_message_asks_for_every_attribute() {
        let attributes = Attributes::new().unwrap();
        // SAFETY: the CFArray just built.
        assert_eq!(unsafe { CFArrayGetCount(attributes.0.0) }, 8);
    }

    /// The live calls against a process this test starts, which has no
    /// windows and answers no Accessibility message: each call fails, and
    /// a failure is nothing read, never a crash.
    #[test]
    fn a_process_that_answers_nothing_reads_as_nothing() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let application = super::super::foreign_application(child.id() as i32).unwrap();
        let attributes = Attributes::new().unwrap();
        let none = windows(&application, &attributes);
        let front = frontmost(&application);
        let element = Live::new(application, &attributes);
        let answer = element.read(10);
        let value = element.value();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(none.is_empty());
        assert_eq!(front, None);
        assert!(answer.is_none());
        assert_eq!(value, None);
    }
}
