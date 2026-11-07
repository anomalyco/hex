//! The capsule's words for what happened, and host errors as a person
//! would say them.
use super::short;
use crate::desktop::i18n::{tr, tr_fill};

/// A written capture, on the clipboard and sent nowhere yet.
pub(crate) fn copied() -> &'static str {
    tr("Copied")
}
pub(super) fn not_running() -> &'static str {
    tr("OpenCode isn't running")
}

pub(super) fn already_sent_to(to: &str) -> String {
    tr_fill("Already sent to {}", &short(to))
}

/// A host error (`{error:#}`, outer context first) as the capsule's reason,
/// in words a person would say, or nothing: the technical cause goes to
/// the log, never beside the message.
pub(super) fn plain_reason(error: &str) -> Option<String> {
    const MAX: usize = 44;
    let line = error.lines().next().unwrap_or_default().trim();
    let lower = line.to_lowercase();
    if lower.contains("could not reach server")
        || lower.contains("isn't running")
        || lower.contains("connection refused")
    {
        return Some(not_running().into());
    }
    if lower.contains("didn't answer in time") || lower.contains("timed out") {
        return Some(tr("OpenCode didn't answer").into());
    }
    let cause = line.rsplit(": ").next().unwrap_or(line).trim();
    let technical = ["/", "\\", "://", "os error", "error:", "{", "0x"]
        .iter()
        .any(|mark| cause.contains(mark));
    if cause.is_empty() || technical || cause.chars().count() > MAX {
        tracing::warn!(error = %error, "capture capsule: reason left off the capsule");
        return None;
    }
    Some(cause.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_errors_read_plainly_or_not_at_all() {
        for (error, plain) in [
            (
                "Screen Recording permission is off for Hex",
                Some("Screen Recording permission is off for Hex"),
            ),
            (
                "Could not reach server at http://127.0.0.1:4096",
                Some("OpenCode isn't running"),
            ),
            ("it isn't running", Some("OpenCode isn't running")),
            ("didn't answer in time", Some("OpenCode didn't answer")),
            (
                "OpenCode said: Unable to read attachment file:///Users/alex/a.png",
                None,
            ),
            ("writing the bundle: Permission denied (os error 13)", None),
        ] {
            assert_eq!(plain_reason(error).as_deref(), plain, "{error}");
        }
    }
}
