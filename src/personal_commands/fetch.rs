//! The HTTP requests behind a script's `fetch`: one blocking ureq request on
//! its own thread, through `net` (TLS through the platform and its trusted
//! roots, and the system's proxy). Requests and responses are
//! whole bodies, never streams. Logs name the host, never the path, query,
//! headers, or bodies.

use std::time::{Duration, Instant};

use serde::Deserialize;
use ureq::ResponseExt;

/// The most response body a script can read.
const MAX_RESPONSE_BYTES: u64 = 16 << 20;
const MAX_REDIRECTS: u32 = 20;

/// What `fetch` asks for, as `web.js` sends it.
#[derive(Debug, Deserialize)]
pub struct FetchRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

pub struct FetchResponse {
    pub status: u16,
    pub status_text: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl FetchRequest {
    /// The URL's host, after checking it is an absolute http or https URL.
    pub fn host(&self) -> Result<String, String> {
        let url = url::Url::parse(&self.url).map_err(|error| format!("invalid URL: {error}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(format!("unsupported URL scheme \"{}\"", url.scheme()));
        }
        url.host_str()
            .map(str::to_owned)
            .ok_or_else(|| "the URL has no host".to_string())
    }
}

/// Sends `request` and reads the whole response within `timeout`.
pub fn send(request: FetchRequest, timeout: Duration) -> Result<FetchResponse, String> {
    let host = request.host()?;
    let started = Instant::now();
    let request_bytes = request.body.as_ref().map_or(0, String::len);
    let method = request.method.clone();
    let result = exchange(request, timeout);
    let duration_ms = started.elapsed().as_millis() as u64;
    match &result {
        Ok(response) => tracing::info!(
            target: "hex_config",
            host,
            method,
            status = response.status,
            duration_ms,
            request_bytes,
            response_bytes = response.body.len(),
            "fetch"
        ),
        Err(error) => tracing::warn!(
            target: "hex_config",
            host,
            method,
            duration_ms,
            request_bytes,
            error,
            "fetch failed"
        ),
    }
    result
}

fn exchange(request: FetchRequest, timeout: Duration) -> Result<FetchResponse, String> {
    // The system's proxy and trusted roots, as model downloads use (`net`).
    let agent = crate::net::agent(&request.url, |config| {
        config
            .http_status_as_error(false)
            .max_redirects(MAX_REDIRECTS)
            .timeout_global(Some(timeout))
    });
    let mut builder = ureq::http::Request::builder()
        .method(request.method.as_str())
        .uri(request.url.as_str());
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let invalid = |error: ureq::http::Error| format!("invalid request: {error}");
    let response = match request.body {
        Some(body) => agent.run(builder.body(body.into_bytes()).map_err(invalid)?),
        None => agent.run(builder.body(()).map_err(invalid)?),
    };
    let mut response = response.map_err(|error| describe(&error))?;
    let status = response.status();
    let url = response.get_uri().to_string();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|error| describe(&error))?;
    Ok(FetchResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_owned(),
        url,
        headers,
        body,
    })
}

/// A request failure without the URL, which can carry dictated text.
fn describe(error: &ureq::Error) -> String {
    match error {
        ureq::Error::BadUri(_) => "invalid URL".into(),
        ureq::Error::Http(_) => "invalid request".into(),
        ureq::Error::Timeout(_) => "timed out".into(),
        ureq::Error::BodyExceedsLimit(_) => {
            format!("the response is larger than {MAX_RESPONSE_BYTES} bytes")
        }
        error => crate::net::reason(error),
    }
}
