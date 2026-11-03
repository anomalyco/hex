//! SSH connection reuse and bounded reads. Remote API calls obey the caller's
//! deadline too; in particular the optional pre-send idle check is only 600 ms.

use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};

use super::{CLI_STDERR_LIMIT, SERVER_RESPONSE_LIMIT};

#[derive(Debug)]
pub(super) struct MissingClient;

impl std::fmt::Display for MissingClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Python 3 is unavailable on the agent host")
    }
}

impl std::error::Error for MissingClient {}

pub(super) fn reuse_connection(command: &mut Command) {
    // Windows OpenSSH doesn't support connection multiplexing.
    #[cfg(unix)]
    if crate::env::get().live {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        // Keep below sockaddr_un's 104-byte limit, even on macOS where TMPDIR
        // alone can exceed it. This user owns the directory and its sockets.
        let uid = unsafe { libc::geteuid() };
        let directory = std::path::PathBuf::from(format!("/tmp/hex-router-ssh-{uid}"));
        let _ = std::fs::DirBuilder::new().mode(0o700).create(&directory);
        if let Ok(meta) = std::fs::symlink_metadata(&directory)
            && meta.is_dir()
            && meta.uid() == uid
            && meta.mode() & 0o777 == 0o700
        {
            command.args(["-o", "ControlMaster=auto", "-o", "ControlPersist=60"]);
            command
                .arg("-o")
                .arg(format!("ControlPath={}/%C", directory.display()));
        }
    }
    #[cfg(not(unix))]
    let _ = command;
}

/// Drain both pipes while waiting, keeping their byte limits and one wall-clock
/// deadline. A remote command that never closes a pipe cannot extend the wait.
pub(super) fn bounded_output(mut command: Command, timeout: Duration) -> Result<Output> {
    bounded_input(&mut command, None, timeout)
}

pub(crate) fn bounded_input(
    command: &mut Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Output> {
    let deadline = Instant::now() + timeout;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Could not reach the agent host")?;
    if let Some(input) = input {
        let mut stdin = child.stdin.take().context("No request pipe")?;
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let stdout = child.stdout.take().context("No agent host output")?;
    let stderr = child.stderr.take().context("No agent host stderr")?;
    let (output_tx, output_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| {
            let mut bytes = Vec::new();
            stdout
                .take(SERVER_RESPONSE_LIMIT + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= SERVER_RESPONSE_LIMIT,
                "OpenCode response too large"
            );
            Ok::<_, anyhow::Error>(bytes)
        })();
        let _ = output_tx.send(result);
    });
    let (error_tx, error_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut kept = Vec::new();
        let _ = (&mut stderr).take(CLI_STDERR_LIMIT).read_to_end(&mut kept);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        let _ = error_tx.send(kept);
    });
    let left = || deadline.saturating_duration_since(Instant::now());
    let result = (|| {
        let stdout = output_rx
            .recv_timeout(left())
            .context("Agent host request timed out")??;
        let stderr = error_rx
            .recv_timeout(left())
            .context("Agent host request timed out")?;
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .context("Could not wait for the agent host")?
            {
                break status;
            }
            ensure!(!left().is_zero(), "Agent host request timed out");
            std::thread::park_timeout(left().min(Duration::from_millis(5)));
        };
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })();
    if result.is_err() {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

/// A tiny standard-library HTTP client over the existing SSH connection. The
/// request travels on stdin, not argv; credentials stay on the remote host.
pub(super) fn request(
    host: &str,
    method: &str,
    path: &str,
    data: Option<&serde_json::Value>,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let request = serde_json::to_vec(&serde_json::json!({
        "method": method.to_ascii_uppercase(), "path": path, "body": data,
        "timeout": timeout.as_secs_f64(),
    }))?;
    let mut command = super::ssh(host);
    command.arg(format!(
        "command -v python3 >/dev/null 2>&1 || exit 127; exec python3 -c {}",
        super::shell_quote(include_str!("request.py"))
    ));
    let output = bounded_input(&mut command, Some(request), timeout)?;
    if output.status.code() == Some(127) {
        return Err(MissingClient.into());
    }
    ensure!(
        output.status.success(),
        "Remote HTTP client failed (Python 3 is required): {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Invalid remote HTTP response")?;
    let status = response["status"]
        .as_u64()
        .context("Missing remote HTTP status")? as u16;
    let body = response["body"]
        .as_str()
        .context("Missing remote HTTP body")?
        .as_bytes()
        .to_vec();
    if !(200..300).contains(&status) {
        let error = anyhow::Error::new(super::HttpStatus(status));
        return Err(match super::said(&body) {
            Some(message) => error.context(message),
            None => error,
        });
    }
    Ok(body)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_remote_http_client_posts_inline_files_and_preserves_http_errors() {
        use std::net::TcpListener;
        for status in [200, 409, 401] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let root =
                std::env::temp_dir().join(format!("hex-remote-{}-{status}", std::process::id()));
            std::fs::create_dir_all(root.join("opencode")).unwrap();
            std::fs::write(
                root.join("opencode/service.json"),
                serde_json::json!({
                    "url": format!("http://{address}"), "password": "fixture-only", "pid": 42
                })
                .to_string(),
            )
            .unwrap();
            let server = std::thread::spawn(move || {
                let (mut info, _) = listener.accept().unwrap();
                info.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    info.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                assert!(request.starts_with(b"GET /api/info "));
                let body = r#"{"pid":42}"#;
                write!(
                    info,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                drop(info);
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        break request.len();
                    }
                };
                let headers = String::from_utf8_lossy(&request).into_owned();
                assert!(headers.starts_with("POST /api/session/ses_fixture/prompt "));
                assert!(headers.contains("Authorization: Basic "));
                let length: usize = headers
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                request.resize(header_end + length, 0);
                stream.read_exact(&mut request[header_end..]).unwrap();
                let body: serde_json::Value =
                    serde_json::from_slice(&request[header_end..]).unwrap();
                assert_eq!(body["files"][0]["uri"], "data:image/png;base64,AA==");
                std::thread::sleep(Duration::from_millis(60));
                let body = r#"{"message":"fixture"}"#;
                write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let mut command = Command::new("python3");
            command
                .args(["-c", include_str!("request.py")])
                .env_clear()
                .env("HOME", &root)
                .env("XDG_STATE_HOME", &root);
            let input = serde_json::to_vec(&serde_json::json!({
                "method": "POST", "path": "/api/session/ses_fixture/prompt", "timeout": 3.0,
                "body": {"text":"fixture", "files":[{"uri":"data:image/png;base64,AA=="}]}
            }))
            .unwrap();
            let output = bounded_input(&mut command, Some(input), Duration::from_secs(5)).unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(response["status"], status);
            server.join().unwrap();
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn a_child_that_never_reads_stdin_cannot_extend_the_deadline() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 5"]);
        let start = Instant::now();
        assert!(
            bounded_input(
                &mut command,
                Some(vec![0; 1024 * 1024]),
                Duration::from_millis(80)
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_hung_remote_command_obeys_the_idle_check_deadline() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 5"]);
        let start = Instant::now();
        let error = bounded_output(command, super::super::IDLE_CHECK).unwrap_err();
        eprintln!("METRIC hung_idle_check_ms={}", start.elapsed().as_millis());
        assert!(error.to_string().contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_descendant_holding_the_pipe_cannot_extend_the_deadline() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 5 & exit 0"]);
        let start = Instant::now();
        assert!(bounded_output(command, Duration::from_millis(80)).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn an_oversized_response_is_rejected_before_waiting_for_exit() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "head -c 16777217 /dev/zero; sleep 5"]);
        let error = bounded_output(command, Duration::from_secs(3)).unwrap_err();
        assert!(
            error.to_string().contains("response too large"),
            "{error:#}"
        );
    }
}
