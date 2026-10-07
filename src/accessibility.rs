use std::ffi::{c_char, c_void};
use std::ptr;

use color_eyre::eyre::{Result, eyre};

type CfTypeRef = *const c_void;
type CfStringRef = *const c_void;
type AxUiElementRef = *const c_void;

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const MAX_SELECTED_TEXT_BYTES: usize = 64 * 1024;

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
    fn CFRelease(value: CfTypeRef);
}

pub fn capture_optional() -> Option<String> {
    capture_accessibility()
        .ok()
        .filter(|text| !text.is_empty() && text.len() <= MAX_SELECTED_TEXT_BYTES)
}

pub fn focused_window_title(pid: i32) -> Option<String> {
    capture_focused_window_title(pid)
        .ok()
        .filter(|title| !title.is_empty())
}

struct CfObject(CfTypeRef);

impl Drop for CfObject {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

fn bounded_element(
    element: AxUiElementRef,
    null_error: &'static str,
    timeout_error: &'static str,
) -> Result<CfObject> {
    if element.is_null() {
        return Err(eyre!(null_error));
    }
    let element = CfObject(element);
    if unsafe { AXUIElementSetMessagingTimeout(element.0, 0.25) } != 0 {
        return Err(eyre!(timeout_error));
    }
    Ok(element)
}

fn copy_attribute(
    element: &CfObject,
    name: &std::ffi::CStr,
    missing_error: &'static str,
) -> Result<CfObject> {
    let attribute = CfObject(cf_string_literal(name)?);
    let mut value = ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element.0, attribute.0, &mut value) };
    if status != 0 || value.is_null() {
        return Err(eyre!(missing_error));
    }
    Ok(CfObject(value))
}

fn capture_focused_window_title(pid: i32) -> Result<String> {
    // SAFETY: The create/copy APIs return retained Core Foundation objects.
    let application = bounded_element(
        unsafe { AXUIElementCreateApplication(pid) },
        "could not inspect the foreground application",
        "could not bound communication with the foreground application",
    )?;
    let window = copy_attribute(
        &application,
        c"AXFocusedWindow",
        "the foreground application has no focused window",
    )?;
    if unsafe { AXUIElementSetMessagingTimeout(window.0, 0.25) } != 0 {
        return Err(eyre!(
            "could not bound communication with the foreground window"
        ));
    }
    let title = copy_attribute(
        &window,
        c"AXTitle",
        "the foreground window does not expose a title",
    )?;
    cf_string(title.0)
}

fn capture_accessibility() -> Result<String> {
    let focused = focused_element()?;
    let selected = copy_attribute(
        &focused,
        c"AXSelectedText",
        "the focused control does not expose selected text",
    )?;
    cf_string(selected.0)
}

fn focused_element() -> Result<CfObject> {
    // SAFETY: The create/copy APIs return retained Core Foundation objects.
    let system = bounded_element(
        unsafe { AXUIElementCreateSystemWide() },
        "could not inspect the focused control",
        "could not bound communication with the Accessibility server",
    )?;
    let focused = copy_attribute(
        &system,
        c"AXFocusedUIElement",
        "the foreground application has no focused text control",
    )?;
    if unsafe { AXUIElementSetMessagingTimeout(focused.0, 0.25) } != 0 {
        return Err(eyre!(
            "could not bound communication with the focused text control"
        ));
    }
    Ok(focused)
}

fn cf_string_literal(value: &std::ffi::CStr) -> Result<CfStringRef> {
    let string =
        unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), CF_STRING_ENCODING_UTF8) };
    (!string.is_null())
        .then_some(string)
        .ok_or_else(|| eyre!("could not create an Accessibility attribute name"))
}

fn cf_string(value: CfTypeRef) -> Result<String> {
    if unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
        return Err(eyre!("the selected text has an unsupported value type"));
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
        return Err(eyre!("could not decode the selected text"));
    }
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    bytes.truncate(length);
    String::from_utf8(bytes).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a trusted signed process and a focused selected text fixture"]
    fn captures_the_focused_accessibility_selection() {
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert_eq!(
            capture_optional().as_deref(),
            Some("HEX selected text fixture")
        );
    }
}
