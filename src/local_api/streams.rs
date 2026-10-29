//! `/streams`: live transcription of an app's own audio on the dictation
//! engine's model (`docs/streams.md`). The routes are thin; the stream
//! itself, its windows, and dictation priority live in
//! `crate::transcription_stream`.

use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::Deserialize;

use super::{
    HttpContext, HttpRequest, HttpResponse, RequestAction, body_read_response, generate_token,
    json_post_action, read_body_bounded, write_response, write_sse, write_sse_headers,
};
use crate::transcription_stream::{
    CreateError, End, LEASE, PushError, SAMPLE_RATE, SampleFormat, Stream,
};

const MAX_CREATE_BYTES: usize = 1024;
/// About 4 s of f32 audio per request; pushes of 100 ms to 1 s are typical.
pub(super) const MAX_AUDIO_BYTES: usize = 256 * 1024;
const AUDIO_TIMEOUT: Duration = Duration::from_secs(5);
/// How long `finish` waits for the last windows, such as behind a long
/// dictation, before answering `504` (the stream keeps going).
const FINISH_TIMEOUT: Duration = Duration::from_secs(120);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

pub(super) enum StreamAction {
    Create {
        content_length: usize,
        body_prefix: Vec<u8>,
    },
    Audio {
        stream: Arc<Stream>,
        content_length: usize,
        body_prefix: Vec<u8>,
    },
    Events(Arc<Stream>),
    Finish(Arc<Stream>),
}

pub(super) fn create_action(request: HttpRequest) -> RequestAction {
    json_post_action(
        request,
        true,
        "streams-unavailable",
        MAX_CREATE_BYTES,
        "request-too-large",
        |content_length, body_prefix| {
            RequestAction::Stream(StreamAction::Create {
                content_length,
                body_prefix,
            })
        },
    )
}

/// `/streams/{id}/{action}` with its numeric id.
pub(super) fn stream_path(path: &str) -> Option<(u64, &str)> {
    let mut parts = path.strip_prefix("/streams/")?.split('/');
    let id = parts.next()?.parse().ok()?;
    let action = parts.next()?;
    (parts.next().is_none()).then_some((id, action))
}

pub(super) fn route(
    request: HttpRequest,
    id: u64,
    action: &str,
    context: &HttpContext,
) -> RequestAction {
    let respond = RequestAction::Respond;
    let stream = request.stream_token.as_deref().and_then(|token| {
        context
            .streams
            .get(id)
            .filter(|stream| stream.owned_by(token))
    });
    let Some(stream) = stream else {
        return respond(HttpResponse::code(404, "stream-not-found"));
    };
    match (request.method.as_str(), action) {
        ("POST", "audio") => {
            if !request.has_content_type("application/octet-stream") {
                return respond(HttpResponse::code(415, "unsupported-content-type"));
            }
            let Some(content_length) = request.content_length else {
                return respond(HttpResponse::code(411, "length-required"));
            };
            if content_length > MAX_AUDIO_BYTES {
                return respond(HttpResponse::code(413, "audio-too-large"));
            }
            RequestAction::Stream(StreamAction::Audio {
                stream,
                content_length,
                body_prefix: request.body_prefix,
            })
        }
        ("GET", "events") => RequestAction::Stream(StreamAction::Events(stream)),
        ("POST", "finish") => RequestAction::Stream(StreamAction::Finish(stream)),
        ("POST", "cancel") => {
            stream.cancel();
            respond(HttpResponse::empty(204))
        }
        ("POST", "heartbeat") => respond(if stream.heartbeat() {
            HttpResponse::empty(204)
        } else {
            HttpResponse::code(409, "stream-ended")
        }),
        _ => respond(HttpResponse::code(404, "not_found")),
    }
}

pub(super) fn handle(connection: &mut TcpStream, action: StreamAction, context: &HttpContext) {
    let response = match action {
        StreamAction::Create {
            content_length,
            body_prefix,
        } => create(connection, content_length, body_prefix, context),
        StreamAction::Audio {
            stream,
            content_length,
            body_prefix,
        } => audio(connection, &stream, content_length, body_prefix, context),
        StreamAction::Events(stream) => return events(connection, &stream, context),
        StreamAction::Finish(stream) => finish(&stream, context),
    };
    if let Err(error) = write_response(connection, response) {
        tracing::debug!(%error, "stream client disconnected");
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRequest {
    sample_format: String,
    sample_rate: u32,
    model: Option<String>,
    language: Option<String>,
}

fn create(
    connection: &mut TcpStream,
    content_length: usize,
    mut body: Vec<u8>,
    context: &HttpContext,
) -> HttpResponse {
    if let Err(error) = read_body_bounded(
        connection,
        content_length,
        &mut body,
        &context.shutdown,
        MAX_CREATE_BYTES,
        AUDIO_TIMEOUT,
    ) {
        return body_read_response(error);
    }
    let Ok(request) = serde_json::from_slice::<CreateRequest>(&body) else {
        return HttpResponse::code(400, "invalid-request");
    };
    if request.sample_rate as usize != SAMPLE_RATE {
        return HttpResponse::code(400, "unsupported-sample-rate");
    }
    let Some(format) = SampleFormat::parse(&request.sample_format) else {
        return HttpResponse::code(400, "unsupported-sample-format");
    };
    let model = match request
        .model
        .as_deref()
        .map(str::parse::<crate::transcription_models::TranscriptionModelId>)
    {
        None => None,
        Some(Ok(model)) => Some(model),
        Some(Err(_)) => return HttpResponse::code(400, "unknown-model"),
    };
    let (engine, selection) = match (context.stream_engine)() {
        Ok(engine) => engine,
        Err(code) => return HttpResponse::code(503, code),
    };
    if model.is_some_and(|model| model != selection.model)
        || request
            .language
            .as_ref()
            .is_some_and(|language| *language != selection.language)
    {
        return HttpResponse::json(
            409,
            &serde_json::json!({
                "code": "model-mismatch",
                "model": selection.model.as_str(),
                "language": selection.language,
            }),
        );
    }
    let owner_token = match generate_token() {
        Ok(token) => token,
        Err(error) => {
            tracing::error!(%error, "could not generate stream owner token");
            return HttpResponse::code(500, "token-generation-failed");
        }
    };
    match context
        .streams
        .create(engine, format, selection, owner_token.clone())
    {
        Ok(stream) => HttpResponse::json(
            201,
            &serde_json::json!({
                "id": stream.id,
                "ownerToken": owner_token,
                "model": stream.selection.model.as_str(),
                "language": stream.selection.language,
                "sampleRate": SAMPLE_RATE,
                "sampleFormat": format.name(),
                "partials": stream.partials,
                "leaseMs": LEASE.as_millis() as u64,
            }),
        ),
        Err(CreateError::Busy) => HttpResponse::code(409, "stream-busy"),
        Err(CreateError::Spawn) => HttpResponse::code(500, "stream-start-failed"),
    }
}

fn audio(
    connection: &mut TcpStream,
    stream: &Stream,
    content_length: usize,
    mut body: Vec<u8>,
    context: &HttpContext,
) -> HttpResponse {
    if let Err(error) = read_body_bounded(
        connection,
        content_length,
        &mut body,
        &context.shutdown,
        MAX_AUDIO_BYTES,
        AUDIO_TIMEOUT,
    ) {
        return body_read_response(error);
    }
    let Some(samples) = stream.format.decode(&body) else {
        return HttpResponse::code(400, "invalid-audio");
    };
    match stream.push(&samples) {
        Ok(()) => HttpResponse::empty(204),
        Err(PushError::Ended) => HttpResponse::code(409, "stream-ended"),
        Err(PushError::Finishing) => HttpResponse::code(409, "stream-finishing"),
        Err(PushError::BacklogFull) => HttpResponse::code(429, "stream-backlog-full"),
    }
}

fn finish(stream: &Stream, context: &HttpContext) -> HttpResponse {
    let stop = || context.shutdown.load(Ordering::Acquire);
    match stream.finish(FINISH_TIMEOUT, &stop) {
        Some(End::Final {
            text,
            segments,
            duration_ms,
        }) => HttpResponse::json(
            200,
            &serde_json::json!({
                "text": text,
                "segments": segments,
                "durationMs": duration_ms,
            }),
        ),
        Some(End::Failed { code, message }) => HttpResponse::json(
            409,
            &serde_json::json!({ "code": code, "message": message }),
        ),
        Some(End::Cancelled) => HttpResponse::code(409, "stream-cancelled"),
        None if stop() => HttpResponse::code(503, "shutting-down"),
        None => HttpResponse::code(504, "stream-finish-timeout"),
    }
}

fn events(connection: &mut TcpStream, stream: &Stream, context: &HttpContext) {
    if write_sse_headers(connection).is_err() {
        return;
    }
    let ready = serde_json::json!({
        "type": "ready",
        "id": stream.id,
        "model": stream.selection.model.as_str(),
        "language": stream.selection.language,
        "sampleFormat": stream.format.name(),
        "partials": stream.partials,
    });
    if write_sse(connection, &ready).is_err() {
        return;
    }
    let stop = || context.shutdown.load(Ordering::Acquire);
    let mut writer = connection.try_clone();
    let Ok(writer) = writer.as_mut() else {
        return;
    };
    let mut keep_alive = match connection.try_clone() {
        Ok(connection) => connection,
        Err(_) => return,
    };
    stream.subscribe(
        KEEP_ALIVE,
        &stop,
        &mut |event| write_sse(writer, event).is_ok(),
        &mut || {
            keep_alive
                .write_all(b": keep-alive\n\n")
                .and_then(|()| keep_alive.flush())
                .is_ok()
        },
    );
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::sync::atomic::AtomicU64;

    use super::super::{LocalApi, LocalApiOptions, current_health};
    use super::*;
    use crate::events::EventLog;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn start(stream_engine: Option<crate::transcription_stream::EngineSource>) -> LocalApi {
        let directory = std::env::temp_dir().join(format!(
            "hex-streams-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        LocalApi::start_bound(
            None,
            Arc::new(current_health),
            events,
            LocalApiOptions {
                stream_engine,
                ..LocalApiOptions::default()
            },
            None,
        )
        .unwrap()
    }

    /// Sends one request and returns its status and body.
    fn send(api: &LocalApi, request: &str, body: &[u8]) -> (u16, String) {
        let mut connection = TcpStream::connect(("127.0.0.1", api.document.port)).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        connection.write_all(request.as_bytes()).unwrap();
        connection.write_all(body).unwrap();
        let mut response = Vec::new();
        connection.read_to_end(&mut response).unwrap();
        let response = String::from_utf8(response).unwrap();
        let status = response[9..12].parse().unwrap();
        let body = response.split_once("\r\n\r\n").unwrap().1.to_string();
        (status, body)
    }

    fn post_json(api: &LocalApi, path: &str, body: &str) -> (u16, String) {
        send(
            api,
            &format!(
                "POST {path} HTTP/1.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                api.document.token,
                body.len()
            ),
            body.as_bytes(),
        )
    }

    fn call(
        api: &LocalApi,
        method: &str,
        path: &str,
        owner: &str,
        extra: &str,
        body: &[u8],
    ) -> (u16, String) {
        send(
            api,
            &format!(
                "{method} {path} HTTP/1.1\r\nAuthorization: Bearer {}\r\nX-Hex-Stream-Token: {owner}\r\n{extra}Content-Length: {}\r\n\r\n",
                api.document.token,
                body.len()
            ),
            body,
        )
    }

    /// Declares a body without sending it, for requests refused at their
    /// headers (a sent body would go unread, and the close would reset).
    fn refused_audio(
        api: &LocalApi,
        id: u64,
        owner: &str,
        extra: &str,
        length: usize,
    ) -> (u16, String) {
        send(
            api,
            &format!(
                "POST /streams/{id}/audio HTTP/1.1\r\nAuthorization: Bearer {}\r\nX-Hex-Stream-Token: {owner}\r\n{extra}Content-Length: {length}\r\n\r\n",
                api.document.token,
            ),
            &[],
        )
    }

    fn audio(api: &LocalApi, id: u64, owner: &str, body: &[u8]) -> (u16, String) {
        call(
            api,
            "POST",
            &format!("/streams/{id}/audio"),
            owner,
            "Content-Type: application/octet-stream\r\n",
            body,
        )
    }

    const CREATE: &str = r#"{"sampleFormat":"f32le","sampleRate":16000}"#;

    fn created(api: &LocalApi) -> (u64, String) {
        let (status, body) = post_json(api, "/streams", CREATE);
        assert_eq!(status, 201, "{body}");
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["sampleRate"], 16_000);
        assert_eq!(body["partials"], true);
        (
            body["id"].as_u64().unwrap(),
            body["ownerToken"].as_str().unwrap().to_string(),
        )
    }

    #[test]
    fn stream_paths_need_a_numeric_id_and_one_action() {
        assert_eq!(stream_path("/streams/7/audio"), Some((7, "audio")));
        assert_eq!(stream_path("/streams/x/audio"), None);
        assert_eq!(stream_path("/streams/7"), None);
        assert_eq!(stream_path("/streams/7/audio/x"), None);
        assert_eq!(stream_path("/streams"), None);
    }

    #[test]
    fn creating_a_stream_without_the_running_apps_engine_is_unavailable() {
        let api = start(Some(Arc::new(|| Err("engine-unavailable"))));
        let (status, body) = post_json(&api, "/streams", CREATE);
        assert_eq!(
            (status, body.as_str()),
            (503, r#"{"code":"engine-unavailable"}"#)
        );
    }

    #[test]
    fn create_validates_the_format_and_the_loaded_model_and_admits_one_stream() {
        let api = start(Some(crate::transcription_stream::tests::fake_source()));
        for (body, status, code) in [
            (
                r#"{"sampleFormat":"f32le","sampleRate":44100}"#,
                400,
                "unsupported-sample-rate",
            ),
            (
                r#"{"sampleFormat":"wav","sampleRate":16000}"#,
                400,
                "unsupported-sample-format",
            ),
            (r#"{"sampleRate":16000}"#, 400, "invalid-request"),
            (
                r#"{"sampleFormat":"f32le","sampleRate":16000,"model":"nope"}"#,
                400,
                "unknown-model",
            ),
            (
                r#"{"sampleFormat":"f32le","sampleRate":16000,"language":"zz"}"#,
                409,
                "model-mismatch",
            ),
        ] {
            let (got, response) = post_json(&api, "/streams", body);
            assert_eq!(got, status, "{body}: {response}");
            assert!(response.contains(code), "{body}: {response}");
        }
        let (status, _) = send(
            &api,
            &format!(
                "POST /streams HTTP/1.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                api.document.token,
                MAX_CREATE_BYTES + 1
            ),
            &[],
        );
        assert_eq!(status, 413);
        created(&api);
        let (status, body) = post_json(&api, "/streams", CREATE);
        assert_eq!((status, body.as_str()), (409, r#"{"code":"stream-busy"}"#));
    }

    #[test]
    fn a_stream_takes_bounded_audio_from_its_owner_and_finishes_with_its_segments() {
        let api = start(Some(crate::transcription_stream::tests::fake_source()));
        let (id, owner) = created(&api);
        let half_second = [0.0_f32; 8_000]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        let octets = "Content-Type: application/octet-stream\r\n";
        assert_eq!(refused_audio(&api, id, "hex_wrong", octets, 4).0, 404);
        assert_eq!(refused_audio(&api, id + 1, &owner, octets, 4).0, 404);
        assert_eq!(
            refused_audio(&api, id, &owner, "Content-Type: audio/wav\r\n", 4).0,
            415
        );
        assert_eq!(
            refused_audio(&api, id, &owner, octets, MAX_AUDIO_BYTES + 1),
            (413, r#"{"code":"audio-too-large"}"#.into())
        );
        assert_eq!(audio(&api, id, &owner, &[0; 3]).0, 400);
        assert_eq!(audio(&api, id, &owner, &half_second).0, 204);
        let heartbeat = format!("/streams/{id}/heartbeat");
        assert_eq!(call(&api, "POST", &heartbeat, &owner, "", &[]).0, 204);

        let (status, body) = call(
            &api,
            "POST",
            &format!("/streams/{id}/finish"),
            &owner,
            "",
            &[],
        );
        assert_eq!(status, 200, "{body}");
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["text"], "live 8000");
        assert_eq!(body["durationMs"], 500);
        assert_eq!(body["segments"][0]["endMs"], 500);

        // Events replay the ended stream from its start.
        let (status, events) = call(
            &api,
            "GET",
            &format!("/streams/{id}/events"),
            &owner,
            "",
            &[],
        );
        assert_eq!(status, 200);
        let types = events
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|data| serde_json::from_str::<serde_json::Value>(data).unwrap()["type"].clone())
            .collect::<Vec<_>>();
        assert_eq!(types, ["ready", "segment", "final"]);

        assert_eq!(
            audio(&api, id, &owner, &half_second),
            (409, r#"{"code":"stream-ended"}"#.into())
        );
        assert_eq!(call(&api, "POST", &heartbeat, &owner, "", &[]).0, 409);

        let (id, owner) = created(&api);
        assert_eq!(
            call(
                &api,
                "POST",
                &format!("/streams/{id}/cancel"),
                &owner,
                "",
                &[]
            )
            .0,
            204
        );
        let (status, body) = call(
            &api,
            "POST",
            &format!("/streams/{id}/finish"),
            &owner,
            "",
            &[],
        );
        assert_eq!(
            (status, body.as_str()),
            (409, r#"{"code":"stream-cancelled"}"#)
        );
    }
}
