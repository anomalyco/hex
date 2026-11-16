//! Labs › Stop phrase: a spoken phrase at the end of a hands-free dictation
//! finishes it as if the shortcut were pressed again, and the phrase never
//! reaches the pasted text.
//!
//! Matching reuses [`super::voice_prompt::words`], so it is whole-word and
//! case- and punctuation-insensitive the same way a voice prompt's is:
//! "Stop dictation." matches, while "stops dictation" and "stop dictations"
//! don't. A hyphen joins a word, so "stop-dictation" is one word and doesn't
//! match the two-word phrase.

use super::voice_prompt::{trim_joining_end, words};

/// The phrase Labs › Stop phrase ends a hands-free dictation with.
pub const DEFAULT_PHRASE: &str = "stop dictation";

/// Where the phrase starts in `text`, when its last words are the phrase's.
fn trailing_start(text: &str, phrase: &str) -> Option<usize> {
    let phrase: Vec<String> = words(phrase).into_iter().map(|word| word.key).collect();
    if phrase.is_empty() {
        return None;
    }
    let text = words(text);
    if text.len() < phrase.len() {
        return None;
    }
    let start = text.len() - phrase.len();
    text[start..]
        .iter()
        .zip(&phrase)
        .all(|(word, phrase)| word.key == *phrase)
        .then(|| text[start].start)
}

/// Whether the last words of `text` are exactly `phrase`'s, whole words and
/// ignoring case and punctuation.
pub(crate) fn ends_with_phrase(text: &str, phrase: &str) -> bool {
    trailing_start(text, phrase).is_some()
}

/// `text` without its trailing `phrase`, or `None` when it doesn't end with
/// it. Whitespace and the joins a voice prompt drops next to its phrase
/// (`, ; :` and en/em dashes) leave with it, while a sentence's own final
/// `.?!` stays: "we ship friday. stop dictation." -> "we ship friday.".
/// A capture that is only the phrase leaves an empty string.
pub(crate) fn strip_trailing(text: &str, phrase: &str) -> Option<String> {
    let start = trailing_start(text, phrase)?;
    Some(trim_joining_end(&text[..start]).to_owned())
}

/// A finished transcript from a capture the phrase ended: the phrase itself
/// never reaches the text. A transcript that doesn't end with it stays as it
/// is, and so does one whose capture the phrase didn't finish (`None`).
/// A capture that is only the phrase reads as an empty transcript.
pub(crate) fn strip_finished(text: String, phrase: Option<&str>) -> String {
    match phrase.and_then(|phrase| strip_trailing(&text, phrase)) {
        Some(stripped) => stripped,
        None => text,
    }
}

/// One capture's stop-phrase detector. The phrase fires on the observation
/// that commits it (a stable partial ends with it), or after two observations
/// in a row whose stable and tentative words together end with it, so a
/// flickering tentative word can't finish a capture; it never fires twice.
///
/// The engine publishes only changes (`Partials::publish` drops a partial
/// identical to the last), so a commit that the speaker then leaves quiet is
/// often the capture's last observation: waiting for it twice would never
/// fire.
#[derive(Debug, Default)]
pub(crate) struct Detector {
    /// The previous observation ended with the phrase.
    ended: bool,
    /// It has fired; this capture is finished.
    fired: bool,
}

impl Detector {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reads one live partial: `stable` is what the model committed and
    /// `tentative` is its volatile hypothesis after it. Returns true once,
    /// when the committed words end with `phrase`, or when two consecutive
    /// observations' committed and tentative words do.
    pub(crate) fn observe(&mut self, stable: &str, tentative: &str, phrase: &str) -> bool {
        if self.fired {
            return false;
        }
        let committed = ends_with_phrase(stable, phrase);
        let ended = ends_with_phrase(&format!("{stable} {tentative}"), phrase);
        let fire = committed || (ended && self.ended);
        self.ended = ended;
        if fire {
            self.fired = true;
        }
        fire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phrase_matches_the_last_words_in_any_case() {
        assert!(ends_with_phrase(
            "we ship friday. Stop Dictation.",
            DEFAULT_PHRASE
        ));
        assert!(ends_with_phrase("stop dictation", DEFAULT_PHRASE));
        assert!(ends_with_phrase(
            "we ship friday, stop dictation!",
            DEFAULT_PHRASE
        ));
    }

    #[test]
    fn words_that_only_contain_the_phrase_never_match() {
        for text in [
            "stop dictation please",
            "stops dictation",
            "stop dictations",
            "unstoppable dictation",
            "stop-dictation",
            "",
            "dictation",
        ] {
            assert!(!ends_with_phrase(text, DEFAULT_PHRASE), "{text}");
        }
        assert!(!ends_with_phrase("stop dictation", ""));
        assert!(!ends_with_phrase("", DEFAULT_PHRASE));
    }

    #[test]
    fn stripping_keeps_a_sentences_final_punctuation_and_drops_the_joins() {
        assert_eq!(
            strip_trailing("we ship friday. stop dictation.", DEFAULT_PHRASE).as_deref(),
            Some("we ship friday.")
        );
        assert_eq!(
            strip_trailing("we ship friday, stop dictation.", DEFAULT_PHRASE).as_deref(),
            Some("we ship friday")
        );
        assert_eq!(
            strip_trailing("we ship friday; stop dictation", DEFAULT_PHRASE).as_deref(),
            Some("we ship friday")
        );
        assert_eq!(
            strip_trailing("we ship friday: stop dictation", DEFAULT_PHRASE).as_deref(),
            Some("we ship friday")
        );
        assert_eq!(
            strip_trailing("we ship friday — stop dictation", DEFAULT_PHRASE).as_deref(),
            Some("we ship friday")
        );
        assert_eq!(
            strip_trailing("we ship friday – stop dictation", DEFAULT_PHRASE).as_deref(),
            Some("we ship friday")
        );
        assert_eq!(
            strip_trailing("we ship friday stop dictation", DEFAULT_PHRASE).as_deref(),
            Some("we ship friday")
        );
    }

    #[test]
    fn a_capture_that_is_only_the_phrase_strips_to_nothing() {
        for text in ["Stop dictation.", "stop dictation", "  stop dictation!  "] {
            assert_eq!(
                strip_trailing(text, DEFAULT_PHRASE).as_deref(),
                Some(""),
                "{text}"
            );
        }
    }

    #[test]
    fn stripping_leaves_a_dictation_without_the_phrase_alone() {
        for text in [
            "we ship friday.",
            "stop dictation please",
            "stops dictation",
            "",
        ] {
            assert_eq!(strip_trailing(text, DEFAULT_PHRASE), None, "{text}");
        }
        assert_eq!(strip_trailing("stop dictation", ""), None);
    }

    #[test]
    fn a_finished_job_strips_its_phrase_and_leaves_every_other_transcript_alone() {
        assert_eq!(
            strip_finished(
                "we ship friday. stop dictation.".into(),
                Some(DEFAULT_PHRASE)
            ),
            "we ship friday."
        );
        assert_eq!(
            strip_finished("we ship friday.".into(), Some(DEFAULT_PHRASE)),
            "we ship friday.",
            "a transcript that doesn't end with the phrase is unchanged"
        );
        assert_eq!(
            strip_finished("stop dictation".into(), Some(DEFAULT_PHRASE)),
            "",
            "a capture that is only the phrase reads as empty"
        );
        assert_eq!(
            strip_finished("stop dictation".into(), None),
            "stop dictation",
            "a capture the phrase didn't finish keeps its words"
        );
        assert_eq!(
            strip_finished("".into(), Some(DEFAULT_PHRASE)),
            "",
            "an empty transcript stays empty"
        );
    }

    #[test]
    fn a_committed_phrase_fires_the_detector_at_once() {
        let mut detector = Detector::new();
        assert!(
            detector.observe("we ship friday. stop dictation.", "", DEFAULT_PHRASE),
            "the model committed the phrase"
        );
        assert!(
            !detector.observe("we ship friday. stop dictation.", "", DEFAULT_PHRASE),
            "a capture finishes once"
        );
    }

    #[test]
    fn a_tentative_phrase_fires_once_after_two_observations_in_a_row() {
        let mut detector = Detector::new();
        assert!(!detector.observe("we ship friday. stop", "dictation", DEFAULT_PHRASE));
        assert!(
            detector.observe("we ship friday. stop", "dictation.", DEFAULT_PHRASE),
            "a second partial that still ends with the phrase fires"
        );
        assert!(
            !detector.observe("we ship friday. stop", "dictation.", DEFAULT_PHRASE),
            "a capture finishes once"
        );
    }

    #[test]
    fn a_flickering_tentative_word_never_fires_the_detector() {
        let mut detector = Detector::new();
        assert!(!detector.observe("we ship friday.", "stop dictation", DEFAULT_PHRASE));
        assert!(!detector.observe("we ship friday.", "stop dictation please", DEFAULT_PHRASE));
        assert!(!detector.observe("we ship friday.", "stop dictation", DEFAULT_PHRASE));
        assert!(!detector.observe("we ship friday.", "stops dictation", DEFAULT_PHRASE));
    }

    #[test]
    fn an_ordinary_dictation_never_fires_the_detector() {
        let mut detector = Detector::new();
        for text in ["we ship", "we ship friday", "we ship friday."] {
            assert!(!detector.observe(text, "", DEFAULT_PHRASE), "{text}");
        }
    }
}
