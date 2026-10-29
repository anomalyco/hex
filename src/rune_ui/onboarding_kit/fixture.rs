//! Stand-ins for what the running app reports, in the shapes it reports
//! them: model bytes from `transcription_preparation`, the microphone's
//! `Meter { average, peak }`, and a recognizer's partial transcripts.
//! Every effect the stories show reads only these.

use crate::transcription_models::{TranscriptionModelId, definition};

/// The language picked before the download starts. English gets the
/// streaming English model; anything else gets the multilingual one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Language {
    English,
    Other,
}

impl Language {
    pub(crate) fn model(self) -> TranscriptionModelId {
        match self {
            Self::English => TranscriptionModelId::ParakeetUnifiedEnglish,
            Self::Other => TranscriptionModelId::ParakeetV3,
        }
    }

    pub(crate) fn bytes(self) -> u64 {
        definition(self.model()).download_bytes().unwrap_or(0)
    }

    /// Whether the downloaded model shows words while you speak (it
    /// advertises streaming), rather than all at once on release.
    pub(crate) fn streams(self) -> bool {
        definition(self.model()).streams
    }
}

/// Where words come from before the download finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Speech {
    /// macOS 26 and later: Apple's SpeechAnalyzer streams words with no
    /// download.
    Apple,
    /// Windows, Linux, and older macOS: nothing until the model lands.
    AfterDownload,
}

/// A download as `transcription_preparation` reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Download {
    pub(crate) language: Language,
    pub(crate) downloaded: u64,
    pub(crate) total: u64,
}

impl Download {
    pub(crate) fn fraction(self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        (self.downloaded as f64 / self.total as f64).clamp(0.0, 1.0) as f32
    }

    pub(crate) fn done(self) -> bool {
        self.downloaded >= self.total
    }

    /// "312 of 731 MB".
    pub(crate) fn megabytes(self) -> String {
        let mb = |bytes: u64| (bytes as f64 / 1_000_000.0).round() as u64;
        format!("{} of {} MB", mb(self.downloaded), mb(self.total))
    }
}

/// The bytes a download `elapsed` seconds in has, when the whole takes
/// `seconds`: a quick start, a slower middle, a quick finish, as a real
/// connection wobbles.
pub(crate) fn downloaded(total: u64, elapsed: f64, seconds: f64) -> u64 {
    let u = (elapsed / seconds).clamp(0.0, 1.0);
    let eased = u + 0.06 * (u * std::f64::consts::TAU).sin();
    (total as f64 * eased.clamp(0.0, 1.0)).round() as u64
}

/// One recorded utterance: the partial transcripts a streaming recognizer
/// emitted, by seconds after the key went down, and when it was released.
pub(crate) struct Take {
    pub(crate) updates: &'static [(f64, &'static str)],
    pub(crate) release: f64,
}

impl Take {
    /// The newest partial `elapsed` seconds in.
    pub(crate) fn partial(&self, elapsed: f64) -> &'static str {
        self.updates
            .iter()
            .take_while(|(at, _)| *at <= elapsed)
            .last()
            .map_or("", |(_, text)| text)
    }

    pub(crate) fn finished(&self, elapsed: f64) -> bool {
        elapsed >= self.release
    }

    /// The final transcript.
    pub(crate) fn text(&self) -> &'static str {
        self.updates.last().map_or("", |(_, text)| text)
    }

    /// A microphone meter `elapsed` seconds in: speech-shaped bursts while
    /// held, the room's floor after.
    pub(crate) fn meter(&self, elapsed: f64) -> (f32, f32) {
        if elapsed < 0.0 || elapsed > self.release {
            return (0.02, 0.04);
        }
        let t = elapsed as f32;
        let syllables = ((t * 7.3).sin() * (t * 2.9 + 0.7).sin()).abs();
        let breath = 0.6 + 0.4 * (t * 1.3).sin().abs();
        let average = (0.06 + 0.5 * syllables * breath).min(1.0);
        (average, (average * 1.5).min(1.0))
    }
}

/// Jay asked about moving the sync; Apple Speech partials, revised as it
/// hears more.
pub(crate) const ANSWER_JAY: Take = Take {
    updates: &[
        (0.45, "Hey"),
        (0.7, "Hey Jay"),
        (0.95, "Hey Jay can"),
        (1.15, "Hey Jay can we"),
        (1.35, "Hey Jay can we move"),
        (1.6, "Hey Jay can we move our"),
        (1.85, "Hey Jay can we move our sink"),
        (2.1, "Hey Jay can we move our sync to"),
        (2.4, "Hey Jay can we move our sync to Thursday"),
        (2.8, "Hey Jay, can we move our sync to Thursday afternoon?"),
    ],
    release: 3.2,
};

/// Saying anything on the try screen.
pub(crate) const ANSWER_TRY: Take = Take {
    updates: &[
        (0.4, "Okay"),
        (0.7, "Okay this"),
        (0.88, "Okay this is"),
        (1.2, "Okay this is actually"),
        (1.45, "Okay this is actually kind"),
        (1.6, "Okay this is actually kind of"),
        (2.0, "Okay, this is actually kind of magic."),
    ],
    release: 2.4,
};

/// The examples that type themselves into the demo cards.
pub(crate) const EXAMPLE_REPLIES: [&str; 2] = ["Yes, call me after this.", "Give me five minutes."];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_download_runs_from_nothing_to_its_size() {
        let total = Language::English.bytes();
        assert!(total > 700_000_000);
        assert_eq!(downloaded(total, 0.0, 9.0), 0);
        assert_eq!(downloaded(total, 9.0, 9.0), total);
        let mut last = 0;
        for step in 0..=90 {
            let now = downloaded(total, f64::from(step) / 10.0, 9.0);
            assert!(now >= last, "bytes never go backwards");
            last = now;
        }
    }

    #[test]
    fn only_english_streams_while_you_speak() {
        assert!(Language::English.streams());
        assert!(!Language::Other.streams());
    }

    #[test]
    fn a_take_reports_its_newest_partial() {
        assert_eq!(ANSWER_JAY.partial(0.0), "");
        assert_eq!(ANSWER_JAY.partial(0.8), "Hey Jay");
        assert!(ANSWER_JAY.partial(9.0).ends_with('?'));
        assert!(ANSWER_JAY.finished(3.2));
    }
}
