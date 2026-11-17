//! `/control` and `/dev/control`: the `hex` CLI's control commands and the
//! desktop-control lab's scripted commands, handed to the app's own control
//! thread and answered with its reply.

use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};

use super::http::HttpResponse;
use super::{
    DEVELOPER_CONTROL_TIMEOUT, MAX_CONTROL_BYTES, MAX_DEVELOPER_CONTROL_BYTES, body_read_response,
    read_body_bounded,
};

pub(super) fn developer_control(
    stream: &mut TcpStream,
    content_length: usize,
    mut body: Vec<u8>,
    controls: Option<&SyncSender<crate::control::developer_control::DeveloperCall>>,
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
        return HttpResponse::code(400, "invalid-developer-command");
    };
    let Some(controls) = controls else {
        return HttpResponse::code(404, "developer-control-unavailable");
    };
    let (reply, response) = mpsc::sync_channel(1);
    match controls.try_send(crate::control::developer_control::DeveloperCall { command, reply }) {
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

/// Hands one `hex` control request to the app and waits for its reply.
/// Refusals travel as a 200 `ControlReply::Error`; an HTTP error means the
/// request never reached the app.
pub(super) fn control(
    stream: &mut TcpStream,
    content_length: usize,
    mut body: Vec<u8>,
    controls: Option<&SyncSender<crate::control::ControlCall>>,
    shutdown: &AtomicBool,
) -> HttpResponse {
    use crate::control::{ControlCall, ControlReply, ControlRequest, ControlResponse};

    if let Err(error) = read_body_bounded(
        stream,
        content_length,
        &mut body,
        shutdown,
        MAX_CONTROL_BYTES,
        DEVELOPER_CONTROL_TIMEOUT,
    ) {
        return body_read_response(error);
    }
    let request = match serde_json::from_slice::<ControlRequest>(&body) {
        Ok(request) => request,
        // A newer `hex` may name a command this app doesn't know.
        Err(_) => match serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("version")?.as_u64())
        {
            Some(version) if version != u64::from(crate::control::CONTROL_VERSION) => {
                let error = crate::control::version_mismatch(version);
                return HttpResponse::json(200, &ControlResponse::from(ControlReply::from(error)));
            }
            _ => return HttpResponse::code(400, "invalid-control-command"),
        },
    };
    if let Err(error) = crate::control::check_version(&request) {
        return HttpResponse::json(200, &ControlResponse::from(ControlReply::from(error)));
    }
    let Some(controls) = controls else {
        return HttpResponse::code(404, "control-unavailable");
    };
    let deadline = request.command.deadline();
    let (reply, response) = mpsc::sync_channel(1);
    match controls.try_send(ControlCall { request, reply }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => return HttpResponse::code(503, "control-busy"),
        Err(TrySendError::Disconnected(_)) => {
            return HttpResponse::code(503, "control-unavailable");
        }
    }
    match response.recv_timeout(deadline) {
        Ok(reply) => HttpResponse::json(200, &reply),
        Err(RecvTimeoutError::Timeout) => HttpResponse::code(504, "control-timeout"),
        Err(RecvTimeoutError::Disconnected) => HttpResponse::code(503, "control-unavailable"),
    }
}
