//! One line of progress on stderr, only when stderr is a terminal, cleared
//! before the result prints so a terminal ends with the result alone.

use std::io::{IsTerminal as _, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};

static SHOWN: AtomicBool = AtomicBool::new(false);

/// Replaces the progress line with `text`.
pub(super) fn show(text: &str) {
    let mut stderr = std::io::stderr();
    if !stderr.is_terminal() {
        return;
    }
    let _ = write!(stderr, "\r\x1b[2K{text}");
    let _ = stderr.flush();
    SHOWN.store(true, Ordering::Release);
}

/// Removes the progress line, if one is showing.
pub(super) fn clear() {
    if SHOWN.swap(false, Ordering::AcqRel) {
        let mut stderr = std::io::stderr();
        let _ = write!(stderr, "\r\x1b[2K");
        let _ = stderr.flush();
    }
}

/// Bytes as whole megabytes.
pub(super) fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes / 1_000_000)
}
