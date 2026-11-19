use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Serialize;

use crate::app_settings::AppSettings;
use crate::desktop::events::{EventLog, VoiceEvent, now_ms};

mod access;
mod connect;
mod consent;
mod control;
mod dictation;
mod discovery;
mod http;
pub(crate) mod icon;
#[cfg(target_os = "macos")]
mod mac_app;
mod meeting_events;
mod native;
pub(crate) mod permissions;
pub(crate) mod processing;
mod record;
mod streams;
pub(crate) mod timestamps;
mod transcriptions;

pub(crate) use access::{Access, AppConsent};
pub(crate) use connect::Connect;
pub(crate) use consent::{
    Answer, ConsentGate, ConsentIcon, Intent, Prompt, Recipient, SandboxKind, Verification,
};
#[cfg_attr(not(target_os = "macos"), allow(unused_imports))]
pub(crate) use permissions::Permissions;
pub(crate) use permissions::View as PermissionView;
pub(crate) use permissions::{Capability as PermissionCapability, WebsiteAction, Websites};
pub(crate) use record::View as RecordView;
pub(crate) use record::{
    GrantView as RecordGrant, Phase as RecordPhase, Sessions as RecordSessions,
};

#[cfg(debug_assertions)]
#[cfg_attr(not(target_os = "macos"), allow(unused_imports))]
pub use discovery::call_developer;
use discovery::*;
pub(crate) use discovery::{Discovered, Endpoint, discover, error_code};
#[cfg(not(target_os = "macos"))]
pub use discovery::{ExitWatch, watch_running};
pub use discovery::{call_control, running_pid};
use http::*;
pub use processing::Processing;
pub use timestamps::Timestamps;
use transcriptions::transcribe_audio;
const API_VERSION: &str = "2";

pub(crate) use native::socket_path as native_socket_path;
/// The recording slice's app-facing names.
#[cfg_attr(not(target_os = "macos"), allow(unused_imports))]
pub(crate) use record::app_sessions as record_sessions;

const LIVE_PROBE_TIMEOUT: Duration = Duration::from_millis(150);
/// The longest the accept loop sleeps without a connection. Shutdown wakes
/// it at once ([`wake_acceptor`]); this only bounds a shutdown whose wake
/// could not connect.
const ACCEPT_WAIT: Duration = Duration::from_secs(1);
/// The pause after an accept error before the next try.
const ACCEPT_RETRY: Duration = Duration::from_millis(10);
const WAKE_TIMEOUT: Duration = Duration::from_millis(250);
const CONNECTION_TIMEOUT: Duration = Duration::from_millis(500);
const HEADER_DEADLINE: Duration = Duration::from_secs(2);
const UPLOAD_DEADLINE: Duration = Duration::from_secs(30);
const TRANSCRIPTION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const AUTH_OBSERVATION_INTERVAL: Duration = Duration::from_secs(5);
const HTTP_WORKERS: usize = 4;
const CONNECTION_QUEUE_CAPACITY: usize = 16;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADERS: usize = 64;
const MAX_PATH_BYTES: usize = 2 * 1024;
pub(crate) const MAX_AUDIO_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEVELOPER_CONTROL_BYTES: usize = 16 * 1024;
const DEVELOPER_CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONTROL_BYTES: usize = 64 * 1024;
const MAX_DICTATION_SOURCE_BYTES: usize = 128;

type HealthProvider = Arc<dyn Fn() -> HealthResponse + Send + Sync>;
type ModelPreparer =
    Box<dyn FnOnce(Arc<AtomicBool>, Arc<AtomicU64>, Arc<AtomicU8>) -> Result<()> + Send + 'static>;

pub struct LocalApi {
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    discovery: Option<DiscoveryLease>,
    document: DiscoveryDocument,
    events: EventLog,
    stop_observed: Arc<AtomicBool>,
    transcription_service: Option<crate::transcription::service::TranscriptionService>,
    native: Option<native::Listener>,
    _instance: Option<crate::desktop::instance::InstanceLock>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedEndpoint {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub url: String,
    pub token: String,
    pub api_version: String,
    pub pid: u32,
}

#[derive(Default)]
struct AuthObservationLimiter {
    last: Mutex<Option<Instant>>,
}

struct HttpContext {
    token: Arc<str>,
    health: HealthProvider,
    events: EventLog,
    auth_limiter: AuthObservationLimiter,
    model_preparing: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    transcription: crate::transcription::service::TranscriptionServiceHandle,
    developer_control: Option<SyncSender<crate::control::developer_control::DeveloperCall>>,
    control: Option<SyncSender<crate::control::ControlCall>>,
    dictations: dictation::Dictations,
    streams: Arc<crate::transcription::stream::Streams>,
    stream_engine: crate::transcription::stream::EngineSource,
    meeting_events: meeting_events::Source,
    processing: processing::Processing,
    timestamps: timestamps::Timestamps,
    /// Labs › Connected apps, when this API serves its asks: the consent
    /// routes' own CORS.
    connected: Option<Access>,
    /// Browser connect (Labs › Connected apps): a web page's
    /// one-transcription ask. Absent without a window to ask.
    connect: Option<connect::Connect>,
    /// Consented one-recording sessions; absent without a capture loop.
    record: Option<Arc<record::Sessions>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    version: &'static str,
    api_version: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilitiesResponse {
    audio_formats: [&'static str; 1],
    partial_transcripts: bool,
    service_capture: bool,
    /// `/dictations/{id}/partials` for a start with `"partials": true`.
    dictation_partials: bool,
    /// `/streams` (`docs/streams.md`).
    streams: bool,
    /// `process=global` on `/transcriptions` and `/streams/{id}/finish`
    /// (Labs › Process API transcripts).
    output_processing: bool,
    developer_control: bool,
    control: bool,
    /// The browser connect routes, with a window that can ask.
    connect: bool,
}

/// Optional control channels the running app exposes through its local API.
#[derive(Default)]
pub struct LocalApiOptions {
    pub developer_control: Option<SyncSender<crate::control::developer_control::DeveloperCall>>,
    /// The `hex` control commands (`POST /control`).
    pub control: Option<SyncSender<crate::control::ControlCall>>,
    /// Running-app dictation (`/dictations`): the macOS recognition loop or
    /// the Linux or Windows listener.
    pub dictation_control: Option<dictation::Control>,
    /// The running app's Global-mode output processor for `process=global`.
    /// Uninstalled, `/transcriptions` and `/streams/{id}/finish` answer
    /// `409 processing-unavailable` when asked to process.
    pub processing: Processing,
    /// Native timings on `/transcriptions` (Labs › Timestamps in API
    /// transcripts). Off, a timed request answers
    /// `403 timestamps-disabled`.
    pub timestamps: Timestamps,
    /// Where `/streams` finds its engine; the running app's shared engine
    /// when unset.
    pub(crate) stream_engine: Option<crate::transcription::stream::EngineSource>,
    /// Browser connect (Labs › Browser access to the local API): the window's
    /// consent gate and one-use grants. `None` leaves the routes answering
    /// `403 connect-unavailable`.
    pub(crate) connect: Option<Connect>,
    /// Consented one-recording sessions, when the app can record and ask.
    pub(crate) record: Option<Arc<record::Sessions>>,
    /// Where native clients' owner-only socket binds (on Windows, the file
    /// that names their pipe); unset leaves them without one (the app sets
    /// it; tests use a temporary path).
    pub(crate) native_socket: Option<PathBuf>,
}

impl LocalApi {
    /// Start the one app-owned local API and publish its discovery document.
    pub fn start(events: EventLog, options: LocalApiOptions) -> Result<Self> {
        let instance = crate::desktop::instance::acquire("local-api")?;
        Self::start_bound(
            Some(crate::app_paths::local_api_discovery_file()?),
            Arc::new(current_health),
            events,
            options,
            Some(instance),
        )
    }

    /// Start the running Linux or Windows app's local API. Hex runs on
    /// without it, so a failure is only logged.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn start_for_app(
        event_path: &Path,
        control: SyncSender<crate::control::ControlCall>,
        dictation_control: Option<dictation::Control>,
        processing: Option<Processing>,
        consent: Option<AppConsent>,
    ) -> Option<Self> {
        let (connect, record) = consent.map_or((None, None), |consent| {
            (Some(consent.connect), Some(consent.record))
        });
        let started = EventLog::create(event_path)
            .map_err(Into::into)
            .and_then(|events| {
                Self::start(
                    events,
                    LocalApiOptions {
                        control: Some(control),
                        dictation_control,
                        processing: processing.unwrap_or_default(),
                        connect,
                        record,
                        native_socket: native_socket_path(),
                        ..Default::default()
                    },
                )
            });
        started
            .inspect_err(
                |error| tracing::warn!(error = %format!("{error:#}"), "local API is unavailable"),
            )
            .ok()
    }

    /// Start an unpublished API whose endpoint is handed directly to one host.
    pub fn start_embedded(events: EventLog) -> Result<Self> {
        Self::start_bound(
            None,
            Arc::new(current_health),
            events,
            LocalApiOptions::default(),
            None,
        )
    }

    fn start_bound(
        discovery_path: Option<PathBuf>,
        health: HealthProvider,
        events: EventLog,
        options: LocalApiOptions,
        instance: Option<crate::desktop::instance::InstanceLock>,
    ) -> Result<Self> {
        Self::start_with_meeting_events(
            discovery_path,
            health,
            events,
            options,
            instance,
            meeting_events::Source::default(),
        )
    }

    /// [`Self::start_bound`] with `/meetings/events` on `meeting_events`.
    fn start_with_meeting_events(
        discovery_path: Option<PathBuf>,
        health: HealthProvider,
        events: EventLog,
        options: LocalApiOptions,
        instance: Option<crate::desktop::instance::InstanceLock>,
        meeting_events: meeting_events::Source,
    ) -> Result<Self> {
        if let Some(path) = &discovery_path {
            prepare_discovery_path(path)?;
        }
        let (transcription_service, transcription) =
            crate::transcription::service::TranscriptionService::start()?;
        let listener = TcpListener::bind("127.0.0.1:0").wrap_err("could not bind the local API")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let document = DiscoveryDocument {
            port: address.port(),
            token: generate_token()?,
            api_version: API_VERSION.into(),
            pid: std::process::id(),
        };
        let discovery = if let Some(path) = discovery_path {
            write_discovery(&path, &document)?;
            Some(DiscoveryLease {
                path,
                document: document.clone(),
            })
        } else {
            None
        };
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = shutdown.clone();
        let worker_events = events.clone();
        let token = document.token.clone();
        let worker_discovery = discovery
            .as_ref()
            .map(|lease| (lease.path.clone(), lease.document.clone()));
        let stop_observed = Arc::new(AtomicBool::new(false));
        let worker_stop_observed = stop_observed.clone();
        let unexpected_shutdown = worker_shutdown.clone();
        let native_socket = options.native_socket.clone();
        let http_context = HttpContext {
            token: token.into(),
            health,
            events: worker_events.clone(),
            auth_limiter: AuthObservationLimiter::default(),
            model_preparing: Arc::new(AtomicBool::new(false)),
            shutdown: worker_shutdown,
            transcription,
            developer_control: options.developer_control,
            control: options.control,
            dictations: dictation::Dictations::new(options.dictation_control),
            streams: Arc::default(),
            stream_engine: options
                .stream_engine
                .unwrap_or_else(|| Arc::new(crate::transcription::stream::shared_engine)),
            meeting_events,
            processing: options.processing,
            timestamps: options.timestamps,
            connected: options
                .connect
                .as_ref()
                .map(connect::Connect::access)
                .or_else(|| options.record.as_ref().map(|record| record.access())),
            connect: options.connect,
            record: options.record,
        };
        let http_context = Arc::new(http_context);
        // Native recording clients get their own owner-only socket; it
        // serves `/record/*` only and needs a capture loop to be useful.
        let native =
            native::start_if_configured(native_socket.as_deref(), &http_context, &shutdown);
        let (ready, readiness) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("local-api".into())
            .spawn(move || {
                serve(listener, http_context.clone(), ready);
                if !unexpected_shutdown.load(Ordering::Acquire) {
                    if let Some((path, document)) = worker_discovery {
                        remove_discovery_if_owned(&path, &document);
                    }
                    observe_stopped(&worker_events, &worker_stop_observed);
                    tracing::error!("local API stopped unexpectedly");
                }
            })
            .wrap_err("could not start the local API thread")?;
        match readiness.recv_timeout(Duration::from_secs(1)) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                shutdown.store(true, Ordering::Release);
                wake_acceptor(document.port);
                let _ = worker.join();
                bail!("local API HTTP workers could not start");
            }
        }
        if let Err(error) = events.emit(&VoiceEvent::ApiServerStarted {
            timestamp_ms: now_ms(),
            port: document.port,
        }) {
            tracing::error!(%error, "could not record local API startup");
        }
        tracing::info!(port = document.port, "local API started");
        Ok(Self {
            shutdown,
            worker: Some(worker),
            discovery,
            document,
            events,
            stop_observed,
            transcription_service: Some(transcription_service),
            native,
            _instance: instance,
        })
    }

    pub fn embedded_endpoint(&self) -> EmbeddedEndpoint {
        EmbeddedEndpoint {
            kind: "ready",
            url: format!("http://127.0.0.1:{}", self.document.port),
            token: self.document.token.clone(),
            api_version: self.document.api_version.clone(),
            pid: self.document.pid,
        }
    }

    pub fn shutdown(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        // Retract discovery before draining so a replacement Hex can publish
        // while this listener is still bound and finishing in-flight requests.
        if let Some(discovery) = &self.discovery {
            discovery.remove_if_owned();
        }
        self.shutdown.store(true, Ordering::Release);
        wake_acceptor(self.document.port);
        if let Some(native) = self.native.take() {
            native.shutdown();
        }
        if worker.join().is_err() {
            tracing::error!("local API thread panicked during shutdown");
        }
        if let Some(mut service) = self.transcription_service.take() {
            service.shutdown();
        }
        observe_stopped(&self.events, &self.stop_observed);
        tracing::info!("local API stopped");
    }
}

/// Runs `hex service`: the local API without the app, until `shutdown`.
/// Published, it is the one discoverable service. Embedded (`--embedded`,
/// the TypeScript SDK's direct child), it prints its endpoint as one JSON line
/// on stdout instead and exits when its host closes stdin.
pub fn run_service(
    log_dir: &Path,
    event_path: PathBuf,
    embedded: bool,
    shutdown: &'static AtomicBool,
) -> Result<()> {
    let service_event_path = if embedded {
        log_dir.join(format!("embedded-{}.ndjson", std::process::id()))
    } else {
        event_path
    };
    let events = EventLog::create(&service_event_path)?;
    let local_api = if embedded {
        // This service runs no launch that publishes settings, so read them
        // here: transcription biases on the saved vocabulary rather than an
        // empty one.
        AppSettings::apply_for_embedded_service();
        let api = LocalApi::start_embedded(events)?;
        thread::Builder::new()
            .name("embedded-host-lease".into())
            .spawn(move || {
                let mut stdin = std::io::stdin().lock();
                let mut buffer = [0_u8; 1];
                loop {
                    match stdin.read(&mut buffer) {
                        Ok(0) => {
                            shutdown.store(true, Ordering::Release);
                            thread::sleep(Duration::from_secs(5));
                            tracing::warn!(
                                "forcing embedded service shutdown after host lease closed"
                            );
                            std::process::exit(0);
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => {
                            tracing::warn!(%error, "embedded host lease failed");
                            shutdown.store(true, Ordering::Release);
                            break;
                        }
                    }
                }
            })?;
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(&mut stdout, &api.embedded_endpoint())?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        drop(stdout);
        api
    } else {
        LocalApi::start(events, LocalApiOptions::default())?
    };
    while !shutdown.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
    }
    drop(local_api);
    if embedded
        && let Err(error) = fs::remove_file(&service_event_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, "could not remove embedded service event log");
    }
    Ok(())
}

impl Drop for LocalApi {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn observe_stopped(events: &EventLog, observed: &AtomicBool) {
    if observed.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Err(error) = events
        .emit(&VoiceEvent::ApiServerStopped {
            timestamp_ms: now_ms(),
        })
        .and_then(|()| events.flush())
    {
        tracing::error!(%error, "could not record local API shutdown");
    }
}

fn serve(listener: TcpListener, context: Arc<HttpContext>, ready: mpsc::SyncSender<bool>) {
    let (connections, pending) = mpsc::sync_channel(CONNECTION_QUEUE_CAPACITY);
    let pending = Arc::new(Mutex::new(pending));
    let shutdown = context.shutdown.clone();
    let workers = (0..HTTP_WORKERS)
        .filter_map(|index| {
            let pending = pending.clone();
            let context = context.clone();
            thread::Builder::new()
                .name(format!("local-api-http-{index}"))
                .spawn(move || http_worker(pending, context))
                .map_err(|error| {
                    tracing::error!(%error, "could not start local API HTTP worker");
                })
                .ok()
        })
        .collect::<Vec<_>>();
    if workers.is_empty() {
        let _ = ready.send(false);
        tracing::error!("local API has no HTTP workers");
        return;
    }
    if ready.send(true).is_err() {
        return;
    }

    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            // The connection that woke a shutdown is not a request.
            Ok(_) if shutdown.load(Ordering::Acquire) => break,
            Ok((stream, _)) => match connections.try_send(stream) {
                Ok(()) => {}
                Err(TrySendError::Full(mut stream)) => {
                    let _ = write_response(&mut stream, HttpResponse::empty(503));
                }
                Err(TrySendError::Disconnected(_)) => break,
            },
            // Waking on the next connection (or shutdown's wake) instead of
            // polling took a loopback request from 12.5 ms to 0.12 ms, and
            // an idle Hex from 100 wakeups a second to one.
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_connection(&listener, ACCEPT_WAIT);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                tracing::error!(%error, "local API accept failed");
                thread::sleep(ACCEPT_RETRY);
            }
        }
    }
    drop(connections);
    for worker in workers {
        if worker.join().is_err() {
            tracing::error!("local API HTTP worker panicked during shutdown");
        }
    }
}

/// Sleeps until `listener` has a connection to accept, or `limit` passes.
fn wait_for_connection(listener: &TcpListener, limit: Duration) {
    let timeout = limit.as_millis() as i32;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let mut pfd = libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` is one valid pollfd for the listener's fd.
        unsafe {
            libc::poll(&raw mut pfd, 1, timeout);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{POLLRDNORM, WSAPOLLFD, WSAPoll};
        let mut pfd = WSAPOLLFD {
            fd: listener.as_raw_socket() as usize,
            events: POLLRDNORM,
            revents: 0,
        };
        // SAFETY: `pfd` is one valid entry for the listener's socket.
        unsafe {
            WSAPoll(&raw mut pfd, 1, timeout);
        }
    }
}

/// Ends the accept loop's wait: a connection to the listener wakes it, and
/// it sees the shutdown flag set before. A wake that cannot connect leaves
/// the loop to notice within [`ACCEPT_WAIT`].
fn wake_acceptor(port: u16) {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let _ = TcpStream::connect_timeout(&address, WAKE_TIMEOUT);
}

fn http_worker(pending: Arc<Mutex<Receiver<TcpStream>>>, context: Arc<HttpContext>) {
    loop {
        let stream = pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .recv();
        let Ok(mut stream) = stream else {
            return;
        };
        let _ = stream.set_nodelay(true);
        if let Err(error) = stream.set_read_timeout(Some(CONNECTION_TIMEOUT)) {
            tracing::debug!(%error, "could not set local API read timeout");
            continue;
        }
        if let Err(error) = stream.set_write_timeout(Some(CONNECTION_TIMEOUT)) {
            tracing::debug!(%error, "could not set local API write timeout");
            continue;
        }
        let action = match read_request(&mut stream, &context.shutdown) {
            Ok(request) => handle_request(request, &context),
            Err(RequestReadError::TooLarge) => RequestAction::Respond(HttpResponse::empty(431)),
            Err(RequestReadError::Invalid) => RequestAction::Respond(HttpResponse::empty(400)),
            Err(RequestReadError::Deadline) => RequestAction::Respond(HttpResponse::empty(408)),
            Err(RequestReadError::Shutdown) => return,
            Err(RequestReadError::Io(error)) => {
                tracing::debug!(%error, "local API client disconnected while sending a request");
                continue;
            }
        };
        match action {
            RequestAction::Respond(response) => {
                if let Err(error) = write_response(&mut stream, response) {
                    tracing::debug!(%error, "local API client disconnected");
                }
            }
            RequestAction::Prepare { model, selection } => {
                stream_model_prepare(
                    &mut stream,
                    model,
                    selection,
                    context.model_preparing.clone(),
                    &context.shutdown,
                    context.transcription.clone(),
                );
            }
            RequestAction::Transcribe {
                selection,
                content_length,
                body_prefix,
                admission,
                processing,
                timestamps,
            } => {
                let response = transcribe_audio(
                    &mut stream,
                    transcriptions::Admitted {
                        selection,
                        admission,
                        guard: processing,
                        timings: timestamps,
                    },
                    content_length,
                    body_prefix,
                    &context,
                );
                if let Some(response) = response
                    && let Err(error) = write_response(&mut stream, response)
                {
                    tracing::debug!(%error, "transcription client disconnected");
                }
            }
            RequestAction::DeveloperControl {
                content_length,
                body_prefix,
            } => {
                let response = control::developer_control(
                    &mut stream,
                    content_length,
                    body_prefix,
                    context.developer_control.as_ref(),
                    &context.shutdown,
                );
                if let Err(error) = write_response(&mut stream, response) {
                    tracing::debug!(%error, "developer control client disconnected");
                }
            }
            RequestAction::Control {
                content_length,
                body_prefix,
            } => {
                let response = control::control(
                    &mut stream,
                    content_length,
                    body_prefix,
                    context.control.as_ref(),
                    &context.shutdown,
                );
                if let Err(error) = write_response(&mut stream, response) {
                    tracing::debug!(%error, "control client disconnected");
                }
            }
            RequestAction::DictationStart {
                content_length,
                body_prefix,
            } => {
                let response = dictation::start(&mut stream, content_length, body_prefix, &context);
                if let Err(error) = write_response(&mut stream, response) {
                    tracing::debug!(%error, "dictation client disconnected");
                }
            }
            RequestAction::Dictation {
                id,
                owner_token,
                action,
            } => {
                dictation::handle(&mut stream, id, owner_token, action, &context);
            }
            RequestAction::Stream(action) => streams::handle(&mut stream, action, &context),
            RequestAction::MeetingEvents(subscription) => {
                meeting_events::stream(&mut stream, subscription, &context);
            }
            RequestAction::Connect(call) => {
                let response = match context.connect.as_ref() {
                    Some(connect) => connect.respond(call, &mut stream, &context.shutdown),
                    None => HttpResponse::code(403, "connect-unavailable"),
                };
                if let Err(error) = write_response(&mut stream, response, cors) {
                    tracing::debug!(%error, "connect client disconnected");
                }
            }
            RequestAction::Record(request) => {
                let response = request.respond(&context, &mut stream, &context.shutdown);
                if let Err(error) = write_response(&mut stream, response, cors) {
                    tracing::debug!(%error, "record client disconnected");
                }
            }
        }
    }
}

fn handle_request(
    request: HttpRequest,
    context: &HttpContext,
) -> (RequestAction, Option<CorsOrigin>) {
    // Labs › Connected apps answers any website on its own routes only; no
    // other route answers a browser.
    let connected = context.connected.and_then(|access| access.cors(&request));
    if request.method == "OPTIONS"
        && let Some(cors) = connected
    {
        return (
            RequestAction::Respond(HttpResponse::consent_preflight(cors.private_network)),
            Some(cors),
        );
    }
    (route(request, context), connected)
}

fn route(request: HttpRequest, context: &HttpContext) -> RequestAction {
    // Browser connect comes before the bearer token (`connect::intercept`).
    let request = match connect::intercept(request, context.connect.as_ref()) {
        connect::Intercept::Refuse(response) => return RequestAction::Respond(response),
        connect::Intercept::Connect(call) => return RequestAction::Connect(call),
        connect::Intercept::Audio(request) => {
            return transcription_action(
                request,
                &context.transcription,
                &context.processing,
                &context.timestamps,
                true,
            );
        }
        connect::Intercept::Pass(request) => request,
    };
    // One consented recording: an allowed page asks without the bearer token,
    // and its session token reaches only the record action it names.
    let request = match record::route(request, context) {
        record::Routed::Record(request) => return RequestAction::Record(request),
        record::Routed::Pass(request) => request,
    };
    let path = request.path.split('?').next().unwrap_or("/");
    if request.authorization.as_deref() != Some(&format!("Bearer {}", context.token)) {
        context
            .auth_limiter
            .observe(&context.events, &request.method, path);
        return RequestAction::Respond(HttpResponse::empty(401));
    }
    if request.method == "POST" && path == "/transcriptions" {
        return transcription_action(
            request,
            &context.transcription,
            &context.processing,
            &context.timestamps,
            false,
        );
    }
    if request.method == "POST" && path == "/dev/control" {
        return developer_control_action(request, context.developer_control.is_some());
    }
    if request.method == "POST" && path == "/control" {
        return json_post_action(
            request,
            context.control.is_some(),
            "control-unavailable",
            MAX_CONTROL_BYTES,
            "control-too-large",
            |content_length, body_prefix| RequestAction::Control {
                content_length,
                body_prefix,
            },
        );
    }
    if path == meeting_events::PATH {
        return meeting_events::action(&request, context);
    }
    if request.method == "POST" && path == "/streams" {
        return streams::create_action(request);
    }
    if let Some((id, action)) = streams::stream_path(path) {
        let action = action.to_owned();
        return streams::route(request, id, &action, context);
    }
    if request.method == "POST" && path == "/dictations" {
        return dictation_start_action(request, context.dictations.available());
    }
    if let Some((id, action)) = dictation_path(path) {
        let Some(owner_token) = request.dictation_token else {
            return RequestAction::Respond(HttpResponse::code(404, "dictation-not-found"));
        };
        let action = match (request.method.as_str(), action) {
            ("POST", "finish") => Some(DictationAction::Finish),
            ("POST", "cancel") => Some(DictationAction::Cancel),
            ("GET", "levels") => Some(DictationAction::Levels),
            ("GET", "partials") => Some(DictationAction::Partials),
            ("GET", "audio") => Some(DictationAction::Audio),
            ("POST", "heartbeat") => Some(DictationAction::Heartbeat),
            _ => None,
        };
        return match action {
            Some(action) if context.dictations.available() => RequestAction::Dictation {
                id,
                owner_token,
                action,
            },
            _ => RequestAction::Respond(HttpResponse::code(404, "service-capture-unavailable")),
        };
    }

    let response = match (request.method.as_str(), path) {
        ("GET", "/health") => HttpResponse::json(200, &(context.health)()),
        ("GET", "/capabilities") => HttpResponse::json(
            200,
            &CapabilitiesResponse {
                audio_formats: ["audio/wav"],
                partial_transcripts: false,
                service_capture: context.dictations.available(),
                dictation_partials: context.dictations.partials_available(),
                streams: true,
                output_processing: context.processing.available(),
                developer_control: context.developer_control.is_some(),
                control: context.control.is_some(),
                connect: context.connect.is_some(),
            },
        ),
        ("GET", "/profile/vitals") => match procwatch::snapshot() {
            Some(snapshot) => HttpResponse {
                status: 200,
                reason: reason(200),
                content_type: Some("application/json"),
                body: snapshot.to_json().into_bytes(),
            },
            None => HttpResponse::code(503, "vitals-unavailable"),
        },
        ("GET", "/models") => match model_infos(&request.path) {
            Ok(models) => HttpResponse::json(200, &models),
            Err(code) => HttpResponse::code(400, code),
        },
        ("POST", path) => {
            let Some(id) = model_prepare_path(path) else {
                return RequestAction::Respond(HttpResponse::code(404, "not_found"));
            };
            let Ok(id) = id.parse::<crate::transcription::models::TranscriptionModelId>() else {
                return RequestAction::Respond(HttpResponse::code(404, "unknown-model"));
            };
            let model = crate::transcription::models::definition(id);
            if !model.available() {
                return RequestAction::Respond(HttpResponse::code(404, "unknown-model"));
            }
            let Ok(selection) = installation_selection(&model, &request.path) else {
                return RequestAction::Respond(HttpResponse::code(400, "unsupported-model"));
            };
            if context
                .model_preparing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return RequestAction::Respond(HttpResponse::code(409, "prepare-busy"));
            }
            return RequestAction::Prepare { model, selection };
        }
        _ => HttpResponse::code(404, "not_found"),
    };
    RequestAction::Respond(response)
}

fn transcription_action(
    request: HttpRequest,
    transcription: &crate::transcription::service::TranscriptionServiceHandle,
    processing: &processing::Processing,
    timestamps: &timestamps::Timestamps,
    connect_grant: bool,
) -> RequestAction {
    // A browser connect grant is one plain transcription: no output
    // processing, no timings, nothing beyond the upload and the model.
    if connect_grant && let Some(response) = connect::connect_scope_refusal(&request.path) {
        return RequestAction::Respond(response);
    }
    let guard = if connect_grant {
        None
    } else {
        match processing::wanted(&request.path) {
            Ok(wanted) => match processing::gate(wanted, processing) {
                Ok(guard) => guard,
                Err(response) => return RequestAction::Respond(response),
            },
            Err(code) => return RequestAction::Respond(HttpResponse::code(400, code)),
        }
    };
    let requested = if connect_grant {
        crate::transcription::transcript::TimestampLevel::None
    } else {
        let requested = match timestamps::requested(&request.path) {
            Ok(requested) => requested,
            Err(code) => return RequestAction::Respond(HttpResponse::code(400, code)),
        };
        if let Err(response) = timestamps::gate(requested, timestamps) {
            return RequestAction::Respond(response);
        }
        requested
    };

    if !request.has_content_type("audio/wav") {
        return RequestAction::Respond(HttpResponse::code(415, "unsupported-audio"));
    }
    let Some(content_length) = request.content_length else {
        return RequestAction::Respond(HttpResponse::code(411, "length-required"));
    };
    if content_length > MAX_AUDIO_BYTES {
        return RequestAction::Respond(HttpResponse::code(413, "resource-exhausted"));
    }
    let selection = match transcription_selection(&request.path) {
        Ok(selection) => selection,
        Err(code) => {
            return RequestAction::Respond(HttpResponse::code(400, code));
        }
    };
    let model = crate::transcription::models::definition(selection.model);
    if !crate::transcription::readiness::is_ready(&model, &selection.language) {
        return RequestAction::Respond(HttpResponse::code(409, "model-not-ready"));
    }
    let Some(admission) = transcription.admit_audio() else {
        return RequestAction::Respond(HttpResponse::code(429, "queue-full"));
    };
    RequestAction::Transcribe {
        selection,
        content_length,
        body_prefix: request.body_prefix,
        admission,
        processing: guard,
        timestamps: requested,
    }
}

fn developer_control_action(request: HttpRequest, available: bool) -> RequestAction {
    json_post_action(
        request,
        available,
        "developer-control-unavailable",
        MAX_DEVELOPER_CONTROL_BYTES,
        "developer-control-too-large",
        |content_length, body_prefix| RequestAction::DeveloperControl {
            content_length,
            body_prefix,
        },
    )
}

fn dictation_start_action(request: HttpRequest, available: bool) -> RequestAction {
    json_post_action(
        request,
        available,
        "service-capture-unavailable",
        MAX_DICTATION_SOURCE_BYTES + 32,
        "source-too-large",
        |content_length, body_prefix| RequestAction::DictationStart {
            content_length,
            body_prefix,
        },
    )
}

fn json_post_action(
    request: HttpRequest,
    available: bool,
    unavailable_code: &'static str,
    max_bytes: usize,
    too_large_code: &'static str,
    build: impl FnOnce(usize, Vec<u8>) -> RequestAction,
) -> RequestAction {
    if !available {
        return RequestAction::Respond(HttpResponse::code(404, unavailable_code));
    }
    if !request.has_content_type("application/json") {
        return RequestAction::Respond(HttpResponse::code(415, "unsupported-content-type"));
    }
    let Some(content_length) = request.content_length else {
        return RequestAction::Respond(HttpResponse::code(411, "length-required"));
    };
    if content_length > max_bytes {
        return RequestAction::Respond(HttpResponse::code(413, too_large_code));
    }
    build(content_length, request.body_prefix)
}

fn dictation_path(path: &str) -> Option<(u64, &str)> {
    let mut parts = path.strip_prefix("/dictations/")?.split('/');
    let id = parts.next()?.parse().ok()?;
    let action = parts.next()?;
    (parts.next().is_none()).then_some((id, action))
}

fn transcription_selection(
    path: &str,
) -> std::result::Result<crate::transcription::models::TranscriptionSelection, &'static str> {
    let query = selection_query(path)?;
    let model = query.model.ok_or("unknown-model")?;
    validate_selection(model, query.into_language())
}

fn validate_selection(
    model: crate::transcription::models::TranscriptionModelId,
    language: String,
) -> std::result::Result<crate::transcription::models::TranscriptionSelection, &'static str> {
    let selection = crate::transcription::models::TranscriptionSelection { model, language };
    crate::transcription::models::validate(&selection).map_err(|_| "unsupported-model")?;
    Ok(selection)
}

#[derive(Default)]
struct SelectionQuery {
    model: Option<crate::transcription::models::TranscriptionModelId>,
    language: Option<String>,
}

impl SelectionQuery {
    fn into_language(self) -> String {
        self.language.unwrap_or_else(|| {
            crate::transcription::models::TranscriptionSelection::default().language
        })
    }
}

fn selection_query(path: &str) -> std::result::Result<SelectionQuery, &'static str> {
    let query = path
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default();
    let mut selection = SelectionQuery::default();
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match name.as_ref() {
            "model" if selection.model.is_none() => selection.model = value.parse().ok(),
            "language" if selection.language.is_none() => {
                selection.language = Some(value.into_owned());
            }
            "model" | "language" => return Err("invalid-request"),
            _ => {}
        }
    }
    Ok(selection)
}

fn client_aborted(stream: &TcpStream) -> bool {
    let mut byte = [0_u8; 1];
    match stream.peek(&mut byte) {
        // A read-side EOF can be a valid HTTP client half-close; only a socket
        // error proves the client cannot receive the response.
        Ok(_) => false,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            false
        }
        Err(_) => true,
    }
}

fn model_prepare_path(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/models/")?.strip_suffix("/prepare")?;
    (!id.is_empty() && !id.contains('/')).then_some(id)
}

fn model_infos(path: &str) -> std::result::Result<Vec<ModelInfo>, &'static str> {
    let language = selection_query(path)?.into_language();
    if !crate::transcription::models::is_known_language(&language) {
        return Err("unsupported-language");
    }
    Ok(crate::transcription::readiness::catalog_models(&language)
        .into_iter()
        .map(|model| ModelInfo {
            id: model.id_str,
            name: model.name,
            installed: model.readiness.installed,
            verified: model.readiness.verified,
            managed: model.managed,
            download_bytes: model.download_bytes,
            languages: model.languages,
            supports_language_detection: model.supports_language_detection,
            source: model.source,
        })
        .collect())
}

fn installation_selection(
    model: &crate::transcription::models::ModelDefinition,
    path: &str,
) -> std::result::Result<crate::transcription::models::TranscriptionSelection, &'static str> {
    let query = selection_query(path)?;
    validate_selection(model.id, query.into_language())
}

fn stream_model_prepare(
    stream: &mut TcpStream,
    model: crate::transcription::models::ModelRef,
    selection: crate::transcription::models::TranscriptionSelection,
    preparing: Arc<AtomicBool>,
    shutdown: &AtomicBool,
    transcription: crate::transcription::service::TranscriptionServiceHandle,
) {
    let preparer: ModelPreparer = Box::new({
        let model = model.clone();
        move |canceled, downloaded, stage| {
            if model.runtime.is_gguf() {
                crate::transcription::models::download_with_stage_progress(
                    &model,
                    &canceled,
                    &downloaded,
                    &stage,
                )?;
            }
            if canceled.load(Ordering::Acquire) {
                bail!("model preparation canceled");
            }
            crate::transcription::models::ModelPreparationStage::Loading.store(&stage);
            transcription
                .prepare(selection, canceled)
                .map_err(|error| eyre!("model preparation failed: {error:?}"))
        }
    });
    stream_model_prepare_with(stream, model, preparing, shutdown, preparer);
}

fn stream_model_prepare_with(
    stream: &mut TcpStream,
    model: crate::transcription::models::ModelRef,
    preparing: Arc<AtomicBool>,
    shutdown: &AtomicBool,
    preparer: ModelPreparer,
) {
    if let Err(error) = write_sse_headers(stream) {
        preparing.store(false, Ordering::Release);
        tracing::debug!(%error, "model preparation client disconnected before streaming");
        return;
    }
    let canceled = Arc::new(AtomicBool::new(false));
    let downloaded = Arc::new(AtomicU64::new(0));
    let initial_stage = if matches!(
        &model.runtime,
        crate::transcription::models::ModelRuntime::AppleSpeech
    ) {
        crate::transcription::models::ModelPreparationStage::Loading as u8
    } else {
        crate::transcription::models::ModelPreparationStage::Downloading as u8
    };
    let stage = Arc::new(AtomicU8::new(initial_stage));
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let guard = PrepareGuard(preparing);
    let worker = thread::spawn({
        let canceled = canceled.clone();
        let downloaded = downloaded.clone();
        let stage = stage.clone();
        move || {
            let _guard = guard;
            let result = preparer(canceled, downloaded, stage);
            let _ = result_sender.send(result);
        }
    });

    let mut last_stage = None;
    let mut last_downloaded = u64::MAX;
    let mut last_heartbeat = Instant::now();
    let total_bytes = model.download_bytes().unwrap_or(0);
    let mut connected = true;
    let mut finished = None;
    let result = loop {
        if shutdown.load(Ordering::Acquire) {
            canceled.store(true, Ordering::Release);
            connected = false;
            break None;
        }
        let current_stage = crate::transcription::models::ModelPreparationStage::load(&stage);
        let current_downloaded = downloaded.load(Ordering::Relaxed);
        let progress = match current_stage {
            crate::transcription::models::ModelPreparationStage::Downloading
                if last_stage != Some(current_stage) || last_downloaded != current_downloaded =>
            {
                Some(serde_json::json!({
                    "type": "downloading",
                    "downloadedBytes": current_downloaded,
                    "totalBytes": total_bytes,
                }))
            }
            crate::transcription::models::ModelPreparationStage::Verifying
                if last_stage != Some(current_stage) =>
            {
                Some(serde_json::json!({ "type": "verifying" }))
            }
            crate::transcription::models::ModelPreparationStage::Loading
                if last_stage != Some(current_stage) =>
            {
                Some(serde_json::json!({ "type": "loading" }))
            }
            _ => None,
        };
        if let Some(progress) = progress
            && let Err(error) = write_sse(stream, &progress)
        {
            tracing::debug!(%error, "model preparation client disconnected");
            canceled.store(true, Ordering::Release);
            connected = false;
            break None;
        }
        last_stage = Some(current_stage);
        last_downloaded = current_downloaded;
        // The last stage has been reported, so a finished result ends here.
        if let Some(result) = finished.take() {
            break Some(result);
        }
        if last_heartbeat.elapsed() >= Duration::from_secs(5) {
            if stream.write_all(b": keep-alive\n\n").is_err() || stream.flush().is_err() {
                canceled.store(true, Ordering::Release);
                connected = false;
                break None;
            }
            last_heartbeat = Instant::now();
        }
        match result_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => finished = Some(result),
            Err(RecvTimeoutError::Disconnected) => {
                finished = Some(Err(eyre!("model preparer stopped unexpectedly")));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    };
    if worker.join().is_err() {
        tracing::error!(model = model.id.as_str(), "model preparer panicked");
        if connected {
            let _ = write_sse(
                stream,
                &model_prepare_error("internal", "Model preparation stopped unexpectedly."),
            );
        }
        return;
    }
    let Some(result) = result else {
        return;
    };
    match result {
        Ok(()) => {
            let _ = write_sse(stream, &serde_json::json!({ "type": "ok" }));
        }
        Err(error) if canceled.load(Ordering::Acquire) => {
            tracing::debug!(%error, model = model.id.as_str(), "model preparation canceled");
            let _ = write_sse(
                stream,
                &model_prepare_error("cancelled", "Model preparation was cancelled."),
            );
        }
        Err(error) => {
            let failed_stage = crate::transcription::models::ModelPreparationStage::load(&stage);
            let code = match failed_stage {
                crate::transcription::models::ModelPreparationStage::Downloading => {
                    "download-failed"
                }
                crate::transcription::models::ModelPreparationStage::Verifying => {
                    "verification-failed"
                }
                crate::transcription::models::ModelPreparationStage::Loading => "load-failed",
            };
            tracing::error!(%error, model = model.id.as_str(), code, "model preparation failed");
            let _ = write_sse(
                stream,
                &model_prepare_error(code, "The model could not be prepared."),
            );
        }
    }
}

fn model_prepare_error(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "error",
        "error": { "code": code, "message": message },
    })
}

impl AuthObservationLimiter {
    fn observe(&self, events: &EventLog, method: &str, path: &str) {
        let now = Instant::now();
        let mut last = self.last.lock().unwrap_or_else(|error| error.into_inner());
        if last.is_some_and(|last| now.duration_since(last) < AUTH_OBSERVATION_INTERVAL) {
            return;
        }
        *last = Some(now);
        drop(last);
        if let Err(error) = events.emit(&VoiceEvent::ApiAuthFailed {
            timestamp_ms: now_ms(),
            method: method.into(),
            path: path.into(),
        }) {
            tracing::error!(%error, "could not record local API authentication failure");
        }
    }
}

enum RequestAction {
    Respond(HttpResponse),
    Prepare {
        model: crate::transcription::models::ModelRef,
        selection: crate::transcription::models::TranscriptionSelection,
    },
    Transcribe {
        selection: crate::transcription::models::TranscriptionSelection,
        content_length: usize,
        body_prefix: Vec<u8>,
        admission: crate::transcription::service::AudioAdmission,
        /// `process=global` was asked for; the guard holds its one slot.
        processing: Option<processing::Guard>,
        /// `timestamps=segment|word` was asked for, and the lab allows it.
        timestamps: crate::transcription::transcript::TimestampLevel,
    },
    DeveloperControl {
        content_length: usize,
        body_prefix: Vec<u8>,
    },
    Control {
        content_length: usize,
        body_prefix: Vec<u8>,
    },
    DictationStart {
        content_length: usize,
        body_prefix: Vec<u8>,
    },
    Dictation {
        id: u64,
        owner_token: String,
        action: DictationAction,
    },
    Stream(streams::StreamAction),
    MeetingEvents(crate::meeting::finished_events::Subscription),
    /// A browser page's connect request, answered after its body arrives.
    Connect(connect::Call),
    /// A consented-recording request, answered after its body arrives.
    Record(record::Request),
}

#[derive(Clone, Copy, Debug)]
enum DictationAction {
    Finish,
    Cancel,
    Levels,
    Partials,
    Audio,
    Heartbeat,
}

struct PrepareGuard(Arc<AtomicBool>);

impl Drop for PrepareGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelInfo {
    id: String,
    name: std::borrow::Cow<'static, str>,
    installed: bool,
    verified: bool,
    managed: bool,
    download_bytes: Option<u64>,
    languages: Vec<&'static str>,
    supports_language_detection: bool,
    /// `found: owner/name@abcdef1` (another app's download, used where it
    /// is), `imported`, `hex`, or `download`.
    source: String,
}

fn current_health() -> HealthResponse {
    HealthResponse {
        version: crate::VERSION,
        api_version: API_VERSION,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::dictation::service::DictationControl;

    mod connected_apps;
    mod requester_icons;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    /// How long a test waits on another thread before failing instead of hanging.
    const TEST_WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn authenticated_server_reports_live_health_and_cleans_up_discovery() {
        let directory = temp_directory();
        let discovery_path = directory.join("local-api.json");
        let event_path = directory.join("events.ndjson");
        let events = EventLog::create(&event_path).unwrap();
        let mut api = start_published(discovery_path.clone(), events).unwrap();
        let document: DiscoveryDocument =
            serde_json::from_slice(&fs::read(&discovery_path).unwrap()).unwrap();

        let unauthorized = request(document.port, None, "/health");
        assert!(unauthorized.starts_with("HTTP/1.1 401"));
        assert_eq!(response_body(&unauthorized), "");
        let wrong_token = request(document.port, Some("hex_wrong"), "/health");
        assert!(wrong_token.starts_with("HTTP/1.1 401"));
        let preflight = raw_request(
            document.port,
            "OPTIONS /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        );
        assert!(preflight.starts_with("HTTP/1.1 401"));

        let first = request(document.port, Some(&document.token), "/health");
        assert!(first.starts_with("HTTP/1.1 200"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(response_body(&first)).unwrap()["apiVersion"],
            "2"
        );
        let capabilities = request(document.port, Some(&document.token), "/capabilities");
        let capabilities: serde_json::Value =
            serde_json::from_str(response_body(&capabilities)).unwrap();
        assert_eq!(capabilities["partialTranscripts"], false);
        assert_eq!(capabilities["serviceCapture"], false);
        assert_eq!(capabilities["developerControl"], false);
        assert_eq!(capabilities["control"], false);
        assert_eq!(capabilities["outputProcessing"], false);
        assert_eq!(capabilities["audioFormats"][0], "audio/wav");
        let vitals = request(document.port, Some(&document.token), "/profile/vitals");
        #[cfg(target_os = "macos")]
        {
            assert!(vitals.starts_with("HTTP/1.1 200"));
            let vitals: serde_json::Value = serde_json::from_str(response_body(&vitals)).unwrap();
            assert!(vitals["footprint"].as_u64().unwrap() > 0);
            assert!(vitals["heap"]["live"].as_u64().unwrap() > 0);
        }
        // procwatch reads a process's footprint only on macOS.
        #[cfg(not(target_os = "macos"))]
        assert!(vitals.starts_with("HTTP/1.1 503"));
        let models = request(document.port, Some(&document.token), "/models");
        let models: Vec<serde_json::Value> = serde_json::from_str(response_body(&models)).unwrap();
        assert_eq!(
            models.len(),
            crate::transcription::models::MODELS
                .iter()
                .filter(|model| model.available())
                .count()
        );
        assert_eq!(models[0]["id"], "parakeet_unified_en");
        assert!(models[0]["downloadBytes"].is_number());
        assert_eq!(models[0]["supportsLanguageDetection"], false);
        let whisper = models
            .iter()
            .find(|model| model["id"] == "whisper_large_v3_turbo")
            .unwrap();
        assert_eq!(whisper["supportsLanguageDetection"], true);
        assert!(
            whisper["languages"]
                .as_array()
                .unwrap()
                .contains(&"auto".into())
        );
        assert!(models.iter().all(|model| model["id"] != "apple_speech"));
        let unknown_model = request_with_method(
            document.port,
            Some(&document.token),
            "POST",
            "/models/unknown/prepare",
        );
        assert!(unknown_model.starts_with("HTTP/1.1 404"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(response_body(&unknown_model)).unwrap()["code"],
            "unknown-model"
        );
        let disabled_model = request_with_method(
            document.port,
            Some(&document.token),
            "POST",
            "/models/apple_speech/prepare?language=en",
        );
        assert!(disabled_model.starts_with("HTTP/1.1 404"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(response_body(&disabled_model)).unwrap()["code"],
            "unknown-model"
        );

        api.shutdown();
        assert!(!discovery_path.exists());
        let observations = fs::read_to_string(event_path).unwrap();
        assert!(observations.contains("api_server_started"));
        assert!(observations.contains("api_auth_failed"));
        assert_eq!(observations.matches("api_auth_failed").count(), 1);
        assert!(observations.contains("api_server_stopped"));
        assert!(!observations.contains(&document.token));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn authenticated_developer_control_is_bounded_and_typed() {
        let directory = temp_directory();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let (sender, controls) = mpsc::sync_channel(1);
        let mut api = start_unpublished(
            events,
            LocalApiOptions {
                developer_control: Some(sender),
                ..Default::default()
            },
        )
        .unwrap();
        let port = api.document.port;
        let token = api.document.token.clone();
        let client = thread::spawn(move || {
            let body = r#"{"command":"hud","state":"recording"}"#;
            raw_request(
                port,
                &format!(
                    "POST /dev/control HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ),
            )
        });

        let call = controls.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            call.command,
            crate::control::developer_control::DeveloperCommand::Hud {
                state: crate::control::developer_control::DeveloperHudState::Recording
            }
        ));
        call.reply
            .send(crate::control::developer_control::DeveloperReply::Ok)
            .unwrap();
        let response = client.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert_eq!(response_body(&response), r#"{"status":"ok"}"#);

        api.shutdown();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn control_requests_are_authenticated_versioned_and_typed() {
        use crate::control::{ControlCommand, ControlReply, ControlResponse};

        let directory = temp_directory();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let (sender, controls) = mpsc::sync_channel(1);
        let mut api = start_unpublished(
            events,
            LocalApiOptions {
                control: Some(sender),
                ..Default::default()
            },
        )
        .unwrap();
        let port = api.document.port;
        let token = api.document.token.clone();
        fn post_to(port: u16, token: &str, body: &str) -> String {
            raw_request(
                port,
                &format!(
                    "POST /control HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ),
            )
        }
        let post = |token: &str, body: &str| post_to(port, token, body);

        let client_token = token.clone();
        let client = thread::spawn(move || {
            post_to(
                port,
                &client_token,
                r#"{"version":4,"command":"meeting-record","title":"Standup"}"#,
            )
        });
        let call = controls.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            call.request.command,
            ControlCommand::MeetingRecord {
                title: Some("Standup".into())
            }
        );
        call.reply
            .send(ControlReply::done("Started recording").into())
            .unwrap();
        let response = client.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert_eq!(
            serde_json::from_str::<ControlResponse>(response_body(&response))
                .unwrap()
                .reply,
            ControlReply::done("Started recording")
        );

        let unauthorized = post("hex_wrong", r#"{"version":4,"command":"status"}"#);
        assert!(unauthorized.starts_with("HTTP/1.1 401"));
        let unknown = post(&token, r#"{"version":4,"command":"explode"}"#);
        assert!(unknown.starts_with("HTTP/1.1 400"));
        // Another protocol version is answered without reaching the app.
        for body in [
            r#"{"version":1,"command":"status"}"#,
            r#"{"version":1,"command":"explode"}"#,
        ] {
            let mismatch = post(&token, body);
            let reply = serde_json::from_str::<ControlResponse>(response_body(&mismatch))
                .unwrap()
                .reply;
            assert!(
                matches!(&reply, ControlReply::Error { code, .. } if code == "version-mismatch"),
                "{reply:?}"
            );
        }
        let too_large = raw_request(
            port,
            &format!(
                "POST /control HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_CONTROL_BYTES + 1
            ),
        );
        assert!(too_large.starts_with("HTTP/1.1 413"));
        assert!(controls.try_recv().is_err());

        api.shutdown();
        fs::remove_dir_all(directory).unwrap();
    }

    /// The next `/dictations` control the capture loop receives.
    #[cfg(target_os = "macos")]
    fn next_dictation_control(
        controls: &mpsc::Receiver<crate::recognition::RecognitionControl>,
    ) -> DictationControl {
        match controls.recv_timeout(TEST_WAIT).unwrap() {
            crate::recognition::RecognitionControl::Dictation(control) => control,
            control => panic!("expected a dictation control, got {control:?}"),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn next_dictation_control(controls: &mpsc::Receiver<DictationControl>) -> DictationControl {
        controls.recv_timeout(TEST_WAIT).unwrap()
    }

    #[test]
    fn authenticated_running_app_dictation_streams_observations_and_returns_raw_text() {
        let directory = temp_directory();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let (sender, controls) = mpsc::sync_channel(4);
        let mut api = start_unpublished(
            events,
            LocalApiOptions {
                dictation_control: Some(sender),
                ..Default::default()
            },
        )
        .unwrap();
        let port = api.document.port;
        let token = api.document.token.clone();
        let (attached, audio_attached) = mpsc::channel();
        let controller = thread::spawn(move || {
            let DictationControl::Start {
                source,
                owner_token,
                levels,
                reply,
                ..
            } = next_dictation_control(&controls)
            else {
                panic!("expected dictation start")
            };
            assert_eq!(source, "tp7");
            assert!(owner_token.starts_with("hex_"));
            reply
                .send(Ok(crate::dictation::service::ProgrammaticDictationStart {
                    id: 7,
                    sample_rate: 48_000,
                }))
                .unwrap();
            levels
                .send(crate::dictation::service::DictationLevel {
                    rms_db: -24.5,
                    peak_db: -8.0,
                })
                .unwrap();

            let DictationControl::Heartbeat {
                id,
                owner_token: heartbeat_token,
                reply,
            } = next_dictation_control(&controls)
            else {
                panic!("expected dictation heartbeat")
            };
            assert_eq!(id, 7);
            assert_eq!(heartbeat_token, owner_token);
            reply.send(Ok(())).unwrap();

            let DictationControl::AttachAudio {
                id,
                owner_token: audio_token,
                audio,
                reply,
            } = next_dictation_control(&controls)
            else {
                panic!("expected dictation audio subscription")
            };
            assert_eq!(id, 7);
            assert_eq!(audio_token, owner_token);
            reply.send(Ok(())).unwrap();
            audio
                .send(crate::dictation::service::DictationAudioChunk {
                    samples: vec![0.25, -0.5],
                })
                .unwrap();
            attached.send(()).unwrap();

            let DictationControl::Finish {
                id,
                owner_token: finish_token,
                reply,
            } = next_dictation_control(&controls)
            else {
                panic!("expected dictation finish")
            };
            assert_eq!(id, 7);
            assert_eq!(finish_token, owner_token);
            reply
                .send(Ok(crate::dictation::service::ProgrammaticDictationResult {
                    transcript: "raw words".into(),
                    duration_ms: 875,
                }))
                .unwrap();
            drop(levels);
            drop(audio);
        });

        let body = r#"{"source":"tp7"}"#;
        let start = raw_request(
            port,
            &format!(
                "POST /dictations HTTP/1.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(start.starts_with("HTTP/1.1 201"));
        let started = serde_json::from_str::<serde_json::Value>(response_body(&start)).unwrap();
        assert_eq!(started["id"], 7);
        assert_eq!(started["sampleRate"], 48_000);
        let owner_token = started["ownerToken"].as_str().unwrap().to_owned();
        assert!(owner_token.starts_with("hex_"));

        let missing_owner = request_with_method(port, Some(&token), "POST", "/dictations/7/finish");
        assert!(missing_owner.starts_with("HTTP/1.1 404"));

        let heartbeat = request_with_dictation_token(
            port,
            &token,
            &owner_token,
            "POST",
            "/dictations/7/heartbeat",
        );
        assert!(heartbeat.starts_with("HTTP/1.1 204"));

        let wrong_levels =
            request_with_dictation_token(port, &token, "hex_wrong", "GET", "/dictations/7/levels");
        assert!(wrong_levels.starts_with("HTTP/1.1 404"));

        let level_token = token.clone();
        let level_owner = owner_token.clone();
        let (subscribed, levels_subscribed) = mpsc::channel();
        let levels = thread::spawn(move || {
            // The level sent at start arrives once the stream has subscribed.
            raw_request_signalling(
                port,
                &format!(
                    "GET /dictations/7/levels HTTP/1.1\r\nAuthorization: Bearer {level_token}\r\nX-Hex-Dictation-Token: {level_owner}\r\nConnection: close\r\n\r\n"
                ),
                b"rmsDb",
                subscribed,
            )
        });
        let audio_token = token.clone();
        let audio_owner = owner_token.clone();
        let audio = thread::spawn(move || {
            request_bytes(port, &audio_token, &audio_owner, "/dictations/7/audio")
        });
        levels_subscribed.recv_timeout(TEST_WAIT).unwrap();
        audio_attached.recv_timeout(TEST_WAIT).unwrap();
        let finish = request_with_dictation_token(
            port,
            &token,
            &owner_token,
            "POST",
            "/dictations/7/finish",
        );
        assert!(finish.starts_with("HTTP/1.1 200"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(response_body(&finish)).unwrap(),
            serde_json::json!({ "transcript": "raw words", "durationMs": 875 })
        );
        let levels = String::from_utf8(levels.join().unwrap()).unwrap();
        assert!(levels.starts_with("HTTP/1.1 200"));
        assert!(levels.contains(r#""rmsDb":-24.5"#));
        assert!(levels.contains(r#""peakDb":-8.0"#));
        let audio = audio.join().unwrap();
        let body = audio
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .map(|index| &audio[index + 4..])
            .unwrap();
        assert_eq!(
            body,
            [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat()
        );

        controller.join().unwrap();
        api.shutdown();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn embedded_server_returns_its_endpoint_without_publishing_discovery() {
        let directory = temp_directory();
        let event_path = directory.join("events.ndjson");
        let discovery_path = directory.join("local-api.json");
        let events = EventLog::create(&event_path).unwrap();
        let mut api = start_unpublished(events, LocalApiOptions::default()).unwrap();
        let endpoint = api.embedded_endpoint();

        assert_eq!(endpoint.kind, "ready");
        assert_eq!(
            endpoint.url,
            format!("http://127.0.0.1:{}", api.document.port)
        );
        assert_eq!(endpoint.token, api.document.token);
        assert_eq!(endpoint.api_version, API_VERSION);
        assert_eq!(endpoint.pid, std::process::id());
        assert!(!discovery_path.exists());

        let health = request(api.document.port, Some(&endpoint.token), "/health");
        assert!(health.starts_with("HTTP/1.1 200"));
        api.shutdown();
        assert!(!discovery_path.exists());
        assert!(
            !fs::read_to_string(event_path)
                .unwrap()
                .contains(&endpoint.token)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn model_preparation_rejects_disabled_apple_speech() {
        let apple = crate::transcription::models::definition(
            crate::transcription::models::TranscriptionModelId::AppleSpeech,
        );
        assert!(
            installation_selection(&apple, "/models/apple_speech/prepare?language=de").is_err()
        );
    }

    #[test]
    fn model_selection_accepts_auto_only_for_detection_models() {
        assert!(
            transcription_selection("/transcriptions?model=whisper_large_v3_turbo&language=auto")
                .is_ok()
        );
        assert!(
            transcription_selection("/transcriptions?model=parakeet_v2&language=auto").is_err()
        );
    }

    #[test]
    fn embedded_hosts_have_independent_authenticated_endpoints() {
        let directory = temp_directory();
        let first_events = EventLog::create(&directory.join("first.ndjson")).unwrap();
        let second_events = EventLog::create(&directory.join("second.ndjson")).unwrap();
        let first = start_unpublished(first_events, LocalApiOptions::default()).unwrap();
        let second = start_unpublished(second_events, LocalApiOptions::default()).unwrap();
        let first_endpoint = first.embedded_endpoint();
        let second_endpoint = second.embedded_endpoint();

        assert_ne!(first.document.port, second.document.port);
        assert_ne!(first_endpoint.token, second_endpoint.token);
        assert!(
            request(first.document.port, Some(&first_endpoint.token), "/health")
                .starts_with("HTTP/1.1 200")
        );
        assert!(
            request(
                second.document.port,
                Some(&second_endpoint.token),
                "/health"
            )
            .starts_with("HTTP/1.1 200")
        );
        assert!(
            request(first.document.port, Some(&second_endpoint.token), "/health")
                .starts_with("HTTP/1.1 401")
        );

        drop(first);
        drop(second);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn discovery_is_owner_only_and_stale_files_are_replaced() {
        let directory = temp_directory();
        let discovery_path = directory.join("local-api.json");
        fs::write(&discovery_path, b"not json").unwrap();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let api = start_published(discovery_path.clone(), events).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&discovery_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        drop(api);
        assert!(!discovery_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn current_pid_with_a_closed_port_is_stale() {
        let directory = temp_directory();
        let discovery_path = directory.join("local-api.json");
        let stale = DiscoveryDocument {
            port: unused_port(),
            token: "hex_0000000000000000000000000000000000000000000000000000000000000000".into(),
            api_version: API_VERSION.into(),
            pid: std::process::id(),
        };
        write_discovery(&discovery_path, &stale).unwrap();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let api = start_published(discovery_path.clone(), events).unwrap();
        let current: DiscoveryDocument =
            serde_json::from_slice(&fs::read(&discovery_path).unwrap()).unwrap();
        assert_ne!(current.token, stale.token);
        drop(api);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn old_server_does_not_remove_newer_discovery_generation() {
        let directory = temp_directory();
        let discovery_path = directory.join("local-api.json");
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let api = start_published(discovery_path.clone(), events).unwrap();
        let replacement = DiscoveryDocument {
            port: 1234,
            token: "hex_replacement".into(),
            api_version: API_VERSION.into(),
            pid: std::process::id(),
        };
        write_discovery(&discovery_path, &replacement).unwrap();
        drop(api);
        assert_eq!(
            serde_json::from_slice::<DiscoveryDocument>(&fs::read(&discovery_path).unwrap())
                .unwrap(),
            replacement
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn live_discovery_cannot_be_replaced() {
        let directory = temp_directory();
        let discovery_path = directory.join("local-api.json");
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let api = start_published(discovery_path.clone(), events.clone()).unwrap();
        assert!(start_published(discovery_path.clone(), events).is_err());
        drop(api);
        assert!(!discovery_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn oversized_headers_are_rejected_without_reaching_auth() {
        let directory = temp_directory();
        let discovery_path = directory.join("local-api.json");
        let event_path = directory.join("events.ndjson");
        let events = EventLog::create(&event_path).unwrap();
        let api = start_published(discovery_path.clone(), events).unwrap();
        let document: DiscoveryDocument =
            serde_json::from_slice(&fs::read(&discovery_path).unwrap()).unwrap();
        let response = raw_request(
            document.port,
            &format!(
                "GET /health HTTP/1.1\r\nX-Oversized: {}\r\n\r\n",
                "a".repeat(MAX_HEADER_BYTES)
            ),
        );
        assert!(response.starts_with("HTTP/1.1 431"));
        assert!(
            !fs::read_to_string(&event_path)
                .unwrap()
                .contains("api_auth_failed")
        );
        drop(api);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn request_parser_preserves_bounded_body_bytes_and_metadata() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .write_all(
                    b"POST /transcriptions?model=parakeet_v2 HTTP/1.1\r\nContent-Type: audio/wav\r\nContent-Length: 3\r\n\r\nabc",
                )
                .unwrap();
        });
        let (mut stream, _) = listener.accept().unwrap();

        let request = read_request(&mut stream, &AtomicBool::new(false)).unwrap();

        client.join().unwrap();
        assert_eq!(request.content_type.as_deref(), Some("audio/wav"));
        assert_eq!(request.content_length, Some(3));
        assert_eq!(request.body_prefix, b"abc");
    }

    #[test]
    fn request_parser_observes_absolute_deadline_and_shutdown() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let first_client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut first, _) = listener.accept().unwrap();
        assert!(matches!(
            read_request_until(&mut first, &AtomicBool::new(false), Instant::now()),
            Err(RequestReadError::Deadline)
        ));
        drop(first_client);

        let second_client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut second, _) = listener.accept().unwrap();
        assert!(matches!(
            read_request(&mut second, &AtomicBool::new(true)),
            Err(RequestReadError::Shutdown)
        ));
        drop(second_client);
    }

    #[test]
    fn transcription_route_requires_wav_and_a_safe_declared_length() {
        let (_service, transcription) =
            crate::transcription::service::TranscriptionService::start().unwrap();
        let action = transcription_action(
            HttpRequest {
                method: "POST".into(),
                path: "/transcriptions?model=parakeet_v2".into(),
                authorization: None,
                dictation_token: None,
                stream_token: None,
                connect_token: None,
                record_grant: None,
                record_token: None,
                content_type: Some("audio/webm".into()),
                content_length: Some(10),
                body_prefix: Vec::new(),
            },
            &transcription,
            &Processing::default(),
            &Timestamps::default(),
            false,
        );
        let RequestAction::Respond(response) = action else {
            panic!("unsupported audio must be rejected before upload");
        };
        assert_eq!(response.status, 415);

        let action = transcription_action(
            HttpRequest {
                method: "POST".into(),
                path: "/transcriptions?model=parakeet_v2".into(),
                authorization: None,
                dictation_token: None,
                stream_token: None,
                connect_token: None,
                record_grant: None,
                record_token: None,
                content_type: Some("audio/wav".into()),
                content_length: Some(MAX_AUDIO_BYTES + 1),
                body_prefix: Vec::new(),
            },
            &transcription,
            &Processing::default(),
            &Timestamps::default(),
            false,
        );
        let RequestAction::Respond(response) = action else {
            panic!("unsafe body size must be rejected before upload");
        };
        assert_eq!(response.status, 413);
    }

    #[test]
    fn concurrent_model_preparation_is_rejected_before_streaming() {
        let directory = temp_directory();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let preparing = Arc::new(AtomicBool::new(true));
        let health: HealthProvider = Arc::new(test_health);
        let (_service, transcription) =
            crate::transcription::service::TranscriptionService::start().unwrap();
        let context = HttpContext {
            token: "test-token".into(),
            health,
            events,
            auth_limiter: AuthObservationLimiter::default(),
            model_preparing: preparing,
            shutdown: Arc::new(AtomicBool::new(false)),
            transcription,
            developer_control: None,
            control: None,
            dictations: dictation::Dictations::new(None),
            streams: Arc::default(),
            stream_engine: Arc::new(crate::transcription::stream::shared_engine),
            meeting_events: meeting_events::Source::default(),
            processing: Processing::default(),
            timestamps: Timestamps::default(),
            connected: None,
            connect: None,
            record: None,
        };
        let action = handle_request(
            HttpRequest {
                method: "POST".into(),
                path: "/models/parakeet_v2/prepare".into(),
                authorization: Some("Bearer test-token".into()),
                dictation_token: None,
                stream_token: None,
                connect_token: None,
                record_grant: None,
                record_token: None,
                content_type: None,
                content_length: None,
                body_prefix: Vec::new(),
            },
            &context,
        );
        let RequestAction::Respond(response) = action else {
            panic!("busy preparation must return a buffered conflict");
        };
        assert_eq!(response.status, 409);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["code"],
            "prepare-busy"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn model_prepare_stream_orders_progress_and_terminal_success() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        });
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let preparing = Arc::new(AtomicBool::new(true));
        // Each stage outlasts several 100 ms progress polls, so a busy machine
        // can't skip one.
        let preparer: ModelPreparer = Box::new(|_, downloaded, stage| {
            downloaded.store(123, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(400));
            crate::transcription::models::ModelPreparationStage::Verifying.store(&stage);
            thread::sleep(Duration::from_millis(400));
            crate::transcription::models::ModelPreparationStage::Loading.store(&stage);
            thread::sleep(Duration::from_millis(400));
            Ok(())
        });
        stream_model_prepare_with(
            &mut stream,
            crate::transcription::models::definition(
                crate::transcription::models::TranscriptionModelId::ParakeetV2,
            ),
            preparing.clone(),
            &AtomicBool::new(false),
            preparer,
        );
        drop(stream);
        let response = client.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        let downloading = response.find("\"type\":\"downloading\"").unwrap();
        let verifying = response.find("\"type\":\"verifying\"").unwrap();
        let loading = response.find("\"type\":\"loading\"").unwrap();
        let ok = response.find("\"type\":\"ok\"").unwrap();
        assert!(downloading < verifying && verifying < loading && loading < ok);
        assert!(!preparing.load(Ordering::Acquire));
    }

    #[test]
    fn a_prepare_that_finishes_at_once_still_reports_its_last_stage() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        });
        let (mut stream, _) = listener.accept().unwrap();
        let preparer: ModelPreparer = Box::new(|_, _, stage| {
            crate::transcription::models::ModelPreparationStage::Loading.store(&stage);
            Ok(())
        });
        stream_model_prepare_with(
            &mut stream,
            crate::transcription::models::definition(
                crate::transcription::models::TranscriptionModelId::ParakeetV2,
            ),
            Arc::new(AtomicBool::new(true)),
            &AtomicBool::new(false),
            preparer,
        );
        drop(stream);
        let response = client.join().unwrap();
        let loading = response.find("\"type\":\"loading\"").unwrap();
        let ok = response.find("\"type\":\"ok\"").unwrap();
        assert!(loading < ok);
    }

    #[test]
    fn an_idle_api_answers_at_once_and_shutdown_wakes_it() {
        let directory = temp_directory();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let mut api = start_unpublished(events, LocalApiOptions::default()).unwrap();
        // The accept loop is asleep in its long wait by now.
        thread::sleep(Duration::from_millis(100));
        let asked = Instant::now();
        let token = api.document.token.clone();
        let health = request(api.document.port, Some(&token), "/health");
        assert!(health.starts_with("HTTP/1.1 200"), "{health}");
        assert!(asked.elapsed() < ACCEPT_WAIT / 2);
        thread::sleep(Duration::from_millis(100));
        let stopping = Instant::now();
        api.shutdown();
        assert!(
            stopping.elapsed() < ACCEPT_WAIT / 2,
            "shutdown took {:?}",
            stopping.elapsed()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn tokens_have_256_bits_of_random_lowercase_hex() {
        let first = generate_token().unwrap();
        let second = generate_token().unwrap();
        assert!(valid_token(&first));
        assert_ne!(first, second);
    }

    fn start_published(discovery_path: PathBuf, events: EventLog) -> Result<LocalApi> {
        LocalApi::start_bound(
            Some(discovery_path),
            Arc::new(test_health),
            events,
            LocalApiOptions::default(),
            None,
        )
    }

    fn start_unpublished(events: EventLog, options: LocalApiOptions) -> Result<LocalApi> {
        LocalApi::start_bound(None, Arc::new(test_health), events, options, None)
    }

    fn on() -> bool {
        true
    }

    /// Labs › Connected apps on, websites included.
    fn connected() -> Access {
        Access::gated(on, on)
    }

    /// `store` with `origins` allowed to ask in Labs › Connected apps.
    fn allowing(store: Arc<Permissions>, origins: &[String]) -> Arc<Permissions> {
        for origin in origins {
            assert!(store.website_action(origin, WebsiteAction::Allow));
        }
        store
    }

    }

    fn two_origins() -> Vec<String> {
        vec![
            "https://app.example".to_owned(),
            "https://other.example".to_owned(),
        ]
    }

    /// A consent gate with a fixed answer, remembering every ask.
    struct ScriptedGate {
        answer: Answer,
        asked: Mutex<Vec<Prompt>>,
    }

    impl ScriptedGate {
        fn new(answer: Answer) -> Arc<Self> {
            Arc::new(Self {
                answer,
                asked: Mutex::new(Vec::new()),
            })
        }

        fn asks(&self) -> usize {
            self.asked.lock().unwrap().len()
        }

        fn last_prompt(&self) -> Option<Prompt> {
            self.asked.lock().unwrap().last().cloned()
        }
    }

    impl ConsentGate for ScriptedGate {
        fn ask(&self, prompt: &Prompt, _wait: Duration) -> Answer {
            self.asked.lock().unwrap().push(prompt.clone());
            self.answer
        }
    }

    fn start_connect(
        origins: fn() -> Vec<String>,
        gate: Arc<dyn ConsentGate>,
        timings: Option<(Duration, Duration, Duration, Duration)>,
    ) -> LocalApi {
        let directory = temp_directory();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        // Labs › Connected apps on, with `origins` allowed to ask; Labs ›
        // Browser access lists nothing.
        let store = allowing(
            Permissions::new(Some(directory.join("permissions.json"))),
            &origins(),
        );
        let connect = match timings {
            Some((code, token, wait, cooldown)) => connect::Connect::with_timings(
                connected(),
                Some(gate),
                store,
                code,
                token,
                wait,
                cooldown,
            ),
            None => connect::Connect::new(connected(), Some(gate), store),
        };
        start_unpublished(
            events,
            LocalApiOptions {
                cors: Cors::gated(Vec::new),
                connect: Some(connect),
                stream_engine: Some(crate::transcription::stream::tests::fake_source()),
                ..LocalApiOptions::default()
            },
        )
        .unwrap()
    }

    /// The RFC 7636 S256 example: verifier, and the challenge it hashes to.
    const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    /// A browser's JSON POST from `origin`, optionally with a connect token.
    fn connect_post(port: u16, origin: &str, path: &str, body: &str) -> String {
        connect_post_with(port, origin, path, None, "application/json", body)
    }

    fn connect_post_with(
        port: u16,
        origin: &str,
        path: &str,
        token: Option<&str>,
        content_type: &str,
        body: &str,
    ) -> String {
        let token = token.map_or_else(String::new, |token| {
            format!("X-Hex-Connect-Token: {token}\r\n")
        });
        raw_request(
            port,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {origin}\r\n{token}Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        )
    }

    /// One full consent, answered with `answer`, returning (port, code).
    fn authorize(port: u16, origin: &str) -> String {
        let response = connect_post(
            port,
            origin,
            connect::AUTHORIZE_PATH,
            &format!(
                r#"{{"codeChallenge":"{CHALLENGE}","codeChallengeMethod":"S256","state":"s1","clientName":"Field Notes"}}"#
            ),
        );
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        serde_json::from_str::<serde_json::Value>(response_body(&response)).unwrap()["code"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn exchange(port: u16, origin: &str, code: &str) -> String {
        connect_post(
            port,
            origin,
            connect::TOKEN_PATH,
            &format!(r#"{{"code":"{code}","codeVerifier":"{VERIFIER}"}}"#),
        )
    }

    fn upload_token(response: &str) -> String {
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        serde_json::from_str::<serde_json::Value>(response_body(response)).unwrap()["audioToken"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn body_code(response: &str) -> String {
        serde_json::from_str::<serde_json::Value>(response_body(response))
            .unwrap()
            .get("code")
            .and_then(|code| code.as_str())
            .unwrap_or_default()
            .to_owned()
    }

    #[test]
    fn an_allowed_page_trades_one_code_for_one_upload() {
        let gate = ScriptedGate::new(Answer::Once);
        let api = start_connect(app_origin, gate.clone(), None);
        let port = api.document.port;

        let response = connect_post(
            port,
            "https://app.example",
            connect::AUTHORIZE_PATH,
            &format!(
                r#"{{"codeChallenge":"{CHALLENGE}","state":"s1","clientName":"Field Notes"}}"#
            ),
        );
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(
            response.contains("Access-Control-Allow-Origin: https://app.example\r\n"),
            "{response}"
        );
        let answered: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();
        assert_eq!(answered["origin"], "https://app.example");
        assert_eq!(answered["scope"], "transcription:transcribe");
        assert_eq!(answered["state"], "s1");
        let code = answered["code"].as_str().unwrap().to_owned();
        // The dialog named the page and its verified origin.
        let prompt = gate.last_prompt().unwrap();
        assert_eq!(prompt.recipient.label(), "https://app.example");
        assert_eq!(prompt.recipient.claimed_name(), Some("Field Notes"));
        assert_eq!(prompt.intent, Intent::Transcribe);

        let token = upload_token(&exchange(port, "https://app.example", &code));
        // The code was spent by its exchange.
        let reused = exchange(port, "https://app.example", &code);
        assert!(reused.starts_with("HTTP/1.1 403"), "{reused}");
        assert_eq!(body_code(&reused), "invalid-grant");

        // The token reaches the transcription route (a non-WAV body is the
        // route's own refusal, after auth), and only once.
        let upload = connect_post_with(
            port,
            "https://app.example",
            "/transcriptions",
            Some(&token),
            "audio/webm",
            "not a wav",
        );
        assert!(upload.starts_with("HTTP/1.1 415"), "{upload}");
        assert_eq!(body_code(&upload), "unsupported-audio");
        let again = connect_post_with(
            port,
            "https://app.example",
            "/transcriptions",
            Some(&token),
            "audio/webm",
            "not a wav",
        );
        assert!(again.starts_with("HTTP/1.1 401"), "{again}");
    }

    #[test]
    fn a_denied_page_gets_no_code_and_a_short_cooldown() {
        let gate = ScriptedGate::new(Answer::Deny);
        let api = start_connect(app_origin, gate.clone(), None);
        let port = api.document.port;
        let body = format!(r#"{{"codeChallenge":"{CHALLENGE}"}}"#);
        let denied = connect_post(port, "https://app.example", connect::AUTHORIZE_PATH, &body);
        assert!(denied.starts_with("HTTP/1.1 403"), "{denied}");
        assert_eq!(body_code(&denied), "consent-denied");
        assert!(!denied.contains("hexc_"));

        // The page may not reopen the dialog at once, and the gate was not
        // asked a second time.
        let cooled = connect_post(port, "https://app.example", connect::AUTHORIZE_PATH, &body);
        assert!(cooled.starts_with("HTTP/1.1 429"), "{cooled}");
        assert_eq!(body_code(&cooled), "consent-cooldown");
        assert_eq!(gate.asks(), 1);
    }

    #[test]
    fn an_unlisted_page_never_reaches_the_dialog() {
        let gate = ScriptedGate::new(Answer::Once);
        let api = start_connect(app_origin, gate.clone(), None);
        let port = api.document.port;
        let body = format!(r#"{{"codeChallenge":"{CHALLENGE}"}}"#);
        let other = connect_post(
            port,
            "https://other.example",
            connect::AUTHORIZE_PATH,
            &body,
        );
        // Labs › Connected apps lists it instead, and answers CORS so the
        // page can read why.
        assert!(other.starts_with("HTTP/1.1 403"), "{other}");
        assert_eq!(body_code(&other), "website-not-allowed");
        assert!(other.contains("Access-Control-Allow-Origin: https://other.example"));

        // No Origin at all is not a browser page Hex knows.
        let missing = raw_request(
            port,
            &format!(
                "POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                connect::AUTHORIZE_PATH,
                body.len()
            ),
        );
        assert!(missing.starts_with("HTTP/1.1 403"), "{missing}");
        assert_eq!(body_code(&missing), "origin-not-allowed");
        assert_eq!(gate.asks(), 0);
    }

    #[test]
    fn only_a_browser_json_post_can_prompt() {
        let gate = ScriptedGate::new(Answer::Once);
        let api = start_connect(app_origin, gate.clone(), None);
        let port = api.document.port;
        let body = format!(r#"{{"codeChallenge":"{CHALLENGE}"}}"#);
        // A plain text body would skip the browser's preflight, so it is not
        // a request this flow accepts.
        let simple = connect_post_with(
            port,
            "https://app.example",
            connect::AUTHORIZE_PATH,
            None,
            "text/plain",
            &body,
        );
        assert!(simple.starts_with("HTTP/1.1 415"), "{simple}");
        // A GET is not the exchange.
        let get = raw_request(
            port,
            &format!(
                "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://app.example\r\nConnection: close\r\n\r\n",
                connect::AUTHORIZE_PATH
            ),
        );
        assert!(get.starts_with("HTTP/1.1 401"), "{get}");
        // A challenge that is not a SHA-256 digest is refused before the
        // dialog.
        let bad = connect_post(
            port,
            "https://app.example",
            connect::AUTHORIZE_PATH,
            r#"{"codeChallenge":"not-a-digest"}"#,
        );
        assert!(bad.starts_with("HTTP/1.1 400"), "{bad}");
        assert_eq!(gate.asks(), 0);
    }

    #[test]
    fn a_code_is_bound_to_its_origin_and_its_verifier() {
        let gate = ScriptedGate::new(Answer::Once);
        let api = start_connect(two_origins, gate, None);
        let port = api.document.port;
        let code = authorize(port, "https://app.example");
        let stolen = exchange(port, "https://other.example", &code);
        assert!(stolen.starts_with("HTTP/1.1 403"), "{stolen}");
        assert_eq!(body_code(&stolen), "invalid-grant");
        // The failed exchange spent the code, so the real page cannot use it.
        let spent = exchange(port, "https://app.example", &code);
        assert_eq!(body_code(&spent), "invalid-grant");

        let code = authorize(port, "https://app.example");
        let wrong = connect_post(
            port,
            "https://app.example",
            connect::TOKEN_PATH,
            &format!(r#"{{"code":"{code}","codeVerifier":"{}"}}"#, "a".repeat(43)),
        );
        assert!(wrong.starts_with("HTTP/1.1 403"), "{wrong}");
        assert_eq!(body_code(&wrong), "invalid-grant");
    }

    #[test]
    fn expired_grants_are_refused() {
        let gate = ScriptedGate::new(Answer::Once);
        let api = start_connect(
            app_origin,
            gate.clone(),
            Some((
                Duration::ZERO,
                Duration::from_secs(60),
                Duration::from_secs(1),
                Duration::ZERO,
            )),
        );
        let port = api.document.port;
        let code = authorize(port, "https://app.example");
        let expired = exchange(port, "https://app.example", &code);
        assert!(expired.starts_with("HTTP/1.1 403"), "{expired}");
        assert_eq!(body_code(&expired), "invalid-grant");

        // A token that expires at once cannot upload.
        let api = start_connect(
            app_origin,
            gate,
            Some((
                Duration::from_secs(60),
                Duration::ZERO,
                Duration::from_secs(1),
                Duration::ZERO,
            )),
        );
        let port = api.document.port;
        let code = authorize(port, "https://app.example");
        let token = upload_token(&exchange(port, "https://app.example", &code));
        let upload = connect_post_with(
            port,
            "https://app.example",
            "/transcriptions",
            Some(&token),
            "audio/webm",
            "not a wav",
        );
        assert!(upload.starts_with("HTTP/1.1 401"), "{upload}");
    }

    #[test]
    fn an_upload_token_reaches_no_other_route_and_no_other_scope() {
        let gate = ScriptedGate::new(Answer::Once);
        let api = start_connect(app_origin, gate, None);
        let port = api.document.port;
        let code = authorize(port, "https://app.example");
        let token = upload_token(&exchange(port, "https://app.example", &code));

        for (method, path) in [
            ("GET", "/health"),
            ("GET", "/capabilities"),
            ("POST", "/control"),
            ("POST", "/dev/control"),
            ("POST", "/dictations"),
            ("POST", "/streams"),
        ] {
            let response = raw_request(
                port,
                &format!(
                    "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://app.example\r\nX-Hex-Connect-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                ),
            );
            assert!(
                response.starts_with("HTTP/1.1 401"),
                "{method} {path}: {response}"
            );
        }

        // Even the upload route refuses processing and timings: the grant is
        // one plain transcript.
        let scoped = connect_post_with(
            port,
            "https://app.example",
            "/transcriptions?process=global",
            Some(&token),
            "audio/webm",
            "not a wav",
        );
        assert!(scoped.starts_with("HTTP/1.1 403"), "{scoped}");
        assert_eq!(body_code(&scoped), "connect-scope");

        // A token tried from another origin is refused, and spent.
        let code = authorize(port, "https://app.example");
        let token = upload_token(&exchange(port, "https://app.example", &code));
        let wrong_origin = connect_post_with(
            port,
            "https://other.example",
            "/transcriptions",
            Some(&token),
            "audio/webm",
            "not a wav",
        );
        assert!(wrong_origin.starts_with("HTTP/1.1 401"), "{wrong_origin}");
        let spent = connect_post_with(
            port,
            "https://app.example",
            "/transcriptions",
            Some(&token),
            "audio/webm",
            "not a wav",
        );
        assert!(spent.starts_with("HTTP/1.1 401"), "{spent}");
    }

    #[test]
    fn a_timed_out_ask_answers_without_a_code() {
        let gate = ScriptedGate::new(Answer::Timeout);
        let api = start_connect(app_origin, gate, None);
        let response = connect_post(
            api.document.port,
            "https://app.example",
            connect::AUTHORIZE_PATH,
            &format!(r#"{{"codeChallenge":"{CHALLENGE}"}}"#),
        );
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
        assert_eq!(body_code(&response), "consent-timeout");
    fn test_health() -> HealthResponse {
        HealthResponse {
            version: "test",
            api_version: API_VERSION,
        }
    }

    fn request(port: u16, token: Option<&str>, path: &str) -> String {
        request_with_method(port, token, "GET", path)
    }

    fn request_with_method(port: u16, token: Option<&str>, method: &str, path: &str) -> String {
        let authorization = token.map_or_else(String::new, |token| {
            format!("Authorization: Bearer {token}\r\n")
        });
        raw_request(
            port,
            &format!(
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{authorization}Connection: close\r\n\r\n"
            ),
        )
    }

    fn raw_request(port: u16, request: &str) -> String {
        String::from_utf8(raw_request_bytes(port, request)).unwrap()
    }

    fn raw_request_bytes(port: u16, request: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        response
    }

    /// Like `raw_request_bytes`, signalling once the response contains `marker`.
    fn raw_request_signalling(
        port: u16,
        request: &str,
        marker: &[u8],
        ready: mpsc::Sender<()>,
    ) -> Vec<u8> {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(TEST_WAIT)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        let mut buffer = [0; 1024];
        let mut signalled = false;
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                return response;
            }
            response.extend_from_slice(&buffer[..read]);
            if !signalled && response.windows(marker.len()).any(|bytes| bytes == marker) {
                signalled = true;
                let _ = ready.send(());
            }
        }
    }

    fn request_with_dictation_token(
        port: u16,
        token: &str,
        owner_token: &str,
        method: &str,
        path: &str,
    ) -> String {
        raw_request(
            port,
            &format!(
                "{method} {path} HTTP/1.1\r\nAuthorization: Bearer {token}\r\nX-Hex-Dictation-Token: {owner_token}\r\nConnection: close\r\n\r\n"
            ),
        )
    }

    fn request_bytes(port: u16, token: &str, owner_token: &str, path: &str) -> Vec<u8> {
        raw_request_bytes(
            port,
            &format!(
                "GET {path} HTTP/1.1\r\nAuthorization: Bearer {token}\r\nX-Hex-Dictation-Token: {owner_token}\r\nConnection: close\r\n\r\n"
            ),
        )
    }

    fn response_body(response: &str) -> &str {
        response.split_once("\r\n\r\n").unwrap().1
    }

    fn temp_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hex-local-api-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    // -----------------------------------------------------------------------
    // Consented one-recording sessions

    /// A recording backend that never touches a microphone.
    #[derive(Default)]
    struct FakeRecording {
        starts: AtomicU64,
        finishes: AtomicU64,
        cancels: AtomicU64,
        renews: AtomicU64,
        /// The capture loop lost the recording (microphone gone).
        lost: AtomicBool,
        /// Finishing fails (the capture loop busy, or a timeout).
        finish_fails: AtomicBool,
        text: Mutex<String>,
    }

    impl FakeRecording {
        fn with_text(text: &str) -> Arc<Self> {
            Arc::new(Self {
                text: Mutex::new(text.to_owned()),
                ..Self::default()
            })
        }

        fn starts(&self) -> u64 {
            self.starts.load(Ordering::Acquire)
        }

        fn finishes(&self) -> u64 {
            self.finishes.load(Ordering::Acquire)
        }

        fn cancels(&self) -> u64 {
            self.cancels.load(Ordering::Acquire)
        }

        fn renews(&self) -> u64 {
            self.renews.load(Ordering::Acquire)
        }
    }

    impl record::Recording for FakeRecording {
        fn start(&self) -> Result<record::Started, String> {
            self.starts.fetch_add(1, Ordering::AcqRel);
            Ok(record::Started {
                id: 42,
                owner: "owner".into(),
            })
        }

        fn renew(&self, started: &record::Started) -> Result<(), String> {
            assert_eq!(started.id, 42);
            self.renews.fetch_add(1, Ordering::AcqRel);
            if self.lost.load(Ordering::Acquire) {
                Err("dictation-not-active".into())
            } else {
                Ok(())
            }
        }

        fn finish(&self, started: &record::Started) -> Result<record::Transcript, String> {
            assert_eq!(started.id, 42);
            self.finishes.fetch_add(1, Ordering::AcqRel);
            if self.finish_fails.load(Ordering::Acquire) {
                return Err("service-capture-busy".into());
            }
            Ok(record::Transcript {
                text: self.text.lock().unwrap().clone(),
                duration_ms: 1234,
            })
        }

        fn cancel(&self, started: &record::Started) {
            assert_eq!(started.id, 42);
            self.cancels.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn record_timings() -> record::Timings {
        record::Timings {
            code: Duration::from_secs(60),
            lease: Duration::from_secs(60),
            grace: Duration::from_secs(60),
            prompt: Duration::from_secs(1),
            cooldown: Duration::ZERO,
        }
    }

    fn start_record_api(
        gate: Arc<dyn ConsentGate>,
        recording: Arc<dyn record::Recording>,
        timings: record::Timings,
    ) -> (LocalApi, Arc<record::Sessions>) {
        let directory = temp_directory();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let sessions = Arc::new(record::Sessions::with_timings(
            connected(),
            Some(gate),
            recording,
            allowing(
                Permissions::new(Some(directory.join("permissions.json"))),
                &app_origin(),
            ),
            timings,
        ));
        let api = start_unpublished(
            events,
            LocalApiOptions {
                cors: Cors::gated(Vec::new),
                record: Some(sessions.clone()),
                stream_engine: Some(crate::transcription::stream::tests::fake_source()),
                ..LocalApiOptions::default()
            },
        )
        .unwrap();
        (api, sessions)
    }

    /// One `/record` request: a JSON body for pairing/token, a bare request
    /// for the grant and session actions.
    fn record_request(
        port: u16,
        method: &str,
        path: &str,
        origin: &str,
        grant: Option<&str>,
        token: Option<&str>,
        body: Option<&str>,
    ) -> String {
        let grant = grant.map_or_else(String::new, |grant| {
            format!("X-Hex-Record-Grant: {grant}\r\n")
        });
        let token = token.map_or_else(String::new, |token| {
            format!("X-Hex-Record-Token: {token}\r\n")
        });
        let (headers, body) = match body {
            Some(body) => (
                format!(
                    "Content-Type: application/json\r\nContent-Length: {}\r\n",
                    body.len()
                ),
                body,
            ),
            None => ("Content-Length: 0\r\n".to_owned(), ""),
        };
        raw_request(
            port,
            &format!(
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {origin}\r\n{grant}{token}{headers}Connection: close\r\n\r\n{body}"
            ),
        )
    }

    /// A browser keeps the secret from its first Always Allow response. The
    /// secret is scoped to this capability; normal requests still use PKCE.
    fn web_consent(port: u16, origin: &str, path: &str, proof: Option<&str>) -> String {
        connect_post(
            port,
            origin,
            path,
            &serde_json::json!({"codeChallenge": CHALLENGE, "state":"r1", "permissionProof":proof})
                .to_string(),
        )
    }

    fn consent_json(response: &str) -> serde_json::Value {
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        serde_json::from_str(response_body(response)).unwrap()
    }

    fn record_pair(port: u16, origin: &str) -> String {
        consent_json(&web_consent(port, origin, record::PAIR_PATH, None))["code"]
            .as_str()
            .unwrap()
            .into()
    }

    /// `POST /record/token`: the pairing code for a grant token.
    fn record_grant(port: u16, origin: &str, code: &str) -> (String, u64) {
        let body = format!(r#"{{"code":"{code}","codeVerifier":"{VERIFIER}"}}"#);
        let response = record_request(
            port,
            "POST",
            record::TOKEN_PATH,
            origin,
            None,
            None,
            Some(&body),
        );
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let value: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();
        (
            value["grantToken"].as_str().unwrap().to_owned(),
            value["grantId"].as_u64().unwrap(),
        )
    }

    /// `POST /record/start`: one recording session under a grant.
    fn record_start(port: u16, origin: &str, grant: &str) -> (String, u64, String) {
        let response = record_request(
            port,
            "POST",
            record::START_PATH,
            origin,
            Some(grant),
            None,
            None,
        );
        assert!(response.starts_with("HTTP/1.1 201"), "{response}");
        let value: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();
        (
            value["sessionToken"].as_str().unwrap().to_owned(),
            value["id"].as_u64().unwrap(),
            value["state"].as_str().unwrap().to_owned(),
        )
    }

    #[test]
    fn allow_once_recording_is_spent_and_never_saved() {
        let gate = ScriptedGate::new(Answer::Once);
        let recording = FakeRecording::with_text("only once");
        let (api, sessions) = start_record_api(gate.clone(), recording.clone(), record_timings());
        let port = api.document.port;
        let origin = "https://app.example";
        let grant = record_grant(port, origin, &record_pair(port, origin)).0;
        let session = record_start(port, origin, &grant).0;
        assert!(
            record_request(
                port,
                "POST",
                record::FINISH_PATH,
                origin,
                None,
                Some(&session),
                None
            )
            .starts_with("HTTP/1.1 200")
        );
        assert!(
            record_request(
                port,
                "POST",
                record::START_PATH,
                origin,
                Some(&grant),
                None,
                None
            )
            .starts_with("HTTP/1.1 401")
        );
        assert_eq!(recording.starts(), 1);
        assert!(sessions.permissions().is_empty());
        record_pair(port, origin);
        assert_eq!(gate.asks(), 2);
    }

    #[test]
    fn always_survives_restart_and_revoking_one_capability_preserves_the_other() {
        let directory = temp_directory();
        let path = directory.join("permissions.json");
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("saved");
        let start = || {
            let store = allowing(Permissions::new(Some(path.clone())), &app_origin());
            let sessions = Arc::new(record::Sessions::with_timings(
                connected(),
                Some(gate.clone()),
                recording.clone(),
                store.clone(),
                record_timings(),
            ));
            let api = start_unpublished(
                EventLog::create(&temp_directory().join("events.ndjson")).unwrap(),
                LocalApiOptions {
                    cors: Cors::gated(Vec::new),
                    connect: Some(Connect::new(connected(), Some(gate.clone()), store)),
                    record: Some(sessions.clone()),
                    ..LocalApiOptions::default()
                },
            )
            .unwrap();
            (api, sessions)
        };
        let origin = "https://app.example";
        let (audio_proof, mic_proof);
        {
            let (api, _) = start();
            let proof = |path| {
                consent_json(&web_consent(api.document.port, origin, path, None))["permissionProof"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            };
            audio_proof = proof(connect::AUTHORIZE_PATH);
            mic_proof = proof(record::PAIR_PATH);
            assert_eq!(recording.starts(), 0);
            assert_eq!(gate.asks(), 2);
        }
        {
            let (api, sessions) = start();
            let port = api.document.port;
            for (path, wrong) in [
                (record::PAIR_PATH, &audio_proof),
                (connect::AUTHORIZE_PATH, &mic_proof),
            ] {
                for proof in [None, Some(wrong.as_str())] {
                    assert_eq!(
                        body_code(&web_consent(port, origin, path, proof)),
                        "permission-proof-required"
                    );
                }
            }
            let code_for = |path, proof: &str| {
                consent_json(&web_consent(port, origin, path, Some(proof)))["code"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            };
            let code = code_for(connect::AUTHORIZE_PATH, &audio_proof);
            let token = upload_token(&exchange(
                port,
                origin,
                &code_for(connect::AUTHORIZE_PATH, &audio_proof),
            ));
            let grant = record_grant(port, origin, &code_for(record::PAIR_PATH, &mic_proof)).0;
            record_start(port, origin, &grant);
            assert_eq!(gate.asks(), 2);
            for p in sessions.permissions() {
                if p.capability == PermissionCapability::MicrophoneRecording {
                    assert!(sessions.revoke_permission(p.id));
                }
            }
            assert_eq!(recording.cancels(), 1);
            assert!(
                record_request(
                    port,
                    "POST",
                    record::START_PATH,
                    origin,
                    Some(&grant),
                    None,
                    None
                )
                .starts_with("HTTP/1.1 401")
            );
            code_for(connect::AUTHORIZE_PATH, &audio_proof);
            assert_eq!(gate.asks(), 2);
            let perms = sessions.permissions();
            assert_eq!(perms.len(), 1);
            assert!(sessions.revoke_permission(perms[0].id));
            assert!(exchange(port, origin, &code).starts_with("HTTP/1.1 403"));
            assert!(
                connect_post_with(
                    port,
                    origin,
                    "/transcriptions",
                    Some(&token),
                    "audio/wav",
                    "x"
                )
                .starts_with("HTTP/1.1 401")
            );
        }
        let (api, sessions) = start();
        assert!(sessions.permissions().is_empty());
        authorize(api.document.port, origin);
        record_pair(api.document.port, origin);
        assert_eq!(gate.asks(), 4);
    }

    #[test]
    fn pairing_once_allows_repeated_distinct_recordings() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("the paired words");
        let (api, sessions) = start_record_api(gate.clone(), recording.clone(), record_timings());
        let port = api.document.port;
        let origin = "https://app.example";

        let code = record_pair(port, origin);
        let prompt = gate.last_prompt().unwrap();
        assert_eq!(prompt.intent, Intent::Record);
        assert_eq!(prompt.recipient.label(), origin);
        // Pairing asks once and starts nothing.
        assert_eq!(recording.starts(), 0);
        assert_eq!(gate.asks(), 1);

        let (grant, grant_id) = record_grant(port, origin, &code);
        assert_eq!(grant_id, 1);
        assert_eq!(sessions.grants().len(), 1);
        assert_eq!(sessions.grants()[0].id, grant_id);
        assert_eq!(recording.starts(), 0);
        // Pairing the same verified recipient again is refused, not asked.
        let again = record_request(
            port,
            "POST",
            record::PAIR_PATH,
            origin,
            None,
            None,
            Some(&format!(
                r#"{{"codeChallenge":"{CHALLENGE}","codeChallengeMethod":"S256"}}"#
            )),
        );
        assert!(again.starts_with("HTTP/1.1 409"), "{again}");
        assert_eq!(body_code(&again), "already-paired");
        assert_eq!(gate.asks(), 1);

        let (first, first_id, state) = record_start(port, origin, &grant);
        assert_eq!(state, "recording");
        assert_eq!(recording.starts(), 1);

        let status = record_request(
            port,
            "GET",
            record::STATUS_PATH,
            origin,
            None,
            Some(&first),
            None,
        );
        assert!(status.starts_with("HTTP/1.1 200"), "{status}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(response_body(&status)).unwrap()["state"],
            "recording"
        );

        let finished = record_request(
            port,
            "POST",
            record::FINISH_PATH,
            origin,
            None,
            Some(&first),
            None,
        );
        assert!(finished.starts_with("HTTP/1.1 200"), "{finished}");
        let body: serde_json::Value = serde_json::from_str(response_body(&finished)).unwrap();
        assert_eq!(body["transcript"], "the paired words");
        assert_eq!(body["durationMs"], 1234);
        assert_eq!(recording.finishes(), 1);

        // The transcript is handed over once.
        let repeated = record_request(
            port,
            "POST",
            record::FINISH_PATH,
            origin,
            None,
            Some(&first),
            None,
        );
        assert!(repeated.starts_with("HTTP/1.1 409"), "{repeated}");
        assert_eq!(body_code(&repeated), "session-finished");

        // The same grant opens another, distinct recording without asking
        // again.
        let (second, second_id, _) = record_start(port, origin, &grant);
        assert_ne!(second, first);
        assert_ne!(second_id, first_id);
        assert_eq!(recording.starts(), 2);
        let finished = record_request(
            port,
            "POST",
            record::FINISH_PATH,
            origin,
            None,
            Some(&second),
            None,
        );
        assert!(finished.starts_with("HTTP/1.1 200"), "{finished}");
        assert_eq!(recording.finishes(), 2);
        assert_eq!(gate.asks(), 1);
    }

    #[test]
    fn revoking_a_grant_ends_its_session_and_further_starts() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("revoked words");
        let (api, sessions) = start_record_api(gate.clone(), recording.clone(), record_timings());
        let port = api.document.port;
        let origin = "https://app.example";

        let code = record_pair(port, origin);
        let (grant, _) = record_grant(port, origin, &code);
        let (session, _, _) = record_start(port, origin, &grant);
        assert_eq!(
            sessions.view().map(|view| view.phase),
            Some(record::Phase::Recording)
        );

        let revoked = record_request(
            port,
            "POST",
            record::REVOKE_PATH,
            origin,
            Some(&grant),
            None,
            None,
        );
        assert!(revoked.starts_with("HTTP/1.1 204"), "{revoked}");
        assert_eq!(recording.cancels(), 1);
        assert!(sessions.grants().is_empty());
        assert!(sessions.view().is_none());

        // The live session is cancelled and the grant opens nothing more.
        let cancelled = record_request(
            port,
            "GET",
            record::STATUS_PATH,
            origin,
            None,
            Some(&session),
            None,
        );
        assert!(cancelled.starts_with("HTTP/1.1 200"), "{cancelled}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(response_body(&cancelled)).unwrap()["state"],
            "cancelled"
        );
        let start = record_request(
            port,
            "POST",
            record::START_PATH,
            origin,
            Some(&grant),
            None,
            None,
        );
        assert!(start.starts_with("HTTP/1.1 401"), "{start}");
        let again = record_request(
            port,
            "POST",
            record::REVOKE_PATH,
            origin,
            Some(&grant),
            None,
            None,
        );
        assert!(again.starts_with("HTTP/1.1 401"), "{again}");

        // Revoking lets the recipient pair again, with a fresh prompt.
        let code = record_pair(port, origin);
        let (grant, _) = record_grant(port, origin, &code);
        let (_, _, _) = record_start(port, origin, &grant);
        assert_eq!(gate.asks(), 2);
        assert_eq!(recording.starts(), 2);
    }

    #[test]
    fn revoking_drops_pairing_codes_issued_before_it() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("codes");
        let (api, _sessions) = start_record_api(gate, recording, record_timings());
        let port = api.document.port;
        let origin = "https://app.example";

        // Two allowed asks arrive before either is exchanged; the first
        // becomes the grant.
        let paired = consent_json(&web_consent(port, origin, record::PAIR_PATH, None));
        let first = paired["code"].as_str().unwrap().to_owned();
        let second = consent_json(&web_consent(
            port,
            origin,
            record::PAIR_PATH,
            paired["permissionProof"].as_str(),
        ))["code"]
            .as_str()
            .unwrap()
            .to_owned();
        let (grant, _) = record_grant(port, origin, &first);
        let revoked = record_request(
            port,
            "POST",
            record::REVOKE_PATH,
            origin,
            Some(&grant),
            None,
            None,
        );
        assert!(revoked.starts_with("HTTP/1.1 204"), "{revoked}");

        // The code that was still outstanding cannot pair the same
        // recipient behind the person's back.
        let body = format!(r#"{{"code":"{second}","codeVerifier":"{VERIFIER}"}}"#);
        let late = record_request(
            port,
            "POST",
            record::TOKEN_PATH,
            origin,
            None,
            None,
            Some(&body),
        );
        assert!(late.starts_with("HTTP/1.1 403"), "{late}");
        assert_eq!(body_code(&late), "invalid-grant");
    }

    #[test]
    fn a_denied_pairing_never_pairs_and_cools_down() {
        let gate = ScriptedGate::new(Answer::Deny);
        let recording = FakeRecording::with_text("never");
        let mut timings = record_timings();
        timings.cooldown = Duration::from_secs(30);
        let (api, sessions) = start_record_api(gate.clone(), recording.clone(), timings);
        let port = api.document.port;
        let body = format!(r#"{{"codeChallenge":"{CHALLENGE}"}}"#);
        let denied = record_request(
            port,
            "POST",
            record::PAIR_PATH,
            "https://app.example",
            None,
            None,
            Some(&body),
        );
        assert!(denied.starts_with("HTTP/1.1 403"), "{denied}");
        assert_eq!(body_code(&denied), "consent-denied");
        assert_eq!(recording.starts(), 0);
        assert!(sessions.grants().is_empty());

        let cooled = record_request(
            port,
            "POST",
            record::PAIR_PATH,
            "https://app.example",
            None,
            None,
            Some(&body),
        );
        assert!(cooled.starts_with("HTTP/1.1 429"), "{cooled}");
        assert_eq!(gate.asks(), 1);
    }

    #[test]
    fn a_grant_and_session_reach_no_other_route_and_no_other_origin() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("private words");
        let (api, _sessions) = start_record_api(gate, recording, record_timings());
        let port = api.document.port;
        let origin = "https://app.example";
        let code = record_pair(port, origin);
        let (grant, _) = record_grant(port, origin, &code);
        let (session, _, _) = record_start(port, origin, &grant);

        for (method, path) in [
            ("GET", "/health"),
            ("GET", "/capabilities"),
            ("POST", "/control"),
            ("POST", "/dev/control"),
            ("POST", "/dictations"),
            ("POST", "/transcriptions"),
            ("GET", "/record/audio"),
            ("GET", "/record/levels"),
        ] {
            let response = record_request(
                port,
                method,
                path,
                origin,
                Some(&grant),
                Some(&session),
                None,
            );
            assert!(
                !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.1 201"),
                "{method} {path}: {response}"
            );
        }

        // Neither token crosses to another origin.
        for (grant_header, session_header) in
            [(Some(grant.as_str()), None), (None, Some(session.as_str()))]
        {
            let stolen = record_request(
                port,
                "POST",
                record::START_PATH,
                "https://other.example",
                grant_header,
                session_header,
                None,
            );
            assert!(stolen.starts_with("HTTP/1.1 401"), "{stolen}");
        }
        let right = record_request(
            port,
            "POST",
            record::START_PATH,
            origin,
            Some(&grant),
            None,
            None,
        );
        // The first session is still open, so a second is refused; the grant
        // itself is accepted.
        assert!(right.starts_with("HTTP/1.1 409"), "{right}");
        assert_eq!(body_code(&right), "session-busy");
    }

    #[test]
    fn a_recording_stops_when_its_client_never_checks_in() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("lease words");
        let mut timings = record_timings();
        timings.lease = Duration::from_millis(60);
        timings.grace = Duration::from_millis(60);
        let (api, sessions) = start_record_api(gate, recording.clone(), timings);
        let port = api.document.port;
        let origin = "https://app.example";
        let code = record_pair(port, origin);
        let (grant, _) = record_grant(port, origin, &code);
        let (session, id, _) = record_start(port, origin, &grant);
        // The lease stops the recording on its own.
        let deadline = Instant::now() + Duration::from_secs(2);
        while recording.finishes() == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(recording.finishes(), 1);
        assert_eq!(
            sessions.view().map(|view| (view.id, view.phase)),
            Some((id, record::Phase::Stopped))
        );
        // The client can still collect the transcript inside the grace.
        let finished = record_request(
            port,
            "POST",
            record::FINISH_PATH,
            origin,
            None,
            Some(&session),
            None,
        );
        assert!(finished.starts_with("HTTP/1.1 200"), "{finished}");
        assert_eq!(recording.cancels(), 0);
    }

    /// `POST /record/heartbeat` with a session token.
    fn record_heartbeat(port: u16, origin: &str, token: &str) -> String {
        record_request(
            port,
            "POST",
            record::HEARTBEAT_PATH,
            origin,
            None,
            Some(token),
            None,
        )
    }

    fn wait_for(mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + TEST_WAIT;
        while !done() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(done(), "timed out");
    }

    fn lease_timings(lease_ms: u64) -> record::Timings {
        let mut timings = record_timings();
        timings.lease = Duration::from_millis(lease_ms);
        timings
    }

    /// A recording has no length of its own: one that keeps checking in runs
    /// for many leases (accelerated here; the real lease is 10 s, so this is
    /// the old two-minute cap many times over in lease terms), and Hex keeps
    /// the capture loop's own lease alive the whole time.
    #[test]
    fn a_recording_runs_for_as_long_as_its_client_checks_in() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("a long talk");
        let lease = 80;
        let (api, sessions) = start_record_api(gate, recording.clone(), lease_timings(lease));
        let port = api.document.port;
        let origin = "https://app.example";
        let (grant, _) = record_grant(port, origin, &record_pair(port, origin));
        let (session, id, _) = record_start(port, origin, &grant);
        let began = Instant::now();
        while began.elapsed() < Duration::from_millis(lease * 25) {
            let beat = record_heartbeat(port, origin, &session);
            assert!(beat.starts_with("HTTP/1.1 204"), "{beat}");
            thread::sleep(Duration::from_millis(lease / 4));
        }
        let view = sessions.view().unwrap();
        assert_eq!((view.id, view.phase), (id, record::Phase::Recording));
        assert!(view.elapsed_ms >= lease * 25, "{}", view.elapsed_ms);
        assert!(recording.renews() >= 25, "{}", recording.renews());
        assert_eq!(recording.finishes() + recording.cancels(), 0);
        let status = record_request(
            port,
            "GET",
            record::STATUS_PATH,
            origin,
            None,
            Some(&session),
            None,
        );
        let status: serde_json::Value = serde_json::from_str(response_body(&status)).unwrap();
        assert_eq!(status["state"], "recording");
        assert_eq!(status["leaseMs"], lease);
        assert!(status.get("maxRecordingMs").is_none());

        let finished = record_request(
            port,
            "POST",
            record::FINISH_PATH,
            origin,
            None,
            Some(&session),
            None,
        );
        assert!(finished.starts_with("HTTP/1.1 200"), "{finished}");
        assert!(finished.contains("a long talk"));
    }

    /// The real lease, past the old two-minute cap, in real time. Slow, so
    /// it runs by name, outside nextest's hang guard:
    /// `cargo test --bin hex -- --ignored --exact local_api::tests::a_checked_in_recording_runs_past_two_minutes`.
    #[test]
    #[ignore = "runs for two minutes"]
    fn a_checked_in_recording_runs_past_two_minutes() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("still going");
        let mut timings = record_timings();
        timings.lease = record::LEASE;
        let (api, sessions) = start_record_api(gate, recording.clone(), timings);
        let port = api.document.port;
        let origin = "https://app.example";
        let (grant, _) = record_grant(port, origin, &record_pair(port, origin));
        let (session, _, _) = record_start(port, origin, &grant);
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(125) {
            let beat = record_heartbeat(port, origin, &session);
            assert!(beat.starts_with("HTTP/1.1 204"), "{beat}");
            thread::sleep(Duration::from_secs(3));
        }
        assert_eq!(sessions.view().unwrap().phase, record::Phase::Recording);
        assert_eq!(recording.finishes() + recording.cancels(), 0);
        let finished = record_request(
            port,
            "POST",
            record::FINISH_PATH,
            origin,
            None,
            Some(&session),
            None,
        );
        assert!(finished.contains("still going"), "{finished}");
    }

    /// A check-in after the lease lapsed is refused: the recording stayed
    /// stopped, and its transcript is still the client's within the grace.
    #[test]
    fn a_late_check_in_never_revives_a_recording() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("cut short");
        let (api, sessions) = start_record_api(gate, recording.clone(), lease_timings(60));
        let port = api.document.port;
        let origin = "https://app.example";
        let (grant, _) = record_grant(port, origin, &record_pair(port, origin));
        let (session, _, _) = record_start(port, origin, &grant);
        thread::sleep(Duration::from_millis(150));
        let late = record_heartbeat(port, origin, &session);
        assert!(late.starts_with("HTTP/1.1 409"), "{late}");
        assert_eq!(body_code(&late), "stopped");
        wait_for(|| recording.finishes() == 1);
        assert_eq!(sessions.view().unwrap().phase, record::Phase::Stopped);
        let renews = recording.renews();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(
            recording.renews(),
            renews,
            "nothing renews a stopped recording"
        );
        let finished = record_request(
            port,
            "POST",
            record::FINISH_PATH,
            origin,
            None,
            Some(&session),
            None,
        );
        assert!(finished.contains("cut short"), "{finished}");
        let after = record_heartbeat(port, origin, &session);
        assert_eq!(body_code(&after), "session-finished");
        assert_eq!(recording.starts(), 1);
    }

    /// Only the session's own token, from its own origin, renews it; a
    /// stream of anything else lets the lease lapse.
    #[test]
    fn only_the_sessions_own_check_in_renews_it() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("x");
        let (api, sessions) = start_record_api(gate, recording.clone(), lease_timings(150));
        let port = api.document.port;
        let origin = "https://app.example";
        let (grant, _) = record_grant(port, origin, &record_pair(port, origin));
        let (session, _, _) = record_start(port, origin, &grant);
        let began = Instant::now();
        while recording.finishes() == 0 && began.elapsed() < TEST_WAIT {
            // Another origin with the right token.
            let stolen = record_heartbeat(port, "https://other.example", &session);
            assert!(stolen.starts_with("HTTP/1.1 401"), "{stolen}");
            // The grant token where a session token belongs.
            let grant_as_session = record_heartbeat(port, origin, &grant);
            assert_eq!(body_code(&grant_as_session), "record-not-found");
            // The grant header on the heartbeat route.
            let wrong_header = record_request(
                port,
                "POST",
                record::HEARTBEAT_PATH,
                origin,
                Some(&grant),
                None,
                None,
            );
            assert!(wrong_header.starts_with("HTTP/1.1 401"), "{wrong_header}");
            // A made-up token, and the right one carrying a body.
            let guessed = record_heartbeat(port, origin, "hexs_guess");
            assert_eq!(body_code(&guessed), "record-not-found");
            let with_body = record_request(
                port,
                "POST",
                record::HEARTBEAT_PATH,
                origin,
                None,
                Some(&session),
                Some("{}"),
            );
            assert!(with_body.starts_with("HTTP/1.1 400"), "{with_body}");
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(recording.finishes(), 1);
        assert!(began.elapsed() >= Duration::from_millis(150));
        assert_eq!(sessions.view().unwrap().phase, record::Phase::Stopped);
    }

    /// Stop, Cancel, and Revoke still end a recording that is checking in,
    /// and its check-ins are refused after.
    #[test]
    fn stop_cancel_and_revoke_end_a_checked_in_recording() {
        let origin = "https://app.example";
        for end in ["stop", "cancel", "revoke-window", "revoke-client"] {
            let gate = ScriptedGate::new(Answer::Always);
            let recording = FakeRecording::with_text("ended");
            let (api, sessions) = start_record_api(gate, recording.clone(), lease_timings(100));
            let port = api.document.port;
            let (grant, grant_id) = record_grant(port, origin, &record_pair(port, origin));
            let (session, id, _) = record_start(port, origin, &grant);
            let beat = record_heartbeat(port, origin, &session);
            assert!(beat.starts_with("HTTP/1.1 204"), "{end}: {beat}");
            let expected = match end {
                "stop" => {
                    assert!(sessions.stop(id));
                    "stopped"
                }
                "cancel" => {
                    sessions.cancel(id);
                    "session-cancelled"
                }
                "revoke-window" => {
                    assert!(sessions.revoke(grant_id));
                    "session-cancelled"
                }
                _ => {
                    let revoked = record_request(
                        port,
                        "POST",
                        record::REVOKE_PATH,
                        origin,
                        Some(&grant),
                        None,
                        None,
                    );
                    assert!(revoked.starts_with("HTTP/1.1 204"), "{end}: {revoked}");
                    "session-cancelled"
                }
            };
            let refused = record_heartbeat(port, origin, &session);
            assert!(refused.starts_with("HTTP/1.1 409"), "{end}: {refused}");
            assert_eq!(body_code(&refused), expected, "{end}");
            let renews = recording.renews();
            thread::sleep(Duration::from_millis(80));
            assert_eq!(recording.renews(), renews, "{end}: still renewing");
            if end == "stop" {
                assert_eq!(recording.finishes(), 1);
            } else {
                assert_eq!(recording.cancels(), 1, "{end}");
            }
        }
    }

    /// When the capture loop loses the recording (the microphone went
    /// away), the session ends instead of waiting for check-ins.
    #[test]
    fn a_recording_the_capture_loop_lost_is_cancelled() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("x");
        let (api, sessions) = start_record_api(gate, recording.clone(), lease_timings(60));
        let port = api.document.port;
        let origin = "https://app.example";
        let (grant, _) = record_grant(port, origin, &record_pair(port, origin));
        let (session, _, _) = record_start(port, origin, &grant);
        recording.lost.store(true, Ordering::Release);
        wait_for(|| sessions.view().is_none());
        let refused = record_heartbeat(port, origin, &session);
        assert_eq!(body_code(&refused), "session-cancelled");
        assert_eq!(recording.finishes(), 0);
    }

    #[test]
    fn one_recording_session_at_a_time() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("first");
        let (api, _sessions) = start_record_api(gate, recording, record_timings());
        let port = api.document.port;
        let origin = "https://app.example";
        let code = record_pair(port, origin);
        let (grant, _) = record_grant(port, origin, &code);
        let (session, _, _) = record_start(port, origin, &grant);

        let busy = record_request(
            port,
            "POST",
            record::START_PATH,
            origin,
            Some(&grant),
            None,
            None,
        );
        assert!(busy.starts_with("HTTP/1.1 409"), "{busy}");
        assert_eq!(body_code(&busy), "session-busy");

        let cancelled = record_request(
            port,
            "POST",
            record::CANCEL_PATH,
            origin,
            None,
            Some(&session),
            None,
        );
        assert!(cancelled.starts_with("HTTP/1.1 204"), "{cancelled}");
        let (second, _, _) = record_start(port, origin, &grant);
        assert_ne!(second, session);
    }

    #[test]
    fn a_native_grant_is_bound_to_the_verified_peer() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("native words");
        let sessions = Arc::new(record::Sessions::with_timings(
            connected(),
            Some(gate),
            recording,
            allowing(
                Permissions::new(Some(temp_directory().join("permissions.json"))),
                &app_origin(),
            ),
            record_timings(),
        ));
        let identity = record::Identity {
            identifier: "com.example.recorder".into(),
            cdhash: "fixture-digest".into(),
            verification: crate::local_api::Verification::MacSignature {
                team: Some("ABCDE12345".into()),
            },
            name: None,
            icon: None,
        };
        let peer = record::PeerId([1, 2, 3, 4, 5, 6, 7, 8]);
        let channel = record::Channel::Native { peer, identity };
        let body = format!(
            r#"{{"codeChallenge":"{CHALLENGE}","codeChallengeMethod":"S256","clientName":"ignored"}}"#
        );
        let response = sessions.pair(channel.clone(), body.as_bytes());
        assert_eq!(response.status, 200, "{:?}", response.body);
        let code: String =
            serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["code"]
                .as_str()
                .unwrap()
                .to_owned();

        // Another process's peer cannot spend this code, even with the right
        // verifier; the code is spent by that attempt.
        let other = record::Channel::Native {
            peer: record::PeerId([9, 9, 9, 9, 9, 9, 9, 9]),
            identity: record::Identity {
                identifier: "com.example.other".into(),
                cdhash: "fixture-digest".into(),
                verification: crate::local_api::Verification::MacSignature { team: None },
                name: None,
                icon: None,
            },
        };
        let exchange = format!(r#"{{"code":"{code}","codeVerifier":"{VERIFIER}"}}"#);
        let stolen = sessions.exchange(other.clone(), exchange.as_bytes());
        assert_eq!(stolen.status, 403);
        let spent = sessions.exchange(channel.clone(), exchange.as_bytes());
        assert_eq!(spent.status, 403);

        // A fresh pairing mints a grant bound to the verified peer.
        let response = sessions.pair(channel.clone(), body.as_bytes());
        let code: String =
            serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["code"]
                .as_str()
                .unwrap()
                .to_owned();
        let exchange = format!(r#"{{"code":"{code}","codeVerifier":"{VERIFIER}"}}"#);
        let minted = sessions.exchange(channel.clone(), exchange.as_bytes());
        assert_eq!(minted.status, 200);
        let grant: String =
            serde_json::from_slice::<serde_json::Value>(&minted.body).unwrap()["grantToken"]
                .as_str()
                .unwrap()
                .to_owned();
        assert_eq!(sessions.start(&grant, &other).status, 401);
        let started = sessions.start(&grant, &channel);
        assert_eq!(started.status, 201);
        let session: String =
            serde_json::from_slice::<serde_json::Value>(&started.body).unwrap()["sessionToken"]
                .as_str()
                .unwrap()
                .to_owned();
        assert_eq!(sessions.heartbeat(&session, &other).status, 404);
        assert_eq!(sessions.heartbeat(&session, &channel).status, 204);
        assert_eq!(
            record::action("POST", record::HEARTBEAT_PATH),
            Some(record::Action::Heartbeat)
        );
        assert_eq!(sessions.revoke_token(&grant, &other).status, 401);
    }

    #[test]
    fn a_native_client_name_never_becomes_its_identity() {
        let gate = ScriptedGate::new(Answer::Always);
        let sessions = Arc::new(record::Sessions::with_timings(
            connected(),
            Some(gate.clone()),
            FakeRecording::with_text("x"),
            allowing(
                Permissions::new(Some(temp_directory().join("permissions.json"))),
                &app_origin(),
            ),
            record_timings(),
        ));
        let body = format!(r#"{{"codeChallenge":"{CHALLENGE}","clientName":"Totally Trusted"}}"#);
        let response = sessions.pair(
            record::Channel::Native {
                peer: record::PeerId([3; 8]),
                identity: record::Identity {
                    identifier: "com.example.recorder".into(),
                    cdhash: "fixture-digest".into(),
                    verification: crate::local_api::Verification::MacSignature {
                        team: Some("ABCDE12345".into()),
                    },
                    name: None,
                    icon: None,
                },
            },
            body.as_bytes(),
        );
        assert_eq!(response.status, 200);
        let prompt = gate.last_prompt().unwrap();
        assert_eq!(prompt.recipient.label(), "com.example.recorder");
        assert!(prompt.recipient.claimed_name().is_none());
    }

    /// One HTTP exchange over the native recording socket (on Windows, the
    /// pipe `socket` names).
    fn native_request(socket: &Path, request: &str) -> (u16, String) {
        #[cfg(unix)]
        let mut stream = {
            let stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
            stream.set_read_timeout(Some(TEST_WAIT)).unwrap();
            stream
        };
        #[cfg(windows)]
        let mut stream = native::tests::connect(socket);
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        let status = response
            .split_whitespace()
            .nth(1)
            .and_then(|status| status.parse().ok())
            .unwrap_or(0);
        (status, response)
    }

    static NATIVE_LAB: AtomicBool = AtomicBool::new(false);

    fn native_lab() -> bool {
        NATIVE_LAB.load(Ordering::Acquire)
    }

    /// A quiet client of the native channel: connected, sending nothing.
    #[cfg(unix)]
    fn native_client(socket: &Path) -> std::os::unix::net::UnixStream {
        let stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
        stream.set_read_timeout(Some(TEST_WAIT)).unwrap();
        stream
    }

    #[cfg(windows)]
    fn native_client(socket: &Path) -> fs::File {
        native::tests::connect(socket)
    }

    /// The native channel exists only while Labs › Connected apps is on,
    /// serves a few clients at once (one more hears it is busy), and closes
    /// with its connections when the lab turns off.
    #[test]
    fn the_native_channel_follows_its_lab_and_serves_a_few_at_once() {
        let directory = temp_directory();
        let socket = directory.join(if cfg!(windows) {
            "record.pipe"
        } else {
            "record.sock"
        });
        let sessions = Arc::new(record::Sessions::with_timings(
            Access::gated(native_lab, native_lab),
            Some(ScriptedGate::new(Answer::Deny)),
            FakeRecording::with_text("unused"),
            Permissions::new(Some(temp_directory().join("permissions.json"))),
            record_timings(),
        ));
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let mut api = start_unpublished(
            events,
            LocalApiOptions {
                record: Some(sessions),
                native_socket: Some(socket.clone()),
                stream_engine: Some(crate::transcription::stream::tests::fake_source()),
                ..LocalApiOptions::default()
            },
        )
        .unwrap();
        let settles = |present: bool| {
            let deadline = Instant::now() + TEST_WAIT;
            while socket.exists() != present && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            socket.exists() == present
        };
        thread::sleep(Duration::from_millis(500));
        assert!(!socket.exists(), "no channel while the lab is off");

        NATIVE_LAB.store(true, Ordering::Release);
        assert!(settles(true), "the channel opens with the lab");
        let mut quiet: Vec<_> = (0..8).map(|_| native_client(&socket)).collect();
        thread::sleep(Duration::from_millis(200));
        let (status, response) = native_request(
            &socket,
            "GET /record/status HTTP/1.1\r\nHost: hex\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(status, 503, "{response}");
        assert!(response.contains("native-busy"));
        quiet.truncate(1);

        NATIVE_LAB.store(false, Ordering::Release);
        assert!(settles(false), "the channel closes with the lab");
        // Its open connection ends too.
        let mut rest = Vec::new();
        let _ = quiet[0].read_to_end(&mut rest);
        api.shutdown();
    }

    #[test]
    fn a_native_socket_client_pairs_then_records_twice_under_its_identity() {
        let directory = temp_directory();
        // On Windows, the file naming the pipe.
        let socket = directory.join(if cfg!(windows) {
            "record.pipe"
        } else {
            "record.sock"
        });
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("native words");
        let sessions = Arc::new(record::Sessions::with_timings(
            connected(),
            Some(gate.clone()),
            recording.clone(),
            allowing(
                Permissions::new(Some(temp_directory().join("permissions.json"))),
                &app_origin(),
            ),
            record_timings(),
        ));
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let mut api = start_unpublished(
            events,
            LocalApiOptions {
                cors: Cors::gated(Vec::new),
                record: Some(sessions),
                native_socket: Some(socket.clone()),
                stream_engine: Some(crate::transcription::stream::tests::fake_source()),
                ..LocalApiOptions::default()
            },
        )
        .unwrap();

        // The channel opens with the lab, which these tests keep on.
        let deadline = Instant::now() + TEST_WAIT;
        while !socket.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let body = format!(r#"{{"codeChallenge":"{CHALLENGE}","clientName":"Totally Trusted"}}"#);
        let pair = format!(
            "POST {} HTTP/1.1\r\nHost: hex\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            record::PAIR_PATH,
            body.len()
        );
        let (status, response) = native_request(&socket, &pair);
        if status == 403 && response.contains("identity-unverified") {
            // An unsigned test binary has no identity to verify. The native
            // refusal itself is the contract then; the browser tests cover
            // the rest of the flow.
            eprintln!("skipping native identity flow: this test binary has no signing identity");
            return;
        }
        assert_eq!(status, 200, "{response}");
        let code: String = serde_json::from_str::<serde_json::Value>(response_body(&response))
            .unwrap()["code"]
            .as_str()
            .unwrap()
            .to_owned();
        let prompt = gate.last_prompt().unwrap();
        assert!(matches!(prompt.recipient, Recipient::Native { .. }));
        assert_eq!(prompt.intent, Intent::Record);
        // This unsigned (on macOS at most ad-hoc signed, so unanchored)
        // test binary is named by its executable.
        {
            let Some(Verification::Executable { path }) = prompt.recipient.verification() else {
                panic!("{:?}", prompt.recipient);
            };
            assert!(same_file::is_same_file(path, std::env::current_exe().unwrap()).unwrap());
        }
        #[cfg(windows)]
        assert!(
            fs::read_to_string(&socket)
                .unwrap()
                .starts_with(r"\\.\pipe\anomaly.hex.record.")
        );
        // The claimed name never becomes the identity.
        assert!(prompt.recipient.claimed_name().is_none());
        assert_eq!(recording.starts(), 0);

        let exchange = format!(r#"{{"code":"{code}","codeVerifier":"{VERIFIER}"}}"#);
        let token_path = format!(
            "POST {} HTTP/1.1\r\nHost: hex\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{exchange}",
            record::TOKEN_PATH,
            exchange.len()
        );
        let (status, response) = native_request(&socket, &token_path);
        assert_eq!(status, 200, "{response}");
        let grant: String = serde_json::from_str::<serde_json::Value>(response_body(&response))
            .unwrap()["grantToken"]
            .as_str()
            .unwrap()
            .to_owned();

        let grant_request = |path: &str| {
            format!(
                "POST {path} HTTP/1.1\r\nHost: hex\r\nX-Hex-Record-Grant: {grant}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
        };
        let session_request = |path: &str, method: &str, token: &str| {
            format!(
                "{method} {path} HTTP/1.1\r\nHost: hex\r\nX-Hex-Record-Token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
        };
        let mut sessions_seen = Vec::new();
        for _ in 0..2 {
            let (status, response) = native_request(&socket, &grant_request(record::START_PATH));
            assert_eq!(status, 201, "{response}");
            let started: serde_json::Value =
                serde_json::from_str(response_body(&response)).unwrap();
            let token = started["sessionToken"].as_str().unwrap().to_owned();
            sessions_seen.push(token.clone());
            assert_eq!(recording.starts() as usize, sessions_seen.len());
            let (status, response) = native_request(
                &socket,
                &session_request(record::FINISH_PATH, "POST", &token),
            );
            assert_eq!(status, 200, "{response}");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(response_body(&response)).unwrap()["transcript"],
                "native words"
            );
        }
        assert_ne!(sessions_seen[0], sessions_seen[1]);
        assert_eq!(gate.asks(), 1);

        // The native socket is not the local API: no health, no bearer route.
        let (status, _) = native_request(
            &socket,
            "GET /health HTTP/1.1\r\nHost: hex\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(status, 404);
        let (status, _) = native_request(
            &socket,
            &format!(
                "GET /health HTTP/1.1\r\nHost: hex\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
                api.document.token
            ),
        );
        assert_eq!(status, 404);

        // The paired client can give its grant back over the same socket.
        let (status, response) = native_request(&socket, &grant_request(record::REVOKE_PATH));
        assert_eq!(status, 204, "{response}");
        let (status, _) = native_request(&socket, &grant_request(record::START_PATH));
        assert_eq!(status, 401);

        // Shut down, Hex takes its socket (or the pipe's name) away.
        api.shutdown();
        assert!(!socket.exists());
    }

    /// Sessions that always allow `https://app.example`, with `grace`, and
    /// a grant (Always) for it with one recording started.
    fn recording_session(
        recording: &Arc<FakeRecording>,
        grace: Duration,
    ) -> (Arc<record::Sessions>, record::Channel, String, String) {
        let sessions = Arc::new(record::Sessions::with_timings(
            connected(),
            Some(ScriptedGate::new(Answer::Always)),
            recording.clone(),
            allowing(
                Permissions::new(Some(temp_directory().join("permissions.json"))),
                &app_origin(),
            ),
            record::Timings {
                grace,
                ..record_timings()
            },
        ));
        let channel = record::Channel::Web("https://app.example".into());
        let body = format!(r#"{{"codeChallenge":"{CHALLENGE}"}}"#);
        let paired = sessions.pair(channel.clone(), body.as_bytes());
        let code = serde_json::from_slice::<serde_json::Value>(&paired.body).unwrap()["code"]
            .as_str()
            .unwrap()
            .to_owned();
        let exchange = format!(r#"{{"code":"{code}","codeVerifier":"{VERIFIER}"}}"#);
        let minted: serde_json::Value =
            serde_json::from_slice(&sessions.exchange(channel.clone(), exchange.as_bytes()).body)
                .unwrap();
        let grant = minted["grantToken"].as_str().unwrap().to_owned();
        let started = sessions.start(&grant, &channel);
        assert_eq!(started.status, 201);
        let session =
            serde_json::from_slice::<serde_json::Value>(&started.body).unwrap()["sessionToken"]
                .as_str()
                .unwrap()
                .to_owned();
        (sessions, channel, grant, session)
    }

    /// Stopped from Hex's window and never collected, a recording is
    /// discarded after the grace, as a lapsed lease's is, and frees the one
    /// session for the next start.
    #[test]
    fn a_recording_stopped_in_the_window_expires_without_a_finish() {
        let recording = FakeRecording::with_text("window words");
        let (sessions, channel, grant, session) =
            recording_session(&recording, Duration::from_millis(50));
        let id = sessions.view().unwrap().id;
        assert!(sessions.stop(id));
        assert_eq!(sessions.start(&grant, &channel).status, 409);
        let deadline = Instant::now() + TEST_WAIT;
        while sessions.view().is_some() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(sessions.view().is_none(), "the stopped recording expired");
        assert_eq!(sessions.finish(&session, &channel).status, 409);
        assert_eq!(sessions.start(&grant, &channel).status, 201);
    }

    /// A finish that fails ends the recording like a failed Stop: cancelled,
    /// cleared, and no longer holding the session another start needs.
    #[test]
    fn a_failed_finish_ends_the_recording() {
        let recording = FakeRecording::with_text("lost words");
        recording.finish_fails.store(true, Ordering::Release);
        let (sessions, channel, grant, session) =
            recording_session(&recording, Duration::from_secs(60));
        let finished = sessions.finish(&session, &channel);
        assert_eq!(finished.status, 500);
        assert_eq!(recording.cancels(), 1);
        assert!(sessions.view().is_none());
        assert_eq!(sessions.finish(&session, &channel).status, 409);
        recording.finish_fails.store(false, Ordering::Release);
        assert_eq!(sessions.start(&grant, &channel).status, 201);
    }

    #[test]
    fn the_window_stops_or_cancels_a_live_recording() {
        let gate = ScriptedGate::new(Answer::Always);
        let recording = FakeRecording::with_text("window words");
        let sessions = Arc::new(record::Sessions::with_timings(
            connected(),
            Some(gate),
            recording.clone(),
            allowing(
                Permissions::new(Some(temp_directory().join("permissions.json"))),
                &app_origin(),
            ),
            record_timings(),
        ));
        let origin = "https://app.example";
        let channel = record::Channel::Web(origin.into());
        let body = format!(r#"{{"codeChallenge":"{CHALLENGE}"}}"#);
        let paired = sessions.pair(channel.clone(), body.as_bytes());
        let code: String =
            serde_json::from_slice::<serde_json::Value>(&paired.body).unwrap()["code"]
                .as_str()
                .unwrap()
                .to_owned();
        let exchange = format!(r#"{{"code":"{code}","codeVerifier":"{VERIFIER}"}}"#);
        let minted: serde_json::Value =
            serde_json::from_slice(&sessions.exchange(channel.clone(), exchange.as_bytes()).body)
                .unwrap();
        let grant = minted["grantToken"].as_str().unwrap().to_owned();
        let started = sessions.start(&grant, &channel);
        assert_eq!(started.status, 201);
        let session =
            serde_json::from_slice::<serde_json::Value>(&started.body).unwrap()["sessionToken"]
                .as_str()
                .unwrap()
                .to_owned();
        let id = sessions.view().map(|view| view.id).unwrap();
        assert_eq!(
            sessions.view().map(|view| view.phase),
            Some(record::Phase::Recording)
        );

        // The window's Stop keeps the transcript for the client.
        assert!(sessions.stop(id));
        assert_eq!(recording.finishes(), 1);
        assert_eq!(
            sessions.view().map(|view| view.phase),
            Some(record::Phase::Stopped)
        );
        let finished = sessions.finish(&session, &channel);
        assert_eq!(finished.status, 200);

        // A second recording cancelled from the window is discarded.
        let started = sessions.start(&grant, &channel);
        assert_eq!(started.status, 201);
        let id = sessions.view().map(|view| view.id).unwrap();
        sessions.cancel(id);
        assert_eq!(recording.cancels(), 1);
        assert!(sessions.view().is_none());
    }

    fn unused_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }
}
