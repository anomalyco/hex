//! Labs › Browser access: the CORS headers a page origin Hex lists gets, and
//! the one request it may make without a bearer token. Off, or for an origin
//! that isn't listed, nothing changes: no `Access-Control-*` header, and
//! `OPTIONS` is answered `401` exactly as before.

use crate::labs;

/// Whether browser access is on and which origins it lists, read per request
/// (`fn` pointers like [`super::Processing`]), so a Labs change applies at
/// once and a route test can inject its own answer.
#[derive(Clone, Copy)]
pub struct BrowserAccess {
    enabled: fn() -> bool,
    origins: fn() -> Vec<String>,
}

impl Default for BrowserAccess {
    fn default() -> Self {
        Self {
            enabled: labs::api_browser_access,
            origins: labs::api_browser_origins,
        }
    }
}

impl BrowserAccess {
    /// A fixed switch and list, so route tests need no global Labs state.
    #[cfg(test)]
    pub(super) fn gated(enabled: fn() -> bool, origins: fn() -> Vec<String>) -> Self {
        Self { enabled, origins }
    }

    /// The `Access-Control-Allow-Origin` this request carries: its own
    /// `Origin`, normalized, when browser access is on and lists it.
    pub(super) fn allow_origin(self, origin: Option<&str>) -> Option<String> {
        if !(self.enabled)() {
            return None;
        }
        let origin = labs::origins::normalize(origin?)?;
        (self.origins)().contains(&origin).then_some(origin)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::Receiver;
    use std::thread;
    use std::time::Duration;

    use super::super::{LocalApi, LocalApiOptions, current_health, write_sse_headers};
    use super::*;
    use crate::desktop::events::EventLog;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    const ORIGIN: &str = "http://localhost:3000";

    /// An API whose browser access answers `browser_access`. The returned
    /// receiver keeps its `POST /control` channel open.
    fn start(browser_access: BrowserAccess) -> (LocalApi, Receiver<crate::control::ControlCall>) {
        let directory = std::env::temp_dir().join(format!(
            "hex-api-cors-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let (control, controls) = std::sync::mpsc::sync_channel(1);
        let api = LocalApi::start_bound(
            None,
            Arc::new(current_health),
            events,
            LocalApiOptions {
                browser_access,
                control: Some(control),
                ..LocalApiOptions::default()
            },
            None,
        )
        .unwrap();
        (api, controls)
    }

    /// Sends one request and returns its whole response.
    fn send(api: &LocalApi, request: &str) -> String {
        let mut connection = TcpStream::connect(("127.0.0.1", api.document.port)).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        connection.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        connection.read_to_end(&mut response).unwrap();
        String::from_utf8(response).unwrap()
    }

    fn preflight(api: &LocalApi, origin: &str, extra: &str) -> String {
        send(
            api,
            &format!(
                "OPTIONS /transcriptions HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {origin}\r\nAccess-Control-Request-Method: POST\r\n{extra}Connection: close\r\n\r\n"
            ),
        )
    }

    fn off() -> BrowserAccess {
        BrowserAccess::gated(|| false, Vec::new)
    }

    fn on() -> BrowserAccess {
        BrowserAccess::gated(|| true, || vec![ORIGIN.into()])
    }

    #[test]
    fn the_lab_off_leaves_options_and_every_response_as_before() {
        let (api, _controls) = start(off());
        let response = preflight(&api, ORIGIN, "");
        assert!(response.starts_with("HTTP/1.1 401"), "{response}");
        assert!(!response.contains("Access-Control-"), "{response}");
        assert!(!response.contains("Vary:"), "{response}");

        let authenticated = send(
            &api,
            &format!(
                "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {ORIGIN}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
                api.document.token
            ),
        );
        assert!(authenticated.starts_with("HTTP/1.1 200"), "{authenticated}");
        assert!(
            !authenticated.contains("Access-Control-"),
            "{authenticated}"
        );
    }

    #[test]
    fn a_listed_origin_passes_its_preflight_without_a_token() {
        let (api, _controls) = start(on());
        let response = preflight(&api, ORIGIN, "");
        assert!(response.starts_with("HTTP/1.1 204"), "{response}");
        assert!(
            response.contains("Access-Control-Allow-Origin: http://localhost:3000\r\n"),
            "{response}"
        );
        assert!(response.contains("Vary: Origin\r\n"), "{response}");
        assert!(
            response.contains("Access-Control-Allow-Methods: GET, POST\r\n"),
            "{response}"
        );
        assert!(
            response.contains(
                "Access-Control-Allow-Headers: Authorization, Content-Type, X-Hex-Dictation-Token, X-Hex-Stream-Token\r\n"
            ),
            "{response}"
        );
        assert!(
            response.contains("Access-Control-Max-Age: 600\r\n"),
            "{response}"
        );
        assert!(
            !response.contains("Access-Control-Allow-Private-Network"),
            "{response}"
        );
        assert_eq!(response.split("\r\n\r\n").nth(1), Some(""), "{response}");
    }

    #[test]
    fn private_network_is_granted_only_when_the_page_asks() {
        let (api, _controls) = start(on());
        let asked = preflight(
            &api,
            ORIGIN,
            "Access-Control-Request-Private-Network: true\r\n",
        );
        assert!(asked.starts_with("HTTP/1.1 204"), "{asked}");
        assert!(
            asked.contains("Access-Control-Allow-Private-Network: true\r\n"),
            "{asked}"
        );
        let not_asked = preflight(
            &api,
            ORIGIN,
            "Access-Control-Request-Private-Network: false\r\n",
        );
        assert!(
            !not_asked.contains("Access-Control-Allow-Private-Network"),
            "{not_asked}"
        );
    }

    #[test]
    fn a_listed_origin_gets_the_header_on_authenticated_and_refused_requests() {
        let (api, _controls) = start(on());
        // An authenticated `POST /control`, answered as a version mismatch
        // without reaching the app.
        let body = r#"{"version":1,"command":"status"}"#;
        let control = send(
            &api,
            &format!(
                "POST /control HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {ORIGIN}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                api.document.token,
                body.len()
            ),
        );
        assert!(control.starts_with("HTTP/1.1 200"), "{control}");
        assert!(
            control.contains("Access-Control-Allow-Origin: http://localhost:3000\r\n"),
            "{control}"
        );
        assert!(control.contains("Vary: Origin\r\n"), "{control}");
        // A preflight's allow-headers list never leaks into a normal answer.
        assert!(
            !control.contains("Access-Control-Allow-Methods"),
            "{control}"
        );

        // Without a token the request is still refused, now with the header.
        let refused = send(
            &api,
            &format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {ORIGIN}\r\n\r\n"),
        );
        assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
        assert!(
            refused.contains("Access-Control-Allow-Origin: http://localhost:3000\r\n"),
            "{refused}"
        );
    }

    #[test]
    fn an_origin_not_listed_changes_nothing() {
        let (api, _controls) = start(on());
        let response = preflight(&api, "http://evil.example", "");
        assert!(response.starts_with("HTTP/1.1 401"), "{response}");
        assert!(!response.contains("Access-Control-"), "{response}");

        let authenticated = send(
            &api,
            &format!(
                "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: http://evil.example\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
                api.document.token
            ),
        );
        assert!(authenticated.starts_with("HTTP/1.1 200"), "{authenticated}");
        assert!(
            !authenticated.contains("Access-Control-"),
            "{authenticated}"
        );
    }

    #[test]
    fn a_listed_origin_is_compared_normalized() {
        let (api, _controls) = start(on());
        // A hand-edited uppercase listing still matches what the browser
        // sends, and the answer names the request's own normalized origin.
        let response = preflight(&api, "HTTP://LOCALHOST:3000", "");
        assert!(response.starts_with("HTTP/1.1 204"), "{response}");
        assert!(
            response.contains("Access-Control-Allow-Origin: http://localhost:3000\r\n"),
            "{response}"
        );
    }

    #[test]
    fn an_options_without_a_requested_method_is_not_a_preflight() {
        let (api, _controls) = start(on());
        let response = send(
            &api,
            &format!(
                "OPTIONS /health HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {ORIGIN}\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(response.starts_with("HTTP/1.1 401"), "{response}");
        // It is still the listed origin's answer, so it carries the header.
        assert!(
            response.contains("Access-Control-Allow-Origin: http://localhost:3000\r\n"),
            "{response}"
        );
        assert!(
            !response.contains("Access-Control-Allow-Methods"),
            "{response}"
        );
    }

    #[test]
    fn the_streaming_writer_carries_the_header_and_nothing_without_it() {
        for (allow_origin, expected) in [(Some(ORIGIN), true), (None, false)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let client = thread::spawn(move || {
                let mut stream = TcpStream::connect(address).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut response = Vec::new();
                stream.read_to_end(&mut response).unwrap();
                String::from_utf8(response).unwrap()
            });
            let (mut stream, _) = listener.accept().unwrap();
            write_sse_headers(&mut stream, allow_origin).unwrap();
            drop(stream);
            let response = client.join().unwrap();
            assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
            assert!(response.contains("Content-Type: text/event-stream\r\n"));
            assert!(response.contains("X-Content-Type-Options: nosniff\r\n"));
            assert_eq!(
                response.contains("Access-Control-Allow-Origin: http://localhost:3000\r\n"),
                expected,
                "{response}"
            );
            assert_eq!(
                response.contains("Vary: Origin\r\n"),
                expected,
                "{response}"
            );
        }
    }
}
