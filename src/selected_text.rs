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
    fn AXUIElementSetAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: CfTypeRef,
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
    fn CFStringCreateWithBytes(
        allocator: *const c_void,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        is_external_representation: u8,
    ) -> CfStringRef;
    fn CFRelease(value: CfTypeRef);
}

pub fn capture() -> Result<String> {
    let text = capture_accessibility()?;
    if text.is_empty() {
        return Err(eyre!("select some text before using voice edit"));
    }
    if text.len() > MAX_SELECTED_TEXT_BYTES {
        return Err(eyre!("the selected text is too large to edit safely"));
    }
    Ok(text)
}

pub fn replace(
    expected: &str,
    replacement: &str,
    destination_is_current: impl FnOnce() -> bool,
) -> Result<()> {
    let focused = focused_element()?;
    let selected = match selected_text(focused) {
        Ok(selected) if selected == expected => selected,
        Ok(_) => {
            unsafe { CFRelease(focused) };
            return Err(eyre!(
                "the selected text changed before voice edit completed"
            ));
        }
        Err(error) => {
            unsafe { CFRelease(focused) };
            return Err(error);
        }
    };
    drop(selected);
    let selected_attribute = match cf_string_literal(c"AXSelectedText") {
        Ok(attribute) => attribute,
        Err(error) => {
            unsafe { CFRelease(focused) };
            return Err(error);
        }
    };
    let replacement_value = match cf_string_from_str(replacement) {
        Ok(value) => value,
        Err(error) => {
            unsafe {
                CFRelease(selected_attribute);
                CFRelease(focused);
            }
            return Err(error);
        }
    };
    let status = if !destination_is_current() {
        -1
    } else {
        unsafe {
            AXUIElementSetAttributeValue(focused, selected_attribute, replacement_value.cast())
        }
    };
    unsafe {
        CFRelease(replacement_value);
        CFRelease(selected_attribute);
        CFRelease(focused);
    }
    match status {
        0 => Ok(()),
        -1 => Err(eyre!("input changed before voice edit completed")),
        _ => Err(eyre!(
            "the focused control refused the selected text replacement"
        )),
    }
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

fn cf_string_from_str(value: &str) -> Result<CfStringRef> {
    let string = unsafe {
        CFStringCreateWithBytes(
            ptr::null(),
            value.as_ptr(),
            value.len() as isize,
            CF_STRING_ENCODING_UTF8,
            0,
        )
    };
    (!string.is_null())
        .then_some(string)
        .ok_or_else(|| eyre!("could not encode the selected text replacement"))
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
        assert_eq!(capture().unwrap(), "HEX selected text fixture");
    }
}
