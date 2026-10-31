//! The running OpenCode service, reached without starting anything: its
//! owner-only registration names a loopback URL and password, `/api/info`
//! confirms the registered process still answers, and requests are plain
//! authenticated HTTP over `TcpStream`. Hex never runs an OpenCode
//! executable; when no service answers, OpenCode isn't running.

use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::Deserialize;

/// What features say when no service answers.
pub(crate) const NOT_RUNNING: &str = "OpenCode isn't running";
const MAX_REGISTRATION_BYTES: u64 = 64 * 1024;
const MAX_REGISTRATIONS: usize = 16;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const INFO_DEADLINE: Duration = Duration::from_secs(2);
const CONNECT_DEADLINE: Duration = Duration::from_secs(2);
/// How often a blocked read checks cancellation and the deadline.
const READ_SLICE: Duration = Duration::from_millis(50);

/// One running service: where it listens and how to authenticate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Service {
    address: SocketAddr,
    password: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
}

/// The running service, or `None` when OpenCode isn't running. Reads only
/// registration files and asks each registered service for `/api/info`.
pub(crate) fn find() -> Option<Service> {
    find_in(&state_dir()?)
}

/// Whether a service answers now; a short loopback check for availability.
pub(crate) fn running() -> bool {
    find().is_some()
}

/// OpenCode's state directory on every OS: `$XDG_STATE_HOME` when set, else
/// `~/.local/state`, then `opencode` (Windows included).
fn state_dir() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("state")))?;
    Some(root.join("opencode"))
}

/// `service.json` (the stable, beta and dev channels) first, then other
/// channels' `service-<channel>.json`; the first that answers wins.
fn find_in(state: &Path) -> Option<Service> {
    let mut others: Vec<PathBuf> = fs::read_dir(state)
        .ok()?
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
    std::iter::once(state.join("service.json"))
        .chain(others)
        .take(MAX_REGISTRATIONS)
        .filter_map(|path| read_registration(&path))
        .find_map(|registration| confirm(&registration))
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
fn confirm(registration: &Registration) -> Option<Service> {
    let service = Service {
        address: loopback_address(&registration.url)?,
        password: registration
            .password
            .clone()
            .filter(|password| !password.is_empty()),
    };
    #[derive(Deserialize)]
    struct Info {
        pid: u32,
        version: String,
    }
    let response = service
        .request(
            "GET",
            "/api/info",
            None,
            None,
            INFO_DEADLINE,
            &AtomicBool::new(false),
        )
        .ok()?;
    if response.status != 200 {
        return None;
    }
    let info: Info = serde_json::from_slice(&response.body).ok()?;
    (info.pid == registration.pid
        && registration
            .version
            .as_ref()
            .is_none_or(|version| *version == info.version))
    .then_some(service)
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
    /// One request. `directory` scopes it to an OpenCode workspace (created
    /// when first used). HTTP error statuses return normally; transport
    /// failures, cancellation and the deadline are errors.
    pub(crate) fn request(
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
                encode_uri_component(&workspace(directory)?)
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
        stream.set_write_timeout(Some(remaining().max(Duration::from_millis(1))))?;
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(body.unwrap_or_default()))
            .wrap_err_with(|| format!("could not send OpenCode {path}"))?;
        let mut bytes = Vec::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            check(remaining())?;
            stream.set_read_timeout(Some(remaining().clamp(Duration::from_millis(1), READ_SLICE)))?;
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    bytes.extend_from_slice(&buffer[..read]);
                    if bytes.len() > MAX_RESPONSE_BYTES {
                        return Err(eyre!("OpenCode {path} answered more than 16 MiB"));
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    return Err(error).wrap_err_with(|| format!("could not read OpenCode {path}"));
                }
            }
        }
        parse_response(bytes).ok_or_else(|| eyre!("OpenCode {path} returned invalid HTTP"))
    }
}

/// Creates Hex's OpenCode workspace on first use and returns the path
/// OpenCode should see.
fn workspace(directory: &Path) -> Result<String> {
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

fn parse_response(bytes: Vec<u8>) -> Option<Response> {
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
    } else {
        body.to_vec()
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
        assert_eq!(find_in(&state), None);
        let info = r#"{"pid":7,"version":"2.0.0"}"#.to_owned();
        let (url, server) = serve(vec![("GET /api/info".into(), 200, info)]);
        register(&state, "service.json", &url, 42);
        assert_eq!(find_in(&state), None);
        server.join().unwrap();
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", closed.local_addr().unwrap());
        drop(closed);
        register(&state, "service.json", &url, 42);
        assert_eq!(find_in(&state), None);
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
        let workspace = root.canonicalize().unwrap();
        let headers = server.join().unwrap();
        assert!(headers.contains(&format!(
            "x-opencode-directory: {}\r\n",
            encode_uri_component(workspace.to_str().unwrap())
        )));
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"hello world");
        fs::remove_dir_all(root.parent().unwrap()).unwrap();
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
}
