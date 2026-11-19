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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
use windows::Bound;
#[cfg(all(test, target_os = "windows"))]
pub(super) mod tests {
    pub(in crate::local_api) use super::windows::tests::connect;
}

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
/// How often the channel checks Labs › Connected apps, to open or close.
const LAB_CHECK: Duration = Duration::from_millis(200);
/// How long a channel that failed to open waits before it tries again.
const REOPEN_WAIT: Duration = Duration::from_secs(5);
/// The most native clients served at once; one more is told it is busy.
const CLIENTS: usize = 8;
/// How many clients past [`CLIENTS`] wait to be told so; more are dropped.
const BUSY_QUEUE: usize = 4;
/// How long a client past [`CLIENTS`] has to send its request before it is
/// dropped unanswered.
const BUSY_WAIT: Duration = Duration::from_secs(1);
/// How long one read of a busy client waits, so [`BUSY_WAIT`] holds.
#[cfg(unix)]
const BUSY_POLL: Duration = Duration::from_millis(50);
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

/// Hex's native channel, open only while Labs › Connected apps is on: a
/// thread opens it when the lab turns on and closes it, ending its
/// connections, when the lab turns off or the app shuts down.
pub(super) struct Listener {
    worker: JoinHandle<()>,
}

impl Listener {
    pub(super) fn start(
        path: &Path,
        context: Arc<HttpContext>,
        shutdown: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        let path = path.to_path_buf();
        let worker = std::thread::Builder::new()
            .name("hex-native-record".into())
            .spawn(move || supervise(&path, &context, &shutdown))?;
        Ok(Self { worker })
    }

    pub(super) fn shutdown(self) {
        // Bounded by the lab check's wait after the app sets its flag.
        if self.worker.join().is_err() {
            tracing::error!("native record channel thread panicked");
        }
    }
}

fn supervise(path: &Path, context: &Arc<HttpContext>, shutdown: &AtomicBool) {
    let Some(record) = context.record.clone() else {
        return;
    };
    let mut open: Option<Bound> = None;
    let mut failed: Option<Instant> = None;
    while !shutdown.load(Ordering::Acquire) {
        let on = record.access().lab();
        if on && open.is_none() && failed.is_none_or(|at| at.elapsed() >= REOPEN_WAIT) {
            match Bound::start(path, context.clone()) {
                Ok(bound) => (open, failed) = (Some(bound), None),
                Err(error) => {
                    tracing::warn!(%error, "native record channel is unavailable");
                    failed = Some(Instant::now());
                }
            }
        } else if !on && let Some(bound) = open.take() {
            bound.stop();
        }
        std::thread::sleep(LAB_CHECK);
    }
    if let Some(bound) = open {
        bound.stop();
    }
}

/// One of the [`CLIENTS`] places for a connection being served; freed on
/// drop.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(served: &Arc<AtomicUsize>) -> Option<Self> {
        served
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < CLIENTS).then_some(count + 1)
            })
            .ok()
            .map(|_| Self(served.clone()))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Tells clients past [`CLIENTS`] `503 native-busy`, one at a time on its
/// own thread, so the accept loop never waits on one: each gets
/// [`BUSY_WAIT`] to send its request, which is read whole (so closing never
/// discards what the client sent before it reads the answer), then the
/// answer, then `close`. A full queue drops the connection.
struct Busy<S> {
    queue: std::sync::mpsc::SyncSender<S>,
    worker: JoinHandle<()>,
}

impl<S: Read + Write + Send + 'static> Busy<S> {
    fn start(close: fn(S)) -> std::io::Result<Self> {
        let (queue, waiting) = std::sync::mpsc::sync_channel::<S>(BUSY_QUEUE);
        let worker = std::thread::Builder::new()
            .name("hex-native-busy".into())
            .spawn(move || {
                for mut stream in waiting {
                    if !turn_away(&mut stream) {
                        tracing::debug!("a native client past the limit sent no request in time");
                    }
                    close(stream);
                }
            })?;
        Ok(Self { queue, worker })
    }

    fn turn_away(&self, stream: S) {
        let _ = self.queue.try_send(stream);
    }

    fn stop(self) {
        drop(self.queue);
        if self.worker.join().is_err() {
            tracing::error!("native busy responder panicked");
        }
    }
}

/// Reads one request, body included, within [`BUSY_WAIT`], then answers it
/// busy; false when it didn't arrive in time.
fn turn_away(stream: &mut (impl Read + Write)) -> bool {
    let deadline = Instant::now() + BUSY_WAIT;
    let never = AtomicBool::new(false);
    let Ok(request) = super::http::read_request_until(stream, &never, deadline) else {
        return false;
    };
    let mut body = request.body_prefix;
    if let Some(length) = request.content_length {
        let left = deadline.saturating_duration_since(Instant::now());
        let limit = super::record::MAX_BODY_BYTES;
        if super::read_body_bounded(stream, length, &mut body, &never, limit, left).is_err() {
            return false;
        }
    }
    let busy = HttpResponse::code(503, "native-busy");
    write_response(stream, busy, None).is_ok()
}

/// Serves one connection: no identity work unless the lab is still on,
/// then the request, ending early once `stop` is set.
fn client<S: Read + Write>(
    mut stream: S,
    context: &HttpContext,
    stop: &AtomicBool,
    identify: impl FnOnce(&S) -> Option<(PeerId, Identity)>,
) {
    if !context
        .record
        .as_ref()
        .is_some_and(|record| record.access().lab())
    {
        let unavailable = HttpResponse::code(403, "record-unavailable");
        let _ = write_response(&mut stream, unavailable, None);
        return;
    }
    let identity = identify(&stream);
    handle(stream, identity, context, stop);
}

/// The bound owner-only socket and the thread accepting on it.
#[cfg(unix)]
struct Bound {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

#[cfg(unix)]
impl Bound {
    /// Binds `path` and serves native `/record/*` requests until stopped.
    fn start(path: &Path, context: Arc<HttpContext>) -> std::io::Result<Self> {
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
        // Bind and restrict a fresh name, then move it into place: a client
        // that sees `path` always finds an owner-only socket that is already
        // listening, never one between `bind` and `listen`.
        let fresh = path.with_file_name(format!(
            ".{}.{}",
            path.file_name().and_then(|name| name.to_str()).unwrap_or("record.sock"),
            std::process::id()
        ));
        let _ = fs::remove_file(&fresh);
        let listener = std::os::unix::net::UnixListener::bind(&fresh)?;
        let placed = fs::set_permissions(&fresh, fs::Permissions::from_mode(0o600))
            .and_then(|()| fs::rename(&fresh, path));
        if let Err(error) = placed {
            let _ = fs::remove_file(&fresh);
            return Err(error);
        }
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker = std::thread::Builder::new()
            .name("hex-native-accept".into())
            .spawn({
                let stop = stop.clone();
                move || serve(listener, context, stop)
            })?;
        Ok(Self {
            path: path.to_path_buf(),
            stop,
            worker,
        })
    }

    /// Closes the socket and ends its connections within their read wait.
    fn stop(self) {
        self.stop.store(true, Ordering::Release);
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
    stop: Arc<AtomicBool>,
) {
    use std::os::fd::AsRawFd;
    let served = Arc::new(AtomicUsize::new(0));
    // Half-closes after the answer; the request was read whole, so the
    // close never resets the connection before the client reads it.
    let busy = Busy::start(|stream: std::os::unix::net::UnixStream| {
        let _ = stream.shutdown(std::net::Shutdown::Write);
    });
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_read_timeout(Some(CONNECTION_WAIT));
                let _ = stream.set_write_timeout(Some(CONNECTION_WAIT));
                let Some(slot) = Slot::take(&served) else {
                    // A short read wait, so its deadline holds.
                    let _ = stream.set_read_timeout(Some(BUSY_POLL));
                    if let Ok(busy) = &busy {
                        busy.turn_away(stream);
                    }
                    continue;
                };
                let context = context.clone();
                let stop = stop.clone();
                // Each connection is short; a finish waits on the engine, and
                // reading a peer's identity may wait on the disk, so each gets
                // its own thread rather than blocking the accept loop.
                let spawned = std::thread::Builder::new()
                    .name("hex-native-record-client".into())
                    .spawn(move || {
                        let _slot = slot;
                        client(stream, &context, &stop, |stream| {
                            identify(stream.as_raw_fd())
                        });
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
    if let Ok(busy) = busy {
        busy.stop();
    }
}

fn handle(
    mut stream: impl Read + Write,
    identity: Option<(PeerId, Identity)>,
    context: &HttpContext,
    stop: &AtomicBool,
) {
    let request = match read_request(&mut stream, stop) {
        Ok(request) => request,
        Err(super::http::RequestReadError::Shutdown) => return,
        Err(error) => {
            tracing::debug!(?error, "native record client sent no valid request");
            let _ = write_response(&mut stream, HttpResponse::empty(400), None);
            return;
        }
    };
    let response = route(request, identity, context).respond(context, &mut stream, stop);
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
