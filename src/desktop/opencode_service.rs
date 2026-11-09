//! The running OpenCode service, reached without starting anything: its
//! owner-only registration names a loopback URL and password, `/api/info`
//! confirms the registered process still answers, and requests are plain
//! authenticated HTTP over `TcpStream`. Hex never runs an OpenCode
//! executable; when no service answers, OpenCode isn't running, and when
//! one answers `/api/info` with 404 it predates the current OpenCode 2.
//!
//! On Windows, Labs › OpenCode in WSL also reads the default WSL
//! distribution's registration when no Windows one answers; that service
//! gets Hex's workspace as a Linux path.

mod wsl;

use self::wsl::WslHome;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::Deserialize;

/// What features say when no service answers.
const NOT_RUNNING: &str = "OpenCode isn't running";
/// What features say when a registered service has no `/api/info`.
const OUTDATED: &str = "Update OpenCode 2 from https://opencode.ai/v2/install";
const MAX_REGISTRATION_BYTES: u64 = 64 * 1024;
const MAX_REGISTRATIONS: usize = 16;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const INFO_DEADLINE: Duration = Duration::from_secs(2);
const CONNECT_DEADLINE: Duration = Duration::from_secs(2);
/// How often a stalled read or write checks cancellation and the deadline.
const WAIT_SLICE: Duration = Duration::from_millis(50);
const WRITE_CHUNK: usize = 64 * 1024;

/// One running service: where it listens and how to authenticate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Service {
    address: SocketAddr,
    password: Option<String>,
    /// Set when the registration lives in WSL's filesystem (Windows).
    wsl: Option<WslHome>,
}

#[derive(Debug)]
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
}

/// Why no service was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Missing {
    NotRunning,
    /// A registered service answered `/api/info` with 404: an obsolete
    /// OpenCode build. The current OpenCode 2 serves it.
    Outdated,
}

impl Missing {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Missing::NotRunning => NOT_RUNNING,
            Missing::Outdated => OUTDATED,
        }
    }
}

/// The running service, or why there is none. Reads only registration files
/// and asks each registered service for `/api/info`. On Windows it also
/// looks in WSL while Labs › OpenCode in WSL is on.
pub(crate) fn find() -> Result<Service, Missing> {
    let found = match state_dir() {
        Some(state) => find_in(&state),
        None => Err(Missing::NotRunning),
    };
    with_wsl(found, None, &AtomicBool::new(false))
}

/// [`find`] within the caller's own budget: every `/api/info` confirmation
/// shares `deadline` (still capped by [`INFO_DEADLINE`]) and stops when
/// `cancelled` is set, so a rewrite never waits past its deadline here. The
/// WSL fallback gets only what the Windows search has left.
pub(crate) fn find_cancellable(
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<Service, Missing> {
    let started = Instant::now();
    let found = match state_dir() {
        Some(state) => find_in_cancellable(&state, deadline, cancelled),
        None => Err(Missing::NotRunning),
    };
    with_wsl(
        found,
        Some(deadline.saturating_sub(started.elapsed())),
        cancelled,
    )
}

/// The Windows result, or a WSL service when Labs › OpenCode in WSL is on
/// and no Windows registration confirmed one. A confirmed service wins; with
/// neither, "update OpenCode" is the more useful answer.
fn with_wsl(
    found: Result<Service, Missing>,
    budget: Option<Duration>,
    cancelled: &AtomicBool,
) -> Result<Service, Missing> {
    #[cfg(target_os = "windows")]
    if crate::labs::opencode_in_wsl() {
        if found.is_err() {
            return prefer_wsl(found, wsl::discover(budget, cancelled));
        }
    } else {
        wsl::clear_cache();
    }
    let _ = (budget, cancelled);
    found
}

/// Whether `found` or the WSL search names the service to use: a confirmed
/// service wins, and with neither, "update OpenCode" is more useful than
/// "isn't running".
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn prefer_wsl(
    found: Result<Service, Missing>,
    wsl: Result<Service, Missing>,
) -> Result<Service, Missing> {
    match (found, wsl) {
        (Ok(service), _) | (_, Ok(service)) => Ok(service),
        (Err(Missing::Outdated), _) | (_, Err(Missing::Outdated)) => Err(Missing::Outdated),
        (Err(Missing::NotRunning), Err(Missing::NotRunning)) => Err(Missing::NotRunning),
    }
}

/// Whether a service answers now; a short loopback check for availability.
pub(crate) fn running() -> bool {
    find().is_ok()
}

/// OpenCode's state directory on every OS: `$XDG_STATE_HOME` when set, else
/// `~/.local/state`, then `opencode` (Windows included).
fn state_dir() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::app_paths::home_dir().map(|home| home.join(".local").join("state")))?;
    Some(root.join("opencode"))
}

/// `service.json` (the stable, beta and dev channels) first, then other
/// channels' `service-<channel>.json`; the first that answers wins.
fn find_in(state: &Path) -> Result<Service, Missing> {
    discover(state, None, &AtomicBool::new(false), None)
}

/// [`find_in`] bounded by the caller's remaining time: `Some` stops the whole
/// search once it is spent, so a stalled service can't outlive the rewrite.
fn find_in_cancellable(
    state: &Path,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<Service, Missing> {
    discover(state, Some(deadline), cancelled, None)
}

/// `budget` is the caller's whole remaining time, or `None` for one
/// [`INFO_DEADLINE`] per registration (the Settings and availability paths).
/// `wsl` marks every confirmed service as one whose files live in WSL.
fn discover(
    state: &Path,
    budget: Option<Duration>,
    cancelled: &AtomicBool,
    wsl: Option<&WslHome>,
) -> Result<Service, Missing> {
    let started = Instant::now();
    let mut others: Vec<PathBuf> = fs::read_dir(state)
        .map_err(|_| Missing::NotRunning)?
        .flatten()
        .take(512)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("service-") && name.ends_with(".json"))
        })
        .collect();
    others.sort();
    let mut missing = Missing::NotRunning;
    for registration in std::iter::once(state.join("service.json"))
        .chain(others)
        .take(MAX_REGISTRATIONS)
        .filter_map(|path| read_registration(&path))
    {
        if cancelled.load(Ordering::Acquire) {
            return Err(Missing::NotRunning);
        }
        let deadline = match budget {
            Some(budget) => {
                let remaining = budget.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    return Err(Missing::NotRunning);
                }
                remaining.min(INFO_DEADLINE)
            }
            None => INFO_DEADLINE,
        };
        match confirm(&registration, deadline, cancelled, wsl) {
            Ok(service) => return Ok(service),
            Err(Missing::Outdated) => missing = Missing::Outdated,
            Err(Missing::NotRunning) => {}
        }
    }
    Err(missing)
}

#[derive(Deserialize)]
struct Registration {
    url: String,
    pid: u32,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

fn read_registration(path: &Path) -> Option<Registration> {
    let mut file = open_private(path)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_REGISTRATION_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_REGISTRATION_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// A regular registration file this user owns that no one else can read.
#[cfg(unix)]
fn open_private(path: &Path) -> Option<fs::File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    (metadata.is_file()
        && metadata.uid() == unsafe { libc::geteuid() }
        && metadata.permissions().mode() & 0o077 == 0)
        .then_some(file)
}

/// The profile folder's ACL keeps it to this user on Windows.
#[cfg(windows)]
fn open_private(path: &Path) -> Option<fs::File> {
    let file = fs::File::open(path).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// The service a registration names, when it is on loopback and the process
/// answering `/api/info` is the registered one.
fn confirm(
    registration: &Registration,
    deadline: Duration,
    cancelled: &AtomicBool,
    wsl: Option<&WslHome>,
) -> Result<Service, Missing> {
    let service = Service {
        address: loopback_address(&registration.url).ok_or(Missing::NotRunning)?,
        password: registration
            .password
            .clone()
            .filter(|password| !password.is_empty()),
        wsl: wsl.cloned(),
    };
    #[derive(Deserialize)]
    struct Info {
        pid: u32,
        version: String,
    }
    let response = service
        .request("GET", "/api/info", None, None, deadline, cancelled)
        .map_err(|_| Missing::NotRunning)?;
    if response.status == 404 {
        tracing::warn!(
            version = registration.version.as_deref().unwrap_or("unknown"),
            "the registered OpenCode service has no /api/info; {OUTDATED}"
        );
        return Err(Missing::Outdated);
    }
    if response.status != 200 {
        return Err(Missing::NotRunning);
    }
    let info: Info = serde_json::from_slice(&response.body).map_err(|_| Missing::NotRunning)?;
    (info.pid == registration.pid
        && registration
            .version
            .as_ref()
            .is_none_or(|version| *version == info.version))
    .then_some(service)
    .ok_or(Missing::NotRunning)
}

/// A registered `http://` URL's loopback socket; an unspecified address
/// (`0.0.0.0`, `::`) is reached through loopback. Anything else is refused.
fn loopback_address(url: &str) -> Option<SocketAddr> {
    let url = url::Url::parse(url).ok()?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let ip = match url.host()? {
        url::Host::Domain("localhost") => IpAddr::V4(Ipv4Addr::LOCALHOST),
        url::Host::Ipv4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        url::Host::Ipv6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        url::Host::Ipv4(ip) if ip.is_loopback() => IpAddr::V4(ip),
        url::Host::Ipv6(ip) if ip.is_loopback() => IpAddr::V6(ip),
        _ => return None,
    };
    let port = url.port_or_known_default().filter(|port| *port != 0)?;
    Some(SocketAddr::new(ip, port))
}

impl Service {
    /// Whether this service's registration and files live in the default WSL
    /// distribution.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn is_wsl(&self) -> bool {
        self.wsl.is_some()
    }

    /// `host` as this service's WSL server sees it, or `None` for a Windows
    /// service and for a path the distribution can't reach.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn wsl_directory(&self, host: &Path) -> Option<String> {
        self.wsl.as_ref()?.directory(host)
    }

    /// Hex's OpenCode workspace as this service sees it: a Linux path inside
    /// WSL, else the host path.
    fn workspace(&self, directory: &Path) -> Result<String> {
        match &self.wsl {
            Some(wsl) => wsl.workspace(),
            None => host_workspace(directory),
        }
    }

    /// One request. `directory` scopes it to an OpenCode workspace (created
    /// when first used). HTTP error statuses return normally; transport
    /// failures, cancellation and the deadline are errors. A scoped request
    /// to a WSL service that fails makes the next one create the workspace
    /// again.
    pub(crate) fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        directory: Option<&Path>,
        deadline: Duration,
        cancelled: &AtomicBool,
    ) -> Result<Response> {
        let scoped = directory.is_some();
        let result = self.request_inner(method, path, body, directory, deadline, cancelled);
        if scoped
            && let Some(wsl) = &self.wsl
            && match &result {
                Ok(response) => response.status == 404,
                Err(_) => true,
            }
        {
            wsl.forget_workspace();
        }
        result
    }

    fn request_inner(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        directory: Option<&Path>,
        deadline: Duration,
        cancelled: &AtomicBool,
    ) -> Result<Response> {
        let started = Instant::now();
        let remaining = || deadline.saturating_sub(started.elapsed());
        let check = |remaining: Duration| -> Result<()> {
            if cancelled.load(Ordering::Acquire) {
                return Err(eyre!("OpenCode {path} was cancelled"));
            }
            if remaining.is_zero() {
                return Err(eyre!(
                    "OpenCode {path} exceeded {} seconds",
                    deadline.as_secs()
                ));
            }
            Ok(())
        };
        check(remaining())?;
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n",
            self.address
        );
        if let Some(password) = &self.password {
            if password.contains(['\r', '\n']) {
                return Err(eyre!("OpenCode registration has an invalid password"));
            }
            head.push_str(&format!(
                "Authorization: Basic {}\r\n",
                base64(format!("opencode:{password}").as_bytes())
            ));
        }
        if let Some(directory) = directory {
            head.push_str(&format!(
                "x-opencode-directory: {}\r\n",
                encode_uri_component(&self.workspace(directory)?)
            ));
        }
        if let Some(body) = body {
            head.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ));
        } else if method != "GET" {
            head.push_str("Content-Length: 0\r\n");
        }
        head.push_str("\r\n");
        let mut stream =
            TcpStream::connect_timeout(&self.address, remaining().min(CONNECT_DEADLINE))
                .map_err(|_| eyre!(NOT_RUNNING))?;
        // Non-blocking, so a service that stops reading or answering can't hold
        // the request past cancellation or the deadline. Returning drops the
        // stream, which closes the socket and discards anything unsent.
        stream.set_nonblocking(true)?;
        let mut step = |interest: Interest,
                        op: &mut dyn FnMut(&mut TcpStream) -> io::Result<usize>|
         -> Result<io::Result<usize>> {
            loop {
                let left = remaining();
                check(left)?;
                match op(&mut stream) {
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if wait(&stream, interest, left.min(WAIT_SLICE))
                            .wrap_err_with(|| format!("could not reach OpenCode {path}"))?
                        {
                            return Ok(Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "OpenCode stopped reading the request",
                            )));
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    result => return Ok(result),
                }
            }
        };
        // Why the request stopped short, when OpenCode answered or hung up
        // before reading all of it. An early answer (a 404 or 413, say) still
        // returns normally.
        let mut unsent = None;
        'send: for mut pending in [head.as_bytes(), body.unwrap_or_default()] {
            while !pending.is_empty() {
                let chunk = &pending[..pending.len().min(WRITE_CHUNK)];
                match step(Interest::Write, &mut |stream| stream.write(chunk))? {
                    Ok(0) => {
                        unsent = Some(io::Error::from(io::ErrorKind::WriteZero));
                        break 'send;
                    }
                    Ok(written) => pending = &pending[written..],
                    Err(error) => {
                        unsent = Some(error);
                        break 'send;
                    }
                }
            }
        }
        let not_sent = |error: io::Error| -> Result<Response> {
            Err(error).wrap_err_with(|| format!("could not send OpenCode {path}"))
        };
        let mut bytes = Vec::new();
        let mut buffer = [0; 64 * 1024];
        // A service that answers early and closes with the body unread resets
        // the connection after its answer, so the answer can end in an error
        // instead of a clean close.
        let mut closed = true;
        loop {
            match step(Interest::Read, &mut |stream| stream.read(&mut buffer))? {
                Ok(0) => break,
                Ok(read) => {
                    bytes.extend_from_slice(&buffer[..read]);
                    if bytes.len() > MAX_RESPONSE_BYTES {
                        return Err(eyre!("OpenCode {path} answered more than 16 MiB"));
                    }
                }
                Err(_) if unsent.is_some() => {
                    closed = false;
                    break;
                }
                Err(error) => {
                    return Err(error).wrap_err_with(|| format!("could not read OpenCode {path}"));
                }
            }
        }
        match (parse_response(bytes, closed), unsent) {
            (Some(response), _) => Ok(response),
            (None, Some(unsent)) => not_sent(unsent),
            (None, None) => Err(eyre!("OpenCode {path} returned invalid HTTP")),
        }
    }
}

#[derive(Clone, Copy)]
enum Interest {
    Read,
    Write,
}

/// Waits up to `timeout` for the socket to become ready. It may return early
/// or spuriously; the caller retries the operation and rechecks cancellation
/// and the deadline. While writing it also watches for an answer or a hangup:
/// `true` means the peer can't take more of the request, so writing stops
/// instead of spinning on a socket that will never become writable.
#[cfg(unix)]
fn wait(stream: &TcpStream, interest: Interest, timeout: Duration) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    let mut fd = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: match interest {
            Interest::Read => libc::POLLIN,
            Interest::Write => libc::POLLOUT | libc::POLLIN,
        },
        revents: 0,
    };
    // SAFETY: one valid pollfd for a socket this call borrows.
    if unsafe { libc::poll(&mut fd, 1, poll_millis(timeout)) } < 0 {
        let error = io::Error::last_os_error();
        return match error.kind() {
            io::ErrorKind::Interrupted => Ok(false),
            _ => Err(error),
        };
    }
    readiness(
        interest,
        fd.revents,
        [
            libc::POLLOUT,
            libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            libc::POLLNVAL,
        ],
    )
}

#[cfg(windows)]
fn wait(stream: &TcpStream, interest: Interest, timeout: Duration) -> io::Result<bool> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        POLLERR, POLLHUP, POLLNVAL, POLLRDNORM, POLLWRNORM, SOCKET_ERROR, WSAGetLastError,
        WSAPOLLFD, WSAPoll,
    };
    let mut fd = WSAPOLLFD {
        fd: stream.as_raw_socket() as usize,
        events: match interest {
            Interest::Read => POLLRDNORM,
            Interest::Write => POLLWRNORM | POLLRDNORM,
        },
        revents: 0,
    };
    // SAFETY: one valid WSAPOLLFD for a socket this call borrows.
    if unsafe { WSAPoll(&mut fd, 1, poll_millis(timeout)) } == SOCKET_ERROR {
        let error = io::Error::from_raw_os_error(unsafe { WSAGetLastError() });
        return match error.kind() {
            io::ErrorKind::Interrupted => Ok(false),
            _ => Err(error),
        };
    }
    readiness(
        interest,
        fd.revents,
        [POLLWRNORM, POLLRDNORM | POLLHUP | POLLERR, POLLNVAL],
    )
}

/// `[writable, answered or hung up, invalid]` are the platform's poll flags.
fn readiness(
    interest: Interest,
    revents: i16,
    [writable, stopped, invalid]: [i16; 3],
) -> io::Result<bool> {
    if revents & invalid != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the socket is no longer open",
        ));
    }
    Ok(matches!(interest, Interest::Write) && revents & writable == 0 && revents & stopped != 0)
}

/// At least one millisecond, so a short remainder still waits instead of spinning.
fn poll_millis(timeout: Duration) -> i32 {
    timeout.as_millis().clamp(1, i32::MAX as u128) as i32
}

/// Creates Hex's OpenCode workspace on first use and returns the path
/// OpenCode should see.
fn host_workspace(directory: &Path) -> Result<String> {
    fs::create_dir_all(directory).wrap_err_with(|| {
        format!(
            "could not create OpenCode workspace {}",
            directory.display()
        )
    })?;
    let directory = directory.canonicalize().wrap_err_with(|| {
        format!(
            "could not resolve OpenCode workspace {}",
            directory.display()
        )
    })?;
    let directory = directory
        .to_str()
        .ok_or_else(|| eyre!("OpenCode workspace path is not valid UTF-8"))?;
    // canonicalize returns a verbatim path on Windows; OpenCode wants the drive form.
    Ok(directory
        .strip_prefix(r"\\?\")
        .unwrap_or(directory)
        .to_owned())
}

/// `closed` says the service closed cleanly, so a body without a length or
/// chunked framing ends there; otherwise such a body may be cut short and is
/// refused.
fn parse_response(bytes: Vec<u8>, closed: bool) -> Option<Response> {
    let split = bytes.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&bytes[..split]).ok()?;
    let mut lines = head.split("\r\n");
    let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let (key, value) = line.split_once(':')?;
        let value = value.trim();
        if key.eq_ignore_ascii_case("content-length") {
            length = Some(value.parse::<usize>().ok()?);
        } else if key.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.eq_ignore_ascii_case("chunked");
        }
    }
    let body = &bytes[split + 4..];
    let body = if chunked {
        dechunk(body)?
    } else if let Some(length) = length {
        body.get(..length)?.to_vec()
    } else if closed {
        body.to_vec()
    } else {
        return None;
    };
    Some(Response { status, body })
}

fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line = body.windows(2).position(|window| window == b"\r\n")?;
        let size = std::str::from_utf8(&body[..line]).ok()?;
        let size = size.split(';').next()?.trim();
        let size = usize::from_str_radix(size, 16).ok()?;
        body = &body[line + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(body.get(..size)?);
        body = body.get(size..)?.strip_prefix(b"\r\n")?;
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from_be_bytes([
            0,
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ]);
        let ch = |shift: u32| char::from(TABLE[((n >> shift) & 63) as usize]);
        out.extend([
            ch(18),
            ch(12),
            if chunk.len() > 1 { ch(6) } else { '=' },
            if chunk.len() > 2 { ch(0) } else { '=' },
        ]);
    }
    out
}

fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            encoded.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;

    /// The headers and body of each request a fixture received.
    pub(crate) type Received = thread::JoinHandle<Vec<(String, Vec<u8>)>>;

    /// A service fixture answering each `(request line, status, body)` in
    /// order; returns its URL and what it received.
    pub(crate) fn serve(routes: Vec<(String, u16, String)>) -> (String, Received) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            routes
                .into_iter()
                .map(|(start, status, response)| {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut reader = BufReader::new(&mut stream);
                    let mut headers = String::new();
                    loop {
                        let mut line = String::new();
                        assert!(reader.read_line(&mut line).unwrap() > 0);
                        if line == "\r\n" {
                            break;
                        }
                        headers.push_str(&line);
                    }
                    assert!(
                        headers.starts_with(&format!("{start} HTTP/1.1\r\n")),
                        "{headers}"
                    );
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    write!(
                        stream,
                        "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                        response.len()
                    )
                    .unwrap();
                    (headers, body)
                })
                .collect()
        });
        (url, server)
    }

    pub(crate) fn fixture(url: &str) -> Service {
        Service {
            address: loopback_address(url).unwrap(),
            password: Some("fixture-password".into()),
            wsl: None,
        }
    }

    fn state_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hex-opencode-service-{label}-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn register(state: &Path, name: &str, url: &str, pid: u32) {
        let path = state.join(name);
        fs::write(
            &path,
            serde_json::json!({
                "id": "fixture", "version": "2.0.0", "url": url, "pid": pid,
                "password": "fixture-password"
            })
            .to_string(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn a_registered_service_is_confirmed_by_its_pid() {
        let state = state_root("confirm");
        let info = r#"{"pid":42,"version":"2.0.0"}"#.to_owned();
        let (url, server) = serve(vec![("GET /api/info".into(), 200, info)]);
        register(&state, "service.json", &url, 42);
        let service = find_in(&state).unwrap();
        let (headers, _) = server.join().unwrap().remove(0);
        assert!(headers.contains("Authorization: Basic b3BlbmNvZGU6Zml4dHVyZS1wYXNzd29yZA==\r\n"));
        assert_eq!(service, fixture(&url));
        fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn a_stale_registration_means_opencode_isnt_running() {
        let state = state_root("stale");
        assert_eq!(find_in(&state), Err(Missing::NotRunning));
        let info = r#"{"pid":7,"version":"2.0.0"}"#.to_owned();
        let (url, server) = serve(vec![("GET /api/info".into(), 200, info)]);
        register(&state, "service.json", &url, 42);
        assert_eq!(find_in(&state), Err(Missing::NotRunning));
        server.join().unwrap();
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", closed.local_addr().unwrap());
        drop(closed);
        register(&state, "service.json", &url, 42);
        assert_eq!(find_in(&state), Err(Missing::NotRunning));
        fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn a_service_search_is_bounded_by_the_callers_deadline() {
        let state = state_root("search-deadline");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        register(&state, "service.json", &url, 42);
        let started = Instant::now();
        assert_eq!(
            find_in_cancellable(&state, Duration::from_millis(100), &AtomicBool::new(false)),
            Err(Missing::NotRunning)
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn a_service_search_stops_when_the_caller_cancels() {
        let state = state_root("search-cancel");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        register(&state, "service.json", &url, 42);
        let cancelled = AtomicBool::new(false);
        let started = Instant::now();
        thread::scope(|scope| {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(100));
                cancelled.store(true, Ordering::Release);
            });
            assert_eq!(
                find_in_cancellable(&state, Duration::from_secs(30), &cancelled),
                Err(Missing::NotRunning)
            );
        });
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn a_service_without_api_info_needs_an_opencode_update() {
        let state = state_root("outdated");
        let (url, server) = serve(vec![("GET /api/info".into(), 404, "Not Found".into())]);
        register(&state, "service.json", &url, 42);
        assert_eq!(find_in(&state), Err(Missing::Outdated));
        server.join().unwrap();
        fs::remove_dir_all(state).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_registration_others_can_read_is_ignored() {
        use std::os::unix::fs::PermissionsExt;
        let state = state_root("private");
        register(&state, "service.json", "http://127.0.0.1:9/", 42);
        let path = state.join("service.json");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_registration(&path).is_none());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_registration(&path).is_some());
        fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn only_loopback_http_endpoints_are_used() {
        assert_eq!(
            loopback_address("http://0.0.0.0:4096"),
            Some("127.0.0.1:4096".parse().unwrap())
        );
        assert_eq!(
            loopback_address("http://localhost:4096/"),
            Some("127.0.0.1:4096".parse().unwrap())
        );
        assert_eq!(
            loopback_address("http://[::]:4096"),
            Some("[::1]:4096".parse().unwrap())
        );
        for refused in [
            "https://127.0.0.1:4096",
            "http://192.168.1.2:4096",
            "http://example.com:4096",
            "http://user:pass@127.0.0.1:4096",
            "http://127.0.0.1:4096/prefix",
            "http://127.0.0.1:0",
        ] {
            assert_eq!(loopback_address(refused), None, "{refused}");
        }
    }

    #[test]
    fn requests_carry_the_workspace_and_decode_chunked_answers() {
        let root = state_root("workspace").join("scope %2F caf\u{00e9}");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                headers.push_str(&line);
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
                )
                .unwrap();
            headers
        });
        let response = fixture(&url)
            .request(
                "GET",
                "/api/model",
                None,
                Some(&root),
                Duration::from_secs(5),
                &AtomicBool::new(false),
            )
            .unwrap();
        // The drive form on Windows, without canonicalize's verbatim prefix.
        let workspace = host_workspace(&root).unwrap();
        assert!(!workspace.starts_with(r"\\?\"));
        let headers = server.join().unwrap();
        assert!(headers.contains(&format!(
            "x-opencode-directory: {}\r\n",
            encode_uri_component(&workspace)
        )));
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"hello world");
        fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_wsl_service_maps_a_capture_folder_into_its_distribution() {
        let root = state_root("wsl-directory");
        let service = Service {
            wsl: Some(WslHome::for_test(root.join("home"), "/home/dax")),
            ..fixture("http://127.0.0.1:1/")
        };
        assert!(service.is_wsl());
        assert_eq!(
            service.wsl_directory(Path::new(r"C:\Users\dax\captures\a.scrawl")),
            Some("/mnt/c/Users/dax/captures/a.scrawl".into())
        );
        assert_eq!(service.wsl_directory(Path::new(r"\\server\share\a")), None);
        let windows = fixture("http://127.0.0.1:1/");
        assert!(!windows.is_wsl());
        assert_eq!(windows.wsl_directory(Path::new(r"C:\a")), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_wsl_service_gets_the_workspace_its_linux_server_can_use() {
        let root = state_root("wsl-workspace");
        let (url, server) = serve(vec![("GET /api/model".into(), 200, "{}".into())]);
        let service = Service {
            wsl: Some(WslHome::for_test(root.join("home"), "/home/dax")),
            ..fixture(&url)
        };
        let windows = root.join("windows");
        let response = service
            .request(
                "GET",
                "/api/model",
                None,
                Some(&windows),
                Duration::from_secs(5),
                &AtomicBool::new(false),
            )
            .unwrap();
        let (headers, _) = server.join().unwrap().remove(0);
        let linux = "/home/dax/.local/share/hex/opencode";
        assert!(headers.contains(&format!(
            "x-opencode-directory: {}\r\n",
            encode_uri_component(linux)
        )));
        assert!(root.join("home/.local/share/hex/opencode").is_dir());
        assert!(!windows.exists(), "the Windows workspace stays untouched");
        assert_eq!(response.status, 200);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failed_wsl_request_forgets_the_workspace() {
        let root = state_root("wsl-retry-error");
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", closed.local_addr().unwrap());
        drop(closed);
        let service = Service {
            wsl: Some(WslHome::for_test(root.join("home"), "/home/dax")),
            ..fixture(&url)
        };
        let windows = root.join("windows");
        let request = || {
            service.request(
                "GET",
                "/api/model",
                None,
                Some(&windows),
                Duration::from_millis(500),
                &AtomicBool::new(false),
            )
        };
        assert!(request().is_err());
        let workspace = root.join("home/.local/share/hex/opencode");
        assert!(workspace.is_dir());
        fs::remove_dir_all(&workspace).unwrap();
        assert!(request().is_err());
        assert!(
            workspace.is_dir(),
            "a failed request creates the workspace again"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_not_found_wsl_request_forgets_the_workspace() {
        let root = state_root("wsl-retry-404");
        let (url, server) = serve(vec![
            ("GET /api/model".into(), 404, "{}".into()),
            ("GET /api/model".into(), 404, "{}".into()),
        ]);
        let service = Service {
            wsl: Some(WslHome::for_test(root.join("home"), "/home/dax")),
            ..fixture(&url)
        };
        let windows = root.join("windows");
        let request = || {
            service
                .request(
                    "GET",
                    "/api/model",
                    None,
                    Some(&windows),
                    Duration::from_secs(5),
                    &AtomicBool::new(false),
                )
                .unwrap()
                .status
        };
        assert_eq!(request(), 404);
        let workspace = root.join("home/.local/share/hex/opencode");
        assert!(workspace.is_dir());
        fs::remove_dir_all(&workspace).unwrap();
        assert_eq!(request(), 404);
        assert!(workspace.is_dir(), "a 404 creates the workspace again");
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_wsl_service_is_a_fallback_only_without_a_windows_one() {
        let service = fixture("http://127.0.0.1:1/");
        let wsl = || Service {
            wsl: Some(WslHome::for_test(
                PathBuf::from(r"\\wsl.localhost\Ubuntu\home\dax"),
                "/home/dax",
            )),
            ..service.clone()
        };
        assert_ne!(wsl(), service, "a WSL service is not the Windows one");
        assert_eq!(
            prefer_wsl(Ok(service.clone()), Ok(wsl())),
            Ok(service.clone()),
            "a Windows service wins"
        );
        assert_eq!(
            prefer_wsl(Err(Missing::NotRunning), Ok(wsl())),
            Ok(wsl()),
            "with no Windows service, the WSL one is used"
        );
        assert_eq!(
            prefer_wsl(Err(Missing::Outdated), Ok(wsl())),
            Ok(wsl()),
            "a working WSL service is used over a Windows update notice"
        );
        assert_eq!(
            prefer_wsl(Err(Missing::Outdated), Err(Missing::NotRunning)),
            Err(Missing::Outdated),
            "a Windows service that needs an update is still reported"
        );
        assert_eq!(
            prefer_wsl(Err(Missing::NotRunning), Err(Missing::Outdated)),
            Err(Missing::Outdated),
            "a WSL service that needs an update is reported too"
        );
        assert_eq!(
            prefer_wsl(Err(Missing::NotRunning), Err(Missing::NotRunning)),
            Err(Missing::NotRunning)
        );
    }

    #[test]
    fn a_silent_service_is_bounded_by_cancellation_and_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let service = fixture(&url);
        let started = Instant::now();
        let error = service
            .request(
                "GET",
                "/api/info",
                None,
                None,
                Duration::from_millis(200),
                &AtomicBool::new(false),
            )
            .unwrap_err();
        assert!(error.to_string().contains("exceeded"), "{error}");
        let cancelled = AtomicBool::new(false);
        thread::scope(|scope| {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(100));
                cancelled.store(true, Ordering::Release);
            });
            let error = service
                .request(
                    "GET",
                    "/api/info",
                    None,
                    None,
                    Duration::from_secs(30),
                    &cancelled,
                )
                .unwrap_err();
            assert!(error.to_string().contains("cancelled"), "{error}");
        });
        assert!(started.elapsed() < Duration::from_secs(5));
        drop(listener);
    }

    /// Far more than loopback socket buffers hold, so sending it stalls when
    /// the service stops reading. A prime period, so a skipped or repeated
    /// chunk can't line up with the data.
    fn large_body() -> Vec<u8> {
        (0..64 * 1024 * 1024)
            .map(|index: u32| (index % 251) as u8)
            .collect()
    }

    /// Accepts connections and holds them, never reading, until `released`
    /// fires or its sender drops.
    fn hold(
        listener: TcpListener,
        released: std::sync::mpsc::Receiver<()>,
    ) -> thread::JoinHandle<()> {
        listener.set_nonblocking(true).unwrap();
        thread::spawn(move || {
            let mut streams = Vec::new();
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                released.recv_timeout(Duration::from_millis(10))
            {
                if let Ok((stream, _)) = listener.accept() {
                    streams.push(stream);
                }
            }
        })
    }

    #[test]
    fn a_service_that_stops_reading_is_bounded_by_cancellation_and_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let service = fixture(&url);
        let body = large_body();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let held = hold(listener, released);
        let started = Instant::now();
        let error = service
            .request(
                "POST",
                "/generate",
                Some(&body),
                None,
                Duration::from_millis(300),
                &AtomicBool::new(false),
            )
            .unwrap_err();
        assert!(error.to_string().contains("exceeded"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        let cancelled = AtomicBool::new(false);
        thread::scope(|scope| {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(100));
                cancelled.store(true, Ordering::Release);
            });
            let started = Instant::now();
            let error = service
                .request(
                    "POST",
                    "/generate",
                    Some(&body),
                    None,
                    Duration::from_secs(30),
                    &cancelled,
                )
                .unwrap_err();
            assert!(error.to_string().contains("cancelled"), "{error}");
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{:?}",
                started.elapsed()
            );
        });
        release.send(()).unwrap();
        held.join().unwrap();
    }

    #[test]
    fn an_answer_before_the_whole_body_is_sent_still_returns() {
        answered_early(true);
        // Closing with the body unread resets the connection. Windows drops
        // data already received when a reset arrives, so this case is Linux's.
        #[cfg(target_os = "linux")]
        answered_early(false);
    }

    /// The service answers 404 after the request line, then either half-closes
    /// and keeps the socket open, or closes it with the body unread.
    fn answered_early(half_close: bool) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (release, released) = std::sync::mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&mut stream).read_line(&mut line).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngone",
                )
                .unwrap();
            if half_close {
                stream.shutdown(std::net::Shutdown::Write).unwrap();
                let _ = released.recv_timeout(Duration::from_secs(10));
            }
        });
        let started = Instant::now();
        let response = fixture(&url)
            .request(
                "POST",
                "/generate",
                Some(&large_body()),
                None,
                Duration::from_secs(30),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(response.status, 404);
        assert_eq!(response.body, b"gone");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        let _ = release.send(());
        server.join().unwrap();
    }

    #[test]
    fn a_large_body_arrives_intact() {
        let body = large_body();
        let (url, server) = serve(vec![("POST /generate".into(), 200, "{}".into())]);
        let response = fixture(&url)
            .request(
                "POST",
                "/generate",
                Some(&body),
                None,
                Duration::from_secs(30),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{}");
        let (_, received) = server.join().unwrap().remove(0);
        assert!(received == body, "the body changed in transit");
    }
}
