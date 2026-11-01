//! macOS' proxy for a URL: CFNetwork applies the network's proxy settings
//! and their exceptions list (`CFNetworkCopyProxiesForURL`). A PAC file
//! (automatic proxy configuration) isn't evaluated, so such networks need
//! `HTTPS_PROXY`.

use std::ffi::{c_char, c_void};

type CFTypeRef = *const c_void;

const UTF8: u32 = 0x0800_0100;
const SINT32: isize = 3;

#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CFNetwork", kind = "framework")]
unsafe extern "C" {
    fn CFNetworkCopySystemProxySettings() -> CFTypeRef;
    fn CFNetworkCopyProxiesForURL(url: CFTypeRef, settings: CFTypeRef) -> CFTypeRef;
    fn CFURLCreateWithBytes(
        allocator: CFTypeRef,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        base: CFTypeRef,
    ) -> CFTypeRef;
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
    fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFEqual(left: CFTypeRef, right: CFTypeRef) -> u8;
    fn CFStringGetCString(string: CFTypeRef, buffer: *mut c_char, size: isize, encoding: u32)
    -> u8;
    fn CFNumberGetValue(number: CFTypeRef, kind: isize, value: *mut c_void) -> u8;
    fn CFRelease(object: CFTypeRef);

    static kCFProxyTypeKey: CFTypeRef;
    static kCFProxyHostNameKey: CFTypeRef;
    static kCFProxyPortNumberKey: CFTypeRef;
    static kCFProxyTypeHTTP: CFTypeRef;
    static kCFProxyTypeHTTPS: CFTypeRef;
}

/// Releases a Core Foundation object when dropped.
struct Owned(CFTypeRef);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a +1 reference from a Copy or Create function.
            unsafe { CFRelease(self.0) };
        }
    }
}

pub(super) fn proxy(url: &str) -> Option<String> {
    // SAFETY: plain Core Foundation calls; every Copy/Create result is owned
    // by an `Owned` and released, and Get results are used only while their
    // container lives.
    unsafe {
        let settings = Owned(CFNetworkCopySystemProxySettings());
        if settings.0.is_null() {
            return None;
        }
        let cf_url = Owned(CFURLCreateWithBytes(
            std::ptr::null(),
            url.as_ptr(),
            url.len() as isize,
            UTF8,
            std::ptr::null(),
        ));
        if cf_url.0.is_null() {
            return None;
        }
        let proxies = Owned(CFNetworkCopyProxiesForURL(cf_url.0, settings.0));
        if proxies.0.is_null() {
            return None;
        }
        // The first entry is the one to use; `None` means connect directly.
        if CFArrayGetCount(proxies.0) < 1 {
            return None;
        }
        let entry = CFArrayGetValueAtIndex(proxies.0, 0);
        let kind = CFDictionaryGetValue(entry, kCFProxyTypeKey);
        if kind.is_null()
            || (CFEqual(kind, kCFProxyTypeHTTPS) == 0 && CFEqual(kind, kCFProxyTypeHTTP) == 0)
        {
            return None;
        }
        let host = string(CFDictionaryGetValue(entry, kCFProxyHostNameKey))?;
        let number = CFDictionaryGetValue(entry, kCFProxyPortNumberKey);
        let mut port: i32 = 0;
        if number.is_null() || CFNumberGetValue(number, SINT32, (&raw mut port).cast()) == 0 {
            return None;
        }
        Some(format!("{host}:{port}"))
    }
}

/// A CFString's text.
unsafe fn string(value: CFTypeRef) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let mut buffer = [0 as c_char; 512];
    // SAFETY: `value` is a CFString from the proxy dictionary; the buffer is
    // writable for its length.
    let ok = unsafe { CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) };
    if ok == 0 {
        return None;
    }
    // SAFETY: CFStringGetCString NUL-terminates on success.
    let text = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
    Some(text.to_string_lossy().into_owned()).filter(|text| !text.is_empty())
}
