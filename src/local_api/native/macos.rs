//! macOS peer identity: the audit token `LOCAL_PEERTOKEN` reports for the
//! socket peer (whose user must be Hex's), and the signing identifier and
//! team `Security.framework` resolves for that token.
//!
//! Only a signature that satisfies `anchor apple generic` (Developer ID,
//! the App Store, or Apple) counts as verified (`MacSignature`). Anything
//! else valid, an ad-hoc signature included, is named like an unsigned
//! program elsewhere: by its path (`Executable`), still bound to its
//! code-directory hash.

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
type SecRequirementRef = *mut c_void;
type CFURLRef = *const c_void;

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
    fn SecRequirementCreateWithString(
        text: CFStringRef,
        flags: u32,
        requirement: *mut SecRequirementRef,
    ) -> i32;
    fn SecCodeCopyPath(code: SecStaticCodeRef, flags: u32, path: *mut CFURLRef) -> i32;
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
    fn CFStringCreateWithCString(
        alloc: CFTypeRef,
        text: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFURLGetFileSystemRepresentation(
        url: CFURLRef,
        resolve_against_base: bool,
        buffer: *mut u8,
        length: isize,
    ) -> bool;
}

/// The peer behind `fd`: its audit token and the signing identity the
/// system reports for that token. `None` when the token or identity can't be
/// read, in which case the native flow refuses rather than guessing.
pub(super) fn identify(fd: RawFd) -> Option<(PeerId, Identity)> {
    if !same_user(fd) {
        tracing::debug!("a native client of another user was refused");
        return None;
    }
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

/// Whether the peer's effective user is Hex's, beyond the socket's mode.
fn same_user(fd: RawFd) -> bool {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: `fd` is the accepted socket; both out-pointers are live.
    let read = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } == 0;
    // SAFETY: geteuid has no preconditions.
    read && uid == unsafe { libc::geteuid() }
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
                let anchored = anchored(code);
                let path = if anchored {
                    None
                } else {
                    Some(code_path(stat)?)
                };
                let (identifier, verification) = verified(identifier, team, path);
                Some(Identity {
                    identifier,
                    cdhash,
                    verification,
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

/// The grant key and how the OS vouches for it: an anchored signature by
/// its signing identifier and team, anything else by its `path`.
fn verified(
    identifier: String,
    team: Option<String>,
    path: Option<std::path::PathBuf>,
) -> (String, Verification) {
    match path {
        None => (identifier, Verification::MacSignature { team }),
        Some(path) => (
            path.to_string_lossy().into_owned(),
            Verification::Executable { path },
        ),
    }
}

/// Whether `code` satisfies `anchor apple generic`: Developer ID, the App
/// Store, or Apple's own signature.
unsafe fn anchored(code: SecCodeRef) -> bool {
    // SAFETY: the requirement and its text are created and released here.
    unsafe {
        let text = CFStringCreateWithCString(
            std::ptr::null(),
            c"anchor apple generic".as_ptr(),
            CF_STRING_ENCODING_UTF8,
        );
        if text.is_null() {
            return false;
        }
        let mut requirement: SecRequirementRef = std::ptr::null_mut();
        let created = SecRequirementCreateWithString(text, 0, &mut requirement) == 0
            && !requirement.is_null();
        let anchored = created && SecCodeCheckValidity(code, 0, requirement) == 0;
        if !requirement.is_null() {
            CFRelease(requirement);
        }
        CFRelease(text);
        anchored
    }
}

/// The file `code` was loaded from: its bundle, or a bare executable.
unsafe fn code_path(code: SecStaticCodeRef) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: the URL is created by Security and released here; `buffer`
    // is writable for its length.
    unsafe {
        let mut url: CFURLRef = std::ptr::null();
        if SecCodeCopyPath(code, 0, &mut url) != 0 || url.is_null() {
            return None;
        }
        let mut buffer = [0_u8; 4096];
        let copied =
            CFURLGetFileSystemRepresentation(url, true, buffer.as_mut_ptr(), buffer.len() as isize);
        CFRelease(url);
        if !copied {
            return None;
        }
        let path = CStr::from_ptr(buffer.as_ptr().cast()).to_bytes();
        let path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(path));
        path.is_absolute().then_some(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Only an anchored signature names an app by its signing identity; an
    /// ad-hoc or otherwise unanchored one is named by its path, like an
    /// unsigned program elsewhere, never as verified.
    #[test]
    fn only_an_anchored_signature_is_verified() {
        let anchored = verified("com.example.notes".into(), Some("ABCDE12345".into()), None);
        assert_eq!(
            anchored,
            (
                "com.example.notes".to_owned(),
                Verification::MacSignature {
                    team: Some("ABCDE12345".into())
                }
            )
        );
        let ad_hoc = verified("us.zoom.xos".into(), None, Some("/tmp/Zoom.app".into()));
        assert_eq!(
            ad_hoc,
            (
                "/tmp/Zoom.app".to_owned(),
                Verification::Executable {
                    path: "/tmp/Zoom.app".into()
                }
            )
        );
    }

    /// This test binary, built by cargo, is at most ad-hoc signed: never
    /// anchored.
    #[test]
    fn this_test_binary_is_not_anchored() {
        let mut code: SecCodeRef = std::ptr::null_mut();
        // SAFETY: Hex's own process as a guest of the system host.
        let status = unsafe { SecCodeCopySelf(0, &mut code) };
        if status != 0 || code.is_null() {
            return;
        }
        // SAFETY: `code` is valid until released.
        unsafe {
            assert!(!anchored(code));
            CFRelease(code);
        }
    }

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecCodeCopySelf(flags: u32, code: *mut SecCodeRef) -> i32;
    }
}
