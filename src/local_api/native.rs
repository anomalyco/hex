//! Native clients' owner-only channel for consented recordings.
//!
//! A native process (Electron's main process, a Rune host, a CLI) cannot be
//! identified by a name it sends or by an `Origin`, so it does not use the
//! loopback HTTP channel at all. It connects to Hex's own channel, which only
//! this user can open, and Hex asks the OS who the peer is:
//!
//! - macOS: `record.sock` in Hex's data folder (mode 0600). Hex reads the
//!   peer's audit token, and `Security.framework` reports the signing
//!   identifier and team for it ([`macos`]).
//! - Linux: the same socket, in Hex's private data folder (0700). Hex reads
//!   `SO_PEERCRED`, pins the process, and names it by its sandbox app id or
//!   the executable the kernel reports ([`linux`]).
//! - Windows: a named pipe only this user can open, whose name Hex writes to
//!   `record.pipe` in its data folder. Hex names the client by its image's
//!   Authenticode publisher, or by the image path when it has none
//!   ([`windows`]).
//!
//! The consent dialog names that verified identity, and the grant is bound to
//! the one peer process. This channel serves `/record/*` and nothing else: it
//! has no bearer token, no `Authorization` path, and no route for audio,
//! dictation, control, or settings.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
#[cfg(unix)]
use std::time::Duration;

use super::HttpContext;
use super::consent::{ConsentIcon, Verification};
use super::http::HttpResponse;
use super::record::{self, Channel};
use super::{read_request, write_response};

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod display;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux::identify;
#[cfg(target_os = "macos")]
use macos::identify;
#[cfg(target_os = "windows")]
pub(super) use windows::Listener;

/// The one native process a connection came from: its audit token on macOS,
/// its pid and start time elsewhere.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PeerId(pub(crate) [u32; 8]);

/// What the OS verified about a native client.
#[derive(Clone, Debug)]
pub(crate) struct Identity {
    /// The stable grant key: the signing identifier (`com.example.app` for a
    /// signed bundle) on macOS, the sandbox app id or executable path on
    /// Linux, the signed image's file name (with its publisher) or the
    /// image path on Windows.
    pub(crate) identifier: String,
    /// Verified code-directory hash on macOS, including for ad-hoc
    /// signatures; empty elsewhere.
    pub(crate) cdhash: String,
    pub(crate) verification: Verification,
    /// Display data Hex derived from verified data. Never compared, so a
    /// name or icon found later never unbinds a grant.
    pub(crate) name: Option<String>,
    pub(crate) icon: Option<ConsentIcon>,
}

impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier
            && self.cdhash == other.cdhash
            && self.verification == other.verification
    }
}

impl Eq for Identity {}

#[cfg(unix)]
const ACCEPT_WAIT: Duration = Duration::from_millis(50);
#[cfg(unix)]
const CONNECTION_WAIT: Duration = Duration::from_millis(500);

/// Hex's native recording channel beside `local-api.json`: the socket on
/// macOS and Linux, the file naming the pipe on Windows.
pub(crate) fn socket_path() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "record.pipe"
    } else {
        "record.sock"
    };
    crate::app_paths::support_dir()
        .ok()
        .map(|folder| folder.join(name))
}

/// The bound owner-only socket and the thread accepting on it.
#[cfg(unix)]
pub(super) struct Listener {
    path: PathBuf,
    worker: std::thread::JoinHandle<()>,
}

#[cfg(unix)]
impl Listener {
    /// Binds `path` and serves native `/record/*` requests until the app's
    /// shutdown flag is set.
    pub(super) fn start(
        path: &Path,
        context: Arc<HttpContext>,
        shutdown: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        #[cfg(target_os = "linux")]
        linux::prepare(path)?;
        #[cfg(not(target_os = "linux"))]
        {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            // The local API's instance lock already makes this the one Hex; a
            // socket left by a crashed run is stale.
            let _ = fs::remove_file(path);
        }
        let listener = std::os::unix::net::UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let path = path.to_path_buf();
        let worker = std::thread::Builder::new()
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
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
fn serve(
    listener: std::os::unix::net::UnixListener,
    context: Arc<HttpContext>,
    shutdown: Arc<AtomicBool>,
) {
    use std::os::fd::AsRawFd;
    while !shutdown.load(std::sync::atomic::Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_read_timeout(Some(CONNECTION_WAIT));
                let _ = stream.set_write_timeout(Some(CONNECTION_WAIT));
                let context = context.clone();
                // Each connection is short; a finish waits on the engine, and
                // reading a peer's identity may wait on the disk, so each gets
                // its own thread rather than blocking the accept loop.
                let spawned = std::thread::Builder::new()
                    .name("hex-native-record-client".into())
                    .spawn(move || {
                        let identity = identify(stream.as_raw_fd());
                        handle(stream, identity, &context);
                    });
                if let Err(error) = spawned {
                    tracing::warn!(%error, "could not serve a native record client");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_WAIT);
            }
            Err(error) => {
                tracing::warn!(%error, "native record socket accept failed");
                std::thread::sleep(ACCEPT_WAIT);
            }
        }
    }
}

fn handle(
    mut stream: impl Read + Write,
    identity: Option<(PeerId, Identity)>,
    context: &HttpContext,
) {
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

/// The native channel's whole router: `/record/*` only.
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

/// Starts the native channel when the app configured one and can record.
pub(super) fn start_if_configured(
    path: Option<&Path>,
    context: &Arc<HttpContext>,
    shutdown: &Arc<AtomicBool>,
) -> Option<Listener> {
    let (Some(path), true) = (path, context.record.is_some()) else {
        return None;
    };
    match Listener::start(path, context.clone(), shutdown.clone()) {
        Ok(listener) => Some(listener),
        Err(error) => {
            tracing::warn!(%error, "native record channel is unavailable");
            None
        }
    }
}
