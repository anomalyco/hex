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

fn capture_accessibility() -> Result<String> {
    let focused = focused_element()?;
    let result = selected_text(focused);
    unsafe { CFRelease(focused) };
    result
}

fn focused_element() -> Result<AxUiElementRef> {
    // SAFETY: The create/copy APIs return retained Core Foundation objects.
    let system = unsafe { AXUIElementCreateSystemWide() };
    if system.is_null() {
        return Err(eyre!("could not inspect the focused control"));
    }
    if unsafe { AXUIElementSetMessagingTimeout(system, 0.25) } != 0 {
        unsafe { CFRelease(system) };
        return Err(eyre!(
            "could not bound communication with the Accessibility server"
        ));
    }
    let focused_attribute = cf_string_literal(c"AXFocusedUIElement")?;
    let mut focused = ptr::null();
    let focused_status =
        unsafe { AXUIElementCopyAttributeValue(system, focused_attribute, &mut focused) };
    unsafe { CFRelease(focused_attribute) };
    unsafe { CFRelease(system) };
    if focused_status != 0 || focused.is_null() {
        return Err(eyre!(
            "the foreground application has no focused text control"
        ));
    }
    let timeout_status = unsafe { AXUIElementSetMessagingTimeout(focused.cast(), 0.25) };
    if timeout_status != 0 {
        unsafe { CFRelease(focused) };
        return Err(eyre!(
            "could not bound communication with the focused text control"
        ));
    }

    Ok(focused.cast())
}

fn selected_text(focused: AxUiElementRef) -> Result<String> {
    let selected_attribute = cf_string_literal(c"AXSelectedText")?;
    let mut selected = ptr::null();
    let selected_status =
        unsafe { AXUIElementCopyAttributeValue(focused, selected_attribute, &mut selected) };
    unsafe { CFRelease(selected_attribute) };
    if selected_status != 0 || selected.is_null() {
        return Err(eyre!("the focused control does not expose selected text"));
    }
    let result = cf_string(selected);
    unsafe { CFRelease(selected) };
    result
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
    String::from_utf8(bytes[..length].to_vec()).map_err(Into::into)
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
