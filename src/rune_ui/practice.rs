//! What the tour's practice dictation hears: the listener's dictation
//! outcomes, read from its event log (`live.ndjson`). The listener runs in
//! the window's process on every OS, so each host reads the same log.

use std::path::Path;

use crate::desktop::events::{DictationPhase, EventReader, VoiceEvent};
use crate::desktop::onboarding::{Input, Observation};

/// The listener's dictation outcomes logged since the practice step began.
pub(super) struct PracticeFeed {
    reader: EventReader,
    since_ms: u64,
}

impl PracticeFeed {
    pub(super) fn new(event_path: &Path) -> Self {
        Self {
            reader: EventReader::open(event_path),
            since_ms: crate::desktop::events::now_ms(),
        }
    }

    pub(super) fn observations(&mut self) -> Vec<Observation> {
        if let Err(error) = self.reader.refresh() {
            tracing::warn!(%error, "could not read dictation events for practice");
            return Vec::new();
        }
        let since_ms = self.since_ms;
        self.reader
            .drain()
            .filter_map(|event| match event {
                VoiceEvent::Dictation {
                    timestamp_ms,
                    phase,
                    ..
                } if timestamp_ms >= since_ms => Some(observation(&phase)?),
                _ => None,
            })
            .collect()
    }
}

/// The practice outcome one dictation phase reports, or none for the phases
/// the tour doesn't hear.
fn observation(phase: &DictationPhase) -> Option<Observation> {
    match phase {
        DictationPhase::Started => Some(Observation::Started),
        DictationPhase::Transcribing => Some(Observation::Transcribing),
        DictationPhase::Discarded => Some(Observation::Discarded),
        DictationPhase::Cancelled => Some(Observation::Cancelled),
        // A copy is a finished tour dictation too: the words are on the
        // clipboard even when the practice field couldn't take a paste.
        DictationPhase::Pasted | DictationPhase::Copied | DictationPhase::Edited => {
            Some(Observation::Pasted)
        }
        DictationPhase::Failed(_) => Some(Observation::Failed),
        _ => None,
    }
}

/// The tour's inputs from the listener on Linux and Windows, whose window
/// hosts have no native host: practice outcomes from the process's event log
/// while `practicing`.
#[derive(Default)]
pub(super) struct LocalPractice {
    feed: Option<PracticeFeed>,
}

impl LocalPractice {
    pub(super) fn tour_inputs(&mut self, practicing: bool) -> Vec<Input> {
        if !practicing {
            self.feed = None;
            return Vec::new();
        }
        if self.feed.is_none() {
            self.feed = crate::app_paths::logs_dir()
                .ok()
                .map(|logs| PracticeFeed::new(&logs.join("live.ndjson")));
        }
        self.feed.as_mut().map_or_else(Vec::new, |feed| {
            feed.observations()
                .into_iter()
                .map(Input::Dictation)
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copied_dictation_completes_the_practice_like_a_paste() {
        // The practice field can't receive text when the shell is in front,
        // but the listener copied the words, so the step still counts as done.
        assert_eq!(
            observation(&DictationPhase::Copied),
            Some(Observation::Pasted)
        );
        assert_eq!(
            observation(&DictationPhase::Pasted),
            Some(Observation::Pasted)
        );
        assert_eq!(
            observation(&DictationPhase::Transcribing),
            Some(Observation::Transcribing)
        );
        assert_eq!(observation(&DictationPhase::Logged), None);
    }

    #[test]
    fn practice_hears_dictations_logged_after_it_began() {
        let path = std::env::temp_dir().join(format!("hex-practice-{}.ndjson", std::process::id()));
        let dictation = |timestamp_ms, phase| {
            serde_json::to_string(&VoiceEvent::Dictation {
                timestamp_ms,
                phase,
                text: String::new(),
                processing: None,
                reason: None,
            })
            .unwrap()
                + "\n"
        };
        std::fs::write(&path, dictation(1, DictationPhase::Started)).unwrap();
        let mut practice = PracticeFeed::new(&path);
        let now = practice.since_ms;
        let log = [
            dictation(now, DictationPhase::Started),
            dictation(now + 1, DictationPhase::Transcribing),
            dictation(now + 2, DictationPhase::Logged),
            dictation(now + 3, DictationPhase::Edited),
        ];
        std::fs::write(&path, dictation(1, DictationPhase::Started) + &log.concat()).unwrap();
        assert_eq!(
            practice.observations(),
            [
                Observation::Started,
                Observation::Transcribing,
                Observation::Pasted
            ]
        );
        assert!(
            practice.observations().is_empty(),
            "each event is heard once"
        );
        std::fs::remove_file(path).unwrap();
    }
}
