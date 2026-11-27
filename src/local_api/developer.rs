//! The developer routes: `POST /dev/control`, which hands a debug build's
//! `hex dev` commands to the window, and `POST /dev/accessibility-probe`,
//! which reads another app's windows for `hex dev call-window` in dev builds
//! on macOS (`accessibility::probe`).

use std::net::TcpStream;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};

use super::{
    DEVELOPER_CONTROL_TIMEOUT, HttpContext, HttpRequest, HttpResponse, MAX_DEVELOPER_CONTROL_BYTES,
    RequestAction, body_read_response, json_post_action, read_body_bounded,
};
use crate::control::developer_control::DeveloperCall;

/// The probe's route; named here so every OS's API knows to refuse it.
const PROBE_PATH: &str = "/dev/accessibility-probe";

/// A developer request whose body is still to be read.
pub(super) struct Action {
    route: Route,
    content_length: usize,
    body_prefix: Vec<u8>,
}

#[derive(Clone, Copy)]
enum Route {
    Control,
    Probe,
}

/// Whether `POST path` is a developer route.
pub(super) fn routes(path: &str) -> bool {
    path == "/dev/control" || path == PROBE_PATH
}

pub(super) fn action(request: HttpRequest, context: &HttpContext) -> RequestAction {
    let probe = request.path.split('?').next() == Some(PROBE_PATH);
    let (route, available, unavailable) = if probe {
        (
            Route::Probe,
            probe_offered(),
            "accessibility-probe-unavailable",
        )
    } else {
        (
            Route::Control,
            context.developer_control.is_some(),
            "developer-control-unavailable",
        )
    };
    json_post_action(
        request,
        available,
        unavailable,
        MAX_DEVELOPER_CONTROL_BYTES,
        "developer-control-too-large",
        |content_length, body_prefix| {
            RequestAction::Developer(Action {
                route,
                content_length,
                body_prefix,
            })
        },
    )
}

/// Reads the request's body and answers it.
pub(super) fn handle(
    stream: &mut TcpStream,
    action: Action,
    context: &HttpContext,
) -> HttpResponse {
    let mut body = action.body_prefix;
    if let Err(error) = read_body_bounded(
        stream,
        action.content_length,
        &mut body,
        &context.shutdown,
        MAX_DEVELOPER_CONTROL_BYTES,
        DEVELOPER_CONTROL_TIMEOUT,
    ) {
        return body_read_response(error);
    }
    match action.route {
        Route::Control => control(&body, context.developer_control.as_ref()),
        Route::Probe => probe(&body),
    }
}

fn control(body: &[u8], controls: Option<&SyncSender<DeveloperCall>>) -> HttpResponse {
    let Ok(command) = serde_json::from_slice(body) else {
        return HttpResponse::code(400, "invalid-developer-command");
    };
    let Some(controls) = controls else {
        return HttpResponse::code(404, "developer-control-unavailable");
    };
    let (reply, response) = mpsc::sync_channel(1);
    match controls.try_send(DeveloperCall { command, reply }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            return HttpResponse::code(503, "developer-control-busy");
        }
        Err(TrySendError::Disconnected(_)) => {
            return HttpResponse::code(503, "developer-control-unavailable");
        }
    }
    match response.recv_timeout(DEVELOPER_CONTROL_TIMEOUT) {
        Ok(reply) => HttpResponse::json(200, &reply),
        Err(RecvTimeoutError::Timeout) => HttpResponse::code(504, "developer-ui-timeout"),
        Err(RecvTimeoutError::Disconnected) => {
            HttpResponse::code(503, "developer-control-unavailable")
        }
    }
}

/// Whether this Hex answers the probe: a dev build on macOS, the one OS
/// whose Hex has an Accessibility client to read another app with.
fn probe_offered() -> bool {
    #[cfg(target_os = "macos")]
    return crate::accessibility::probe::offered(crate::dev_build::is_dev_build());
    #[cfg(not(target_os = "macos"))]
    false
}

/// Runs one read on this API worker: the app holds the Accessibility grant,
/// and the read is bounded by its own deadline.
#[cfg(target_os = "macos")]
fn probe(body: &[u8]) -> HttpResponse {
    match serde_json::from_slice(body) {
        Ok(request) => HttpResponse::json(200, &crate::accessibility::probe::answer(&request)),
        Err(_) => HttpResponse::code(400, "invalid-accessibility-probe"),
    }
}

#[cfg(not(target_os = "macos"))]
fn probe(_body: &[u8]) -> HttpResponse {
    HttpResponse::code(404, "accessibility-probe-unavailable")
}

/// Asks the running Hex for one read. The reply may hold a whole tree, so
/// it may be far larger than a control reply.
#[cfg(target_os = "macos")]
pub fn accessibility_probe(
    request: &crate::accessibility::probe::Request,
) -> color_eyre::Result<crate::accessibility::probe::Reply> {
    use color_eyre::eyre::{WrapErr, bail};

    const MAX_REPLY_BYTES: usize = 64 * 1024 * 1024;
    let body = serde_json::to_vec(request)?;
    let wait = std::time::Duration::from_millis(request.deadline_ms.min(5_000))
        + std::time::Duration::from_secs(5);
    let (status, body) = super::discovery::post_json(PROBE_PATH, &body, wait, MAX_REPLY_BYTES)
        .wrap_err(
            "the read runs in the running Hex, which holds the Accessibility grant \
             (start it with `hex start`)",
        )?;
    if status != 200 {
        let code = super::error_code(status, &body);
        bail!(match code.as_str() {
            "accessibility-probe-unavailable" | "not_found" => {
                "the running Hex doesn't offer the call-window probe: it is a stable build, or \
                 an older one"
                    .to_string()
            }
            _ => format!("Hex refused the request ({code})"),
        });
    }
    crate::accessibility::probe::Reply::from_json_slice(&body)
        .wrap_err("Hex sent an invalid call-window reply")
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::sync::Arc;

    use crate::desktop::events::EventLog;
    use crate::local_api::{API_VERSION, HealthResponse, LocalApi, LocalApiOptions};

    /// A test build is a dev build, so macOS answers; no other OS's Hex has
    /// an Accessibility client. The request names a pid nothing has, so no
    /// app is asked anything.
    #[test]
    fn the_accessibility_probe_route_answers_on_macos_only() {
        let directory = std::env::temp_dir().join(format!("hex-probe-api-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let events = EventLog::create(&directory.join("events.ndjson")).unwrap();
        let health = || HealthResponse {
            version: "test",
            api_version: API_VERSION,
        };
        let mut api = LocalApi::start_bound(
            None,
            Arc::new(health),
            events,
            LocalApiOptions::default(),
            None,
        )
        .unwrap();
        let (port, token) = (api.document.port, api.document.token.clone());
        let send = |request: String| {
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        };
        let post = |body: &str| {
            send(format!(
                "POST /dev/accessibility-probe HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ))
        };
        let response =
            post(r#"{"target":{"pid":2147483647},"max_nodes":1,"max_depth":1,"deadline_ms":1}"#);
        #[cfg(target_os = "macos")]
        {
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            let body = response.split_once("\r\n\r\n").unwrap().1;
            assert!(
                body.starts_with(r#"{"status":"error","code":"target-not-found""#),
                "{response}"
            );
            let invalid = post("{}");
            assert!(invalid.starts_with("HTTP/1.1 400"), "{invalid}");
            assert!(invalid.contains("invalid-accessibility-probe"));
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(response.starts_with("HTTP/1.1 404"), "{response}");
            assert!(response.contains("accessibility-probe-unavailable"));
        }
        let unauthenticated = send(
            "POST /dev/accessibility-probe HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .into(),
        );
        assert!(
            unauthenticated.starts_with("HTTP/1.1 401"),
            "{unauthenticated}"
        );

        api.shutdown();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn deep_call_window_replies_deserialize_without_hitting_json_recursion_limit() {
        use crate::accessibility::probe::{DEPTH_CEILING, Reading, Reply};
        use crate::accessibility::tree::Node;

        let build = |depth: usize| {
            let mut node = Node {
                role: Some("AXStaticText".into()),
                title: Some("leaf".into()),
                ..Default::default()
            };
            for level in (0..depth).rev() {
                node = Node {
                    role: Some(if level == 0 { "AXWindow" } else { "AXGroup" }.into()),
                    children: vec![node],
                    ..Default::default()
                };
            }
            Reply::Read(Reading {
                pid: 42,
                frontmost: Some(false),
                elapsed_ms: 1.0,
                windows: 1,
                nodes: depth + 1,
                unreadable: 0,
                cut_short: Vec::new(),
                web_areas: 0,
                web_nodes: 0,
                web_accessibility_requested: None,
                matches: None,
                tree: Some(vec![node]),
            })
        };

        for depth in [80, DEPTH_CEILING] {
            let reply = build(depth);
            let json = serde_json::to_vec(&reply).unwrap();
            assert!(
                serde_json::from_slice::<Reply>(&json).is_err(),
                "depth {depth} exceeds serde_json's default 128-level limit"
            );
            assert_eq!(Reply::from_json_slice(&json).unwrap(), reply);
        }
        assert!(
            Reply::from_json_slice(br#"{"status":"error","code":"x","message":"y"} trailing"#)
                .is_err()
        );
    }
}
