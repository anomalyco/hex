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
    live_discovery().ok().map(|document| document.pid)
}

pub(super) fn live_discovery() -> Result<DiscoveryDocument> {
    let path = crate::app_paths::local_api_discovery_file()?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("Hex is not running")
        }
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("could not read {}", path.display()));
        }
    };
    let document: DiscoveryDocument =
        serde_json::from_slice(&bytes).wrap_err("Hex's local API discovery file is invalid")?;
    if !valid_discovery(&document) || !discovery_is_live(&document) {
        bail!("Hex is not running");
    }
    Ok(document)
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
    let document = live_discovery()?;
    Endpoint::of(&document).post(path, content_type, body, read_timeout, max_response)
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
    let path = crate::app_paths::local_api_discovery_file()?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Discovered::Absent);
        }
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("couldn't read {}", path.display()));
        }
    };
    let document: DiscoveryDocument =
        serde_json::from_slice(&bytes).wrap_err("Hex's local API discovery file is invalid")?;
    classify(&document, discovery_is_live(&document))
}

fn classify(document: &DiscoveryDocument, live: bool) -> Result<Discovered> {
    if !live {
        return Ok(Discovered::Absent);
    }
    if document.api_version != API_VERSION {
        bail!(
            "the running Hex speaks local API version {}, not {API_VERSION}; \
             update Hex or quit it",
            document.api_version
        );
    }
    if !valid_token(&document.token) {
        bail!("the running Hex's local API token is malformed; restart Hex");
    }
    Ok(Discovered::Live(Endpoint::of(document)))
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

pub(super) fn prepare_discovery_path(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        bail!("local API discovery path has no parent");
    };
    fs::create_dir_all(parent)?;
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

#[cfg(all(test, unix))]
mod tests {
    use std::io::{BufRead, BufReader, Write as _};
    use std::net::{Ipv4Addr, TcpListener};
    use std::thread;
    use std::time::Duration;

    use super::{Discovered, DiscoveryDocument, Endpoint, classify, send};

    const TOKEN: &str = "hex_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn document(api_version: &str, token: &str) -> DiscoveryDocument {
        DiscoveryDocument {
            port: 4100,
            token: token.into(),
            api_version: api_version.into(),
            pid: 42,
        }
    }

    #[test]
    fn only_a_stopped_hex_counts_as_absent() {
        assert_eq!(
            classify(&document("1", "bad"), false).unwrap(),
            Discovered::Absent,
            "a stale file starts a service whatever it says"
        );
        assert!(matches!(
            classify(&document(super::API_VERSION, TOKEN), true).unwrap(),
            Discovered::Live(_)
        ));
        let other = classify(&document("1", TOKEN), true).unwrap_err();
        assert!(format!("{other}").contains("update Hex"), "{other}");
        assert!(classify(&document(super::API_VERSION, "hex_x"), true).is_err());
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
