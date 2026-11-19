//! A native requester's name and icon on macOS, from its verified code.
//!
//! The consent dialog names a native app by what `Security.framework`
//! verified: its signing identifier, team, and code-directory hash. This adds
//! the app's display name (`CFBundleDisplayName`, else `CFBundleName`) and
//! its Finder icon, read from the bundle at the path `Security.framework`
//! reports for that same verified static code, never from anything the
//! client sent. The code is resolved again from the peer's audit token and
//! used only when its code-directory hash matches the identity the grant is
//! bound to, so a process that exec'd or was swapped since can't lend
//! another app's name.
//!
//! It runs on the request's worker, never a UI thread, and is bounded: the
//! lookup gets [`DEADLINE`] and is otherwise skipped, and results are cached
//! by code-directory hash. Name and icon are display data only.

use std::collections::VecDeque;
use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use objc2::rc::autoreleasepool;
use objc2_foundation::{NSBundle, NSString};

use super::consent::ConsentIcon;
use super::record::PeerId;

/// How long Hex waits for a name and icon before asking without them.
const DEADLINE: Duration = Duration::from_millis(800);
/// The longest name the dialog shows; a longer one is cut.
const MAX_NAME_CHARS: usize = 64;
/// How many apps' lookups are remembered.
const CACHE: usize = 8;

type Described = (Option<String>, Option<ConsentIcon>);

static CACHED: Mutex<VecDeque<(String, Described)>> = Mutex::new(VecDeque::new());

/// The display name and icon of the app behind `peer`, whose verified code
/// directory hash is `cdhash`; both `None` when they can't be read in time.
pub(super) fn describe(peer: &PeerId, cdhash: &str) -> Described {
    if let Some((_, described)) = lock(&CACHED).iter().find(|(key, _)| key == cdhash) {
        return described.clone();
    }
    let token: Vec<u8> = peer.0.iter().flat_map(|word| word.to_ne_bytes()).collect();
    let expected = cdhash.to_owned();
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("hex-requester-icon".into())
        .spawn(move || {
            let described = verified_path(&token, &expected)
                .map(|path| (name(&path), icon(&path)))
                .unwrap_or((None, None));
            let _ = sender.send(described);
        });
    if spawned.is_err() {
        return (None, None);
    }
    let Ok(described) = receiver.recv_timeout(DEADLINE) else {
        tracing::debug!("a requester's name and icon took too long; asking without them");
        return (None, None);
    };
    let mut cached = lock(&CACHED);
    cached.push_front((cdhash.to_owned(), described.clone()));
    cached.truncate(CACHE);
    described
}

/// `CFBundleDisplayName`, else `CFBundleName`, of the bundle at `path`:
/// plain text only, cut to [`MAX_NAME_CHARS`].
fn name(path: &Path) -> Option<String> {
    autoreleasepool(|_| {
        let bundle = NSBundle::bundleWithPath(&NSString::from_str(path.to_str()?))?;
        ["CFBundleDisplayName", "CFBundleName"]
            .into_iter()
            .find_map(|key| {
                let value = bundle.objectForInfoDictionaryKey(&NSString::from_str(key))?;
                let value = value.downcast::<NSString>().ok()?.to_string();
                plain(&value)
            })
    })
}

fn plain(name: &str) -> Option<String> {
    let name: String = name
        .chars()
        .filter(|c| !super::consent::hidden(*c))
        .take(MAX_NAME_CHARS)
        .collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The bundle's Finder icon, at the window's largest size. Only a bundle
/// gets one: a bare executable's generic icon would say nothing.
fn icon(path: &Path) -> Option<ConsentIcon> {
    if !path.is_dir() {
        return None;
    }
    let (width, height, pixels) = rune::image::app_icon_rgba(path, ConsentIcon::MAX_SIZE).ok()?;
    super::icon::from_rgba(width, height, pixels)
}

type CFTypeRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFStringRef = *const c_void;
type CFDataRef = *const c_void;
type CFUrlRef = *const c_void;
type SecCodeRef = *mut c_void;
type SecStaticCodeRef = *const c_void;

const SEC_CS_SIGNING_INFORMATION: u32 = 0x2;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    static kSecGuestAttributeAudit: CFStringRef;
    static kSecCodeInfoUnique: CFStringRef;
    fn SecCodeCopyGuestWithAttributes(
        host: CFTypeRef,
        attributes: CFDictionaryRef,
        flags: u32,
        code: *mut SecCodeRef,
    ) -> i32;
    fn SecCodeCheckValidity(code: SecCodeRef, flags: u32, requirement: CFTypeRef) -> i32;
    fn SecCodeCopyStaticCode(code: SecCodeRef, flags: u32, out: *mut SecStaticCodeRef) -> i32;
    fn SecCodeCopySigningInformation(
        code: SecStaticCodeRef,
        flags: u32,
        out: *mut CFDictionaryRef,
    ) -> i32;
    fn SecCodeCopyPath(code: SecStaticCodeRef, flags: u32, path: *mut CFUrlRef) -> i32;
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
    fn CFURLGetFileSystemRepresentation(
        url: CFUrlRef,
        resolve: bool,
        buffer: *mut u8,
        length: isize,
    ) -> bool;
    fn CFRelease(value: CFTypeRef);
}

/// The path of the valid code behind `token`, when its code-directory hash
/// is `cdhash`.
fn verified_path(token: &[u8], cdhash: &str) -> Option<PathBuf> {
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
        let mut code: SecCodeRef = std::ptr::null_mut();
        let path = if !attributes.is_null()
            && SecCodeCopyGuestWithAttributes(std::ptr::null(), attributes, 0, &mut code) == 0
            && !code.is_null()
        {
            let path = path_of(code, cdhash);
            CFRelease(code);
            path
        } else {
            None
        };
        if !attributes.is_null() {
            CFRelease(attributes);
        }
        CFRelease(data);
        path
    }
}

unsafe fn path_of(code: SecCodeRef, cdhash: &str) -> Option<PathBuf> {
    unsafe {
        if SecCodeCheckValidity(code, 0, std::ptr::null()) != 0 {
            return None;
        }
        let mut stat: SecStaticCodeRef = std::ptr::null();
        if SecCodeCopyStaticCode(code, 0, &mut stat) != 0 || stat.is_null() {
            return None;
        }
        let path = if unique(stat).as_deref() == Some(cdhash) {
            let mut url: CFUrlRef = std::ptr::null();
            if SecCodeCopyPath(stat, 0, &mut url) == 0 && !url.is_null() {
                let mut buffer = [0_u8; 4096];
                let ok = CFURLGetFileSystemRepresentation(
                    url,
                    true,
                    buffer.as_mut_ptr(),
                    buffer.len() as isize,
                );
                CFRelease(url);
                ok.then(|| {
                    let path = CStr::from_ptr(buffer.as_ptr().cast::<c_char>());
                    PathBuf::from(path.to_string_lossy().into_owned())
                })
            } else {
                None
            }
        } else {
            None
        };
        CFRelease(stat);
        path
    }
}

/// The static code's code-directory hash, as `native` writes it.
unsafe fn unique(stat: SecStaticCodeRef) -> Option<String> {
    unsafe {
        let mut info: CFDictionaryRef = std::ptr::null();
        if SecCodeCopySigningInformation(stat, SEC_CS_SIGNING_INFORMATION, &mut info) != 0
            || info.is_null()
        {
            return None;
        }
        let digest = CFDictionaryGetValue(info, kSecCodeInfoUnique);
        let hex = (!digest.is_null()).then(|| {
            let len = CFDataGetLength(digest).clamp(0, 64) as usize;
            std::slice::from_raw_parts(CFDataGetBytePtr(digest), len)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        });
        CFRelease(info);
        hex
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_name_and_icon_come_from_the_bundle() {
        let finder = Path::new("/System/Library/CoreServices/Finder.app");
        if !finder.exists() {
            return;
        }
        assert_eq!(name(finder).as_deref(), Some("Finder"));
        let icon = icon(finder).unwrap();
        assert_eq!(icon.size(), ConsentIcon::MAX_SIZE);
        assert!(icon.pixels().chunks(4).any(|p| p[3] > 0));
    }

    #[test]
    fn a_name_is_plain_text_and_bounded() {
        assert_eq!(plain("  Notes\u{7}  ").as_deref(), Some("Notes"));
        assert_eq!(plain("\n").as_deref(), None);
        assert_eq!(
            plain(&"x".repeat(200)).unwrap().chars().count(),
            MAX_NAME_CHARS
        );
    }

    #[test]
    fn a_peer_that_isnt_a_process_describes_nothing() {
        assert_eq!(describe(&PeerId([0; 8]), "not-a-hash"), (None, None));
    }
}
