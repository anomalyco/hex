//! Optional output processing on transcription responses: the `process`
//! query on `POST /transcriptions` and `POST /streams/{id}/finish`
//! (Labs › Process API transcripts). The app installs a processor built from
//! its dictation pipeline; this module owns the query, the gates, and the
//! `processed` field.
//!
//! Processing never pastes, saves History, updates the last dictation, or
//! touches the clipboard: the app's processor only returns text, and every
//! failed stage keeps its input, exactly as a paste's processing does.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::HttpResponse;

/// The app's processor: a raw transcript, its language, and the API's
/// shutdown flag, and the text a paste in Global mode would use. It reads the
/// current settings each call and stops at its next stage boundary once
/// `cancelled` is set.
pub type Processor = Arc<dyn Fn(&str, &str, &AtomicBool) -> String + Send + Sync>;

/// What this local API can process with, read per request so turning the lab
/// off stops processing at once.
#[derive(Clone)]
pub struct Processing {
    processor: Option<Processor>,
    enabled: fn() -> bool,
    /// One processing run at a time, so a slow rewrite never occupies every
    /// HTTP worker.
    busy: Arc<AtomicBool>,
}

impl Default for Processing {
    fn default() -> Self {
        Self {
            processor: None,
            enabled: crate::labs::api_processing,
            busy: Arc::default(),
        }
    }
}

impl Processing {
    /// The running app's processor, while Labs › Process API transcripts is
    /// on.
    pub fn installed(processor: Processor) -> Self {
        Self {
            processor: Some(processor),
            ..Self::default()
        }
    }

    /// A processor under another gate, so route tests need no global Labs
    /// state.
    #[cfg(test)]
    pub(super) fn gated(processor: Option<Processor>, enabled: fn() -> bool) -> Self {
        Self {
            processor,
            enabled,
            busy: Arc::default(),
        }
    }

    /// Whether a `process` request would run: a processor is installed and
    /// the lab is on.
    pub(super) fn available(&self) -> bool {
        (self.enabled)() && self.processor.is_some()
    }

    /// Claims the one processing slot, or `None` while another run holds it.
    fn begin(&self) -> Option<Guard> {
        let processor = self.processor.as_ref()?;
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Guard {
                processor: processor.clone(),
                busy: self.busy.clone(),
            })
    }
}

/// One admitted processing run. Dropping it frees the slot, whether the
/// processor finished, failed, or the client went away.
pub(super) struct Guard {
    processor: Processor,
    busy: Arc<AtomicBool>,
}

impl Guard {
    /// The processed text; every failed stage keeps its input. `cancelled`
    /// is the API's shutdown flag, so quitting stops at a stage boundary.
    pub(super) fn text(&self, raw: &str, language: &str, cancelled: &AtomicBool) -> String {
        (self.processor)(raw, language, cancelled)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::Release);
    }
}

/// Adds `processed` to a transcription response when the request asked for
/// it; every existing field stays as it is.
pub(super) fn attach(
    body: &mut serde_json::Value,
    guard: Option<&Guard>,
    raw: &str,
    language: &str,
    cancelled: &AtomicBool,
) {
    if let Some(guard) = guard {
        body["processed"] = serde_json::Value::String(guard.text(raw, language, cancelled));
    }
}

/// The refusal for a request that asked for processing, or the slot its run
/// holds. Checks that a processor exists before the lab, so `hex service`
/// answers `processing-unavailable` even while the lab is off, and claims the
/// one slot before any audio body is read or a stream finishes.
pub(super) fn gate(wanted: bool, processing: &Processing) -> Result<Option<Guard>, HttpResponse> {
    if !wanted {
        return Ok(None);
    }
    if processing.processor.is_none() {
        return Err(HttpResponse::code(409, "processing-unavailable"));
    }
    if !(processing.enabled)() {
        return Err(HttpResponse::code(403, "processing-disabled"));
    }
    processing
        .begin()
        .map(Some)
        .ok_or_else(|| HttpResponse::code(429, "queue-full"))
}

/// Whether a request's query asked for output processing. `process=global`
/// is the only mode; `true` and `1` are aliases. Any other value, or a
/// repeated parameter, is `invalid-request`.
pub(super) fn wanted(path: &str) -> Result<bool, &'static str> {
    let query = path
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default();
    let mut wanted = None;
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if name != "process" {
            continue;
        }
        if wanted.is_some() {
            return Err("invalid-request");
        }
        match value.as_ref() {
            "global" | "true" | "1" => wanted = Some(true),
            _ => return Err("invalid-request"),
        }
    }
    Ok(wanted.unwrap_or(false))
}

/// A processor over the Windows and Linux listeners' live mode runtime and
/// the dictation pipeline's transformation client, so edits and the
/// personal-commands host apply without a restart.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn dictation_processor(
    processing: crate::dictation::modes::ProcessingState,
) -> Processor {
    Arc::new(move |raw, language, cancelled| {
        let modes = processing
            .modes
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        modes
            .process_cancellable(
                raw,
                language,
                &crate::dictation::context::ContextSnapshot::default(),
                &processing.transformations,
                cancelled,
                crate::dictation::processor::Source::Dictation,
            )
            .text
    })
}

/// The macOS app's processor, ready for [`super::LocalApiOptions::processing`].
#[cfg(target_os = "macos")]
pub(crate) fn profiles_processing(
    transformations: Arc<crate::personal_commands::TransformationClient>,
) -> Processing {
    Processing::installed(profiles_processor(transformations))
}

/// A processor over the macOS profiles snapshot and the recognition loop's
/// transformation client.
#[cfg(target_os = "macos")]
pub(crate) fn profiles_processor(
    transformations: Arc<crate::personal_commands::TransformationClient>,
) -> Processor {
    Arc::new(move |raw, language, cancelled| {
        let context = crate::dictation::context::ContextSnapshot::default();
        let mut processed = crate::dictation::config::dictation_profiles().process_cancellable(
            raw,
            language,
            &context,
            cancelled,
            crate::dictation::processor::Source::Dictation,
        );
        processed.apply_transformations(
            &transformations,
            &context,
            cancelled,
            crate::dictation::processor::Source::Dictation,
        );
        processed.text
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Duration;

    use super::super::{LocalApi, LocalApiOptions, current_health};
    use super::*;
    use crate::desktop::events::EventLog;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn start(processing: Processing) -> LocalApi {
        let directory = std::env::temp_dir().join(format!(
            "hex-api-processing-{}-{}",
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
                processing,
                stream_engine: Some(crate::transcription::stream::tests::fake_source()),
                ..LocalApiOptions::default()
            },
            None,
        )
        .unwrap()
    }

    fn processor() -> Processor {
        Arc::new(|raw: &str, language: &str, _cancelled: &AtomicBool| {
            format!("{raw} [{language}] processed")
        })
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

    /// Declares an audio body without sending it: a refused request must
    /// answer without reading it.
    fn refused_transcription(api: &LocalApi, path: &str) -> (u16, String) {
        send(
            api,
            &format!(
                "POST {path} HTTP/1.1\r\nAuthorization: Bearer {}\r\nContent-Type: audio/wav\r\nContent-Length: 4\r\n\r\n",
                api.document.token,
            ),
            &[],
        )
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

    fn finish(api: &LocalApi, id: u64, owner: &str, query: &str) -> (u16, String) {
        send(
            api,
            &format!(
                "POST /streams/{id}/finish{query} HTTP/1.1\r\nAuthorization: Bearer {}\r\nX-Hex-Stream-Token: {owner}\r\nContent-Length: 0\r\n\r\n",
                api.document.token,
            ),
            &[],
        )
    }

    fn cancel(api: &LocalApi, id: u64, owner: &str) {
        let (status, body) = send(
            api,
            &format!(
                "POST /streams/{id}/cancel HTTP/1.1\r\nAuthorization: Bearer {}\r\nX-Hex-Stream-Token: {owner}\r\nContent-Length: 0\r\n\r\n",
                api.document.token,
            ),
            &[],
        );
        assert_eq!(status, 204, "{body}");
    }

    const CREATE: &str = r#"{"sampleFormat":"f32le","sampleRate":16000}"#;

    fn created(api: &LocalApi) -> (u64, String) {
        let (status, body) = post_json(api, "/streams", CREATE);
        assert_eq!(status, 201, "{body}");
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        (
            body["id"].as_u64().unwrap(),
            body["ownerToken"].as_str().unwrap().to_string(),
        )
    }

    /// Half a second of silence: the fake engine names a finished stream's
    /// text after the samples it heard.
    fn half_second(api: &LocalApi, id: u64, owner: &str) {
        let samples = [0.0_f32; 8_000]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        let (status, body) = send(
            api,
            &format!(
                "POST /streams/{id}/audio HTTP/1.1\r\nAuthorization: Bearer {}\r\nX-Hex-Stream-Token: {owner}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n",
                api.document.token,
                samples.len(),
            ),
            &samples,
        );
        assert_eq!(status, 204, "{body}");
    }

    fn request(api: &LocalApi, path: &str) -> (u16, String) {
        send(
            api,
            &format!(
                "GET {path} HTTP/1.1\r\nAuthorization: Bearer {}\r\nContent-Length: 0\r\n\r\n",
                api.document.token,
            ),
            &[],
        )
    }

    /// The flags as a router test wants them: the lab gate and a processor.
    fn off() -> Processing {
        Processing::gated(Some(processor()), || false)
    }

    fn on_without_processor() -> Processing {
        Processing::gated(None, || true)
    }

    fn on() -> Processing {
        Processing::gated(Some(processor()), || true)
    }

    #[test]
    fn the_lab_gates_processing_before_the_audio_body_is_read() {
        let api = start(off());
        assert_eq!(
            refused_transcription(&api, "/transcriptions?model=parakeet_v2&process=global"),
            (403, r#"{"code":"processing-disabled"}"#.into())
        );
        let (id, owner) = created(&api);
        assert_eq!(
            finish(&api, id, &owner, "?process=global"),
            (403, r#"{"code":"processing-disabled"}"#.into())
        );
        // The refused finish did not end the stream.
        let (status, _) = finish(&api, id, &owner, "");
        assert_eq!(status, 200);
    }

    #[test]
    fn a_service_without_a_processor_answers_processing_unavailable() {
        let api = start(on_without_processor());
        assert_eq!(
            refused_transcription(&api, "/transcriptions?model=parakeet_v2&process=global"),
            (409, r#"{"code":"processing-unavailable"}"#.into())
        );
        let (id, owner) = created(&api);
        assert_eq!(
            finish(&api, id, &owner, "?process=global"),
            (409, r#"{"code":"processing-unavailable"}"#.into())
        );
        let (status, _) = finish(&api, id, &owner, "");
        assert_eq!(status, 200);

        // `hex service` is this state: no processor and the lab off. The
        // missing processor is named first, not `processing-disabled`.
        let api = start(Processing::gated(None, || false));
        assert_eq!(
            refused_transcription(&api, "/transcriptions?model=parakeet_v2&process=global"),
            (409, r#"{"code":"processing-unavailable"}"#.into())
        );
        let (id, owner) = created(&api);
        assert_eq!(
            finish(&api, id, &owner, "?process=global"),
            (409, r#"{"code":"processing-unavailable"}"#.into())
        );
        let (status, _) = finish(&api, id, &owner, "");
        assert_eq!(status, 200);
    }

    #[test]
    fn a_busy_processor_answers_queue_full_before_the_body_or_finish() {
        let processing = on();
        let api = start(processing.clone());
        let _guard = gate(true, &processing).ok().flatten().unwrap();
        assert_eq!(
            refused_transcription(&api, "/transcriptions?model=parakeet_v2&process=global"),
            (429, r#"{"code":"queue-full"}"#.into())
        );
        let (id, owner) = created(&api);
        assert_eq!(
            finish(&api, id, &owner, "?process=global"),
            (429, r#"{"code":"queue-full"}"#.into())
        );
        // The refused finish leaves the stream running.
        let (status, body) = finish(&api, id, &owner, "");
        assert_eq!(status, 200, "{body}");
    }

    #[test]
    fn any_other_or_repeated_value_is_invalid() {
        for query in [
            "process=",
            "process=local",
            "process=GLOBAL",
            "process=0",
            "process=false",
            "process=global&process=global",
            "process=global&process=true",
        ] {
            let api = start(on());
            assert_eq!(
                refused_transcription(&api, &format!("/transcriptions?model=parakeet_v2&{query}")),
                (400, r#"{"code":"invalid-request"}"#.into()),
                "{query}"
            );
            let (id, owner) = created(&api);
            assert_eq!(
                finish(&api, id, &owner, &format!("?{query}")),
                (400, r#"{"code":"invalid-request"}"#.into()),
                "{query}"
            );
            cancel(&api, id, &owner);
        }
    }

    #[test]
    fn an_aliased_process_value_asks_for_global() {
        for query in ["process=global", "process=true", "process=1"] {
            let api = start(on());
            let (id, owner) = created(&api);
            half_second(&api, id, &owner);
            let (status, body) = finish(&api, id, &owner, &format!("?{query}"));
            assert_eq!(status, 200, "{query}: {body}");
            let body: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(
                body["processed"], "live 8000 [en] processed",
                "{query}: {body}"
            );
        }
    }

    #[test]
    fn processing_adds_the_processed_text_and_keeps_the_final() {
        let api = start(on());
        let (id, owner) = created(&api);
        half_second(&api, id, &owner);
        let (status, body) = finish(&api, id, &owner, "?process=global");
        assert_eq!(status, 200, "{body}");
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["text"], "live 8000");
        assert_eq!(body["durationMs"], 500);
        assert_eq!(body["segments"][0]["endMs"], 500);
        assert_eq!(body["processed"], "live 8000 [en] processed");
    }

    #[test]
    fn an_absent_process_parameter_keeps_the_response_as_before() {
        let api = start(on());
        let (id, owner) = created(&api);
        half_second(&api, id, &owner);
        let (status, body) = finish(&api, id, &owner, "");
        assert_eq!(status, 200, "{body}");
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["text"], "live 8000");
        assert!(body.get("processed").is_none(), "{body}");

        let (status, body) = refused_transcription(&api, "/transcriptions?model=parakeet_v2");
        assert_eq!(status, 409, "{body}");
        assert_eq!(body, r#"{"code":"model-not-ready"}"#);
    }

    #[test]
    fn a_transcriptions_response_gains_only_the_processed_field() {
        let processing = on();
        let guard = gate(true, &processing).ok().flatten().unwrap();
        let cancelled = AtomicBool::new(false);
        let transcript = serde_json::json!({ "transcript": "hello there", "durationMs": 750 });
        let mut processed = transcript.clone();
        attach(
            &mut processed,
            Some(&guard),
            "hello there",
            "en",
            &cancelled,
        );
        assert_eq!(
            processed,
            serde_json::json!({
                "transcript": "hello there",
                "durationMs": 750,
                "processed": "hello there [en] processed",
            })
        );
        let mut raw = transcript.clone();
        attach(&mut raw, None, "hello there", "en", &cancelled);
        assert_eq!(raw, transcript);
    }

    #[test]
    fn capabilities_report_processing_only_when_installed_and_on() {
        let cases = [
            (off(), false),
            (on_without_processor(), false),
            (Processing::gated(None, || false), false),
            (on(), true),
        ];
        for (processing, expected) in cases {
            let api = start(processing);
            let (status, body) = request(&api, "/capabilities");
            assert_eq!(status, 200, "{body}");
            let body: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(body["outputProcessing"], expected, "{body}");
        }
    }
}
