//! macOS peer identity: the audit token `LOCAL_PEERTOKEN` reports for the
//! socket peer, and the signing identifier and team `Security.framework`
//! resolves for that token.

use std::ffi::{CStr, c_char, c_void};
use std::os::fd::RawFd;

use super::{Identity, PeerId};
use crate::local_api::consent::Verification;

type CFTypeRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFStringRef = *const c_void;
type CFDataRef = *const c_void;
type SecCodeRef = *mut c_void;
type SecStaticCodeRef = *const c_void;

const SOL_LOCAL: libc::c_int = 0;
/// `LOCAL_PEERTOKEN` (Darwin's `<sys/un.h>`): the peer's audit token.
const LOCAL_PEERTOKEN: libc::c_int = 0x006;
const SEC_CS_SIGNING_INFORMATION: u32 = 0x2;
const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    static kSecGuestAttributeAudit: CFStringRef;
    static kSecCodeInfoUnique: CFStringRef;
    fn SecCodeCheckValidity(code: SecCodeRef, flags: u32, requirement: CFTypeRef) -> i32;
    static kSecCodeInfoIdentifier: CFStringRef;
    static kSecCodeInfoTeamIdentifier: CFStringRef;
    fn SecCodeCopyGuestWithAttributes(
        host: CFTypeRef,
        attributes: CFDictionaryRef,
        flags: u32,
        code: *mut SecCodeRef,
    ) -> i32;
    fn SecCodeCopyStaticCode(code: SecCodeRef, flags: u32, out: *mut SecStaticCodeRef) -> i32;
    fn SecCodeCopySigningInformation(
        code: SecStaticCodeRef,
        flags: u32,
        out: *mut CFDictionaryRef,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    fn CFDataGetLength(data: CFDataRef) -> isize;
    fn CFDataCreate(alloc: CFTypeRef, bytes: *const u8, len: isize) -> CFDataRef;
    fn CFDictionaryCreate(
        alloc: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFDictionaryRef;
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        size: isize,
        encoding: u32,
    ) -> bool;
    fn CFRelease(value: CFTypeRef);
}

/// The peer behind `fd`: its audit token and the signing identity the
/// system reports for that token. `None` when the token or identity can't be
/// read, in which case the native flow refuses rather than guessing.
pub(super) fn identify(fd: RawFd) -> Option<(PeerId, Identity)> {
    let token = peer_audit_token(fd)?;
    let peer = PeerId([
        u32::from_ne_bytes(token[0..4].try_into().ok()?),
        u32::from_ne_bytes(token[4..8].try_into().ok()?),
        u32::from_ne_bytes(token[8..12].try_into().ok()?),
        u32::from_ne_bytes(token[12..16].try_into().ok()?),
        u32::from_ne_bytes(token[16..20].try_into().ok()?),
        u32::from_ne_bytes(token[20..24].try_into().ok()?),
        u32::from_ne_bytes(token[24..28].try_into().ok()?),
        u32::from_ne_bytes(token[28..32].try_into().ok()?),
    ]);
    let identity = signing_identity(&token)?;
    Some((peer, identity))
}

fn peer_audit_token(fd: RawFd) -> Option<[u8; 32]> {
    let mut token = [0_u8; 32];
    let mut length = token.len() as libc::socklen_t;
    // SAFETY: `fd` is the accepted socket for this call, and `token`/`length`
    // describe a live buffer and its size.
    let result = unsafe {
        libc::getsockopt(
            fd,
            SOL_LOCAL,
            LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut length,
        )
    };
    (result == 0 && length as usize == token.len()).then_some(token)
}

fn signing_identity(token: &[u8; 32]) -> Option<Identity> {
    // SAFETY: every CoreFoundation/Security value is created and released
    // within this call, and every pointer is null-checked before use.
    unsafe {
        let data = CFDataCreate(std::ptr::null(), token.as_ptr(), token.len() as isize);
        if data.is_null() {
            return None;
        }
        let keys = [kSecGuestAttributeAudit];
        let values = [data];
        let attributes = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            std::ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
            std::ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
        );
        if attributes.is_null() {
            CFRelease(data);
            return None;
        }
        let mut code: SecCodeRef = std::ptr::null_mut();
        let status = SecCodeCopyGuestWithAttributes(std::ptr::null(), attributes, 0, &mut code);
        let identity = if status == 0 && !code.is_null() {
            let identity = signing_identity_of(code);
            CFRelease(code);
            identity
        } else {
            None
        };
        CFRelease(attributes);
        CFRelease(data);
        identity
    }
}

unsafe fn signing_identity_of(code: SecCodeRef) -> Option<Identity> {
    unsafe {
        if SecCodeCheckValidity(code, 0, std::ptr::null()) != 0 {
            return None;
        }
        let mut stat: SecStaticCodeRef = std::ptr::null();
        if SecCodeCopyStaticCode(code, 0, &mut stat) != 0 || stat.is_null() {
            return None;
        }
        let mut info: CFDictionaryRef = std::ptr::null();
        let status = SecCodeCopySigningInformation(stat, SEC_CS_SIGNING_INFORMATION, &mut info);
        let identity = if status == 0 && !info.is_null() {
            let identifier = cf_string(CFDictionaryGetValue(info, kSecCodeInfoIdentifier));
            let team = cf_string(CFDictionaryGetValue(info, kSecCodeInfoTeamIdentifier));
            let digest = CFDictionaryGetValue(info, kSecCodeInfoUnique);
            identifier.and_then(|identifier| {
                if digest.is_null() {
                    return None;
                }
                let len = CFDataGetLength(digest);
                if !(20..=64).contains(&len) {
                    return None;
                }
                let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(digest), len as usize);
                let cdhash = bytes.iter().map(|b| format!("{b:02x}")).collect();
                Some(Identity {
                    identifier,
                    cdhash,
                    verification: Verification::MacSignature { team },
                    name: None,
                    icon: None,
                })
            })
        } else {
            None
        };
        if !info.is_null() {
            CFRelease(info);
        }
        CFRelease(stat);
        identity
    }
}

unsafe fn cf_string(value: CFTypeRef) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let mut buffer = [0 as c_char; 1024];
    // SAFETY: `value` is a CFString from the signing-information dictionary,
    // and `buffer` is writable for its stated length.
    let copied = unsafe {
        CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            CF_STRING_ENCODING_UTF8,
        )
    };
    copied.then(|| {
        // SAFETY: a successful `CFStringGetCString` wrote a NUL-terminated
        // UTF-8 string into `buffer`.
        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    })
}
