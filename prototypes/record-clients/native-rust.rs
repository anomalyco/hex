// Two recordings after one pairing, from any Rust host, including a Rune
// desktop app.
//
// Build it as a small binary or paste `record_twice` into a Rune host's
// runtime code. It talks to Hex's owner-only channel (`record.sock` on macOS
// and Linux, the named pipe `record.pipe` names on Windows), never to the
// loopback HTTP API, so Hex identifies the calling process itself (its code
// signature, sandbox app id, or executable) instead of a name or an Origin.
// No API token is read.
//
// Pairing asks the person once and starts nothing. The grant it returns opens
// each recording; `record_once` starts one session under it, which runs for
// as long as this process checks in (`POST /record/heartbeat`) and stops
// itself within one lease (10 s) after the check-ins stop. Revoking
// the grant (here, or in Hex's window under Labs › Connected apps) ends the
// permission; the next run pairs and asks again.
//
// Dependencies: `sha2 = "0.11"`, `serde_json = "1"` and `getrandom = "0.4"`
// (all already in Hex's tree); everything else is std.
//
//     cargo new --bin record-once && cargo add sha2 serde_json getrandom
//     cargo run --release            # Hex's default channel
//     HEX_RECORD_SOCKET=/path/record.sock cargo run --release
//
// The same functions run inside a Rune app's host: Rune is a Rust host, and
// these only block their caller on a socket or pipe.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// Pairs once: asks Hex's window for a revocable grant and exchanges the
/// PKCE code for it. No recording starts here.
pub fn pair(socket: &Path) -> Result<(String, bool), String> {
    let verifier = random_verifier()?;
    let challenge = base64url(&Sha256::digest(verifier.as_bytes()));

    let paired = call(
        socket,
        "POST",
        "/record/pair",
        Some(&format!(
            r#"{{"codeChallenge":"{challenge}","codeChallengeMethod":"S256"}}"#
        )),
        None,
    )?;
    // 403 consent-denied means the person pressed Deny (or let it time out).
    if !paired.starts_with("HTTP/1.1 200") {
        return Err(format!("Hex did not allow pairing: {paired}"));
    }
    let paired = json(&paired)?;
    let code = paired
        .get("code")
        .and_then(|code| code.as_str())
        .ok_or("Hex did not allow pairing")?;

    let exchanged = call(
        socket,
        "POST",
        "/record/token",
        Some(&format!(
            r#"{{"code":"{code}","codeVerifier":"{verifier}"}}"#
        )),
        None,
    )?;
    let exchanged = json(&exchanged)?;
    exchanged
        .get("grantToken")
        .and_then(|token| token.as_str())
        .map(|token| (token.to_owned(), exchanged["duration"] == "always"))
        .ok_or_else(|| "Hex refused the grant".to_owned())
}

/// One recording under `grant`: a fresh session token, start, check in until
/// `stop` fires (or is dropped), then finish.
pub fn record_once(socket: &Path, grant: &str, stop: &Receiver<()>) -> Result<String, String> {
    let started = call(
        socket,
        "POST",
        "/record/start",
        None,
        Some(("X-Hex-Record-Grant", grant)),
    )?;
    if !started.starts_with("HTTP/1.1 201") {
        return Err(format!("Hex could not start recording: {started}"));
    }
    let session = json(&started)?;
    let token = session
        .get("sessionToken")
        .and_then(|token| token.as_str())
        .ok_or("Hex returned no session token")?;
    let lease = Duration::from_millis(session["leaseMs"].as_u64().unwrap_or(10_000));

    // Check in well inside the lease while the recording runs. A refused
    // check-in means it already ended: stopped or cancelled in Hex, or the
    // grant was revoked. A stopped recording's transcript is still finishable.
    loop {
        match stop.recv_timeout(lease / 3) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
        }
        let beat = call(
            socket,
            "POST",
            "/record/heartbeat",
            None,
            Some(("X-Hex-Record-Token", token)),
        )?;
        if !beat.starts_with("HTTP/1.1 204") {
            eprintln!("recording ended in Hex: {beat}");
            break;
        }
    }

    // Finish (keep the transcript) or `/record/cancel` (discard it).
    let finished = call(
        socket,
        "POST",
        "/record/finish",
        None,
        Some(("X-Hex-Record-Token", token)),
    )?;
    let finished = json(&finished)?;
    finished
        .get("transcript")
        .and_then(|transcript| transcript.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Hex returned no transcript".to_owned())
}

/// Gives the grant back, so nothing can start another session under it.
pub fn revoke(socket: &Path, grant: &str) -> Result<(), String> {
    let revoked = call(
        socket,
        "POST",
        "/record/revoke",
        None,
        Some(("X-Hex-Record-Grant", grant)),
    )?;
    if revoked.starts_with("HTTP/1.1 204") {
        Ok(())
    } else {
        Err(format!("Hex did not revoke the grant: {revoked}"))
    }
}

fn call(
    socket: &Path,
    method: &str,
    path: &str,
    body: Option<&str>,
    header: Option<(&str, &str)>,
) -> Result<String, String> {
    let mut stream = connect(socket)?;
    let body = body.unwrap_or("");
    let mut header = header
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .unwrap_or_default();
    if !body.is_empty() {
        header.push_str("Content-Type: application/json\r\n");
    }
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: hex\r\n{header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    Ok(response)
}

#[cfg(unix)]
fn connect(socket: &Path) -> Result<std::os::unix::net::UnixStream, String> {
    let stream = std::os::unix::net::UnixStream::connect(socket).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(180)))
        .map_err(|e| e.to_string())?;
    Ok(stream)
}

#[cfg(windows)]
fn connect(pipe: &Path) -> Result<std::fs::File, String> {
    // Hex keeps one pipe instance waiting; between two clients it is
    // briefly busy (ERROR_PIPE_BUSY).
    for _ in 0..50 {
        match std::fs::OpenOptions::new().read(true).write(true).open(pipe) {
            Err(error) if error.raw_os_error() == Some(231) => {
                std::thread::sleep(Duration::from_millis(20));
            }
            opened => return opened.map_err(|e| e.to_string()),
        }
    }
    Err("Hex's pipe stayed busy".into())
}

/// Hex's channel in its data folder: the socket, or on Windows the pipe
/// `record.pipe` names.
fn default_channel() -> Result<PathBuf, String> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let folder = match env("HEX_APPLICATION_SUPPORT_DIR") {
        Some(folder) => folder,
        None if cfg!(target_os = "macos") => {
            env("HOME").ok_or("no HOME")?.join("Library/Application Support/hex")
        }
        None if cfg!(windows) => env("APPDATA").ok_or("no APPDATA")?.join("hex"),
        None => env("XDG_DATA_HOME")
            .filter(|dir| dir.is_absolute())
            .or_else(|| env("HOME").map(|home| home.join(".local/share")))
            .ok_or("no HOME")?
            .join("hex"),
    };
    if cfg!(windows) {
        let named = std::fs::read_to_string(folder.join("record.pipe"))
            .map_err(|error| format!("Hex is not running: {error}"))?;
        Ok(PathBuf::from(named.trim()))
    } else {
        Ok(folder.join("record.sock"))
    }
}

fn json(response: &str) -> Result<serde_json::Value, String> {
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or(response);
    serde_json::from_str(body).map_err(|error| error.to_string())
}

fn random_verifier() -> Result<String, String> {
    // 48 bytes of entropy, base64url, 64 characters (inside RFC 7636's
    // 43–128 range).
    let mut bytes = [0_u8; 48];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(base64url(&bytes))
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

fn main() {
    let socket = match std::env::var_os("HEX_RECORD_SOCKET").map(PathBuf::from) {
        Some(socket) => socket,
        None => match default_channel() {
            Ok(socket) => socket,
            Err(error) => return eprintln!("no recording: {error}"),
        },
    };
    // Each demo recording runs five seconds; a real app sends on `stop` when
    // its UI says so.
    let five_seconds = || {
        let (stop, stopped) = mpsc::channel();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            let _ = stop.send(());
        });
        stopped
    };
    let result = pair(&socket).and_then(|(grant, reusable)| {
        let first = record_once(&socket, &grant, &five_seconds())?;
        println!("{first}");
        if !reusable {
            return Ok(());
        }
        let second = record_once(&socket, &grant, &five_seconds())?;
        println!("{second}");
        revoke(&socket, &grant)
    });
    if let Err(error) = result {
        eprintln!("no recording: {error}");
    }
}
