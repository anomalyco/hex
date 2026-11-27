use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};

#[cfg(debug_assertions)]
use super::DEVELOPER_CONTROL_TIMEOUT;
use super::{API_VERSION, CONNECTION_TIMEOUT, LIVE_PROBE_TIMEOUT};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DiscoveryDocument {
    pub(super) port: u16,
    pub(super) token: String,
    pub(super) api_version: String,
    pub(super) pid: u32,
}

pub(super) struct DiscoveryLease {
    pub(super) path: PathBuf,
    pub(super) document: DiscoveryDocument,
}

impl DiscoveryLease {
    pub(super) fn remove_if_owned(&self) {
        remove_discovery_if_owned(&self.path, &self.document);
    }
}

pub(super) fn remove_discovery_if_owned(path: &Path, expected: &DiscoveryDocument) {
    let owned = fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice::<DiscoveryDocument>(&data).ok())
        .is_some_and(|document| document == *expected);
    if owned
        && let Err(error) = fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::error!(%error, path = %path.display(), "could not remove local API discovery file");
    }
}

impl Drop for DiscoveryLease {
    fn drop(&mut self) {
        self.remove_if_owned();
    }
}

#[cfg(debug_assertions)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn call_developer(
    command: &crate::control::developer_control::DeveloperCommand,
) -> Result<crate::control::developer_control::DeveloperReply> {
    let body = serde_json::to_vec(command)?;
    let (status, body) = post_json("/dev/control", &body, DEVELOPER_CONTROL_TIMEOUT, 64 * 1024)?;
    if status != 200 {
        bail!(
            "Hex developer control failed: {}",
            error_code(status, &body)
        );
    }
    serde_json::from_slice(&body).wrap_err("Hex developer response is invalid")
}

/// Sends one `hex` control request to the running app.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn call_control(
    request: &crate::control::ControlRequest,
) -> Result<crate::control::ControlResponse> {
    let body = serde_json::to_vec(request)?;
    // The app answers within the command's deadline or the server replies 504.
    let (status, body) = post_json(
        "/control",
        &body,
        request.command.deadline() + Duration::from_secs(5),
        1024 * 1024,
    )?;
    if status != 200 {
        let code = error_code(status, &body);
        bail!(match code.as_str() {
            "control-unavailable" => {
                "the running Hex doesn't accept control commands; update Hex".to_string()
            }
            "control-busy" => "Hex is busy with other control commands; try again".into(),
            "control-timeout" => "Hex didn't answer in time; check `hex status`".into(),
            "invalid-control-command" => {
                "the running Hex doesn't know this command; update Hex".into()
            }
            _ => format!("Hex refused the request ({code})"),
        });
    }
    serde_json::from_slice(&body).wrap_err("Hex sent an invalid control reply")
}

/// The running app's process id, if its local API is live.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn running_pid() -> Option<u32> {
    match read_discovery() {
        Ok(found) => found.pid(),
        Err(_) => None,
    }
}

pub(super) fn post_json(
    path: &str,
    body: &[u8],
    read_timeout: Duration,
    max_response: usize,
) -> Result<(u16, Vec<u8>)> {
    post(path, "application/json", body, read_timeout, max_response)
}

fn post(
    path: &str,
    content_type: &str,
    body: &[u8],
    read_timeout: Duration,
    max_response: usize,
) -> Result<(u16, Vec<u8>)> {
    control_endpoint(read_discovery()?)?.post(path, content_type, body, read_timeout, max_response)
}

/// Where one local API answers, and the token it takes: the running Hex's,
/// or an embedded service's a client started.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Endpoint {
    address: SocketAddr,
    token: String,
}

/// What the discovery file says about a running Hex.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Discovered {
    /// A Hex answers with an API this build speaks.
    Live(Endpoint),
    /// No Hex runs: no discovery file, or one a stopped Hex left behind.
    Absent,
}

/// The running Hex's endpoint, or [`Discovered::Absent`] when none runs. A
/// live Hex this build can't talk to (another API version, a malformed
/// token) or a discovery file that can't be read is an error, so a client
/// never starts a second service beside a Hex that is running.
pub(crate) fn discover() -> Result<Discovered> {
    select_endpoint(read_discovery()?)
}

/// One classification for control requests, process identity, embedded
/// selection and start. A live incompatible app still owns its process.
#[derive(Debug, Eq, PartialEq)]
enum Discovery {
    Absent,
    Compatible {
        pid: u32,
        endpoint: Endpoint,
    },
    Incompatible {
        pid: u32,
        reason: CompatibilityError,
    },
}

impl Discovery {
    fn pid(&self) -> Option<u32> {
        match self {
            Self::Absent => None,
            Self::Compatible { pid, .. } | Self::Incompatible { pid, .. } => Some(*pid),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum CompatibilityError {
    Version(String),
    Token,
}

impl std::fmt::Display for CompatibilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Version(version) => write!(
                f,
                "the running Hex speaks local API version {version}, not {API_VERSION}; update Hex or quit it"
            ),
            Self::Token => {
                f.write_str("the running Hex's local API token is malformed; restart Hex")
            }
        }
    }
}

impl std::error::Error for CompatibilityError {}

fn read_discovery() -> Result<Discovery> {
    discover_at(
        &crate::app_paths::local_api_discovery_file()?,
        discovery_is_live,
    )
}

fn discover_at(path: &Path, live: impl FnOnce(&DiscoveryDocument) -> bool) -> Result<Discovery> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Discovery::Absent),
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("couldn't read {}", path.display()));
        }
    };
    let document: DiscoveryDocument =
        serde_json::from_slice(&bytes).wrap_err("Hex's local API discovery file is invalid")?;
    Ok(classify(&document, live(&document)))
}

fn classify(document: &DiscoveryDocument, live: bool) -> Discovery {
    if !live {
        return Discovery::Absent;
    }
    let reason = if document.api_version != API_VERSION {
        Some(CompatibilityError::Version(document.api_version.clone()))
    } else if !valid_token(&document.token) {
        Some(CompatibilityError::Token)
    } else {
        None
    };
    match reason {
        Some(reason) => Discovery::Incompatible {
            pid: document.pid,
            reason,
        },
        None => Discovery::Compatible {
            pid: document.pid,
            endpoint: Endpoint::of(document),
        },
    }
}

// Keep the embedded client's two-outcome interface: incompatibility is an
// error, never permission to spawn a service.
fn select_endpoint(discovery: Discovery) -> Result<Discovered> {
    match discovery {
        Discovery::Absent => Ok(Discovered::Absent),
        Discovery::Compatible { endpoint, .. } => Ok(Discovered::Live(endpoint)),
        Discovery::Incompatible { reason, .. } => Err(reason.into()),
    }
}

fn control_endpoint(discovery: Discovery) -> Result<Endpoint> {
    match select_endpoint(discovery)? {
        Discovered::Live(endpoint) => Ok(endpoint),
        Discovered::Absent => bail!("Hex is not running"),
    }
}

/// The ready line `hex service --embedded` prints on stdout.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ready {
    #[serde(rename = "type")]
    kind: String,
    url: String,
    token: String,
    api_version: String,
    pid: u32,
}

impl Endpoint {
    fn of(document: &DiscoveryDocument) -> Self {
        Self {
            address: SocketAddr::from(([127, 0, 0, 1], document.port)),
            token: document.token.clone(),
        }
    }

    /// The endpoint an embedded service's ready line names, checked against
    /// the child that printed it: this build's API version, that child's
    /// pid, a loopback `http://127.0.0.1:<port>` URL, and a valid token.
    pub(crate) fn embedded(line: &[u8], pid: u32) -> Result<Self> {
        let ready: Ready = serde_json::from_slice(line)
            .wrap_err("the embedded service's ready line is invalid")?;
        if ready.kind != "ready" {
            bail!("the embedded service didn't say it was ready");
        }
        if ready.api_version != API_VERSION {
            bail!(
                "the embedded service speaks local API version {}, not {API_VERSION}",
                ready.api_version
            );
        }
        if ready.pid != pid {
            bail!("the embedded service's ready line names another process");
        }
        let port = ready
            .url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|&port| port != 0 && ready.url == format!("http://127.0.0.1:{port}"))
            .ok_or_else(|| eyre!("the embedded service isn't on a loopback URL"))?;
        if !valid_token(&ready.token) {
            bail!("the embedded service's token is malformed");
        }
        Ok(Self {
            address: SocketAddr::from(([127, 0, 0, 1], port)),
            token: ready.token,
        })
    }

    /// One authenticated request, its status and body.
    pub(crate) fn post(
        &self,
        path: &str,
        content_type: &str,
        body: &[u8],
        read_timeout: Duration,
        max_response: usize,
    ) -> Result<(u16, Vec<u8>)> {
        send(
            self.address,
            &self.token,
            path,
            content_type,
            body,
            read_timeout,
            max_response,
        )
    }

    /// An empty authenticated POST answered with server-sent events: each
    /// `data:` event goes to `event` until it returns `true` or the stream
    /// ends. A refusal returns its status and body instead; a 200 returns
    /// an empty body. No event may exceed `max_event` bytes, and the stream
    /// may stay quiet (keep-alives count) at most `idle` at a time.
    pub(crate) fn events(
        &self,
        path: &str,
        idle: Duration,
        max_event: usize,
        mut event: impl FnMut(&serde_json::Value) -> bool,
    ) -> Result<(u16, Vec<u8>)> {
        use std::io::BufRead as _;

        let mut stream = TcpStream::connect_timeout(&self.address, CONNECTION_TIMEOUT)
            .wrap_err("could not connect to Hex's local API")?;
        stream.set_read_timeout(Some(idle))?;
        stream.set_write_timeout(Some(CONNECTION_TIMEOUT))?;
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            self.token,
        )?;
        stream.flush()?;
        let mut reader = std::io::BufReader::new(stream);
        let mut line = Vec::new();
        let next_line = |reader: &mut std::io::BufReader<TcpStream>, line: &mut Vec<u8>| {
            line.clear();
            let read = reader
                .by_ref()
                .take(max_event as u64 + 1)
                .read_until(b'\n', line)?;
            if line.len() > max_event {
                bail!("Hex sent an event that is too large");
            }
            Ok::<_, color_eyre::Report>(read)
        };
        if next_line(&mut reader, &mut line)? == 0 {
            bail!("Hex didn't answer");
        }
        let status = std::str::from_utf8(&line)
            .ok()
            .and_then(|line| line.split_ascii_whitespace().nth(1))
            .and_then(|status| status.parse::<u16>().ok())
            .ok_or_else(|| eyre!("Hex's response has no HTTP status"))?;
        loop {
            if next_line(&mut reader, &mut line)? == 0 {
                bail!("Hex's response is malformed");
            }
            if line == b"\r\n" || line == b"\n" {
                break;
            }
        }
        if status != 200 {
            let mut body = Vec::new();
            reader
                .take(max_event as u64 + 1)
                .read_to_end(&mut body)
                .wrap_err("Hex didn't finish answering")?;
            body.truncate(max_event);
            return Ok((status, body));
        }
        while next_line(&mut reader, &mut line)? != 0 {
            let Some(data) = line.strip_prefix(b"data:") else {
                continue;
            };
            let value = serde_json::from_slice::<serde_json::Value>(data.trim_ascii())
                .wrap_err("Hex sent an unreadable event")?;
            if event(&value) {
                break;
            }
        }
        Ok((200, Vec::new()))
    }
}

fn send(
    address: SocketAddr,
    token: &str,
    path: &str,
    content_type: &str,
    body: &[u8],
    read_timeout: Duration,
    max_response: usize,
) -> Result<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect_timeout(&address, CONNECTION_TIMEOUT)
        .wrap_err("could not connect to Hex's local API")?;
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(CONNECTION_TIMEOUT))?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    )?;
    // Hex answers before reading a body it rejects (an unknown model, a full
    // queue); keep that answer when the connection then resets.
    let upload = stream.write_all(body).and_then(|()| stream.flush());
    let mut response = Vec::new();
    let read = (&mut stream)
        .take(max_response as u64 + 1)
        .read_to_end(&mut response);
    if response.is_empty() {
        read.wrap_err("Hex didn't answer")?;
        upload.wrap_err("Hex didn't take the upload")?;
        bail!("Hex didn't answer");
    }
    if response.len() > max_response {
        bail!("Hex's response is too large");
    }
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        bail!("Hex's response is malformed");
    };
    let headers = std::str::from_utf8(&response[..header_end])?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| eyre!("Hex's response has no HTTP status"))?;
    Ok((status, response.split_off(header_end + 4)))
}

pub(crate) fn error_code(status: u16, body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("code")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("http-{status}"))
}

pub(super) fn generate_token() -> Result<String> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|error| eyre!("could not generate API token: {error}"))?;
    let mut token = String::with_capacity(68);
    token.push_str("hex_");
    for byte in random {
        use std::fmt::Write as _;
        write!(token, "{byte:02x}").expect("writing to a string cannot fail");
    }
    Ok(token)
}

/// Makes sure no other local API publishes at `path` before this one does.
///
/// `holds_instance_lock` is whether the caller holds the local API's
/// instance lock, which sits beside the discovery file. While it is held no
/// other Hex can publish here, so a file found there is one a Hex that
/// stopped uncleanly left behind and is simply replaced. It is not probed:
/// after a reboot its pid and port can both belong to unrelated programs,
/// and the probe then read as a running Hex and stopped every launch (#2887).
pub(super) fn prepare_discovery_path(path: &Path, holds_instance_lock: bool) -> Result<()> {
    let Some(parent) = path.parent() else {
        bail!("local API discovery path has no parent");
    };
    fs::create_dir_all(parent)?;
    if holds_instance_lock {
        return Ok(());
    }
    let existing = match fs::read(path) {
        Ok(data) => serde_json::from_slice::<DiscoveryDocument>(&data).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if existing
        .as_ref()
        .is_some_and(|document| valid_discovery(document) && discovery_is_live(document))
    {
        bail!("another local API service appears to be running");
    }
    Ok(())
}

pub(super) fn valid_discovery(document: &DiscoveryDocument) -> bool {
    document.api_version == API_VERSION && valid_token(&document.token)
}

pub(super) fn valid_token(token: &str) -> bool {
    token.len() == 68
        && token.starts_with("hex_")
        && token[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn discovery_is_live(document: &DiscoveryDocument) -> bool {
    document.port != 0
        && document.pid != 0
        && process_is_alive(document.pid)
        && TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], document.port)),
            LIVE_PROBE_TIMEOUT,
        )
        .is_ok()
}

pub(super) fn write_discovery(path: &Path, document: &DiscoveryDocument) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("local API discovery path has no parent"))?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    if let Err(error) = fs::remove_file(&temporary)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error.into());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    // The token lets any reader drive Hex, so only the owner may read it. On
    // Windows the file inherits the per-user Application Support folder's
    // ACL (the user, SYSTEM and Administrators).
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temporary)?;
    let mut data = serde_json::to_vec(document)?;
    data.push(b'\n');
    file.write_all(&data)?;
    file.sync_all()?;
    #[cfg(unix)]
    fs::set_permissions(
        &temporary,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )?;
    fs::rename(&temporary, path)?;
    crate::durable::sync_directory(parent)?;
    Ok(())
}

#[cfg(unix)]
pub(super) fn process_is_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(windows)]
pub(super) fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: the handle is checked before use and closed once.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0;
        let alive = GetExitCodeProcess(process, &mut code) != 0 && code == STILL_ACTIVE as u32;
        CloseHandle(process);
        alive
    }
}

/// A running Hex that `hex restart` waits on before it starts the successor.
///
/// The local API removes its discovery file at the start of its shutdown and
/// the process lives on after that. What that process holds goes at different
/// points: on Windows the file goes first, then the tray, and the instance
/// event last, at process exit; on Linux the service lock is released before
/// the file, when the host's server drops. Waiting for the process covers
/// both, which is why the local API isn't the signal.
///
/// On Windows the handle is opened while that Hex still runs, so the wait can
/// only be about that process: looking the pid up again after the wait could
/// find whatever reused it (the updater's lesson, #513).
#[cfg(not(target_os = "macos"))]
pub struct ExitWatch {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(unix)]
    pid: u32,
}

#[cfg(not(target_os = "macos"))]
impl ExitWatch {
    /// Watches `pid`, which must still be running: `None` once it has ended,
    /// or when its process can't be opened for waiting.
    fn new(pid: u32) -> Option<Self> {
        // 0 is not a process: on Unix `kill(0, …)` means this process's group.
        if pid == 0 {
            return None;
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
            // SAFETY: opens a handle to `pid`; it is null-checked here and
            // closed once by `Drop`.
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            (!handle.is_null()).then_some(Self { handle })
        }
        #[cfg(unix)]
        {
            process_is_alive(pid).then_some(Self { pid })
        }
    }

    /// Waits up to `timeout` for the watched process to exit, reporting
    /// whether it has. One that ended before the wait began counts as exited.
    pub fn wait(&self, timeout: Duration) -> bool {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0};
            use windows_sys::Win32::System::Threading::WaitForSingleObject;
            // Waits are tick-quantized and can report a timeout a little early,
            // so loop on the clock rather than trusting one call, and ask once
            // more with no timeout when the clock says the time is up.
            let deadline = std::time::Instant::now() + timeout;
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                // INFINITE (u32::MAX) would never time out.
                let millis = if remaining.is_zero() {
                    0
                } else {
                    remaining.as_millis().clamp(1, u128::from(u32::MAX - 1)) as u32
                };
                // SAFETY: `self.handle` is the handle `new` opened and `Drop`
                // closes; the timeout is a plain millisecond count.
                match unsafe { WaitForSingleObject(self.handle, millis) } {
                    WAIT_OBJECT_0 => return true,
                    WAIT_FAILED => return false,
                    _ if remaining.is_zero() => return false,
                    _ => {}
                }
            }
        }
        #[cfg(unix)]
        {
            let started = std::time::Instant::now();
            loop {
                if !process_is_alive(self.pid) {
                    return true;
                }
                if started.elapsed() >= timeout {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

#[cfg(windows)]
impl Drop for ExitWatch {
    fn drop(&mut self) {
        // SAFETY: the handle `new` opened for this watch, closed once.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
    }
}

/// The running Hex, watched from now on, or `None` when none answers or its
/// process can't be opened for waiting. The pid comes from the discovery
/// file, which `running_pid` has just checked is alive and answering, so the
/// watch is taken while that Hex still runs.
#[cfg(not(target_os = "macos"))]
pub fn watch_running() -> Option<ExitWatch> {
    ExitWatch::new(running_pid()?)
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::{BufRead, BufReader, Write as _};
    use std::net::{Ipv4Addr, TcpListener};
    use std::thread;
    use std::time::Duration;

    use super::{Discovered, Discovery, DiscoveryDocument, Endpoint, classify, send};

    const TOKEN: &str = "hex_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn document(api_version: &str, token: &str) -> DiscoveryDocument {
        DiscoveryDocument {
            port: 4100,
            token: token.into(),
            api_version: api_version.into(),
            pid: 42,
        }
    }

    /// A discovery file left by a Hex that stopped uncleanly, whose pid and
    /// port now belong to something else that is alive and listening: here,
    /// this test process and a listener of its own (#2887).
    #[test]
    fn a_leftover_file_naming_a_live_pid_and_port_does_not_stop_the_lock_holder() {
        let directory = std::env::temp_dir().join(format!(
            "hex-discovery-leftover-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("local-api.json");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let leftover = DiscoveryDocument {
            port: listener.local_addr().unwrap().port(),
            pid: std::process::id(),
            ..document(super::API_VERSION, TOKEN)
        };
        std::fs::write(&path, serde_json::to_vec(&leftover).unwrap()).unwrap();
        assert!(
            super::discovery_is_live(&leftover),
            "the probe reads it as live"
        );

        let refused = super::prepare_discovery_path(&path, false).unwrap_err();
        assert!(
            format!("{refused}").contains("appears to be running"),
            "without the lock the probe has the last word: {refused}"
        );
        super::prepare_discovery_path(&path, true)
            .expect("the lock holder replaces a leftover file whatever it names");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn only_a_stopped_hex_counts_as_absent() {
        assert_eq!(
            classify(&document("1", "bad"), false),
            Discovery::Absent,
            "a stale file starts a service whatever it says"
        );
        assert!(matches!(
            classify(&document(super::API_VERSION, TOKEN), true),
            Discovery::Compatible { pid: 42, .. }
        ));
        let other = super::select_endpoint(classify(&document("1", TOKEN), true)).unwrap_err();
        assert!(format!("{other}").contains("update Hex"), "{other}");
        assert!(
            super::select_endpoint(classify(&document(super::API_VERSION, "hex_x"), true)).is_err()
        );
    }

    #[test]
    fn discovery_files_preserve_absence_incompatibility_and_read_errors() {
        let directory = std::env::temp_dir().join(format!(
            "hex-discovery-fixtures-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let _cleanup = Cleanup(directory.clone());
        let path = directory.join("local-api.json");
        assert_eq!(
            super::discover_at(&path, |_| panic!("absent files aren't probed")).unwrap(),
            Discovery::Absent
        );
        for (version, token, live, incompatible) in [
            (super::API_VERSION, TOKEN, false, false),
            ("1", "bad", false, false),
            (super::API_VERSION, TOKEN, true, false),
            ("1", TOKEN, true, true),
            (super::API_VERSION, "bad", true, true),
        ] {
            let document = document(version, token);
            std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
            let found = || super::discover_at(&path, |_| live).unwrap();
            assert_eq!(found().pid(), live.then_some(42));
            assert_eq!(
                matches!(found(), Discovery::Incompatible { pid: 42, .. }),
                incompatible
            );
            let embedded = super::select_endpoint(found());
            let control = super::control_endpoint(found());
            if incompatible {
                assert_eq!(
                    embedded.unwrap_err().to_string(),
                    control.unwrap_err().to_string()
                );
            } else if live {
                let Discovered::Live(endpoint) = embedded.unwrap() else {
                    panic!("live endpoint")
                };
                assert_eq!(control.unwrap(), endpoint);
            } else {
                assert_eq!(embedded.unwrap(), Discovered::Absent);
                assert_eq!(control.unwrap_err().to_string(), "Hex is not running");
            }
        }
        std::fs::write(&path, b"not json").unwrap();
        assert!(super::discover_at(&path, |_| panic!("invalid files aren't probed")).is_err());
        // A directory at the file path deterministically fails read on every
        // Unix runner, including root (unlike chmod 000).
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let error =
            super::discover_at(&path, |_| panic!("unreadable files aren't probed")).unwrap_err();
        assert!(error.to_string().contains("couldn't read"));
    }

    fn ready(url: &str, pid: u32, api_version: &str, token: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "type": "ready", "url": url, "token": token, "apiVersion": api_version, "pid": pid,
        }))
        .unwrap()
    }

    #[test]
    fn an_embedded_ready_line_must_name_its_child_on_loopback() {
        let v = super::API_VERSION;
        let endpoint = Endpoint::embedded(&ready("http://127.0.0.1:4100", 7, v, TOKEN), 7).unwrap();
        assert_eq!(endpoint.address.port(), 4100);
        for line in [
            ready("http://127.0.0.1:4100", 8, v, TOKEN),
            ready("http://127.0.0.1:4100", 7, "1", TOKEN),
            ready("http://127.0.0.1:4100", 7, v, "hex_x"),
            ready("http://localhost:4100", 7, v, TOKEN),
            ready("http://127.0.0.1:04100", 7, v, TOKEN),
            ready("http://127.0.0.1:0", 7, v, TOKEN),
            ready("http://127.0.0.1:4100/", 7, v, TOKEN),
            b"not json".to_vec(),
        ] {
            assert!(
                Endpoint::embedded(&line, 7).is_err(),
                "{}",
                String::from_utf8_lossy(&line)
            );
        }
    }

    #[test]
    fn events_read_until_the_terminal_one() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap() <= 2 {
                    break;
                }
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                      data: {\"type\":\"downloading\",\"downloadedBytes\":5}\n\n\
                      : keep-alive\n\n\
                      data: {\"type\":\"ok\"}\n\n",
                )
                .unwrap();
        });
        let endpoint = Endpoint {
            address,
            token: TOKEN.into(),
        };
        let mut kinds = Vec::new();
        let (status, _) = endpoint
            .events(
                "/models/x/prepare",
                Duration::from_secs(10),
                1024,
                |event| {
                    let kind = event["type"].as_str().unwrap().to_owned();
                    let done = kind == "ok";
                    kinds.push(kind);
                    done
                },
            )
            .unwrap();
        server.join().unwrap();
        assert_eq!(status, 200);
        assert_eq!(kinds, ["downloading", "ok"]);
    }

    /// Hex answers before reading a body it rejects and closes; the answer
    /// must survive a body too large for the socket buffers.
    #[test]
    fn a_refusal_survives_a_large_upload() {
        const BODY: &[u8] = br#"{"code":"model-not-ready"}"#;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap() <= 2 {
                    break;
                }
            }
            let mut response = format!(
                "HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                BODY.len()
            )
            .into_bytes();
            response.extend_from_slice(BODY);
            stream.write_all(&response).unwrap();
        });
        let upload = vec![0_u8; 4 * 1024 * 1024];
        let (status, body) = send(
            address,
            "hex_test",
            "/transcriptions?model=parakeet_v2",
            "audio/wav",
            &upload,
            Duration::from_secs(10),
            1024 * 1024,
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(status, 409);
        assert_eq!(body, BODY);
    }
}

/// The process wait `hex restart` uses, on every OS that takes it. The child
/// half follows `desktop::process_exit`'s pattern: this same test binary,
/// selected with `--exact`, switched into child mode by an environment
/// variable.
#[cfg(all(test, not(target_os = "macos")))]
mod exit_watch_tests {
    use std::io::Read as _;
    use std::process::{Child, Stdio};
    use std::time::{Duration, Instant};

    use super::ExitWatch;

    const CHILD: &str = "HEX_EXIT_WATCH_CHILD";
    const CHILD_TEST: &str = "local_api::discovery::exit_watch_tests::stdin_child";

    /// The child half: ends when its stdin closes. The parent holds the pipe
    /// open while it takes the watch, so the child is running then, and closes
    /// it to let the child exit, with no sleep to race.
    #[test]
    fn stdin_child() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        let mut stdin = std::io::stdin();
        let mut byte = [0_u8; 1];
        let _ = stdin.read(&mut byte);
    }

    fn child() -> Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD_TEST, "--nocapture"])
            .env(CHILD, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    /// `hex restart` takes the watch while the old Hex runs and needs it to
    /// report that Hex's exit; a process still running must not read as gone.
    #[test]
    fn waiting_for_a_process_reports_whether_it_exited() {
        assert!(ExitWatch::new(0).is_none(), "pid 0 is not a process");

        let mut exiting = child();
        let watch = ExitWatch::new(exiting.id()).expect("a running process can be watched");
        // Closing the pipe ends the child; the parent reaps it here, so the
        // watch must then see the pid gone. The output proves the filter found
        // the child's test instead of the child exiting with none.
        drop(exiting.stdin.take());
        let output = exiting.wait_with_output().unwrap();
        assert!(output.status.success(), "the child failed");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "the child didn't run its test: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let started = Instant::now();
        assert!(
            watch.wait(Duration::from_secs(10)),
            "the watch missed the exit"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the watch took {:?} to see the exit",
            started.elapsed()
        );

        let mut staying = child();
        let watch = ExitWatch::new(staying.id()).expect("a running process can be watched");
        let started = Instant::now();
        assert!(
            !watch.wait(Duration::from_millis(300)),
            "a live process read as exited"
        );
        assert!(started.elapsed() >= Duration::from_millis(300));
        let _ = staying.kill();
        let _ = staying.wait();
    }
}
