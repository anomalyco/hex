//! The apps and websites Hex has seen a call in, newest first, for
//! Meetings › Settings › Calls. The meeting watcher notes each call it
//! detects; the window only reads. Kept in the data folder as
//! `seen-calls.json`, owner-only and bounded, beside settings rather than in
//! them, so it has no `hex settings` key.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::detection::MeetingCandidate;

const FILE: &str = "seen-calls.json";
/// The most providers kept; the longest unseen go first.
const LIMIT: usize = 64;
/// A provider seen again within this long is not written again, since the
/// list only shows the day.
const REWRITE_AFTER_MS: i64 = 60 * 60 * 1000;

/// One app or website a call was seen in.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SeenCall {
    /// The provider key its answer is saved under.
    pub key: String,
    pub label: String,
    /// The app's bundle ID (macOS) or executable, for its icon; none for a
    /// call on a website.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    /// Last seen, in milliseconds since the Unix epoch.
    pub last_seen_ms: i64,
}

impl SeenCall {
    /// The call's provider, or None when Hex can't name it.
    pub fn from_candidate(candidate: &MeetingCandidate, now_ms: i64) -> Option<Self> {
        let provider = candidate.provider()?;
        Some(Self {
            key: provider.key,
            label: provider.label,
            application: (!candidate.on_web())
                .then(|| candidate.application.clone())
                .flatten(),
            last_seen_ms: now_ms,
        })
    }
}

pub fn path() -> color_eyre::Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join(FILE))
}

/// The seen calls, newest first; empty when there are none or the file
/// can't be read.
pub fn load() -> Vec<SeenCall> {
    path().map(|path| load_from(&path)).unwrap_or_default()
}

pub fn load_from(path: &Path) -> Vec<SeenCall> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let mut calls: Vec<SeenCall> = serde_json::from_slice(&bytes).unwrap_or_default();
    calls.sort_by_key(|c| std::cmp::Reverse(c.last_seen_ms));
    calls.truncate(LIMIT);
    calls
}

/// Records `call` in `calls`, newest first. Returns whether the list changed
/// enough to write: a new provider, a new name or app, or a provider last
/// written over an hour ago.
pub fn note(calls: &mut Vec<SeenCall>, call: SeenCall) -> bool {
    let position = calls.iter().position(|seen| seen.key == call.key);
    let mut entry = match position {
        Some(index) => {
            let seen = &calls[index];
            let stale = call.last_seen_ms - seen.last_seen_ms >= REWRITE_AFTER_MS;
            let renamed = seen.label != call.label;
            let app = call.application.is_some() && seen.application != call.application;
            if !(stale || renamed || app) {
                return false;
            }
            calls.remove(index)
        }
        None => SeenCall::default(),
    };
    entry.key = call.key;
    entry.label = call.label;
    // A provider seen in its app keeps that app's icon when later seen on
    // its website.
    if call.application.is_some() {
        entry.application = call.application;
    }
    entry.last_seen_ms = entry.last_seen_ms.max(call.last_seen_ms);
    calls.insert(0, entry);
    calls.truncate(LIMIT);
    true
}

pub fn save_to(path: &Path, calls: &[SeenCall]) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(calls).map_err(std::io::Error::other)?;
    crate::durable::write_private(path, &json)
}

/// The watcher's copy of the list, read once and written on each note that
/// changes it.
#[derive(Default)]
pub(crate) struct SeenCalls {
    calls: Option<Vec<SeenCall>>,
    path: Option<PathBuf>,
}

impl SeenCalls {
    pub(crate) fn note(&mut self, candidate: &MeetingCandidate, now_ms: i64) {
        let Some(call) = SeenCall::from_candidate(candidate, now_ms) else {
            return;
        };
        if self.path.is_none() {
            match path() {
                Ok(path) => self.path = Some(path),
                Err(error) => {
                    tracing::warn!(%error, "no data folder for seen calls");
                    return;
                }
            }
        }
        let path = self.path.as_deref().expect("set above");
        let calls = self.calls.get_or_insert_with(|| load_from(path));
        if note(calls, call)
            && let Err(error) = save_to(path, calls)
        {
            tracing::warn!(%error, "couldn't save seen calls");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(key: &str, at: i64) -> SeenCall {
        SeenCall {
            key: key.into(),
            label: key.into(),
            application: None,
            last_seen_ms: at,
        }
    }

    #[test]
    fn a_call_seen_again_moves_to_the_top_once_an_hour() {
        let mut calls = vec![seen("a", 0)];
        assert!(note(&mut calls, seen("b", 10)));
        assert_eq!(calls[0].key, "b");
        // Within the hour: nothing to write.
        assert!(!note(&mut calls, seen("a", 60_000)));
        assert_eq!(calls[0].key, "b");
        assert!(note(&mut calls, seen("a", REWRITE_AFTER_MS)));
        assert_eq!(calls[0].key, "a");
        assert_eq!(calls[0].last_seen_ms, REWRITE_AFTER_MS);
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn the_list_is_bounded_and_keeps_an_app_s_icon() {
        let mut calls = Vec::new();
        for index in 0..(LIMIT as i64 + 5) {
            note(&mut calls, seen(&format!("k{index}"), index));
        }
        assert_eq!(calls.len(), LIMIT);
        assert_eq!(calls[0].key, format!("k{}", LIMIT + 4));

        let mut calls = vec![SeenCall {
            application: Some("us.zoom.xos".into()),
            ..seen("zoom", 0)
        }];
        note(&mut calls, seen("zoom", REWRITE_AFTER_MS));
        assert_eq!(calls[0].application.as_deref(), Some("us.zoom.xos"));
    }

    #[test]
    fn the_file_is_owner_only_and_reads_back_newest_first() {
        let folder = std::env::temp_dir().join(format!("hex-seen-calls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(FILE);
        save_to(&path, &[seen("old", 1), seen("new", 2)]).unwrap();
        let calls = load_from(&path);
        assert_eq!(calls[0].key, "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(load_from(&folder.join("missing.json")).is_empty());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
