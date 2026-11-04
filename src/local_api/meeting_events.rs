//! `GET /meetings/events` (Labs › Meeting finished event): one subscriber
//! at a time receives `data: {"type":"meeting.finished","id":"<id>"}` once
//! per meeting whose final transcript is written, and a comment keepalive
//! between. Only the id is sent; clients read the rest with
//! `hex meetings show <id>`. While the toggle is off the route answers
//! `404 labs-off`, and a stream already open ends.

use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use super::{HttpContext, HttpRequest, HttpResponse, RequestAction, write_sse, write_sse_headers};
use crate::meeting::finished_events::{Hub, Subscription};

pub(super) const PATH: &str = "/meetings/events";
/// How often the stream checks shutdown, the toggle, and the client.
const POLL: Duration = Duration::from_millis(250);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Where the route subscribes, and the gate it reads.
#[derive(Clone, Copy)]
pub(crate) struct Source {
    pub(crate) hub: &'static Hub,
    pub(crate) enabled: fn() -> bool,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            hub: crate::meeting::finished_events::global(),
            enabled: crate::labs::meeting_finished_event,
        }
    }
}

pub(super) fn action(request: &HttpRequest, context: &HttpContext) -> RequestAction {
    if request.method != "GET" {
        return RequestAction::Respond(HttpResponse::code(404, "not_found"));
    }
    let source = context.meeting_events;
    if !(source.enabled)() {
        return RequestAction::Respond(HttpResponse::code(404, "labs-off"));
    }
    match source.hub.subscribe() {
        Some(subscription) => RequestAction::MeetingEvents(subscription),
        None => RequestAction::Respond(HttpResponse::code(409, "subscriber-busy")),
    }
}

/// Streams finished meetings until shutdown, the toggle turning off, or the
/// client leaving; the subscription's drop frees the slot.
pub(super) fn stream(stream: &mut TcpStream, subscription: Subscription, context: &HttpContext) {
    if let Err(error) = write_sse_headers(stream) {
        tracing::debug!(%error, "meeting events client disconnected before streaming");
        return;
    }
    let mut last_write = Instant::now();
    loop {
        if context.shutdown.load(Ordering::Acquire)
            || !(context.meeting_events.enabled)()
            || client_closed(stream)
        {
            return;
        }
        let written = match subscription.next(POLL) {
            Ok(id) => write_sse(
                stream,
                &serde_json::json!({ "type": "meeting.finished", "id": id }),
            ),
            Err(RecvTimeoutError::Timeout) if last_write.elapsed() >= KEEP_ALIVE => stream
                .write_all(b": keep-alive\n\n")
                .and_then(|()| stream.flush()),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if let Err(error) = written {
            tracing::debug!(%error, "meeting events client disconnected");
            return;
        }
        last_write = Instant::now();
    }
}

/// Whether the client closed its end; it never sends anything after the
/// request.
fn client_closed(stream: &TcpStream) -> bool {
    if stream.set_nonblocking(true).is_err() {
        return true;
    }
    let mut byte = [0_u8; 1];
    let closed = match stream.peek(&mut byte) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) => error.kind() != std::io::ErrorKind::WouldBlock,
    };
    stream.set_nonblocking(false).is_err() || closed
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use super::super::{LocalApi, LocalApiOptions, current_health};
    use super::*;
    use crate::desktop::events::EventLog;

    static ON: AtomicBool = AtomicBool::new(false);

    fn on() -> bool {
        ON.load(Ordering::SeqCst)
    }

    fn send(port: u16, request: &str) -> TcpStream {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream
    }

    fn get(port: u16, token: Option<&str>) -> TcpStream {
        let authorization = token
            .map(|token| format!("Authorization: Bearer {token}\r\n"))
            .unwrap_or_default();
        send(
            port,
            &format!("GET {PATH} HTTP/1.1\r\nHost: 127.0.0.1\r\n{authorization}\r\n"),
        )
    }

    fn whole(mut stream: TcpStream) -> String {
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    /// Reads until `needle` arrives.
    fn read_until(stream: &mut TcpStream, needle: &str) -> String {
        let mut seen = Vec::new();
        let mut buffer = [0_u8; 512];
        while !String::from_utf8_lossy(&seen).contains(needle) {
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0, "closed before {needle:?}");
            seen.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8(seen).unwrap()
    }

    #[test]
    fn a_finished_meeting_is_streamed_as_its_id_to_one_authenticated_subscriber() {
        static HUB: Hub = Hub::new();
        let directory = std::env::temp_dir().join(format!(
            "hex-meeting-events-{}-{}",
            std::process::id(),
            crate::desktop::events::now_ms()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let mut api = LocalApi::start_with_meeting_events(
            None,
            Arc::new(current_health),
            events,
            LocalApiOptions::default(),
            None,
            Source {
                hub: &HUB,
                enabled: on,
            },
        )
        .unwrap();
        let port = api.document.port;
        let token = api.document.token.clone();

        assert!(whole(get(port, None)).starts_with("HTTP/1.1 401"));
        let off = whole(get(port, Some(&token)));
        assert!(off.starts_with("HTTP/1.1 404"), "{off}");
        assert!(off.contains("labs-off"));

        ON.store(true, Ordering::SeqCst);
        let mut subscriber = get(port, Some(&token));
        let headers = read_until(&mut subscriber, "\r\n\r\n");
        assert!(headers.starts_with("HTTP/1.1 200"));
        assert!(headers.contains("text/event-stream"));
        let busy = whole(get(port, Some(&token)));
        assert!(busy.starts_with("HTTP/1.1 409"), "{busy}");
        assert!(busy.contains("subscriber-busy"));

        HUB.publish(on(), "1700000000000-42");
        let event = read_until(&mut subscriber, "\n\n");
        let data = event
            .strip_prefix("data: ")
            .and_then(|data| data.strip_suffix("\n\n"))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(data).unwrap(),
            serde_json::json!({ "type": "meeting.finished", "id": "1700000000000-42" }),
            "only the id"
        );

        // Turning the toggle off ends the stream and frees the slot.
        ON.store(false, Ordering::SeqCst);
        assert_eq!(whole(subscriber), "");
        ON.store(true, Ordering::SeqCst);
        let mut again = get(port, Some(&token));
        assert!(read_until(&mut again, "\r\n\r\n").starts_with("HTTP/1.1 200"));
        ON.store(false, Ordering::SeqCst);
        drop(again);

        api.shutdown();
        let _ = std::fs::remove_dir_all(directory);
    }
}
