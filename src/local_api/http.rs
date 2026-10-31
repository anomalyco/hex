use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{HEADER_DEADLINE, MAX_HEADER_BYTES, MAX_HEADERS, MAX_PATH_BYTES};

pub(super) struct HttpRequest {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) authorization: Option<String>,
    pub(super) dictation_token: Option<String>,
    pub(super) stream_token: Option<String>,
    pub(super) content_type: Option<String>,
    pub(super) content_length: Option<usize>,
    pub(super) body_prefix: Vec<u8>,
}

pub(super) struct HttpResponse {
    pub(super) status: u16,
    pub(super) reason: &'static str,
    pub(super) content_type: Option<&'static str>,
    pub(super) body: Vec<u8>,
}

pub(super) fn body_read_response(error: BodyReadError) -> HttpResponse {
    match error {
        BodyReadError::ResourceExhausted => HttpResponse::code(413, "resource-exhausted"),
        BodyReadError::Invalid => HttpResponse::code(400, "invalid-body"),
        BodyReadError::Deadline => HttpResponse::code(408, "request-timeout"),
        BodyReadError::Shutdown => HttpResponse::code(503, "shutting-down"),
        BodyReadError::Io(error) => {
            tracing::debug!(%error, "could not read request body");
            HttpResponse::code(400, "invalid-body")
        }
    }
}

pub(super) enum BodyReadError {
    ResourceExhausted,
    Invalid,
    Deadline,
    Shutdown,
    Io(std::io::Error),
}

pub(super) fn read_body_bounded(
    stream: &mut TcpStream,
    content_length: usize,
    body: &mut Vec<u8>,
    shutdown: &AtomicBool,
    maximum: usize,
    timeout: Duration,
) -> std::result::Result<(), BodyReadError> {
    if content_length > maximum {
        return Err(BodyReadError::ResourceExhausted);
    }
    if body.len() > content_length {
        return Err(BodyReadError::Invalid);
    }
    body.try_reserve_exact(content_length - body.len())
        .map_err(|_| BodyReadError::ResourceExhausted)?;
    let deadline = Instant::now() + timeout;
    let mut buffer = [0_u8; 16 * 1024];
    while body.len() < content_length {
        if shutdown.load(Ordering::Acquire) {
            return Err(BodyReadError::Shutdown);
        }
        if Instant::now() >= deadline {
            return Err(BodyReadError::Deadline);
        }
        let remaining = content_length - body.len();
        let read_length = remaining.min(buffer.len());
        let read = match stream.read(&mut buffer[..read_length]) {
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(BodyReadError::Io(error)),
        };
        if Instant::now() >= deadline {
            return Err(BodyReadError::Deadline);
        }
        if read == 0 {
            return Err(BodyReadError::Invalid);
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Ok(())
}

pub(super) fn write_sse_headers(stream: &mut TcpStream) -> std::io::Result<()> {
    write_stream_headers(stream, "text/event-stream")
}

pub(super) fn write_stream_headers(
    stream: &mut TcpStream,
    content_type: &str,
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n"
    )?;
    stream.flush()
}

pub(super) fn write_sse(stream: &mut TcpStream, value: &impl Serialize) -> std::io::Result<()> {
    stream.write_all(b"data: ")?;
    serde_json::to_writer(&mut *stream, value)?;
    stream.write_all(b"\n\n")?;
    stream.flush()
}

impl HttpResponse {
    pub(super) fn empty(status: u16) -> Self {
        Self {
            status,
            reason: reason(status),
            content_type: None,
            body: Vec::new(),
        }
    }

    pub(super) fn json<T: Serialize>(status: u16, value: &T) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) => Self {
                status,
                reason: reason(status),
                content_type: Some("application/json"),
                body,
            },
            Err(error) => {
                tracing::error!(%error, "could not serialize local API response");
                Self::empty(500)
            }
        }
    }

    pub(super) fn code(status: u16, code: &str) -> Self {
        Self::json(status, &serde_json::json!({ "code": code }))
    }
}

pub(super) fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        408 => "Request Timeout",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => unreachable!("local API responses use a fixed status set"),
    }
}

impl HttpRequest {
    pub(super) fn has_content_type(&self, mime: &str) -> bool {
        self.content_type
            .as_deref()
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case(mime))
    }
}

#[derive(Debug)]
pub(super) enum RequestReadError {
    TooLarge,
    Invalid,
    Deadline,
    Shutdown,
    Io(std::io::Error),
}

pub(super) fn read_request(
    stream: &mut TcpStream,
    shutdown: &AtomicBool,
) -> std::result::Result<HttpRequest, RequestReadError> {
    read_request_until(stream, shutdown, Instant::now() + HEADER_DEADLINE)
}

pub(super) fn read_request_until(
    stream: &mut TcpStream,
    shutdown: &AtomicBool,
    deadline: Instant,
) -> std::result::Result<HttpRequest, RequestReadError> {
    let mut data = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        if shutdown.load(Ordering::Acquire) {
            return Err(RequestReadError::Shutdown);
        }
        if Instant::now() >= deadline {
            return Err(RequestReadError::Deadline);
        }
        let read = match stream.read(&mut buffer) {
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(RequestReadError::Io(error)),
        };
        if Instant::now() >= deadline {
            return Err(RequestReadError::Deadline);
        }
        if read == 0 {
            return Err(RequestReadError::Invalid);
        }
        if data.len() + read > MAX_HEADER_BYTES {
            return Err(RequestReadError::TooLarge);
        }
        data.extend_from_slice(&buffer[..read]);
        if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
    };
    let header = std::str::from_utf8(&data[..header_end]).map_err(|_| RequestReadError::Invalid)?;
    let mut lines = header.split("\r\n");
    let mut request_line = lines
        .next()
        .ok_or(RequestReadError::Invalid)?
        .split_ascii_whitespace();
    let method = request_line.next().ok_or(RequestReadError::Invalid)?;
    let path = request_line.next().ok_or(RequestReadError::Invalid)?;
    let version = request_line.next().ok_or(RequestReadError::Invalid)?;
    if request_line.next().is_some()
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || !path.starts_with('/')
        || path.len() > MAX_PATH_BYTES
    {
        return Err(RequestReadError::Invalid);
    }
    let mut authorization = None;
    let mut dictation_token = None;
    let mut stream_token = None;
    let mut content_type = None;
    let mut content_length = None;
    for (index, line) in lines.enumerate() {
        if index >= MAX_HEADERS {
            return Err(RequestReadError::TooLarge);
        }
        let (name, value) = line.split_once(':').ok_or(RequestReadError::Invalid)?;
        if name.eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("x-hex-dictation-token") {
            if dictation_token.is_some() {
                return Err(RequestReadError::Invalid);
            }
            dictation_token = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("x-hex-stream-token") {
            if stream_token.is_some() {
                return Err(RequestReadError::Invalid);
            }
            stream_token = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("content-type") {
            content_type = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(RequestReadError::Invalid);
            }
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| RequestReadError::Invalid)?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(RequestReadError::Invalid);
        }
    }
    let body_prefix = data[header_end + 4..].to_vec();
    if content_length.is_some_and(|length| body_prefix.len() > length) {
        return Err(RequestReadError::Invalid);
    }
    Ok(HttpRequest {
        method: method.into(),
        path: path.into(),
        authorization,
        dictation_token,
        stream_token,
        content_type,
        content_length,
        body_prefix,
    })
}

pub(super) fn write_response(
    stream: &mut TcpStream,
    response: HttpResponse,
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n",
        response.status,
        response.reason,
        response.body.len()
    )?;
    if let Some(content_type) = response.content_type {
        write!(stream, "Content-Type: {content_type}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(&response.body)?;
    stream.flush()
}
