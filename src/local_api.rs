use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};

use crate::events::{EventLog, VoiceEvent, now_ms};

const API_VERSION: &str = "1";
const LIVE_PROBE_TIMEOUT: Duration = Duration::from_millis(150);
const ACCEPT_INTERVAL: Duration = Duration::from_millis(10);
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
const MAX_AUDIO_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEVELOPER_CONTROL_BYTES: usize = 16 * 1024;
const DEVELOPER_CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

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
    transcription_service: Option<crate::transcription_service::TranscriptionService>,
    _instance: Option<crate::instance::InstanceLock>,
    stopped: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryDocument {
    port: u16,
    token: String,
    api_version: String,
    pid: u32,
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

struct DiscoveryLease {
    path: PathBuf,
    document: DiscoveryDocument,
}

struct HttpRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    content_type: Option<String>,
    content_length: Option<usize>,
    body_prefix: Vec<u8>,
}

struct HttpResponse {
    status: u16,
    reason: &'static str,
    content_type: Option<&'static str>,
    body: Vec<u8>,
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
    transcription: crate::transcription_service::TranscriptionServiceHandle,
    developer_control: Option<SyncSender<crate::developer_control::DeveloperCall>>,
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
    developer_control: bool,
}

impl LocalApi {
    pub fn start(events: EventLog) -> Result<Self> {
        let instance = crate::instance::acquire("local-api")?;
        let mut api = Self::start_at(
            crate::app_paths::local_api_discovery_file()?,
            Arc::new(current_health),
            events,
        )?;
        api._instance = Some(instance);
        Ok(api)
    }

    pub fn start_with_developer_control(
        events: EventLog,
        developer_control: SyncSender<crate::developer_control::DeveloperCall>,
    ) -> Result<Self> {
        let instance = crate::instance::acquire("local-api")?;
        let mut api = Self::start_bound(
            Some(crate::app_paths::local_api_discovery_file()?),
            Arc::new(current_health),
            events,
            Some(developer_control),
        )?;
        api._instance = Some(instance);
        Ok(api)
    }

    pub fn start_embedded(events: EventLog) -> Result<Self> {
        Self::start_without_discovery(Arc::new(current_health), events)
    }

    fn start_at(discovery_path: PathBuf, health: HealthProvider, events: EventLog) -> Result<Self> {
        prepare_discovery_path(&discovery_path)?;
        Self::start_bound(Some(discovery_path), health, events, None)
    }

    fn start_without_discovery(health: HealthProvider, events: EventLog) -> Result<Self> {
        Self::start_bound(None, health, events, None)
    }

    fn start_bound(
        discovery_path: Option<PathBuf>,
        health: HealthProvider,
        events: EventLog,
        developer_control: Option<SyncSender<crate::developer_control::DeveloperCall>>,
    ) -> Result<Self> {
        let (transcription_service, transcription) =
            crate::transcription_service::TranscriptionService::start()?;
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
        let http_context = HttpContext {
            token: token.into(),
            health,
            events: worker_events.clone(),
            auth_limiter: AuthObservationLimiter::default(),
            model_preparing: Arc::new(AtomicBool::new(false)),
            shutdown: worker_shutdown,
            transcription,
            developer_control,
        };
        let (ready, readiness) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("local-api".into())
            .spawn(move || {
                serve(listener, http_context, ready);
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
            _instance: None,
            stopped: false,
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
        if self.stopped {
            return;
        }
        self.stopped = true;
        if let Some(discovery) = &self.discovery {
            discovery.remove_if_owned();
        }
        self.shutdown.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::error!("local API thread panicked during shutdown");
        }
        if let Some(mut service) = self.transcription_service.take() {
            service.shutdown();
        }
        observe_stopped(&self.events, &self.stop_observed);
        tracing::info!("local API stopped");
    }
}

impl Drop for LocalApi {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl DiscoveryLease {
    fn remove_if_owned(&self) {
        remove_discovery_if_owned(&self.path, &self.document);
    }
}

fn remove_discovery_if_owned(path: &Path, expected: &DiscoveryDocument) {
    let owned = fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice::<DiscoveryDocument>(&data).ok())
        .is_some_and(|document| document == *expected);
    if owned
        && let Err(error) = fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::error!(%error, path = %path.display(), "could not remove local API discovery file");
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

impl Drop for DiscoveryLease {
    fn drop(&mut self) {
        self.remove_if_owned();
    }
}

fn serve(listener: TcpListener, context: HttpContext, ready: mpsc::SyncSender<bool>) {
    let (connections, pending) = mpsc::sync_channel(CONNECTION_QUEUE_CAPACITY);
    let pending = Arc::new(Mutex::new(pending));
    let shutdown = context.shutdown.clone();
    let context = Arc::new(context);
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
            Ok((stream, _)) => match connections.try_send(stream) {
                Ok(()) => {}
                Err(TrySendError::Full(mut stream)) => {
                    let _ = write_response(
                        &mut stream,
                        HttpResponse::empty(503, "Service Unavailable"),
                    );
                }
                Err(TrySendError::Disconnected(_)) => break,
            },
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_INTERVAL);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                tracing::error!(%error, "local API accept failed");
                thread::sleep(ACCEPT_INTERVAL);
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

fn http_worker(pending: Arc<Mutex<Receiver<TcpStream>>>, context: Arc<HttpContext>) {
    loop {
        let stream = pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .recv();
        let Ok(mut stream) = stream else {
            return;
        };
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
            Err(RequestReadError::TooLarge) => {
                RequestAction::Respond(HttpResponse::empty(431, "Request Header Fields Too Large"))
            }
            Err(RequestReadError::Invalid) => {
                RequestAction::Respond(HttpResponse::empty(400, "Bad Request"))
            }
            Err(RequestReadError::Deadline) => {
                RequestAction::Respond(HttpResponse::empty(408, "Request Timeout"))
            }
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
            } => {
                let response = transcribe_audio(
                    &mut stream,
                    selection,
                    content_length,
                    body_prefix,
                    &context.transcription,
                    &context.shutdown,
                    admission,
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
                let response = developer_control(
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
        }
    }
}

fn handle_request(request: HttpRequest, context: &HttpContext) -> RequestAction {
    let path = request.path.split('?').next().unwrap_or("/");
    if request.authorization.as_deref() != Some(&format!("Bearer {}", context.token)) {
        context
            .auth_limiter
            .observe(&context.events, &request.method, path);
        return RequestAction::Respond(HttpResponse::empty(401, "Unauthorized"));
    }
    if request.method == "POST" && path == "/transcriptions" {
        return transcription_action(request, &context.transcription);
    }
    if request.method == "POST" && path == "/dev/control" {
        return developer_control_action(request, context.developer_control.is_some());
    }

    let response = match (request.method.as_str(), path) {
        ("GET", "/health") => HttpResponse::json(200, "OK", &(context.health)()),
        ("GET", "/capabilities") => HttpResponse::json(
            200,
            "OK",
            &CapabilitiesResponse {
                audio_formats: ["audio/wav"],
                partial_transcripts: false,
                service_capture: false,
                developer_control: context.developer_control.is_some(),
            },
        ),
        ("GET", "/models") => match model_infos(&request.path) {
            Ok(models) => HttpResponse::json(200, "OK", &models),
            Err(code) => {
                HttpResponse::json(400, "Bad Request", &serde_json::json!({ "code": code }))
            }
        },
        ("POST", path) => {
            let Some(id) = model_prepare_path(path) else {
                return RequestAction::Respond(HttpResponse::json(
                    404,
                    "Not Found",
                    &serde_json::json!({ "code": "not_found" }),
                ));
            };
            let Ok(id) = id.parse::<crate::transcription_models::TranscriptionModelId>() else {
                return RequestAction::Respond(HttpResponse::json(
                    404,
                    "Not Found",
                    &serde_json::json!({ "code": "unknown-model" }),
                ));
            };
            let model = crate::transcription_models::definition(id);
            if !model.available() {
                return RequestAction::Respond(HttpResponse::json(
                    404,
                    "Not Found",
                    &serde_json::json!({ "code": "unknown-model" }),
                ));
            }
            let Ok(selection) = installation_selection(model, &request.path) else {
                return RequestAction::Respond(HttpResponse::json(
                    400,
                    "Bad Request",
                    &serde_json::json!({ "code": "unsupported-model" }),
                ));
            };
            if context
                .model_preparing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return RequestAction::Respond(HttpResponse::json(
                    409,
                    "Conflict",
                    &serde_json::json!({ "code": "prepare-busy" }),
                ));
            }
            return RequestAction::Prepare { model, selection };
        }
        _ => HttpResponse::json(
            404,
            "Not Found",
            &serde_json::json!({ "code": "not_found" }),
        ),
    };
    RequestAction::Respond(response)
}

fn transcription_action(
    request: HttpRequest,
    transcription: &crate::transcription_service::TranscriptionServiceHandle,
) -> RequestAction {
    if !request
        .content_type
        .as_deref()
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("audio/wav"))
    {
        return RequestAction::Respond(HttpResponse::json(
            415,
            "Unsupported Media Type",
            &serde_json::json!({ "code": "unsupported-audio" }),
        ));
    }
    let Some(content_length) = request.content_length else {
        return RequestAction::Respond(HttpResponse::json(
            411,
            "Length Required",
            &serde_json::json!({ "code": "length-required" }),
        ));
    };
    if content_length > MAX_AUDIO_BYTES {
        return RequestAction::Respond(HttpResponse::json(
            413,
            "Content Too Large",
            &serde_json::json!({ "code": "resource-exhausted" }),
        ));
    }
    let selection = match transcription_selection(&request.path) {
        Ok(selection) => selection,
        Err(code) => {
            return RequestAction::Respond(HttpResponse::json(
                400,
                "Bad Request",
                &serde_json::json!({ "code": code }),
            ));
        }
    };
    let model = crate::transcription_models::definition(selection.model);
    let prepared = match model.runtime {
        crate::transcription_models::ModelRuntime::Gguf(_) => {
            crate::transcription_models::is_verified(model)
        }
        crate::transcription_models::ModelRuntime::AppleSpeech => {
            crate::transcription_models::is_installed(model, &selection.language)
        }
    };
    if !prepared {
        return RequestAction::Respond(HttpResponse::json(
            409,
            "Conflict",
            &serde_json::json!({ "code": "model-not-ready" }),
        ));
    }
    let Some(admission) = transcription.admit_audio() else {
        return RequestAction::Respond(HttpResponse::json(
            429,
            "Too Many Requests",
            &serde_json::json!({ "code": "queue-full" }),
        ));
    };
    RequestAction::Transcribe {
        selection,
        content_length,
        body_prefix: request.body_prefix,
        admission,
    }
}

fn developer_control_action(request: HttpRequest, available: bool) -> RequestAction {
    if !available {
        return RequestAction::Respond(HttpResponse::json(
            404,
            "Not Found",
            &serde_json::json!({ "code": "developer-control-unavailable" }),
        ));
    }
    if !request
        .content_type
        .as_deref()
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    {
        return RequestAction::Respond(HttpResponse::json(
            415,
            "Unsupported Media Type",
            &serde_json::json!({ "code": "unsupported-content-type" }),
        ));
    }
    let Some(content_length) = request.content_length else {
        return RequestAction::Respond(HttpResponse::json(
            411,
            "Length Required",
            &serde_json::json!({ "code": "length-required" }),
        ));
    };
    if content_length > MAX_DEVELOPER_CONTROL_BYTES {
        return RequestAction::Respond(HttpResponse::json(
            413,
            "Content Too Large",
            &serde_json::json!({ "code": "developer-control-too-large" }),
        ));
    }
    RequestAction::DeveloperControl {
        content_length,
        body_prefix: request.body_prefix,
    }
}

fn developer_control(
    stream: &mut TcpStream,
    content_length: usize,
    mut body: Vec<u8>,
    controls: Option<&SyncSender<crate::developer_control::DeveloperCall>>,
    shutdown: &AtomicBool,
) -> HttpResponse {
    if let Err(error) = read_body_bounded(
        stream,
        content_length,
        &mut body,
        shutdown,
        MAX_DEVELOPER_CONTROL_BYTES,
        DEVELOPER_CONTROL_TIMEOUT,
    ) {
        return body_read_response(error);
    }
    let Ok(command) = serde_json::from_slice(&body) else {
        return HttpResponse::json(
            400,
            "Bad Request",
            &serde_json::json!({ "code": "invalid-developer-command" }),
        );
    };
    let Some(controls) = controls else {
        return HttpResponse::json(
            404,
            "Not Found",
            &serde_json::json!({ "code": "developer-control-unavailable" }),
        );
    };
    let (reply, response) = mpsc::sync_channel(1);
    match controls.try_send(crate::developer_control::DeveloperCall { command, reply }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            return HttpResponse::json(
                503,
                "Service Unavailable",
                &serde_json::json!({ "code": "developer-control-busy" }),
            );
        }
        Err(TrySendError::Disconnected(_)) => {
            return HttpResponse::json(
                503,
                "Service Unavailable",
                &serde_json::json!({ "code": "developer-control-unavailable" }),
            );
        }
    }
    match response.recv_timeout(DEVELOPER_CONTROL_TIMEOUT) {
        Ok(reply) => HttpResponse::json(200, "OK", &reply),
        Err(RecvTimeoutError::Timeout) => HttpResponse::json(
            504,
            "Gateway Timeout",
            &serde_json::json!({ "code": "developer-ui-timeout" }),
        ),
        Err(RecvTimeoutError::Disconnected) => HttpResponse::json(
            503,
            "Service Unavailable",
            &serde_json::json!({ "code": "developer-control-unavailable" }),
        ),
    }
}

fn body_read_response(error: BodyReadError) -> HttpResponse {
    match error {
        BodyReadError::ResourceExhausted => HttpResponse::json(
            413,
            "Content Too Large",
            &serde_json::json!({ "code": "resource-exhausted" }),
        ),
        BodyReadError::Invalid => HttpResponse::json(
            400,
            "Bad Request",
            &serde_json::json!({ "code": "invalid-body" }),
        ),
        BodyReadError::Deadline => HttpResponse::json(
            408,
            "Request Timeout",
            &serde_json::json!({ "code": "request-timeout" }),
        ),
        BodyReadError::Shutdown => HttpResponse::json(
            503,
            "Service Unavailable",
            &serde_json::json!({ "code": "shutting-down" }),
        ),
        BodyReadError::Io(error) => {
            tracing::debug!(%error, "could not read request body");
            HttpResponse::json(
                400,
                "Bad Request",
                &serde_json::json!({ "code": "invalid-body" }),
            )
        }
    }
}

fn transcription_selection(
    path: &str,
) -> std::result::Result<crate::transcription_models::TranscriptionSelection, &'static str> {
    let query = selection_query(path)?;
    let model = query.model.ok_or("unknown-model")?;
    let selection = crate::transcription_models::TranscriptionSelection {
        model,
        language: query.language.unwrap_or_else(|| {
            crate::transcription_models::TranscriptionSelection::default().language
        }),
        recognition_hints: String::new(),
    };
    crate::transcription_models::validate(&selection).map_err(|_| "unsupported-model")?;
    Ok(selection)
}

#[derive(Default)]
struct SelectionQuery {
    model: Option<crate::transcription_models::TranscriptionModelId>,
    language: Option<String>,
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

fn transcribe_audio(
    stream: &mut TcpStream,
    selection: crate::transcription_models::TranscriptionSelection,
    content_length: usize,
    mut body: Vec<u8>,
    transcription: &crate::transcription_service::TranscriptionServiceHandle,
    shutdown: &AtomicBool,
    admission: crate::transcription_service::AudioAdmission,
) -> Option<HttpResponse> {
    if let Err(error) = read_body(stream, content_length, &mut body, shutdown) {
        return Some(match error {
            BodyReadError::ResourceExhausted => HttpResponse::json(
                413,
                "Content Too Large",
                &serde_json::json!({ "code": "resource-exhausted" }),
            ),
            BodyReadError::Invalid => HttpResponse::json(
                400,
                "Bad Request",
                &serde_json::json!({ "code": "invalid-audio" }),
            ),
            BodyReadError::Io(error) => {
                tracing::debug!(%error, "audio upload stopped before completion");
                HttpResponse::json(
                    400,
                    "Bad Request",
                    &serde_json::json!({ "code": "invalid-audio" }),
                )
            }
            BodyReadError::Deadline => HttpResponse::json(
                408,
                "Request Timeout",
                &serde_json::json!({ "code": "upload-timeout" }),
            ),
            BodyReadError::Shutdown => return None,
        });
    }
    let clip = match crate::transcription_service::AudioClip::decode_wav(Cursor::new(body)) {
        Ok(clip) => clip,
        Err(crate::transcription_service::AudioError::ResourceExhausted) => {
            return Some(HttpResponse::json(
                413,
                "Content Too Large",
                &serde_json::json!({ "code": "resource-exhausted" }),
            ));
        }
        Err(crate::transcription_service::AudioError::Unsupported) => {
            return Some(HttpResponse::json(
                415,
                "Unsupported Media Type",
                &serde_json::json!({ "code": "unsupported-audio" }),
            ));
        }
        Err(crate::transcription_service::AudioError::Invalid) => {
            return Some(HttpResponse::json(
                400,
                "Bad Request",
                &serde_json::json!({ "code": "invalid-audio" }),
            ));
        }
    };
    let duration_ms = clip.duration_ms;
    let job = match transcription.transcribe(selection, clip, admission) {
        Ok(job) => job,
        Err(error) => return Some(transcription_error(error)),
    };
    loop {
        match job.wait_timeout(TRANSCRIPTION_POLL_INTERVAL) {
            Ok(Some(Ok(transcript))) => {
                return Some(HttpResponse::json(
                    200,
                    "OK",
                    &serde_json::json!({
                        "transcript": transcript,
                        "durationMs": duration_ms,
                    }),
                ));
            }
            Ok(Some(Err(error))) | Err(error) => return Some(transcription_error(error)),
            Ok(None) => {
                if shutdown.load(Ordering::Acquire) || client_aborted(stream) {
                    job.cancel();
                    return None;
                }
            }
        }
    }
}

fn transcription_error(
    error: crate::transcription_service::TranscriptionServiceError,
) -> HttpResponse {
    match error {
        crate::transcription_service::TranscriptionServiceError::QueueFull => HttpResponse::json(
            429,
            "Too Many Requests",
            &serde_json::json!({ "code": "queue-full" }),
        ),
        crate::transcription_service::TranscriptionServiceError::Unavailable => HttpResponse::json(
            503,
            "Service Unavailable",
            &serde_json::json!({ "code": "service-gone" }),
        ),
        crate::transcription_service::TranscriptionServiceError::Cancelled => {
            HttpResponse::json(409, "Conflict", &serde_json::json!({ "code": "cancelled" }))
        }
        crate::transcription_service::TranscriptionServiceError::Model(error) => {
            tracing::error!(%error, "prepared transcription model could not load");
            HttpResponse::json(
                500,
                "Internal Server Error",
                &serde_json::json!({ "code": "model-load-failed" }),
            )
        }
        crate::transcription_service::TranscriptionServiceError::Inference(error) => {
            tracing::error!(%error, "local transcription failed");
            HttpResponse::json(
                500,
                "Internal Server Error",
                &serde_json::json!({ "code": "transcription-failed" }),
            )
        }
    }
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

enum BodyReadError {
    ResourceExhausted,
    Invalid,
    Deadline,
    Shutdown,
    Io(std::io::Error),
}

fn read_body(
    stream: &mut TcpStream,
    content_length: usize,
    body: &mut Vec<u8>,
    shutdown: &AtomicBool,
) -> std::result::Result<(), BodyReadError> {
    read_body_bounded(
        stream,
        content_length,
        body,
        shutdown,
        MAX_AUDIO_BYTES,
        UPLOAD_DEADLINE,
    )
}

fn read_body_bounded(
    stream: &mut TcpStream,
    content_length: usize,
    body: &mut Vec<u8>,
    shutdown: &AtomicBool,
    maximum: usize,
    timeout: Duration,
) -> std::result::Result<(), BodyReadError> {
    if content_length > maximum {
        return Err(BodyReadError::ResourceExhausted);
    }
    if body.len() > content_length {
        return Err(BodyReadError::Invalid);
    }
    body.try_reserve_exact(content_length - body.len())
        .map_err(|_| BodyReadError::ResourceExhausted)?;
    let deadline = Instant::now() + timeout;
    let mut buffer = [0_u8; 16 * 1024];
    while body.len() < content_length {
        if shutdown.load(Ordering::Acquire) {
            return Err(BodyReadError::Shutdown);
        }
        if Instant::now() >= deadline {
            return Err(BodyReadError::Deadline);
        }
        let remaining = content_length - body.len();
        let read_length = remaining.min(buffer.len());
        let read = match stream.read(&mut buffer[..read_length]) {
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(BodyReadError::Io(error)),
        };
        if Instant::now() >= deadline {
            return Err(BodyReadError::Deadline);
        }
        if read == 0 {
            return Err(BodyReadError::Invalid);
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Ok(())
}

fn model_prepare_path(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/models/")?.strip_suffix("/prepare")?;
    (!id.is_empty() && !id.contains('/')).then_some(id)
}

fn model_infos(path: &str) -> std::result::Result<Vec<ModelInfo>, &'static str> {
    let language = selection_query(path)?
        .language
        .unwrap_or_else(|| crate::transcription_models::TranscriptionSelection::default().language);
    if !crate::transcription_models::LANGUAGES
        .iter()
        .any(|(code, _)| *code == language)
    {
        return Err("unsupported-language");
    }
    Ok(crate::transcription_models::MODELS
        .iter()
        .filter(|model| model.available())
        .map(|model| {
            let managed = matches!(
                model.runtime,
                crate::transcription_models::ModelRuntime::AppleSpeech
            );
            let installed = crate::transcription_models::is_installed(model, &language);
            ModelInfo {
                id: model.id.as_str(),
                name: model.name,
                installed,
                verified: managed && installed || crate::transcription_models::is_verified(model),
                managed,
                download_bytes: model.download_bytes(),
                languages: model.presentation_languages(),
                supports_language_detection: model.supports_language_detection,
            }
        })
        .collect())
}

fn installation_selection(
    model: &crate::transcription_models::ModelDefinition,
    path: &str,
) -> Result<crate::transcription_models::TranscriptionSelection> {
    let query = selection_query(path).map_err(|code| eyre!(code))?;
    let selection = crate::transcription_models::TranscriptionSelection {
        model: model.id,
        language: query.language.unwrap_or_else(|| {
            crate::transcription_models::TranscriptionSelection::default().language
        }),
        recognition_hints: String::new(),
    };
    crate::transcription_models::validate(&selection)?;
    Ok(selection)
}

fn stream_model_prepare(
    stream: &mut TcpStream,
    model: &'static crate::transcription_models::ModelDefinition,
    selection: crate::transcription_models::TranscriptionSelection,
    preparing: Arc<AtomicBool>,
    shutdown: &AtomicBool,
    transcription: crate::transcription_service::TranscriptionServiceHandle,
) {
    let preparer: ModelPreparer = Box::new(move |canceled, downloaded, stage| {
        if matches!(
            model.runtime,
            crate::transcription_models::ModelRuntime::Gguf(_)
        ) {
            crate::transcription_models::download_with_stage_progress(
                model,
                &canceled,
                &downloaded,
                &stage,
            )?;
        }
        if canceled.load(Ordering::Acquire) {
            bail!("model preparation canceled");
        }
        crate::transcription_models::ModelPreparationStage::Loading.store(&stage);
        transcription
            .prepare(selection)
            .map_err(|error| eyre!("model preparation failed: {error:?}"))
    });
    stream_model_prepare_with(stream, model, preparing, shutdown, preparer);
}

fn stream_model_prepare_with(
    stream: &mut TcpStream,
    model: &'static crate::transcription_models::ModelDefinition,
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
        model.runtime,
        crate::transcription_models::ModelRuntime::AppleSpeech
    ) {
        crate::transcription_models::ModelPreparationStage::Loading as u8
    } else {
        crate::transcription_models::ModelPreparationStage::Downloading as u8
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
    let result = loop {
        if shutdown.load(Ordering::Acquire) {
            canceled.store(true, Ordering::Release);
            connected = false;
            break None;
        }
        let current_stage = crate::transcription_models::ModelPreparationStage::load(&stage);
        let current_downloaded = downloaded.load(Ordering::Relaxed);
        let progress = match current_stage {
            crate::transcription_models::ModelPreparationStage::Downloading
                if last_stage != Some(current_stage) || last_downloaded != current_downloaded =>
            {
                Some(serde_json::json!({
                    "type": "downloading",
                    "downloadedBytes": current_downloaded,
                    "totalBytes": total_bytes,
                }))
            }
            crate::transcription_models::ModelPreparationStage::Verifying
                if last_stage != Some(current_stage) =>
            {
                Some(serde_json::json!({ "type": "verifying" }))
            }
            crate::transcription_models::ModelPreparationStage::Loading
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
        match result_receiver.try_recv() {
            Ok(result) => {
                break Some((
                    result,
                    crate::transcription_models::ModelPreparationStage::load(&stage),
                ));
            }
            Err(TryRecvError::Disconnected) => {
                break Some((
                    Err(eyre!("model preparer stopped unexpectedly")),
                    crate::transcription_models::ModelPreparationStage::load(&stage),
                ));
            }
            Err(TryRecvError::Empty) => {}
        }
        if last_heartbeat.elapsed() >= Duration::from_secs(5) {
            if stream.write_all(b": keep-alive\n\n").is_err() || stream.flush().is_err() {
                canceled.store(true, Ordering::Release);
                connected = false;
                break None;
            }
            last_heartbeat = Instant::now();
        }
        thread::sleep(Duration::from_millis(100));
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
    let Some((result, failed_stage)) = result else {
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
            let code = match failed_stage {
                crate::transcription_models::ModelPreparationStage::Downloading => {
                    "download-failed"
                }
                crate::transcription_models::ModelPreparationStage::Verifying => {
                    "verification-failed"
                }
                crate::transcription_models::ModelPreparationStage::Loading => "load-failed",
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

fn write_sse_headers(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n",
    )?;
    stream.flush()
}

fn write_sse(stream: &mut TcpStream, value: &impl Serialize) -> std::io::Result<()> {
    stream.write_all(b"data: ")?;
    serde_json::to_writer(&mut *stream, value)?;
    stream.write_all(b"\n\n")?;
    stream.flush()
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

impl HttpResponse {
    fn empty(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            content_type: None,
            body: Vec::new(),
        }
    }

    fn json<T: Serialize>(status: u16, reason: &'static str, value: &T) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) => Self {
                status,
                reason,
                content_type: Some("application/json"),
                body,
            },
            Err(error) => {
                tracing::error!(%error, "could not serialize local API response");
                Self::empty(500, "Internal Server Error")
            }
        }
    }
}

#[derive(Debug)]
enum RequestReadError {
    TooLarge,
    Invalid,
    Deadline,
    Shutdown,
    Io(std::io::Error),
}

enum RequestAction {
    Respond(HttpResponse),
    Prepare {
        model: &'static crate::transcription_models::ModelDefinition,
        selection: crate::transcription_models::TranscriptionSelection,
    },
    Transcribe {
        selection: crate::transcription_models::TranscriptionSelection,
        content_length: usize,
        body_prefix: Vec<u8>,
        admission: crate::transcription_service::AudioAdmission,
    },
    DeveloperControl {
        content_length: usize,
        body_prefix: Vec<u8>,
    },
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
    id: &'static str,
    name: &'static str,
    installed: bool,
    verified: bool,
    managed: bool,
    download_bytes: Option<u64>,
    languages: Vec<&'static str>,
    supports_language_detection: bool,
}

fn read_request(
    stream: &mut TcpStream,
    shutdown: &AtomicBool,
) -> std::result::Result<HttpRequest, RequestReadError> {
    read_request_until(stream, shutdown, Instant::now() + HEADER_DEADLINE)
}

fn read_request_until(
    stream: &mut TcpStream,
    shutdown: &AtomicBool,
    deadline: Instant,
) -> std::result::Result<HttpRequest, RequestReadError> {
    let mut data = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        if shutdown.load(Ordering::Acquire) {
            return Err(RequestReadError::Shutdown);
        }
        if Instant::now() >= deadline {
            return Err(RequestReadError::Deadline);
        }
        let read = match stream.read(&mut buffer) {
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(RequestReadError::Io(error)),
        };
        if Instant::now() >= deadline {
            return Err(RequestReadError::Deadline);
        }
        if read == 0 {
            return Err(RequestReadError::Invalid);
        }
        if data.len() + read > MAX_HEADER_BYTES {
            return Err(RequestReadError::TooLarge);
        }
        data.extend_from_slice(&buffer[..read]);
        if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
    };
    let header = std::str::from_utf8(&data[..header_end]).map_err(|_| RequestReadError::Invalid)?;
    let mut lines = header.split("\r\n");
    let mut request_line = lines
        .next()
        .ok_or(RequestReadError::Invalid)?
        .split_ascii_whitespace();
    let method = request_line.next().ok_or(RequestReadError::Invalid)?;
    let path = request_line.next().ok_or(RequestReadError::Invalid)?;
    let version = request_line.next().ok_or(RequestReadError::Invalid)?;
    if request_line.next().is_some()
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || !path.starts_with('/')
        || path.len() > MAX_PATH_BYTES
    {
        return Err(RequestReadError::Invalid);
    }
    let mut authorization = None;
    let mut content_type = None;
    let mut content_length = None;
    for (index, line) in lines.enumerate() {
        if index >= MAX_HEADERS {
            return Err(RequestReadError::TooLarge);
        }
        let (name, value) = line.split_once(':').ok_or(RequestReadError::Invalid)?;
        if name.eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("content-type") {
            content_type = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(RequestReadError::Invalid);
            }
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| RequestReadError::Invalid)?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(RequestReadError::Invalid);
        }
    }
    let body_prefix = data[header_end + 4..].to_vec();
    if content_length.is_some_and(|length| body_prefix.len() > length) {
        return Err(RequestReadError::Invalid);
    }
    Ok(HttpRequest {
        method: method.into(),
        path: path.into(),
        authorization,
        content_type,
        content_length,
        body_prefix,
    })
}

fn write_response(stream: &mut TcpStream, response: HttpResponse) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n",
        response.status,
        response.reason,
        response.body.len()
    )?;
    if let Some(content_type) = response.content_type {
        write!(stream, "Content-Type: {content_type}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn current_health() -> HealthResponse {
    HealthResponse {
        version: env!("CARGO_PKG_VERSION"),
        api_version: API_VERSION,
    }
}

pub fn call_developer(
    command: &crate::developer_control::DeveloperCommand,
) -> Result<crate::developer_control::DeveloperReply> {
    let discovery_path = crate::app_paths::local_api_discovery_file()?;
    let document: DiscoveryDocument = serde_json::from_slice(
        &fs::read(&discovery_path)
            .wrap_err_with(|| format!("could not read {}", discovery_path.display()))?,
    )
    .wrap_err("local API discovery is invalid")?;
    if !valid_discovery(&document) || !discovery_is_live(&document) {
        bail!("HEX local API is not running");
    }
    let body = serde_json::to_vec(command)?;
    let address = SocketAddr::from(([127, 0, 0, 1], document.port));
    let mut stream = TcpStream::connect_timeout(&address, CONNECTION_TIMEOUT)
        .wrap_err("could not connect to HEX local API")?;
    stream.set_read_timeout(Some(DEVELOPER_CONTROL_TIMEOUT))?;
    stream.set_write_timeout(Some(CONNECTION_TIMEOUT))?;
    write!(
        stream,
        "POST /dev/control HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        document.token,
        body.len(),
    )?;
    stream.write_all(&body)?;
    stream.flush()?;

    let mut response = Vec::new();
    (&mut stream)
        .take(64 * 1024 + 1)
        .read_to_end(&mut response)?;
    if response.len() > 64 * 1024 {
        bail!("HEX developer response is too large");
    }
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        bail!("HEX developer response is malformed");
    };
    let headers = std::str::from_utf8(&response[..header_end])?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| eyre!("HEX developer response has no HTTP status"))?;
    let body = &response[header_end + 4..];
    if status != 200 {
        let code = serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|value| value.get("code")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("http-{status}"));
        bail!("HEX developer control failed: {code}");
    }
    serde_json::from_slice(body).wrap_err("HEX developer response is invalid")
}

fn generate_token() -> Result<String> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|error| eyre!("could not generate API token: {error}"))?;
    let mut token = String::with_capacity(68);
    token.push_str("hex_");
    for byte in random {
        use std::fmt::Write as _;
        write!(token, "{byte:02x}").expect("writing to a string cannot fail");
    }
    Ok(token)
}

fn prepare_discovery_path(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        bail!("local API discovery path has no parent");
    };
    fs::create_dir_all(parent)?;
    let existing = match fs::read(path) {
        Ok(data) => serde_json::from_slice::<DiscoveryDocument>(&data).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if existing
        .as_ref()
        .is_some_and(|document| valid_discovery(document) && discovery_is_live(document))
    {
        bail!("another local API service appears to be running");
    }
    Ok(())
}

fn valid_discovery(document: &DiscoveryDocument) -> bool {
    document.api_version == API_VERSION
        && document.token.len() == 68
        && document.token.starts_with("hex_")
        && document.token[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn discovery_is_live(document: &DiscoveryDocument) -> bool {
    document.port != 0
        && document.pid != 0
        && process_is_alive(document.pid)
        && TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], document.port)),
            LIVE_PROBE_TIMEOUT,
        )
        .is_ok()
}

fn write_discovery(path: &Path, document: &DiscoveryDocument) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("local API discovery path has no parent"))?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    if let Err(error) = fs::remove_file(&temporary)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error.into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let mut data = serde_json::to_vec(document)?;
    data.push(b'\n');
    file.write_all(&data)?;
    file.sync_all()?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    fs::rename(&temporary, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn process_is_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    unsafe { kill(pid, 0) == 0 }
}

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn authenticated_server_reports_live_health_and_cleans_up_discovery() {
        let directory = temp_directory();
        let discovery_path = directory.join("local-api.json");
        let event_path = directory.join("events.ndjson");
        let events = EventLog::create(&event_path).unwrap();
        let mut api =
            LocalApi::start_at(discovery_path.clone(), Arc::new(test_health), events).unwrap();
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
            "1"
        );
        let capabilities = request(document.port, Some(&document.token), "/capabilities");
        let capabilities: serde_json::Value =
            serde_json::from_str(response_body(&capabilities)).unwrap();
        assert_eq!(capabilities["partialTranscripts"], false);
        assert_eq!(capabilities["serviceCapture"], false);
        assert_eq!(capabilities["developerControl"], false);
        assert_eq!(capabilities["audioFormats"][0], "audio/wav");
        let models = request(document.port, Some(&document.token), "/models");
        let models: Vec<serde_json::Value> = serde_json::from_str(response_body(&models)).unwrap();
        assert_eq!(
            models.len(),
            crate::transcription_models::MODELS
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
        let mut api =
            LocalApi::start_bound(None, Arc::new(test_health), events, Some(sender)).unwrap();
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
            crate::developer_control::DeveloperCommand::Hud {
                state: crate::developer_control::DeveloperHudState::Recording
            }
        ));
        call.reply
            .send(crate::developer_control::DeveloperReply::Ok)
            .unwrap();
        let response = client.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert_eq!(response_body(&response), r#"{"status":"ok"}"#);

        api.shutdown();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn embedded_server_returns_its_endpoint_without_publishing_discovery() {
        let directory = temp_directory();
        let event_path = directory.join("events.ndjson");
        let discovery_path = directory.join("local-api.json");
        let events = EventLog::create(&event_path).unwrap();
        let mut api = LocalApi::start_without_discovery(Arc::new(test_health), events).unwrap();
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
        let apple = crate::transcription_models::definition(
            crate::transcription_models::TranscriptionModelId::AppleSpeech,
        );
        assert!(installation_selection(apple, "/models/apple_speech/prepare?language=de").is_err());
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
        let first = LocalApi::start_without_discovery(Arc::new(test_health), first_events).unwrap();
        let second =
            LocalApi::start_without_discovery(Arc::new(test_health), second_events).unwrap();
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
        let api =
            LocalApi::start_at(discovery_path.clone(), Arc::new(test_health), events).unwrap();
        let mode = fs::metadata(&discovery_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
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
        let api =
            LocalApi::start_at(discovery_path.clone(), Arc::new(test_health), events).unwrap();
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
        let api =
            LocalApi::start_at(discovery_path.clone(), Arc::new(test_health), events).unwrap();
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
        let api = LocalApi::start_at(
            discovery_path.clone(),
            Arc::new(test_health),
            events.clone(),
        )
        .unwrap();
        assert!(
            LocalApi::start_at(discovery_path.clone(), Arc::new(test_health), events,).is_err()
        );
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
        let api =
            LocalApi::start_at(discovery_path.clone(), Arc::new(test_health), events).unwrap();
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
            crate::transcription_service::TranscriptionService::start().unwrap();
        let action = transcription_action(
            HttpRequest {
                method: "POST".into(),
                path: "/transcriptions?model=parakeet_v2".into(),
                authorization: None,
                content_type: Some("audio/webm".into()),
                content_length: Some(10),
                body_prefix: Vec::new(),
            },
            &transcription,
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
                content_type: Some("audio/wav".into()),
                content_length: Some(MAX_AUDIO_BYTES + 1),
                body_prefix: Vec::new(),
            },
            &transcription,
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
            crate::transcription_service::TranscriptionService::start().unwrap();
        let context = HttpContext {
            token: "test-token".into(),
            health,
            events,
            auth_limiter: AuthObservationLimiter::default(),
            model_preparing: preparing,
            shutdown: Arc::new(AtomicBool::new(false)),
            transcription,
            developer_control: None,
        };
        let action = handle_request(
            HttpRequest {
                method: "POST".into(),
                path: "/models/parakeet_v2/prepare".into(),
                authorization: Some("Bearer test-token".into()),
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
                .set_read_timeout(Some(Duration::from_secs(3)))
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
        let preparer: ModelPreparer = Box::new(|_, downloaded, stage| {
            downloaded.store(123, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(150));
            crate::transcription_models::ModelPreparationStage::Verifying.store(&stage);
            thread::sleep(Duration::from_millis(150));
            crate::transcription_models::ModelPreparationStage::Loading.store(&stage);
            thread::sleep(Duration::from_millis(150));
            Ok(())
        });
        stream_model_prepare_with(
            &mut stream,
            crate::transcription_models::definition(
                crate::transcription_models::TranscriptionModelId::ParakeetV2,
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
    fn tokens_have_256_bits_of_random_lowercase_hex() {
        let first = generate_token().unwrap();
        let second = generate_token().unwrap();
        assert_eq!(first.len(), 68);
        assert!(first.starts_with("hex_"));
        assert!(
            first[4..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert_ne!(first, second);
    }

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
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
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

    fn unused_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }
}
