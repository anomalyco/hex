//! Running-app dictation (`/dictations`): a host starts, finishes, cancels
//! and observes a dictation the app records. Only the macOS recognition loop
//! drives it so far; elsewhere the routes answer
//! `service-capture-unavailable` and `/capabilities` reports
//! `serviceCapture: false`.

#[cfg(target_os = "macos")]
pub(super) use macos::*;
#[cfg(not(target_os = "macos"))]
pub(super) use unavailable::*;

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashMap;
    use std::io::Write;
    use std::net::TcpStream;
    use std::sync::Mutex;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
    use std::time::Duration;

    use serde::Deserialize;

    use super::super::{
        DEVELOPER_CONTROL_TIMEOUT, DictationAction, HttpContext, HttpResponse,
        MAX_DICTATION_SOURCE_BYTES, body_read_response, generate_token, read_body_bounded,
        write_response, write_sse, write_sse_headers, write_stream_headers,
    };
    use crate::recognition::RecognitionControl;

    const DICTATION_CONTROL_TIMEOUT: Duration = Duration::from_secs(120);

    pub(crate) type Control = SyncSender<RecognitionControl>;

    pub(in crate::local_api) struct Dictations {
        control: Option<Control>,
        levels: Mutex<HashMap<u64, DictationLevels>>,
    }

    impl Dictations {
        pub(in crate::local_api) fn new(control: Option<Control>) -> Self {
            Self {
                control,
                levels: Mutex::default(),
            }
        }

        pub(in crate::local_api) fn available(&self) -> bool {
            self.control.is_some()
        }
    }

    struct DictationLevels {
        owner_token: String,
        receiver: Receiver<crate::recognition::DictationLevel>,
    }

    #[derive(Deserialize)]
    struct DictationStartRequest {
        source: String,
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
        context
            .dictations
            .levels
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|_, levels| {
                !matches!(levels.receiver.try_recv(), Err(TryRecvError::Disconnected))
            });
        let owner_token = match generate_token() {
            Ok(token) => token,
            Err(error) => {
                tracing::error!(%error, "could not generate dictation owner token");
                return HttpResponse::code(500, "token-generation-failed");
            }
        };
        let (level_sender, levels) = mpsc::sync_channel(8);
        let started = match call_recognition(
            controls,
            DEVELOPER_CONTROL_TIMEOUT,
            HttpResponse::code(503, "service-capture-unavailable"),
            |reply| RecognitionControl::StartDictation {
                source: request.source,
                owner_token: owner_token.clone(),
                levels: level_sender,
                reply,
            },
        ) {
            Ok(started) => started,
            Err(response) => return response,
        };
        context
            .dictations
            .levels
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                started.id,
                DictationLevels {
                    owner_token: owner_token.clone(),
                    receiver: levels,
                },
            );
        HttpResponse::json(
            201,
            &serde_json::json!({
                "id": started.id,
                "ownerToken": owner_token,
                "sampleRate": started.sample_rate,
            }),
        )
    }

    /// Sends one control to the recognition loop and waits for its typed reply.
    /// A full control queue is `503 service-capture-busy`; a rejected control is
    /// `409` with the recognizer's code; `on_timeout` covers a silent recognizer.
    fn call_recognition<T>(
        controls: &SyncSender<RecognitionControl>,
        timeout: Duration,
        on_timeout: HttpResponse,
        build: impl FnOnce(SyncSender<std::result::Result<T, String>>) -> RecognitionControl,
    ) -> std::result::Result<T, HttpResponse> {
        let (reply, response) = mpsc::sync_channel(1);
        if controls.try_send(build(reply)).is_err() {
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
                return stream_dictation_levels(stream, id, &owner_token, context);
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

    fn dictation_finish(
        id: u64,
        owner_token: String,
        controls: &SyncSender<RecognitionControl>,
    ) -> HttpResponse {
        match call_recognition(
            controls,
            DICTATION_CONTROL_TIMEOUT,
            HttpResponse::code(504, "dictation-timeout"),
            |reply| RecognitionControl::FinishDictation {
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
        controls: &SyncSender<RecognitionControl>,
        context: &HttpContext,
    ) -> HttpResponse {
        match call_recognition(
            controls,
            DEVELOPER_CONTROL_TIMEOUT,
            HttpResponse::code(503, "service-capture-unavailable"),
            |reply| RecognitionControl::CancelDictation {
                id,
                owner_token,
                reply,
            },
        ) {
            Ok(()) => {
                context
                    .dictations
                    .levels
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .remove(&id);
                HttpResponse::empty(204)
            }
            Err(response) => response,
        }
    }

    fn dictation_heartbeat(
        id: u64,
        owner_token: String,
        controls: &SyncSender<RecognitionControl>,
    ) -> HttpResponse {
        match call_recognition(
            controls,
            DEVELOPER_CONTROL_TIMEOUT,
            HttpResponse::code(503, "service-capture-unavailable"),
            |reply| RecognitionControl::HeartbeatDictation {
                id,
                owner_token,
                reply,
            },
        ) {
            Ok(()) => HttpResponse::empty(204),
            Err(response) => response,
        }
    }

    fn stream_dictation_levels(
        stream: &mut TcpStream,
        id: u64,
        owner_token: &str,
        context: &HttpContext,
    ) {
        let levels = {
            let mut levels = context
                .dictations
                .levels
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            levels
                .get(&id)
                .is_some_and(|levels| levels.owner_token == owner_token)
                .then(|| {
                    levels
                        .remove(&id)
                        .expect("dictation levels were checked")
                        .receiver
                })
        };
        let Some(levels) = levels else {
            let _ = write_response(stream, HttpResponse::code(404, "dictation-not-found"));
            return;
        };
        if write_sse_headers(stream).is_err() {
            return;
        }
        while !context.shutdown.load(Ordering::Acquire) {
            match levels.recv_timeout(Duration::from_millis(100)) {
                Ok(level) => {
                    if write_sse(
                        stream,
                        &serde_json::json!({ "rmsDb": level.rms_db, "peakDb": level.peak_db }),
                    )
                    .is_err()
                    {
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
        controls: &SyncSender<RecognitionControl>,
        context: &HttpContext,
    ) {
        let (audio, chunks) = mpsc::sync_channel(4);
        if let Err(response) = call_recognition(
            controls,
            DEVELOPER_CONTROL_TIMEOUT,
            HttpResponse::code(503, "service-capture-unavailable"),
            |reply| RecognitionControl::AttachDictationAudio {
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
}

#[cfg(not(target_os = "macos"))]
mod unavailable {
    use std::net::TcpStream;

    use super::super::{DictationAction, HttpContext, HttpResponse, write_response};

    /// No recognition loop takes dictation controls on this OS.
    pub(crate) type Control = std::convert::Infallible;

    pub(in crate::local_api) struct Dictations;

    impl Dictations {
        pub(in crate::local_api) fn new(control: Option<Control>) -> Self {
            if let Some(control) = control {
                match control {}
            }
            Self
        }

        pub(in crate::local_api) fn available(&self) -> bool {
            false
        }
    }

    pub(in crate::local_api) fn start(
        _stream: &mut TcpStream,
        _content_length: usize,
        _body: Vec<u8>,
        _context: &HttpContext,
    ) -> HttpResponse {
        HttpResponse::code(404, "service-capture-unavailable")
    }

    pub(in crate::local_api) fn handle(
        stream: &mut TcpStream,
        _id: u64,
        _owner_token: String,
        _action: DictationAction,
        _context: &HttpContext,
    ) {
        let _ = write_response(
            stream,
            HttpResponse::code(404, "service-capture-unavailable"),
        );
    }
}
