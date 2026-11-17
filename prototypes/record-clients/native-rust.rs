// Two recordings after one pairing, from any Rust host, including a Rune
// desktop app.
//
// Build it as a small binary or paste `record_twice` into a Rune host's
// runtime code. It talks to Hex's owner-only Unix socket (`record.sock`),
// never to the loopback HTTP API, so Hex identifies the calling process by
// its code signature instead of a name or an Origin. No API token is read.
//
// Pairing asks the person once and starts nothing. The grant it returns opens
// each recording; `record_once` starts one short session under it. Revoking
// the grant (here, or in Hex's window under Labs › Browser access) ends the
// permission; the next run pairs and asks again.
//
// Dependencies: `sha2 = "0.11"` and `serde_json = "1"` (both already in
// Hex's tree); everything else is std.
//
//     cargo new --bin record-once && cargo add sha2 serde_json
//     HEX_RECORD_SOCKET=~/Library/Application\ Support/hex/record.sock \
//         cargo run --release
//
// The same functions run inside a Rune app's host: Rune is a Rust host, and
// these only block their caller on a Unix socket.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
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

/// One recording under `grant`: a fresh session token, start, finish.
pub fn record_once(socket: &Path, grant: &str) -> Result<String, String> {
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
    let mut stream = UnixStream::connect(socket).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(180)))
        .map_err(|error| error.to_string())?;
    let body = body.unwrap_or("");
    let header = header
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .unwrap_or_default();
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

fn json(response: &str) -> Result<serde_json::Value, String> {
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or(response);
    serde_json::from_str(body).map_err(|error| error.to_string())
}

fn random_verifier() -> Result<String, String> {
    // 48 bytes of entropy, base64url, 64 characters (inside RFC 7636's
    // 43–128 range). `/dev/urandom` keeps this example std-only.
    let mut bytes = [0_u8; 48];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| error.to_string())?;
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
    let socket = std::env::var_os("HEX_RECORD_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").expect("HOME"))
                .join("Library/Application Support/hex/record.sock")
        });
    let result = pair(&socket).and_then(|(grant, reusable)| {
        let first = record_once(&socket, &grant)?;
        println!("{first}");
        if !reusable {
            return Ok(());
        }
        let second = record_once(&socket, &grant)?;
        println!("{second}");
        revoke(&socket, &grant)
    });
    if let Err(error) = result {
        eprintln!("no recording: {error}");
    }
}
