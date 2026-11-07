//! Running-app dictation (`/dictations`): a host starts, finishes, cancels
//! and observes a dictation the app records. The macOS recognition loop and
//! the Linux and Windows listeners drive it; without a capture loop (`hex
//! service`) the routes answer `service-capture-unavailable` and
//! `/capabilities` reports `serviceCapture: false`. A start with
//! `"partials": true` also streams the text so far (`/partials`) where the
//! listener streams the recording (`dictationPartials`).

use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::time::Duration;

use serde::Deserialize;

use super::{
    DEVELOPER_CONTROL_TIMEOUT, DictationAction, HttpContext, HttpResponse,
    MAX_DICTATION_SOURCE_BYTES, body_read_response, generate_token, read_body_bounded,
    write_response, write_sse, write_sse_headers, write_stream_headers,
};
use crate::dictation::service::{DictationControl, DictationLevel};
use crate::transcription::engine::Partial;

const DICTATION_CONTROL_TIMEOUT: Duration = Duration::from_secs(120);

/// Where the app's capture loop takes `/dictations` controls: the macOS
/// recognition loop's own control queue, or the Linux or Windows listener's.
#[cfg(target_os = "macos")]
pub(crate) type Control = SyncSender<crate::recognition::RecognitionControl>;
#[cfg(not(target_os = "macos"))]
pub(crate) type Control = SyncSender<DictationControl>;

pub(in crate::local_api) struct Dictations {
    control: Option<Control>,
    levels: Subscriptions<DictationLevel>,
    partials: Subscriptions<Partial>,
}

impl Dictations {
    pub(in crate::local_api) fn new(control: Option<Control>) -> Self {
        Self {
            control,
            levels: Subscriptions::default(),
            partials: Subscriptions::default(),
        }
    }

    pub(in crate::local_api) fn available(&self) -> bool {
        self.control.is_some()
    }

    /// Whether a dictation started with `"partials": true` streams its text
    /// so far. The macOS recognition loop doesn't stream a client's
    /// recording, so it has none.
    pub(in crate::local_api) fn partials_available(&self) -> bool {
        self.available() && cfg!(not(target_os = "macos"))
    }
}

/// Each dictation's observation channel, until its owner subscribes once.
struct Subscriptions<T>(Mutex<HashMap<u64, Subscription<T>>>);

impl<T> Default for Subscriptions<T> {
    fn default() -> Self {
        Self(Mutex::default())
    }
}

struct Subscription<T> {
    owner_token: String,
    receiver: Receiver<T>,
}

impl<T> Subscriptions<T> {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Subscription<T>>> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn insert(&self, id: u64, owner_token: String, receiver: Receiver<T>) {
        self.lock().insert(
            id,
            Subscription {
                owner_token,
                receiver,
            },
        );
    }

    /// Drops the channels of dictations that ended unobserved.
    fn prune(&self) {
        self.lock().retain(|_, subscription| {
            !matches!(
                subscription.receiver.try_recv(),
                Err(TryRecvError::Disconnected)
            )
        });
    }

    fn remove(&self, id: u64) {
        self.lock().remove(&id);
    }

    /// The channel of dictation `id`, for its owner alone.
    fn take(&self, id: u64, owner_token: &str) -> Option<Receiver<T>> {
        let mut subscriptions = self.lock();
        subscriptions
            .get(&id)
            .is_some_and(|subscription| subscription.owner_token == owner_token)
            .then(|| subscriptions.remove(&id))
            .flatten()
            .map(|subscription| subscription.receiver)
    }
}

#[derive(Deserialize)]
struct DictationStartRequest {
    source: String,
    #[serde(default)]
    partials: bool,
}

pub(in crate::local_api) fn start(
    stream: &mut TcpStream,
    content_length: usize,
    mut body: Vec<u8>,
    context: &HttpContext,
) -> HttpResponse {
    if let Err(error) = read_body_bounded(
        stream,
        content_length,
        &mut body,
        &context.shutdown,
        MAX_DICTATION_SOURCE_BYTES + 32,
        DEVELOPER_CONTROL_TIMEOUT,
    ) {
        return body_read_response(error);
    }
    let Ok(request) = serde_json::from_slice::<DictationStartRequest>(&body) else {
        return HttpResponse::code(400, "invalid-request");
    };
    if request.source.trim().is_empty() || request.source.len() > MAX_DICTATION_SOURCE_BYTES {
        return HttpResponse::code(400, "invalid-source");
    }
    let Some(controls) = &context.dictations.control else {
        return HttpResponse::code(404, "service-capture-unavailable");
    };
    context.dictations.levels.prune();
    context.dictations.partials.prune();
    let owner_token = match generate_token() {
        Ok(token) => token,
        Err(error) => {
            tracing::error!(%error, "could not generate dictation owner token");
            return HttpResponse::code(500, "token-generation-failed");
        }
    };
    let (level_sender, levels) = mpsc::sync_channel(8);
    let (partial_sender, partials) = request.partials.then(|| mpsc::sync_channel(8)).unzip();
    let started = match call_recognition(
        controls,
        DEVELOPER_CONTROL_TIMEOUT,
        HttpResponse::code(503, "service-capture-unavailable"),
        |reply| DictationControl::Start {
            source: request.source,
            owner_token: owner_token.clone(),
            levels: level_sender,
            partials: partial_sender,
            reply,
        },
    ) {
        Ok(started) => started,
        Err(response) => return response,
    };
    context
        .dictations
        .levels
        .insert(started.id, owner_token.clone(), levels);
    if let Some(partials) = partials {
        context
            .dictations
            .partials
            .insert(started.id, owner_token.clone(), partials);
    }
    HttpResponse::json(
        201,
        &serde_json::json!({
            "id": started.id,
            "ownerToken": owner_token,
            "sampleRate": started.sample_rate,
        }),
    )
}

/// Sends one control to the capture loop and waits for its typed reply.
/// A full control queue is `503 service-capture-busy`; a rejected control is
/// `409` with the loop's code; `on_timeout` covers a silent loop.
fn call_recognition<T, M: From<DictationControl>>(
    controls: &SyncSender<M>,
    timeout: Duration,
    on_timeout: HttpResponse,
    build: impl FnOnce(SyncSender<std::result::Result<T, String>>) -> DictationControl,
) -> std::result::Result<T, HttpResponse> {
    let (reply, response) = mpsc::sync_channel(1);
    if controls.try_send(M::from(build(reply))).is_err() {
        return Err(HttpResponse::code(503, "service-capture-busy"));
    }
    match response.recv_timeout(timeout) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(code)) => Err(HttpResponse::code(409, &code)),
        Err(_) => Err(on_timeout),
    }
}

pub(in crate::local_api) fn handle(
    stream: &mut TcpStream,
    id: u64,
    owner_token: String,
    action: DictationAction,
    context: &HttpContext,
) {
    let Some(controls) = &context.dictations.control else {
        let _ = write_response(
            stream,
            HttpResponse::code(404, "service-capture-unavailable"),
        );
        return;
    };
    let response = match action {
        DictationAction::Levels => {
            let levels = context.dictations.levels.take(id, &owner_token);
            return stream_observations(
                stream,
                levels,
                context,
                |level| serde_json::json!({ "rmsDb": level.rms_db, "peakDb": level.peak_db }),
            );
        }
        DictationAction::Partials => {
            let partials = context.dictations.partials.take(id, &owner_token);
            return stream_observations(
                stream,
                partials,
                context,
                |partial| serde_json::json!({ "stable": partial.stable, "tentative": partial.tentative }),
            );
        }
        DictationAction::Audio => {
            return stream_dictation_audio(stream, id, owner_token, controls, context);
        }
        DictationAction::Finish => dictation_finish(id, owner_token, controls),
        DictationAction::Cancel => dictation_cancel(id, owner_token, controls, context),
        DictationAction::Heartbeat => dictation_heartbeat(id, owner_token, controls),
    };
    if let Err(error) = write_response(stream, response) {
        tracing::debug!(%error, "dictation client disconnected");
    }
}

fn dictation_finish(id: u64, owner_token: String, controls: &Control) -> HttpResponse {
    match call_recognition(
        controls,
        DICTATION_CONTROL_TIMEOUT,
        HttpResponse::code(504, "dictation-timeout"),
        |reply| DictationControl::Finish {
            id,
            owner_token,
            reply,
        },
    ) {
        Ok(result) => HttpResponse::json(
            200,
            &serde_json::json!({
                "transcript": result.transcript,
                "durationMs": result.duration_ms,
            }),
        ),
        Err(response) => response,
    }
}

fn dictation_cancel(
    id: u64,
    owner_token: String,
    controls: &Control,
    context: &HttpContext,
) -> HttpResponse {
    match call_recognition(
        controls,
        DEVELOPER_CONTROL_TIMEOUT,
        HttpResponse::code(503, "service-capture-unavailable"),
        |reply| DictationControl::Cancel {
            id,
            owner_token,
            reply,
        },
    ) {
        Ok(()) => {
            context.dictations.levels.remove(id);
            context.dictations.partials.remove(id);
            HttpResponse::empty(204)
        }
        Err(response) => response,
    }
}

fn dictation_heartbeat(id: u64, owner_token: String, controls: &Control) -> HttpResponse {
    match call_recognition(
        controls,
        DEVELOPER_CONTROL_TIMEOUT,
        HttpResponse::code(503, "service-capture-unavailable"),
        |reply| DictationControl::Heartbeat {
            id,
            owner_token,
            reply,
        },
    ) {
        Ok(()) => HttpResponse::empty(204),
        Err(response) => response,
    }
}

/// Streams one dictation's levels or partials as server-sent events until
/// the dictation ends.
fn stream_observations<T>(
    stream: &mut TcpStream,
    observations: Option<Receiver<T>>,
    context: &HttpContext,
    event: impl Fn(T) -> serde_json::Value,
) {
    let Some(observations) = observations else {
        let _ = write_response(stream, HttpResponse::code(404, "dictation-not-found"));
        return;
    };
    if write_sse_headers(stream).is_err() {
        return;
    }
    while !context.shutdown.load(Ordering::Acquire) {
        match observations.recv_timeout(Duration::from_millis(100)) {
            Ok(observation) => {
                if write_sse(stream, &event(observation)).is_err() {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn stream_dictation_audio(
    stream: &mut TcpStream,
    id: u64,
    owner_token: String,
    controls: &Control,
    context: &HttpContext,
) {
    let (audio, chunks) = mpsc::sync_channel(4);
    if let Err(response) = call_recognition(
        controls,
        DEVELOPER_CONTROL_TIMEOUT,
        HttpResponse::code(503, "service-capture-unavailable"),
        |reply| DictationControl::AttachAudio {
            id,
            owner_token,
            audio,
            reply,
        },
    ) {
        let _ = write_response(stream, response);
        return;
    }
    if write_stream_headers(stream, "application/octet-stream").is_err() {
        return;
    }
    while !context.shutdown.load(Ordering::Acquire) {
        match chunks.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                let mut bytes = Vec::with_capacity(chunk.samples.len() * size_of::<f32>());
                for sample in chunk.samples {
                    bytes.extend_from_slice(&sample.to_le_bytes());
                }
                if stream.write_all(&bytes).is_err() || stream.flush().is_err() {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}
