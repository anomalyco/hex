//! Native timings on transcription responses: the `timestamps` query on
//! `POST /transcriptions` (Labs › Timestamps in API transcripts). The models
//! produce the times; this module owns the query, the lab gate, and the
//! response fields, and never estimates, interpolates, or downgrades one.
//!
//! `/streams` keeps its own segment timing and does not offer this option.

use crate::transcription::transcript::{TimestampLevel, Transcript, TranscriptSegment};

use super::HttpResponse;

/// Whether this local API may answer a timed request: the Labs opt-in, read
/// per request so turning it off refuses at once.
#[derive(Clone, Copy)]
pub struct Timestamps {
    enabled: fn() -> bool,
}

impl Default for Timestamps {
    fn default() -> Self {
        Self {
            enabled: crate::labs::api_timestamps,
        }
    }
}

impl Timestamps {
    /// Under another gate, so route tests need no global Labs state.
    #[cfg(test)]
    pub(super) fn gated(enabled: fn() -> bool) -> Self {
        Self { enabled }
    }

    /// Whether a timed request would run.
    pub(super) fn available(self) -> bool {
        (self.enabled)()
    }
}

/// The refusal for a request that asked for timings while the lab is off.
/// `none` and an absent parameter never need it.
pub(super) fn gate(requested: TimestampLevel, timestamps: &Timestamps) -> Result<(), HttpResponse> {
    if requested == TimestampLevel::None || timestamps.available() {
        return Ok(());
    }
    Err(HttpResponse::code(403, "timestamps-disabled"))
}

/// The timing level a request's query asked for: `none` (the default),
/// `segment`, or `word`. Any other value, or a repeated parameter, is
/// `invalid-request`.
pub(super) fn requested(path: &str) -> Result<TimestampLevel, &'static str> {
    let query = path
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default();
    let mut requested = None;
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if name != "timestamps" {
            continue;
        }
        if requested.is_some() {
            return Err("invalid-request");
        }
        requested = Some(match value.as_ref() {
            "none" => TimestampLevel::None,
            "segment" => TimestampLevel::Segment,
            "word" => TimestampLevel::Word,
            _ => return Err("invalid-request"),
        });
    }
    Ok(requested.unwrap_or(TimestampLevel::None))
}

/// Adds `timestampGranularity`, `segments`, and (for a word request) `words`
/// to a transcription response; every existing field stays as it is. Times are
/// whole milliseconds from the start of the submitted audio, clamped to its
/// duration because a padded clip's last timings can run past it.
pub(super) fn attach(
    body: &mut serde_json::Value,
    requested: TimestampLevel,
    transcript: &Transcript,
    duration_ms: u64,
) {
    let Some(granularity) = requested.granularity() else {
        return;
    };
    body["timestampGranularity"] = serde_json::Value::String(granularity.to_owned());
    body["segments"] = serde_json::Value::Array(spans(&transcript.segments, duration_ms));
    if requested == TimestampLevel::Word {
        body["words"] = serde_json::Value::Array(spans(&transcript.words, duration_ms));
    }
}

fn spans(spans: &[TranscriptSegment], duration_ms: u64) -> Vec<serde_json::Value> {
    spans
        .iter()
        .map(|span| {
            let (start_ms, end_ms) = clamped(span, duration_ms);
            serde_json::json!({
                "text": span.text,
                "startMs": start_ms,
                "endMs": end_ms,
            })
        })
        .collect()
}

/// One span on the submitted audio's timeline. A padded clip can produce
/// times past its audio, so both ends stop at `duration_ms`; neither ever
/// goes backwards.
fn clamped(span: &TranscriptSegment, duration_ms: u64) -> (i64, i64) {
    let duration = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let start = span.start_ms.clamp(0, duration.max(0));
    let end = span.end_ms.clamp(start, duration.max(start));
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::super::{LocalApi, LocalApiOptions, current_health};
    use super::*;
    use crate::desktop::events::EventLog;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn start(timestamps: Timestamps) -> LocalApi {
        let directory = std::env::temp_dir().join(format!(
            "hex-api-timestamps-{}-{}",
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
                timestamps,
                ..LocalApiOptions::default()
            },
            None,
        )
        .unwrap()
    }

    /// Sends one request and returns its status and body.
    fn send(api: &LocalApi, request: &str) -> (u16, String) {
        let mut connection = TcpStream::connect(("127.0.0.1", api.document.port)).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        connection.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        connection.read_to_end(&mut response).unwrap();
        let response = String::from_utf8(response).unwrap();
        let status = response[9..12].parse().unwrap();
        let body = response.split_once("\r\n\r\n").unwrap().1.to_string();
        (status, body)
    }

    /// Declares an audio body without sending it: a refused request must
    /// answer without reading it.
    fn refused(api: &LocalApi, path: &str) -> (u16, String) {
        send(
            api,
            &format!(
                "POST {path} HTTP/1.1\r\nAuthorization: Bearer {}\r\nContent-Type: audio/wav\r\nContent-Length: 4\r\n\r\n",
                api.document.token,
            ),
        )
    }

    fn off() -> Timestamps {
        Timestamps::gated(|| false)
    }

    fn on() -> Timestamps {
        Timestamps::gated(|| true)
    }

    fn span(start_ms: i64, end_ms: i64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            start_ms,
            end_ms,
            text: text.into(),
        }
    }

    fn transcript() -> Transcript {
        Transcript {
            text: "hello there".into(),
            segments: vec![span(0, 480, "hello there")],
            words: vec![span(0, 240, "hello"), span(250, 480, "there")],
        }
    }

    #[test]
    fn an_absent_parameter_asks_for_no_timings() {
        for path in [
            "/transcriptions",
            "/transcriptions?model=parakeet_v2",
            "/transcriptions?timestamps=none",
        ] {
            assert_eq!(requested(path), Ok(TimestampLevel::None), "{path}");
        }
    }

    #[test]
    fn each_level_parses_and_anything_else_is_invalid() {
        assert_eq!(
            requested("/transcriptions?timestamps=segment").unwrap(),
            TimestampLevel::Segment
        );
        assert_eq!(
            requested("/transcriptions?model=parakeet_v2&timestamps=word").unwrap(),
            TimestampLevel::Word
        );
        for query in [
            "timestamps=",
            "timestamps=nonee",
            "timestamps=Word",
            "timestamps=1",
            "timestamps=segment&timestamps=segment",
            "timestamps=segment&timestamps=word",
            "timestamps=none&timestamps=none",
        ] {
            assert_eq!(
                requested(&format!("/transcriptions?{query}")),
                Err("invalid-request"),
                "{query}"
            );
        }
    }

    #[test]
    fn the_lab_gates_a_timed_request_before_the_audio_body_is_read() {
        let api = start(off());
        assert_eq!(
            refused(&api, "/transcriptions?model=parakeet_v2&timestamps=word"),
            (403, r#"{"code":"timestamps-disabled"}"#.into())
        );
        // The untimed request is not refused by this lab at all.
        let (status, body) = refused(&api, "/transcriptions?model=parakeet_v2");
        assert_eq!(status, 409, "{body}");
        assert_eq!(body, r#"{"code":"model-not-ready"}"#);
        // An invalid value is answered as invalid even while the lab is off.
        assert_eq!(
            refused(&api, "/transcriptions?model=parakeet_v2&timestamps=loud"),
            (400, r#"{"code":"invalid-request"}"#.into())
        );
    }

    #[test]
    fn an_untimed_response_gains_nothing() {
        let mut body = serde_json::json!({ "transcript": "hello there", "durationMs": 480 });
        let before = body.clone();
        attach(&mut body, TimestampLevel::None, &transcript(), 480);
        assert_eq!(body, before);
    }

    #[test]
    fn a_segment_request_adds_only_segments() {
        let mut body = serde_json::json!({ "transcript": "hello there", "durationMs": 480 });
        attach(&mut body, TimestampLevel::Segment, &transcript(), 480);
        assert_eq!(
            body,
            serde_json::json!({
                "transcript": "hello there",
                "durationMs": 480,
                "timestampGranularity": "segment",
                "segments": [{ "text": "hello there", "startMs": 0, "endMs": 480 }],
            })
        );
    }

    #[test]
    fn a_word_request_adds_segments_and_words() {
        let mut body = serde_json::json!({ "transcript": "hello there", "durationMs": 480 });
        attach(&mut body, TimestampLevel::Word, &transcript(), 480);
        assert_eq!(
            body,
            serde_json::json!({
                "transcript": "hello there",
                "durationMs": 480,
                "timestampGranularity": "word",
                "segments": [{ "text": "hello there", "startMs": 0, "endMs": 480 }],
                "words": [
                    { "text": "hello", "startMs": 0, "endMs": 240 },
                    { "text": "there", "startMs": 250, "endMs": 480 },
                ],
            })
        );
    }

    #[test]
    fn times_stop_at_the_submitted_audios_duration() {
        let padded = Transcript {
            text: "hello".into(),
            segments: vec![span(0, 2_000, "hello")],
            words: vec![span(400, 2_000, "hello")],
        };
        let mut body = serde_json::json!({ "transcript": "hello", "durationMs": 480 });
        attach(&mut body, TimestampLevel::Word, &padded, 480);
        assert_eq!(body["segments"][0]["endMs"], 480);
        assert_eq!(body["words"][0]["startMs"], 400);
        assert_eq!(body["words"][0]["endMs"], 480);
        // A span that starts past the audio collapses to its end, never
        // backwards.
        assert_eq!(
            clamped(&span(900, 1_200, "late"), 480),
            (480, 480),
            "a late span is clamped, not reversed"
        );
        assert_eq!(clamped(&span(-40, 120, "early"), 480), (0, 120));
    }

    #[test]
    fn a_silent_clip_answers_empty_arrays() {
        let mut body = serde_json::json!({ "transcript": "", "durationMs": 1_500 });
        attach(
            &mut body,
            TimestampLevel::Word,
            &Transcript::default(),
            1_500,
        );
        assert_eq!(
            body,
            serde_json::json!({
                "transcript": "",
                "durationMs": 1_500,
                "timestampGranularity": "word",
                "segments": [],
                "words": [],
            })
        );
    }

    #[test]
    fn a_timed_request_needs_the_lab() {
        assert!(gate(TimestampLevel::None, &off()).is_ok());
        assert!(gate(TimestampLevel::None, &on()).is_ok());
        let refusal = gate(TimestampLevel::Word, &off()).unwrap_err();
        assert_eq!(refusal.status, 403);
        assert_eq!(refusal.body, br#"{"code":"timestamps-disabled"}"#);
        assert!(gate(TimestampLevel::Segment, &on()).is_ok());
    }

    #[test]
    fn a_model_without_that_level_answers_unsupported_timestamps() {
        let response = super::super::transcriptions::transcription_error(
            crate::transcription::service::TranscriptionServiceError::Timestamps,
        );
        assert_eq!(response.status, 422);
        assert_eq!(response.body, br#"{"code":"unsupported-timestamps"}"#);
    }
}
