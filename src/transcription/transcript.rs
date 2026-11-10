//! A transcript with segment and word timings, as every OS's models give it.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    /// Word timings, when the model produces them.
    #[serde(default)]
    pub words: Vec<TranscriptSegment>,
}

/// The timing level a caller asked a model for. It is always native: nothing
/// estimates one from text length, chunk boundaries, or the other level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TimestampLevel {
    /// No timings at all, as every request was before this option existed.
    #[default]
    None,
    Segment,
    Word,
}

impl TimestampLevel {
    /// The value a response names in `timestampGranularity`; `None` adds no
    /// timing fields.
    pub const fn granularity(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Segment => Some("segment"),
            Self::Word => Some("word"),
        }
    }

    /// Whether a model whose finest native timings are `capability` answers
    /// this request at the level asked for. Word timings are never derived
    /// from segments, and a word-timed model still answers a segment request.
    pub const fn supported_by(self, capability: Self) -> bool {
        matches!(
            (self, capability),
            (Self::None, _)
                | (Self::Segment, Self::Segment | Self::Word)
                | (Self::Word, Self::Word)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_answered_only_at_its_own_level_or_coarser() {
        for capability in [
            TimestampLevel::None,
            TimestampLevel::Segment,
            TimestampLevel::Word,
        ] {
            assert!(
                TimestampLevel::None.supported_by(capability),
                "no timings asks nothing of the model"
            );
        }
        assert!(TimestampLevel::Segment.supported_by(TimestampLevel::Segment));
        assert!(TimestampLevel::Segment.supported_by(TimestampLevel::Word));
        assert!(!TimestampLevel::Segment.supported_by(TimestampLevel::None));
        assert!(TimestampLevel::Word.supported_by(TimestampLevel::Word));
        assert!(
            !TimestampLevel::Word.supported_by(TimestampLevel::Segment),
            "word timings are never cut from segments"
        );
        assert!(!TimestampLevel::Word.supported_by(TimestampLevel::None));
    }

    #[test]
    fn only_a_timed_request_names_a_granularity() {
        assert_eq!(TimestampLevel::None.granularity(), None);
        assert_eq!(TimestampLevel::Segment.granularity(), Some("segment"));
        assert_eq!(TimestampLevel::Word.granularity(), Some("word"));
    }
}
