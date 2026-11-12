//! Keeps Hex's diagnostic logs (`live.ndjson`, `process.log`) bounded.
//!
//! Every Hex process appends to the same files, so a log is compacted in
//! place rather than renamed: once it passes `MAX_BYTES`, its oldest lines
//! are dropped so about `KEEP_BYTES` remain, cut at a line boundary. Other
//! processes keep appending to the same file, where a rename would leave them
//! writing to one nobody reads. A line another process appends during the few
//! milliseconds of a trim may be lost; that's acceptable for diagnostics.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const KEEP_BYTES: u64 = 5 * 1024 * 1024;
const INTERVAL: Duration = Duration::from_secs(60 * 60);
/// A trim takes well under a second, so an older lock is a dead process's.
const STALE_LOCK: Duration = Duration::from_secs(5 * 60);
const CHUNK: usize = 64 * 1024;
const LOGS: [&str; 2] = ["live.ndjson", "process.log"];
/// Present once the logs from before they kept personal text out have been
/// cleared. Named apart from the first version's `.event-log-cleared`, which
/// cleared `live.ndjson` alone, so those installs clear `process.log` too.
const CLEARED_MARKER: &str = ".old-logs-cleared";

/// Clears logs written before they kept personal text out, once, then trims
/// the logs now and every hour on a background thread.
pub fn start(log_dir: &Path) {
    clear_old_logs(log_dir);
    let log_dir = log_dir.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("log-trim".into())
        .spawn(move || {
            loop {
                trim_all(&log_dir);
                std::thread::sleep(INTERVAL);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start trimming the logs");
    }
}

/// Older builds wrote each dictation's text and page addresses into
/// `live.ndjson`, and meeting titles, capture summaries, and scripts' output
/// into `process.log`. Truncating rather than deleting keeps any Hex still
/// appending to them writing to the files everyone reads.
fn clear_old_logs(log_dir: &Path) {
    let marker = log_dir.join(CLEARED_MARKER);
    if marker.exists() {
        return;
    }
    let cleared = LOGS.iter().try_for_each(|name| {
        match OpenOptions::new().write(true).open(log_dir.join(name)) {
            Ok(file) => file.set_len(0),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    });
    match cleared.and_then(|()| fs::write(&marker, b"")) {
        Ok(()) => {}
        Err(error) => tracing::warn!(%error, "could not clear the old logs"),
    }
}

fn trim_all(log_dir: &Path) {
    let lock = log_dir.join(".trim.lock");
    if !take_lock(&lock) {
        return;
    }
    for name in LOGS {
        if let Err(error) = trim(&log_dir.join(name), MAX_BYTES, KEEP_BYTES) {
            tracing::warn!(log = name, %error, "could not trim the log");
        }
    }
    let _ = fs::remove_file(lock);
}

/// Whether this process may trim now. A lock left by a process that died
/// mid-trim is removed, and the trim waits for the next round.
fn take_lock(lock: &Path) -> bool {
    match OpenOptions::new().write(true).create_new(true).open(lock) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let stale = fs::metadata(lock)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age > STALE_LOCK);
            if stale {
                let _ = fs::remove_file(lock);
            }
            false
        }
        Err(_) => false,
    }
}

/// Drops `path`'s oldest lines so about `keep` bytes remain once it passes
/// `max`. Returns whether it trimmed.
fn trim(path: &Path, max: u64, keep: u64) -> io::Result<bool> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let size = file.metadata()?.len();
    if size <= max {
        return Ok(false);
    }
    let start = next_line_start(&mut file, size - keep)?;
    let mut buffer = vec![0; CHUNK];
    let (mut read_at, mut write_at) = (start, 0);
    loop {
        file.seek(SeekFrom::Start(read_at))?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        file.seek(SeekFrom::Start(write_at))?;
        file.write_all(&buffer[..count])?;
        read_at += count as u64;
        write_at += count as u64;
    }
    file.set_len(write_at)?;
    Ok(true)
}

/// The offset just past the first newline at or after `from`, or the end of
/// the file when there is none.
fn next_line_start(file: &mut File, from: u64) -> io::Result<u64> {
    file.seek(SeekFrom::Start(from))?;
    let mut buffer = vec![0; CHUNK];
    let mut offset = from;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            return Ok(offset);
        }
        if let Some(newline) = buffer[..count].iter().position(|byte| *byte == b'\n') {
            return Ok(offset + newline as u64 + 1);
        }
        offset += count as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hex-log-trim-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn a_log_past_its_cap_keeps_its_newest_whole_lines() {
        let directory = directory("cap");
        let path = directory.join("live.ndjson");
        let lines: String = (0..100).map(|line| format!("line {line:03}\n")).collect();
        fs::write(&path, &lines).unwrap();

        assert!(!trim(&path, 2_000, 300).unwrap());
        assert!(trim(&path, 500, 300).unwrap());

        let kept = fs::read_to_string(&path).unwrap();
        assert!(kept.len() <= 300, "{}", kept.len());
        assert!(kept.starts_with("line "), "{kept}");
        assert!(kept.ends_with("line 099\n"), "{kept}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_appender_keeps_writing_to_the_trimmed_file() {
        let directory = directory("appender");
        let path = directory.join("process.log");
        fs::write(&path, "old\n".repeat(200)).unwrap();
        let mut appender = OpenOptions::new().append(true).open(&path).unwrap();

        assert!(trim(&path, 400, 40).unwrap());
        appender.write_all(b"new\n").unwrap();

        let kept = fs::read_to_string(&path).unwrap();
        assert!(kept.ends_with("old\nnew\n"), "{kept}");
        assert!(kept.len() <= 44, "{}", kept.len());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_old_logs_are_cleared_once() {
        let directory = directory("clear");
        let events = directory.join("live.ndjson");
        let process = directory.join("process.log");
        fs::write(&events, "{\"kind\":\"dictation\",\"text\":\"old words\"}\n").unwrap();
        fs::write(&process, "published meeting transcript title=Offer call\n").unwrap();
        // The first version cleared live.ndjson alone under another marker.
        fs::write(directory.join(".event-log-cleared"), b"").unwrap();

        clear_old_logs(&directory);
        assert_eq!(fs::read(&events).unwrap(), b"");
        assert_eq!(fs::read(&process).unwrap(), b"");

        fs::write(&events, "{\"kind\":\"dictation\",\"text\":\"\"}\n").unwrap();
        fs::write(&process, "listener started\n").unwrap();
        clear_old_logs(&directory);
        assert!(!fs::read(&events).unwrap().is_empty());
        assert!(!fs::read(&process).unwrap().is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn only_one_process_trims_at_a_time() {
        let directory = directory("lock");
        let lock = directory.join(".trim.lock");
        assert!(take_lock(&lock));
        assert!(!take_lock(&lock));
        fs::remove_file(&lock).unwrap();
        assert!(take_lock(&lock));
        fs::remove_dir_all(directory).unwrap();
    }
}
