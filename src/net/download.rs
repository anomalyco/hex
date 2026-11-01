//! Resumable downloads into a `.partial` file: one ranged request per piece,
//! so a stalled connection fails within a bounded time instead of hanging,
//! and every piece resumes where the file ends. HTTPS only.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use color_eyre::eyre::{Result, bail, eyre};

/// Bytes asked for per request.
const PIECE: u64 = 8 << 20;
/// How long one piece may take: a slower link than about 27 KB/s fails.
const PIECE_TIME: Duration = Duration::from_secs(300);
const ATTEMPTS: u32 = 3;

/// One file to fetch.
pub struct Download<'a> {
    pub url: &'a str,
    pub user_agent: &'a str,
    /// The file's size, when known in advance; the download stops there.
    pub size: Option<u64>,
    pub canceled: Option<&'a AtomicBool>,
}

/// Fetches `download` into `partial`, appending to what it already holds.
/// `progress` hears the file's length as it grows. A failure says why in a
/// person's words (`net::unreachable`).
pub fn download(
    download: &Download<'_>,
    partial: &Path,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    let canceled = || {
        download
            .canceled
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    };
    // Hugging Face redirects every request to a signed CDN URL; later pieces
    // go there directly, and back through the original when it expires.
    let mut location = download.url.to_owned();
    let mut failures = 0;
    loop {
        let have = std::fs::metadata(partial).map_or(0, |metadata| metadata.len());
        progress(have);
        if download.size.is_some_and(|size| have >= size) {
            return Ok(());
        }
        if canceled() {
            bail!("download canceled");
        }
        match piece(download, &location, partial, have, &canceled, &mut progress) {
            Ok(Piece::More(final_url)) => {
                location = final_url;
                failures = 0;
            }
            Ok(Piece::Done) => return Ok(()),
            Err(error) if canceled() => return Err(error),
            Err(error) => {
                failures += 1;
                tracing::warn!(%error, failures, host = %super::host(download.url), "download piece failed");
                if failures >= ATTEMPTS {
                    return Err(error);
                }
                // A signed CDN link may have expired: ask the original again.
                location = download.url.to_owned();
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

enum Piece {
    /// More to fetch, from this URL.
    More(String),
    /// The server sent the whole rest of the file.
    Done,
}

fn piece(
    download: &Download<'_>,
    url: &str,
    partial: &Path,
    have: u64,
    canceled: &impl Fn() -> bool,
    progress: &mut impl FnMut(u64),
) -> Result<Piece> {
    let end = download
        .size
        .map_or(have + PIECE, |size| size.min(have + PIECE))
        .saturating_sub(1);
    let agent = super::agent(url, |config| {
        config
            .https_only(true)
            .max_redirects(10)
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .timeout_recv_body(Some(PIECE_TIME))
    });
    let response = agent
        .get(url)
        .header("User-Agent", download.user_agent)
        .header("Range", format!("bytes={have}-{end}"))
        .call();
    let mut response = match response {
        Ok(response) => response,
        // A range past the end: the file is already whole.
        Err(ureq::Error::StatusCode(416)) if have > 0 && download.size.is_none() => {
            return Ok(Piece::Done);
        }
        Err(error) => return Err(eyre!(super::unreachable(download.url, &error))),
    };
    let final_url = ureq::ResponseExt::get_uri(&response).to_string();
    let status = response.status().as_u16();
    let mut file = match status {
        206 => {
            let start = response
                .headers()
                .get("content-range")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("bytes "))
                .and_then(|value| value.split_once('-'))
                .and_then(|(start, _)| start.parse::<u64>().ok());
            if start != Some(have) {
                bail!("the server sent a different part of the file than asked for");
            }
            OpenOptions::new().append(true).create(true).open(partial)?
        }
        // No ranges: the whole file, from the start.
        200 => OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(partial)?,
        status => bail!(
            "{}",
            super::unreachable(download.url, &ureq::Error::StatusCode(status))
        ),
    };
    let whole = status == 200;
    let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
    let mut buffer = vec![0; 64 * 1024];
    let mut written = if whole { 0 } else { have };
    loop {
        if canceled() {
            bail!("download canceled");
        }
        let read = reader.read(&mut buffer).map_err(|error| {
            eyre!(
                "{}",
                super::unreachable(download.url, &ureq::Error::Io(error))
            )
        })?;
        if read == 0 {
            break;
        }
        if download
            .size
            .is_some_and(|size| written + read as u64 > size)
        {
            bail!("the server sent more than the file's size");
        }
        file.write_all(&buffer[..read])?;
        written += read as u64;
        progress(written);
    }
    file.flush()?;
    if whole || download.size.is_some_and(|size| written >= size) {
        return Ok(Piece::Done);
    }
    // A short piece ends the file when its size isn't known.
    if download.size.is_none() && written < end + 1 {
        return Ok(Piece::Done);
    }
    Ok(Piece::More(final_url))
}
