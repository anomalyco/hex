//! Labs › Spoken commands (macOS, issue 650): a hotkey dictation that is
//! exactly one fixed English command phrase presses a key instead of
//! pasting. Only the whole dictation counts: a sentence that contains a
//! phrase is pasted as text.

/// The key a spoken command presses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpokenKey {
    Return,
    Tab,
    Space,
    /// Control-Right Arrow: the next Space (Mission Control's default).
    NextSpace,
    /// Control-Left Arrow: the previous Space.
    PreviousSpace,
}

impl SpokenKey {
    /// The name process.log records.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Return => "return",
            Self::Tab => "tab",
            Self::Space => "space",
            Self::NextSpace => "next-space",
            Self::PreviousSpace => "previous-space",
        }
    }
}

const PHRASES: [(&str, SpokenKey); 6] = [
    ("press return", SpokenKey::Return),
    ("press enter", SpokenKey::Return),
    ("press tab", SpokenKey::Tab),
    ("press space", SpokenKey::Space),
    ("next space", SpokenKey::NextSpace),
    ("previous space", SpokenKey::PreviousSpace),
];

/// The command `transcript` is, if the whole of it is one phrase: case,
/// surrounding punctuation, and extra spaces between words are ignored.
pub fn command(transcript: &str) -> Option<SpokenKey> {
    let words: Vec<String> = transcript
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|character: char| !character.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() || words.len() > 2 {
        return None;
    }
    let spoken = words.join(" ");
    PHRASES
        .iter()
        .find(|(phrase, _)| *phrase == spoken)
        .map(|(_, key)| *key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_phrases_press_their_key() {
        assert_eq!(command("press return"), Some(SpokenKey::Return));
        assert_eq!(command("press enter"), Some(SpokenKey::Return));
        assert_eq!(command("press tab"), Some(SpokenKey::Tab));
        assert_eq!(command("press space"), Some(SpokenKey::Space));
        assert_eq!(command("next space"), Some(SpokenKey::NextSpace));
        assert_eq!(command("previous space"), Some(SpokenKey::PreviousSpace));
    }

    #[test]
    fn case_punctuation_and_spacing_are_ignored() {
        assert_eq!(command("Press return."), Some(SpokenKey::Return));
        assert_eq!(command("  PRESS   ENTER!  "), Some(SpokenKey::Return));
        assert_eq!(command("\"Next space?\""), Some(SpokenKey::NextSpace));
        assert_eq!(command("Previous, space."), Some(SpokenKey::PreviousSpace));
    }

    #[test]
    fn anything_else_is_text() {
        for transcript in [
            "",
            "   ",
            "...",
            "press",
            "return",
            "press return now",
            "please press return",
            "I need to press return before the meeting.",
            "next spaceship",
            "press returns",
            "the next space",
        ] {
            assert_eq!(command(transcript), None, "{transcript:?}");
        }
    }
}
