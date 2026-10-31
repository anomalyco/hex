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
    let document = live_discovery()?;
    let address = SocketAddr::from(([127, 0, 0, 1], document.port));
    let mut stream = TcpStream::connect_timeout(&address, CONNECTION_TIMEOUT)
        .wrap_err("could not connect to Hex's local API")?;
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(CONNECTION_TIMEOUT))?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        document.token,
        body.len(),
    )?;
    stream.write_all(body)?;
    stream.flush()?;

    let mut response = Vec::new();
    (&mut stream)
        .take(max_response as u64 + 1)
        .read_to_end(&mut response)
        .wrap_err("Hex didn't answer")?;
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

pub(super) fn error_code(status: u16, body: &[u8]) -> String {
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
