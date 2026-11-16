//! The wire side of one OpenCode generation: the request, its route fallback,
//! the startup retry, and the shared deadline. The parent module decides which
//! model, if any, a request names.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use super::Model;
use crate::desktop::opencode_service::Service;

/// Newest one-shot generation route first; older OpenCode V2 builds only serve the later name.
const GENERATE_ENDPOINTS: [&str; 2] = ["/api/experimental/generate", "/api/generate"];

#[derive(Serialize)]
pub(super) struct Request<'a> {
    pub(super) prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) model: Option<&'a Model>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Response {
    Success { data: ResponseData },
    Error { message: String },
}

#[derive(Deserialize)]
pub(super) struct ResponseData {
    pub(super) text: String,
}

pub(super) fn remaining_generation_deadline(
    started: Instant,
    deadline: Duration,
) -> Result<Duration> {
    let remaining = deadline.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        Err(eyre!(
            "OpenCode generation exceeded {} seconds",
            deadline.as_secs()
        ))
    } else {
        Ok(remaining)
    }
}

pub(super) fn generate_single_model(
    prompt: &str,
    model: Option<&Model>,
    started: Instant,
    deadline: Duration,
    cancelled: &AtomicBool,
    mut post: impl FnMut(&str, Duration, &AtomicBool) -> Result<Response>,
) -> Result<String> {
    let request = Request { prompt, model };
    let data = serde_json::to_string(&request)?;
    for attempt in 0..2 {
        let remaining = remaining_generation_deadline(started, deadline)?;
        match post(&data, remaining, cancelled)? {
            Response::Success { data } => return Ok(data.text),
            Response::Error { message } if attempt == 0 && retryable_generation_error(&message) => {
                tracing::warn!(%message, "OpenCode model unavailable during startup; retrying");
                wait_for_generation_retry(started, deadline, cancelled)?;
            }
            Response::Error { message } => {
                return Err(eyre!("OpenCode generation failed: {message}"));
            }
        }
    }
    unreachable!("generation loop returns on its second attempt")
}

/// Posts to the newest generation route and falls back to the older name only
/// when the server reports that route missing (a 404 that isn't OpenCode's
/// own error). Every attempt shares one deadline.
pub(super) fn post_generation(
    service: &Service,
    data: &str,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<Response> {
    let started = Instant::now();
    let mut response = None;
    for path in GENERATE_ENDPOINTS {
        let attempt = service.request(
            "POST",
            path,
            Some(data.as_bytes()),
            None,
            deadline.saturating_sub(started.elapsed()),
            cancelled,
        )?;
        let missing = route_missing(&attempt);
        response = Some(attempt);
        if !missing {
            break;
        }
    }
    let response = response.expect("at least one generation route");
    if route_missing(&response) {
        return Err(eyre!("OpenCode does not serve a generation route"));
    }
    serde_json::from_slice(&response.body)
        .wrap_err("OpenCode returned an invalid generation response")
}

/// A 404 means the route is missing only when it isn't OpenCode's own tagged
/// error (such as "Model not found"), which the route answered.
fn route_missing(response: &crate::desktop::opencode_service::Response) -> bool {
    response.status == 404
        && !serde_json::from_slice::<serde_json::Value>(&response.body).is_ok_and(|value| {
            value.get("_tag").is_some()
                && value
                    .get("message")
                    .is_some_and(serde_json::Value::is_string)
        })
}

pub(super) fn retryable_generation_error(message: &str) -> bool {
    message.starts_with("Model unavailable:")
}

pub(super) fn wait_for_generation_retry(
    started: Instant,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<()> {
    let wait_started = Instant::now();
    while wait_started.elapsed() < Duration::from_millis(1_500) {
        if cancelled.load(Ordering::Acquire) {
            return Err(eyre!("OpenCode generation was cancelled"));
        }
        if started.elapsed() >= deadline {
            return Err(eyre!(
                "OpenCode generation exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
