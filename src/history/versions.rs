//! An entry's earlier transcripts (Labs › Keep recordings): Replace after
//! Transcribe again keeps the text it replaces, with the model and forced
//! language that made it, newest first and at most [`MAX_VERSIONS`].
//! Versions live in the entry, so they count toward History's text cap and
//! leave with it.

use serde::{Deserialize, Serialize};

use super::{HistoryEntry, HistoryProcessing, MAX_LABEL_BYTES, MAX_TEXT_BYTES, truncated};

/// How many earlier transcripts an entry keeps.
pub const MAX_VERSIONS: usize = 5;

/// One earlier transcript of an entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HistoryVersion {
    pub raw_text: String,
    pub final_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing: Option<HistoryProcessing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// When this transcript was made.
    pub timestamp_ms: u64,
}

impl HistoryVersion {
    pub(super) fn text_bytes(&self) -> usize {
        self.raw_text.len() + self.final_text.len()
    }
}

/// A transcript that takes an entry's place, such as Transcribe again's.
#[derive(Clone, Debug)]
pub struct Transcript {
    pub text: String,
    pub model: Option<String>,
    /// The language chosen for that run; `None` when the model chose.
    pub language: Option<String>,
}

impl HistoryEntry {
    /// The entry's current transcript as a version.
    fn current(&self) -> HistoryVersion {
        HistoryVersion {
            raw_text: self.raw_text.clone(),
            final_text: self.final_text.clone(),
            processing: self.processing.clone(),
            model: self.model.clone(),
            language: self.language.clone(),
            timestamp_ms: self.text_ms.unwrap_or(self.timestamp_ms),
        }
    }

    fn adopt(&mut self, version: HistoryVersion) {
        self.raw_text = version.raw_text;
        self.final_text = version.final_text;
        self.processing = version.processing;
        self.model = version.model;
        self.language = version.language;
        self.text_ms = (version.timestamp_ms != self.timestamp_ms).then_some(version.timestamp_ms);
    }

    fn push(&mut self, version: HistoryVersion) {
        self.versions.insert(0, version);
        self.versions.truncate(MAX_VERSIONS);
    }

    /// `transcript` becomes the text; the current one becomes the newest
    /// version. No processing shaped it, so the processing record goes.
    pub fn replace(&mut self, transcript: Transcript, now_ms: u64) {
        let text = truncated(&transcript.text, MAX_TEXT_BYTES);
        let label = |label: Option<String>| label.map(|label| truncated(&label, MAX_LABEL_BYTES));
        let previous = self.current();
        self.push(previous);
        self.adopt(HistoryVersion {
            raw_text: text.clone(),
            final_text: text,
            processing: None,
            model: label(transcript.model),
            language: label(transcript.language),
            timestamp_ms: now_ms,
        });
    }

    /// Version `index` becomes the text again; the current one becomes the
    /// newest version. False when there is no such version.
    pub fn restore(&mut self, index: usize) -> bool {
        if index >= self.versions.len() {
            return false;
        }
        let version = self.versions.remove(index);
        let previous = self.current();
        self.push(previous);
        self.adopt(version);
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::history::{HistoryDraft, HistoryKind, HistoryRetention, HistoryStore};

    fn store(name: &str) -> (std::path::PathBuf, HistoryStore) {
        let directory = std::env::temp_dir().join(format!(
            "hex-history-versions-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("history.json");
        let store = HistoryStore::open(path.clone(), HistoryRetention::Week, 0);
        (path, store)
    }

    fn draft(text: &str) -> HistoryDraft {
        HistoryDraft {
            kind: HistoryKind::Dictation,
            raw_text: format!("raw {text}"),
            final_text: text.into(),
            application: None,
            processing: None,
            audio_ms: 900,
            inference_ms: 80,
            total_ms: 1_100,
            recording: None,
            model: Some("Parakeet v3".into()),
        }
    }

    fn again(text: &str, model: &str, language: Option<&str>) -> Transcript {
        Transcript {
            text: text.into(),
            model: Some(model.into()),
            language: language.map(str::to_owned),
        }
    }

    #[test]
    fn replacing_keeps_the_earlier_transcript_with_its_model_newest_first() {
        let (path, mut store) = store("replace");
        let id = store.record(draft("first"), 1_000).unwrap().unwrap();
        assert_eq!(
            store.entry(id).unwrap().model.as_deref(),
            Some("Parakeet v3")
        );
        store
            .replace_text(id, again("second", "Whisper", Some("es")), 2_000)
            .unwrap();
        store
            .replace_text(id, again("third", "Cohere", None), 3_000)
            .unwrap();

        let entry = HistoryStore::open(path, HistoryRetention::Week, 4_000)
            .entry(id)
            .cloned()
            .unwrap();
        assert_eq!(entry.final_text, "third");
        assert_eq!(entry.model.as_deref(), Some("Cohere"));
        assert_eq!(entry.language, None);
        assert_eq!(entry.text_ms, Some(3_000));
        let earlier: Vec<_> = entry
            .versions
            .iter()
            .map(|version| {
                (
                    version.final_text.as_str(),
                    version.model.as_deref(),
                    version.language.as_deref(),
                    version.timestamp_ms,
                )
            })
            .collect();
        assert_eq!(
            earlier,
            [
                ("second", Some("Whisper"), Some("es"), 2_000),
                ("first", Some("Parakeet v3"), None, 1_000),
            ]
        );
        assert_eq!(entry.versions[1].raw_text, "raw first");
    }

    #[test]
    fn switching_back_swaps_the_text_and_keeps_every_version() {
        let (_, mut store) = store("restore");
        let id = store.record(draft("first"), 1_000).unwrap().unwrap();
        store
            .replace_text(id, again("second", "Whisper", Some("es")), 2_000)
            .unwrap();
        assert!(store.restore_version(id, 0).unwrap());
        let entry = store.entry(id).unwrap();
        assert_eq!(entry.final_text, "first");
        assert_eq!(entry.raw_text, "raw first");
        assert_eq!(entry.model.as_deref(), Some("Parakeet v3"));
        assert_eq!(entry.text_ms, None);
        assert_eq!(entry.versions.len(), 1);
        assert_eq!(entry.versions[0].final_text, "second");
        assert_eq!(entry.versions[0].language.as_deref(), Some("es"));
        assert!(!store.restore_version(id, 1).unwrap());
        assert!(!store.restore_version(id + 1, 0).unwrap());
    }

    #[test]
    fn an_entry_keeps_at_most_five_versions_and_clear_removes_them() {
        let (path, mut store) = store("bounded");
        let id = store.record(draft("v0"), 1_000).unwrap().unwrap();
        for n in 1..=MAX_VERSIONS + 2 {
            store
                .replace_text(
                    id,
                    again(&format!("v{n}"), "Whisper", None),
                    1_000 + n as u64,
                )
                .unwrap();
        }
        let entry = store.entry(id).unwrap();
        assert_eq!(entry.final_text, format!("v{}", MAX_VERSIONS + 2));
        let earlier: Vec<_> = entry
            .versions
            .iter()
            .map(|version| version.final_text.clone())
            .collect();
        assert_eq!(earlier, ["v6", "v5", "v4", "v3", "v2"]);

        store.clear().unwrap();
        assert!(
            !std::fs::read_to_string(&path).unwrap().contains("versions"),
            "clearing History removes the versions"
        );
    }

    #[test]
    fn versions_count_toward_the_text_cap() {
        let (_, mut store) = store("cap");
        let id = store.record(draft("short"), 1_000).unwrap().unwrap();
        let before = store.entry(id).unwrap().text_bytes();
        let long = "x".repeat(MAX_TEXT_BYTES + 10);
        store
            .replace_text(id, again(&long, "Whisper", None), 2_000)
            .unwrap();
        let entry = store.entry(id).unwrap();
        assert_eq!(entry.final_text.len(), MAX_TEXT_BYTES);
        assert_eq!(entry.text_bytes(), 2 * MAX_TEXT_BYTES + before);
    }
}
