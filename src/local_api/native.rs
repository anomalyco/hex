//! Native clients' owner-only Unix socket for consented recordings.
//!
//! A native process (Electron's main process, a Rune host, a CLI) cannot be
//! identified by a name it sends or by an `Origin`, so it does not use the
//! loopback HTTP channel at all. It connects to `record.sock` in Hex's data
//! folder, which only this user can open (mode 0600), and Hex reads the
//! peer's audit token from the kernel. `Security.framework` then reports the
//! signing identifier and team for that token; the consent dialog names that
//! verified identity, and the grant is bound to the audit token.
//!
//! This socket serves `/record/*` and nothing else: it has no bearer token,
//! no `Authorization` path, and no route for audio, dictation, control, or
//! settings.

use std::ffi::{CStr, c_char, c_void};
use std::fs;

use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::http::HttpResponse;
use super::record::{self, Channel, Identity, PeerId};
use super::{HttpContext, read_request, write_response};

const ACCEPT_WAIT: Duration = Duration::from_millis(50);
const CONNECTION_WAIT: Duration = Duration::from_millis(500);

/// Hex's native recording socket: `record.sock` beside `local-api.json`.
pub(super) fn socket_path() -> Option<PathBuf> {
    crate::app_paths::support_dir()
        .ok()
        .map(|folder| folder.join("record.sock"))
}

/// The bound owner-only socket and the thread accepting on it.
pub(super) struct Listener {
    path: PathBuf,
    worker: JoinHandle<()>,
}

impl Listener {
    /// Binds `path` and serves native `/record/*` requests until the app's
    /// shutdown flag is set.
    pub(super) fn start(
        path: &Path,
        context: Arc<HttpContext>,
        shutdown: Arc<std::sync::atomic::AtomicBool>,
    ) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // The local API's instance lock already makes this the one Hex; a
        // socket left by a crashed run is stale.
        let _ = fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let path = path.to_path_buf();
        let worker = thread::Builder::new()
            .name("hex-native-record".into())
            .spawn(move || serve(listener, context, shutdown))?;
        Ok(Self { path, worker })
    }

    pub(super) fn shutdown(self) {
        // The join is bounded by the accept loop's wait after the app sets
        // its shutdown flag.
        if self.worker.join().is_err() {
            tracing::error!("native record socket thread panicked");
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn serve(
    listener: UnixListener,
    context: Arc<HttpContext>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
) {
    while !shutdown.load(std::sync::atomic::Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_read_timeout(Some(CONNECTION_WAIT));
                let _ = stream.set_write_timeout(Some(CONNECTION_WAIT));
                let identity = identify(stream.as_raw_fd());
                let context = context.clone();
                // Each connection is short; a finish waits on the engine, so
                // it gets its own thread rather than blocking the accept
                // loop.
                let spawned = thread::Builder::new()
                    .name("hex-native-record-client".into())
                    .spawn(move || handle(stream, identity, &context));
                if let Err(error) = spawned {
                    tracing::warn!(%error, "could not serve a native record client");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_WAIT);
            }
            Err(error) => {
                tracing::warn!(%error, "native record socket accept failed");
                thread::sleep(ACCEPT_WAIT);
            }
        }
    }
}

fn handle(mut stream: UnixStream, identity: Option<(PeerId, Identity)>, context: &HttpContext) {
    let request = match read_request(&mut stream, &context.shutdown) {
        Ok(request) => request,
        Err(super::http::RequestReadError::Shutdown) => return,
        Err(error) => {
            tracing::debug!(?error, "native record client sent no valid request");
            let _ = write_response(&mut stream, HttpResponse::empty(400), None);
            return;
        }
    };
    let response =
        route(request, identity, context).respond(context, &mut stream, &context.shutdown);
    if let Err(error) = write_response(&mut stream, response, None) {
        tracing::debug!(%error, "native record client disconnected");
    }
}

/// The native socket's whole router: `/record/*` only.
fn route(
    request: super::HttpRequest,
    identity: Option<(PeerId, Identity)>,
    context: &HttpContext,
) -> record::Request {
    let path = request.path.split('?').next().unwrap_or("/").to_owned();
    let Some(_record) = context.record.as_ref() else {
        return record::Request::Refused(HttpResponse::code(403, "record-unavailable"));
    };
    if request.method == "POST"
        && let Some(endpoint) = record::Endpoint::of(&path)
    {
        let Some((peer, identity)) = identity else {
            return record::Request::Refused(HttpResponse::code(403, "identity-unverified"));
        };
        let channel = Channel::Native { peer, identity };
        return match record::Call::admit(endpoint, request, channel) {
            Ok(call) => record::Request::Pair(call),
            Err(response) => record::Request::Refused(response),
        };
    }
    let Some((peer, identity)) = identity else {
        if request.record_grant.is_some() || request.record_token.is_some() {
            return record::Request::Refused(HttpResponse::empty(401));
        }
        return record::Request::Refused(HttpResponse::empty(404));
    };
    let channel = Channel::Native { peer, identity };
    if let Some(grant) = request.record_grant.clone() {
        if !record::bodyless(&request) {
            return record::Request::Refused(HttpResponse::code(400, "invalid-request"));
        }
        if request.method == "POST" && path == record::START_PATH {
            return record::Request::Session {
                action: record::Action::Start,
                token: grant,
                channel,
            };
        }
        if request.method != "POST" || path != record::REVOKE_PATH {
            return record::Request::Refused(HttpResponse::empty(401));
        }
        return record::Request::Revoke {
            token: grant,
            channel,
        };
    }
    let Some(token) = request.record_token.clone() else {
        return record::Request::Refused(HttpResponse::empty(404));
    };
    let Some(action) = record::action(&request.method, &path) else {
        return record::Request::Refused(HttpResponse::empty(401));
    };
    if !record::bodyless(&request) {
        return record::Request::Refused(HttpResponse::code(400, "invalid-request"));
    }
    record::Request::Session {
        action,
        token,
        channel,
    }
}

/// Starts the native socket when the app configured one and can record.
pub(super) fn start_if_configured(
    path: Option<&Path>,
    context: &Arc<HttpContext>,
    shutdown: &Arc<std::sync::atomic::AtomicBool>,
) -> Option<Listener> {
    let (Some(path), true) = (path, context.record.is_some()) else {
        return None;
    };
    match Listener::start(path, context.clone(), shutdown.clone()) {
        Ok(listener) => Some(listener),
        Err(error) => {
            tracing::warn!(%error, "native record socket is unavailable");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Peer identity

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
fn identify(fd: RawFd) -> Option<(PeerId, Identity)> {
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
                    team,
                    cdhash,
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
