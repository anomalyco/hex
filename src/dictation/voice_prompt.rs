//! Labs › Voice prompt: a spoken trigger phrase inside a dictation splits it
//! into text and a one-off instruction for the mode's OpenCode model.
//!
//! Matching is word-aligned and case-insensitive, ignoring punctuation,
//! hyphens, and apostrophes: "rewrite this" matches "Rewrite-this!" and never
//! "write this" or "overwrite this". No fuzzy matching.

/// The phrase Labs › Voice prompt starts with.
pub const DEFAULT_PHRASE: &str = "rewrite this";

/// The fewest letters a trigger phrase needs, counted after normalization.
const MINIMUM_LETTERS: usize = 6;

/// Why a phrase can't be a trigger phrase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhraseProblem {
    /// Fewer than two words.
    TooFewWords,
    /// Fewer than six letters, counted after normalization.
    TooFewLetters,
}

impl PhraseProblem {
    /// The reason the window and `hex settings` show.
    pub const fn message(self) -> &'static str {
        match self {
            Self::TooFewWords => "Use at least two words",
            Self::TooFewLetters => "Use at least six letters",
        }
    }
}

/// What a dictation carrying the phrase asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Instruction {
    /// Text before the phrase and the instruction after it.
    Rewrite { text: String, prompt: String },
    /// The phrase with nothing after it: rewrite the text clearly and
    /// concisely.
    RewriteText { text: String },
    /// The phrase with nothing before it: write what the instruction asks
    /// for.
    Write { prompt: String },
    /// Only the phrase, or the phrase followed by a courtesy word: nothing
    /// to ask and nothing to paste.
    Nothing,
}

/// Splits `transcript` at its last whole-word `phrase`; `None` when the
/// phrase isn't there, so processing is exactly as it was.
pub fn instruction(transcript: &str, phrase: &str) -> Option<Instruction> {
    let phrase_words: Vec<String> = words(phrase)
        .into_iter()
        .map(|word| word.normalized)
        .collect();
    if phrase_words.is_empty() {
        return None;
    }
    let transcript_words = words(transcript);
    if transcript_words.len() < phrase_words.len() {
        return None;
    }
    let mut found = None;
    for start in 0..=transcript_words.len() - phrase_words.len() {
        if phrase_words
            .iter()
            .zip(&transcript_words[start..])
            .all(|(phrase, word)| *phrase == word.normalized)
        {
            found = Some(start);
        }
    }
    let start = found?;
    let first = &transcript_words[start];
    let last = &transcript_words[start + phrase_words.len() - 1];
    let text = trim_edges(&transcript[..first.start]).to_owned();
    let prompt = trim_edges(&transcript[last.end..]);
    let prompt = if courtesy_words(&words(prompt)) {
        String::new()
    } else {
        prompt.to_owned()
    };
    Some(match (text.is_empty(), prompt.is_empty()) {
        (true, true) => Instruction::Nothing,
        (false, true) => Instruction::RewriteText { text },
        (true, false) => Instruction::Write { prompt },
        (false, false) => Instruction::Rewrite { text, prompt },
    })
}

/// Why `phrase` can't start a voice prompt, or `None` when it can: at least
/// two words and at least six letters, counted after normalization, so a
/// single common word never triggers one.
pub fn phrase_problem(phrase: &str) -> Option<PhraseProblem> {
    let words = words(phrase);
    if words.len() < 2 {
        return Some(PhraseProblem::TooFewWords);
    }
    let letters: usize = words
        .iter()
        .flat_map(|word| word.normalized.chars())
        .filter(|character| character.is_alphabetic())
        .count();
    if letters < MINIMUM_LETTERS {
        return Some(PhraseProblem::TooFewLetters);
    }
    None
}

/// `phrase` as the runtime should hold it: trimmed when valid, else the
/// default, so an invalid saved value never turns matching off.
pub fn accepted_phrase(phrase: &str) -> String {
    let phrase = phrase.trim();
    if phrase_problem(phrase).is_none() {
        phrase.to_owned()
    } else {
        DEFAULT_PHRASE.to_owned()
    }
}

/// A word of a transcript or phrase and where its bytes sit in the original.
struct Word {
    normalized: String,
    start: usize,
    end: usize,
}

/// The alphanumeric runs of `text`, lowercased, at their byte offsets.
/// Everything else is a separator, which ignores punctuation, hyphens, and
/// apostrophes.
fn words(text: &str) -> Vec<Word> {
    let mut words = Vec::new();
    let mut current: Option<Word> = None;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() {
            match &mut current {
                Some(word) => {
                    word.normalized.extend(character.to_lowercase());
                    word.end = index + character.len_utf8();
                }
                None => {
                    current = Some(Word {
                        normalized: character.to_lowercase().collect(),
                        start: index,
                        end: index + character.len_utf8(),
                    })
                }
            }
        } else if let Some(word) = current.take() {
            words.push(word);
        }
    }
    words.extend(current);
    words
}

/// Whether these normalized words say nothing more than thanks or please.
fn courtesy_words(words: &[Word]) -> bool {
    let mut index = 0;
    while index < words.len() {
        match words[index].normalized.as_str() {
            "thanks" | "please" => index += 1,
            "thank"
                if words
                    .get(index + 1)
                    .is_some_and(|word| word.normalized == "you") =>
            {
                index += 2;
            }
            _ => return false,
        }
    }
    !words.is_empty()
}

/// `text` without whitespace or punctuation around it; what sits between the
/// words stays as it was.
fn trim_edges(text: &str) -> &str {
    text.trim_matches(|character: char| character.is_whitespace() || is_punctuation(character))
}

/// Punctuation a split piece can start or end with, ASCII and typographic.
fn is_punctuation(character: char) -> bool {
    character.is_ascii_punctuation()
        || matches!(
            character,
            '…' | '–'
                | '—'
                | '‘'
                | '’'
                | '“'
                | '”'
                | '«'
                | '»'
                | '¡'
                | '¿'
                | '·'
                | '。'
                | '、'
                | '，'
                | '．'
                | '！'
                | '？'
                | '：'
                | '；'
                | '（'
                | '）'
                | '「'
                | '」'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(transcript: &str) -> Option<Instruction> {
        instruction(transcript, DEFAULT_PHRASE)
    }

    #[test]
    fn the_phrase_matches_whole_words_in_any_case() {
        assert_eq!(
            split("Ship it Friday. REWRITE THIS, make it formal."),
            Some(Instruction::Rewrite {
                text: "Ship it Friday".into(),
                prompt: "make it formal".into(),
            })
        );
        assert_eq!(
            split("Rewrite This make it formal"),
            Some(Instruction::Write {
                prompt: "make it formal".into(),
            })
        );
    }

    #[test]
    fn write_this_and_overwrite_this_never_match() {
        for transcript in [
            "write this make it formal",
            "overwrite this make it formal",
            "unrewrite this",
            "rewriting this",
        ] {
            assert_eq!(split(transcript), None, "{transcript}");
        }
    }

    #[test]
    fn punctuation_hyphens_and_apostrophes_are_ignored() {
        assert_eq!(
            split("We're shipping on Friday. Rewrite-this: make it formal!"),
            Some(Instruction::Rewrite {
                text: "We're shipping on Friday".into(),
                prompt: "make it formal".into(),
            })
        );
        assert_eq!(
            split("Rewrite, this. Make it bold."),
            Some(Instruction::Write {
                prompt: "Make it bold".into(),
            })
        );
    }

    #[test]
    fn the_last_occurrence_splits_the_dictation() {
        assert_eq!(
            split("Rewrite this is not the trigger. Ship it. rewrite this make it formal"),
            Some(Instruction::Rewrite {
                text: "Rewrite this is not the trigger. Ship it".into(),
                prompt: "make it formal".into(),
            })
        );
    }

    #[test]
    fn the_outcome_table_covers_every_piece() {
        // Text and a prompt.
        assert_eq!(
            split("We ship Friday. rewrite this make it formal"),
            Some(Instruction::Rewrite {
                text: "We ship Friday".into(),
                prompt: "make it formal".into(),
            })
        );
        // Text only: rewrite it clearly and concisely.
        assert_eq!(
            split("We ship Friday. rewrite this."),
            Some(Instruction::RewriteText {
                text: "We ship Friday".into(),
            })
        );
        // A prompt only: write what it asks for.
        assert_eq!(
            split("rewrite this: an email to Bob about Friday"),
            Some(Instruction::Write {
                prompt: "an email to Bob about Friday".into(),
            })
        );
        // Neither: just the phrase, or it followed by thanks or please.
        for transcript in [
            "rewrite this",
            "Rewrite this.",
            "rewrite this, thanks",
            "rewrite this thank you",
            "rewrite this please.",
            "rewrite this thanks please thank you",
        ] {
            assert_eq!(
                split(transcript),
                Some(Instruction::Nothing),
                "{transcript}"
            );
        }
        // The phrase absent changes nothing.
        assert_eq!(split("We ship Friday."), None);
    }

    #[test]
    fn a_courtesy_word_with_an_instruction_is_still_an_instruction() {
        assert_eq!(
            split("rewrite this please make it formal"),
            Some(Instruction::Write {
                prompt: "please make it formal".into(),
            })
        );
    }

    #[test]
    fn a_phrase_needs_two_words_and_six_letters() {
        assert_eq!(phrase_problem("rewrite this"), None);
        assert_eq!(phrase_problem("  Rewrite-this!  "), None);
        assert_eq!(phrase_problem("okay go"), None, "six letters in two words");
        assert_eq!(phrase_problem("héllo wörld"), None);
        assert_eq!(
            phrase_problem("one two 3"),
            None,
            "digits don't add letters"
        );
        for phrase in ["", "   ", "rewrite", "please", "a b c", "to be 2"] {
            assert!(
                phrase_problem(phrase).is_some(),
                "{phrase:?} should be invalid"
            );
        }
        assert_eq!(phrase_problem("rewrite"), Some(PhraseProblem::TooFewWords));
        assert_eq!(phrase_problem("a b c"), Some(PhraseProblem::TooFewLetters));
        assert_eq!(
            phrase_problem("to be 2"),
            Some(PhraseProblem::TooFewLetters)
        );
        assert_eq!(
            PhraseProblem::TooFewLetters.message(),
            "Use at least six letters"
        );
    }

    #[test]
    fn an_invalid_phrase_falls_back_to_the_default() {
        assert_eq!(accepted_phrase(" rewrite this "), "rewrite this");
        assert_eq!(accepted_phrase("say it better"), "say it better");
        assert_eq!(accepted_phrase("nope"), DEFAULT_PHRASE);
        assert_eq!(accepted_phrase(""), DEFAULT_PHRASE);
    }
}
