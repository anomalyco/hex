//! Stand-ins for what the running app reports: model bytes from
//! `transcription_preparation`, the microphone's `Meter { average, peak }`,
//! and a recognizer's partial transcripts.

use crate::transcription_models::{TranscriptionModelId, definition};

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

    pub(crate) fn streams(self) -> bool {
        definition(self.model()).streams
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Speech {
    Apple,
    AfterDownload,
}

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
}

pub(crate) fn downloaded(total: u64, elapsed: f64, seconds: f64) -> u64 {
    let u = (elapsed / seconds).clamp(0.0, 1.0);
    let eased = u + 0.06 * (u * std::f64::consts::TAU).sin();
    (total as f64 * eased.clamp(0.0, 1.0)).round() as u64
}

pub(crate) struct Take {
    pub(crate) updates: &'static [(f64, &'static str)],
    pub(crate) release: f64,
}

impl Take {
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

    pub(crate) fn text(&self) -> &'static str {
        self.updates.last().map_or("", |(_, text)| text)
    }

    pub(crate) fn meter(&self, elapsed: f64) -> (f32, f32) {
        if elapsed < 0.0 || elapsed > self.release {
            return (0.02, 0.04);
        }
        let t = elapsed as f32;
        let syllables = ((t * 9.2).sin() * (t * 3.7 + 0.7).sin()).abs();
        let breath = 0.65 + 0.35 * (t * 2.1).sin().abs();
        let average = (0.10 + 0.58 * syllables * breath).min(1.0);
        (average, (average * 1.45).min(1.0))
    }
}

/// Confident 1.5 s spoken answer when the investors are in the room: what
/// `"Without Hex"` choked on, then the rest of the thought.
pub(crate) const ANSWER_JAY: Take = Take {
    updates: &[
        (0.16, "Yes,"),
        (0.34, "Yes, we're"),
        (0.54, "Yes, we're ready."),
        (0.80, "Yes, we're ready. Everything's"),
        (1.04, "Yes, we're ready. Everything's tested,"),
        (1.24, "Yes, we're ready. Everything's tested, go"),
        (1.42, "Yes, we're ready. Everything's tested, go ahead."),
    ],
    release: 1.5,
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
    fn a_take_reports_its_newest_partial_in_one_and_a_half_seconds() {
        assert_eq!(ANSWER_JAY.partial(0.0), "");
        assert_eq!(ANSWER_JAY.partial(0.4), "Yes, we're");
        assert!(ANSWER_JAY.partial(2.0).ends_with('.'));
        assert!(ANSWER_JAY.finished(1.5));
    }
}
