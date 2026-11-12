//! Labs › Voice prompt: a spoken trigger phrase inside a dictation splits it
//! into text and a one-off instruction for the mode's OpenCode model.
//!
//! Matching is word-aligned and case-insensitive. Apostrophes and hyphens
//! inside a word are ignored when comparing, and punctuation otherwise
//! separates words: "re-write this" matches "rewrite this", while "write
//! this", "overwrite this", and "this's" do not. A hyphen joins a word, so
//! "Rewrite-this" is one word and doesn't match the two-word phrase.

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
    let phrase_words: Vec<String> = words(phrase).into_iter().map(|word| word.key).collect();
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
            .all(|(phrase, word)| *phrase == word.key)
        {
            found = Some(start);
        }
    }
    let start = found?;
    let first = &transcript_words[start];
    let last = &transcript_words[start + phrase_words.len() - 1];
    // The edges against the phrase lose joins and marks that only set it
    // off, while the text's own outer edge keeps its punctuation and
    // quotes: it is what a failed request pastes.
    let (text, prompt) = pieces(transcript, first.start, last.end);
    let prompt_words = words(prompt);
    let text = if words(text).is_empty() {
        String::new()
    } else {
        text.to_owned()
    };
    let prompt = if courtesy_words(&prompt_words) {
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
        .flat_map(|word| word.key.chars())
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
    /// Letters and digits, lowercased, with apostrophes and hyphens dropped.
    key: String,
    start: usize,
    end: usize,
}

/// The words of `text` at their byte offsets. Letters, digits, apostrophes,
/// and hyphens make a word; everything else separates words. Apostrophes
/// and hyphens stay in the span but leave the key, so "don't" and "dont"
/// compare equal while "this's" stays different from "this".
fn words(text: &str) -> Vec<Word> {
    let mut words = Vec::new();
    let mut current: Option<(String, usize, usize)> = None;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() || is_word_joiner(character) {
            match &mut current {
                Some((key, _, end)) => {
                    if character.is_alphanumeric() {
                        key.extend(character.to_lowercase());
                    }
                    *end = index + character.len_utf8();
                }
                None => {
                    let key = if character.is_alphanumeric() {
                        character.to_lowercase().collect()
                    } else {
                        String::new()
                    };
                    current = Some((key, index, index + character.len_utf8()));
                }
            }
        } else if let Some((key, start, end)) = current.take()
            && !key.is_empty()
        {
            words.push(Word { key, start, end });
        }
    }
    if let Some((key, start, end)) = current
        && !key.is_empty()
    {
        words.push(Word { key, start, end });
    }
    words
}

/// Characters that stay inside a word so its apostrophes and hyphens can be
/// ignored: `re-write`, `don't`, and `don’t` are single words.
fn is_word_joiner(character: char) -> bool {
    matches!(character, '\'' | '’' | '-')
}

/// What sits between a word and the phrase: whitespace and the marks that
/// join a phrase to the words around it.
fn is_joining(character: char) -> bool {
    character.is_whitespace() || matches!(character, ',' | ';' | ':' | '–' | '—')
}

/// The text and prompt around the phrase. Only the edges against the phrase
/// are trimmed: joins, a quote or bracket pair that wraps it, and a lone
/// ASCII dash separator. The pieces keep their own outer punctuation.
fn pieces(transcript: &str, phrase_start: usize, phrase_end: usize) -> (&str, &str) {
    let mut text = &transcript[..phrase_start];
    let mut prompt = &transcript[phrase_end..];
    // Strip a pair only while its opening mark sits directly before the
    // phrase and its closing mark follows it, possibly after the pair's own
    // joining marks and terminal punctuation. Whitespace is not skipped
    // first: `"Ship it" rewrite this "formal"` keeps both quotes.
    while strip_pair(&mut text, &mut prompt) {}
    let text = trim_spaced_dash_end(text.trim_end_matches(is_joining));
    let prompt = trim_spaced_dash_start(prompt.trim_start_matches(is_joining));
    (text.trim_start(), prompt.trim_end())
}

/// Strips one quote or bracket directly touching the phrase from both sides.
/// `false` leaves both pieces as they were.
fn strip_pair<'a>(text: &mut &'a str, prompt: &mut &'a str) -> bool {
    let Some(open) = text.chars().next_back() else {
        return false;
    };
    let mut close = None;
    for (index, character) in prompt.char_indices() {
        if closes(open, character) {
            close = Some((index, character.len_utf8()));
            break;
        }
        if !is_inside_pair(character) {
            return false;
        }
    }
    let Some((close_start, close_len)) = close else {
        return false;
    };
    *text = &text[..text.len() - open.len_utf8()];
    *prompt = &prompt[close_start + close_len..];
    true
}

/// What may sit inside a quote or bracket pair after the phrase before its
/// closing mark: joins and terminal punctuation.
fn is_inside_pair(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            ',' | ';' | ':' | '–' | '—' | '.' | '?' | '!' | '…'
        )
}

/// Whether `close` is the mark that pairs with `open`.
fn closes(open: char, close: char) -> bool {
    matches!(
        (open, close),
        ('(', ')')
            | ('[', ']')
            | ('{', '}')
            | ('<', '>')
            | ('"', '"')
            | ('\'', '\'')
            | ('“', '”')
            | ('‘', '’')
            | ('«', '»')
    )
}

/// Removes a lone `-` or `--` separator at the end of `text`, meaning one
/// preceded by whitespace (or the start).
fn trim_spaced_dash_end(text: &str) -> &str {
    let trimmed = text.trim_end();
    for dash in ["---", "--", "-"] {
        if let Some(rest) = trimmed.strip_suffix(dash)
            && (rest.is_empty() || rest.ends_with(char::is_whitespace))
        {
            return rest.trim_end();
        }
    }
    text
}

/// Removes a lone `-` or `--` separator at the start of `text`, meaning one
/// followed by whitespace (or the end).
fn trim_spaced_dash_start(text: &str) -> &str {
    let trimmed = text.trim_start();
    for dash in ["---", "--", "-"] {
        if let Some(rest) = trimmed.strip_prefix(dash)
            && (rest.is_empty() || rest.starts_with(char::is_whitespace))
        {
            return rest.trim_start();
        }
    }
    text
}

/// Whether these words say only thanks or please, or nothing at all.
fn courtesy_words(words: &[Word]) -> bool {
    let tokens: Vec<&str> = words.iter().map(|word| word.key.as_str()).collect();
    courtesy_sequence(&tokens)
}

/// One or more courtesy phrases in a row, such as "thanks a lot, please".
fn courtesy_sequence(tokens: &[&str]) -> bool {
    if tokens.is_empty() {
        return true;
    }
    COURTESY
        .iter()
        .any(|phrase| tokens.starts_with(phrase) && courtesy_sequence(&tokens[phrase.len()..]))
}

/// The courtesy phrases a prompt may be made of and nothing else.
const COURTESY: &[&[&str]] = &[
    &["thanks"],
    &["thanks", "a", "lot"],
    &["thanks", "so", "much"],
    &["thanks", "very", "much"],
    &["thank", "you"],
    &["thank", "you", "a", "lot"],
    &["thank", "you", "so", "much"],
    &["thank", "you", "very", "much"],
    &["please"],
    &["please", "and", "thank", "you"],
];

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
                text: "Ship it Friday.".into(),
                prompt: "make it formal.".into(),
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
            "rewrite this's ok",
        ] {
            assert_eq!(split(transcript), None, "{transcript}");
        }
    }

    #[test]
    fn hyphens_and_apostrophes_inside_words_are_ignored() {
        assert_eq!(
            split("We're shipping on Friday. Re-write this: make it formal!"),
            Some(Instruction::Rewrite {
                text: "We're shipping on Friday.".into(),
                prompt: "make it formal!".into(),
            })
        );
        // A hyphenated phrase matches the plain words too.
        assert_eq!(
            instruction(
                "We're shipping Friday. rewrite this make it formal",
                "re-write this"
            ),
            Some(Instruction::Rewrite {
                text: "We're shipping Friday.".into(),
                prompt: "make it formal".into(),
            })
        );
        // An apostrophe inside the phrase is ignored the same way.
        assert_eq!(
            instruction("dont do that make it shorter", "don't do that"),
            Some(Instruction::Write {
                prompt: "make it shorter".into(),
            })
        );
        assert_eq!(
            instruction("do not do that make it shorter", "don't do that"),
            None
        );
        // Hyphens join a word, they don't split it into the phrase's words.
        assert_eq!(split("Re-write-this: make it formal!"), None);
    }

    #[test]
    fn the_edges_keep_their_punctuation_and_quotes() {
        assert_eq!(
            split("Are we shipping Friday? rewrite this"),
            Some(Instruction::RewriteText {
                text: "Are we shipping Friday?".into(),
            })
        );
        assert_eq!(
            split("\"Hello,\" she said. rewrite this"),
            Some(Instruction::RewriteText {
                text: "\"Hello,\" she said.".into(),
            })
        );
        // Joins against the phrase leave; the prompt's own punctuation stays.
        assert_eq!(
            split("Ship it. rewrite this, make it formal."),
            Some(Instruction::Rewrite {
                text: "Ship it.".into(),
                prompt: "make it formal.".into(),
            })
        );
        assert_eq!(
            split("Ship it; rewrite this — make it formal"),
            Some(Instruction::Rewrite {
                text: "Ship it".into(),
                prompt: "make it formal".into(),
            })
        );
    }

    #[test]
    fn quotes_brackets_and_dashes_that_set_the_phrase_off_leave() {
        assert_eq!(
            split("He said \"rewrite this\" and left"),
            Some(Instruction::Rewrite {
                text: "He said".into(),
                prompt: "and left".into(),
            })
        );
        assert_eq!(
            split("He said (rewrite this) and left"),
            Some(Instruction::Rewrite {
                text: "He said".into(),
                prompt: "and left".into(),
            })
        );
        assert_eq!(
            split("He said [rewrite this] and left"),
            Some(Instruction::Rewrite {
                text: "He said".into(),
                prompt: "and left".into(),
            })
        );
        assert_eq!(
            split("He said “rewrite this” and left"),
            Some(Instruction::Rewrite {
                text: "He said".into(),
                prompt: "and left".into(),
            })
        );
        assert_eq!(
            split("He said (\"rewrite this\") and left"),
            Some(Instruction::Rewrite {
                text: "He said".into(),
                prompt: "and left".into(),
            })
        );
        assert_eq!(
            split("Ship it - rewrite this - formal"),
            Some(Instruction::Rewrite {
                text: "Ship it".into(),
                prompt: "formal".into(),
            })
        );
        assert_eq!(
            split("Ship it -- rewrite this -- formal"),
            Some(Instruction::Rewrite {
                text: "Ship it".into(),
                prompt: "formal".into(),
            })
        );
        assert_eq!(
            split("Ship it --- rewrite this --- formal"),
            Some(Instruction::Rewrite {
                text: "Ship it".into(),
                prompt: "formal".into(),
            })
        );
        // A pair wraps the phrase only when its opening mark touches it.
        assert_eq!(
            split("\"Ship it\" rewrite this \"formal\""),
            Some(Instruction::Rewrite {
                text: "\"Ship it\"".into(),
                prompt: "\"formal\"".into(),
            })
        );
        assert_eq!(
            split("\"Ship it\" \"rewrite this\" \"formal\""),
            Some(Instruction::Rewrite {
                text: "\"Ship it\"".into(),
                prompt: "\"formal\"".into(),
            })
        );
        assert_eq!(
            split("Ship it.\" rewrite this \"make it formal"),
            Some(Instruction::Rewrite {
                text: "Ship it.\"".into(),
                prompt: "\"make it formal".into(),
            })
        );
        // Joining and terminal punctuation may sit inside the pair.
        assert_eq!(
            split("He said \"rewrite this,\" and left"),
            Some(Instruction::Rewrite {
                text: "He said".into(),
                prompt: "and left".into(),
            })
        );
        assert_eq!(
            split("He said \"rewrite this.\" and left"),
            Some(Instruction::Rewrite {
                text: "He said".into(),
                prompt: "and left".into(),
            })
        );
        // Quotes that don't touch the phrase stay, including one left by a
        // dash that set the phrase off.
        assert_eq!(
            split("\"Hello,\" she said. rewrite this"),
            Some(Instruction::RewriteText {
                text: "\"Hello,\" she said.".into(),
            })
        );
        assert_eq!(
            split("\"Ship it\" - rewrite this"),
            Some(Instruction::RewriteText {
                text: "\"Ship it\"".into(),
            })
        );
    }

    #[test]
    fn the_last_occurrence_splits_the_dictation() {
        assert_eq!(
            split("Rewrite this is not the trigger. Ship it. rewrite this make it formal"),
            Some(Instruction::Rewrite {
                text: "Rewrite this is not the trigger. Ship it.".into(),
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
                text: "We ship Friday.".into(),
                prompt: "make it formal".into(),
            })
        );
        // Text only: rewrite it clearly and concisely.
        assert_eq!(
            split("We ship Friday. rewrite this."),
            Some(Instruction::RewriteText {
                text: "We ship Friday.".into(),
            })
        );
        // A prompt only: write what it asks for.
        assert_eq!(
            split("rewrite this: an email to Bob about Friday."),
            Some(Instruction::Write {
                prompt: "an email to Bob about Friday.".into(),
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
            "rewrite this thanks a lot",
            "rewrite this thank you very much",
            "rewrite this please and thank you",
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
        assert_eq!(
            split("rewrite this thanks a lot make it formal"),
            Some(Instruction::Write {
                prompt: "thanks a lot make it formal".into(),
            })
        );
    }

    #[test]
    fn a_phrase_needs_two_words_and_six_letters() {
        assert_eq!(phrase_problem("rewrite this"), None);
        assert_eq!(phrase_problem("  Re-write this!  "), None);
        assert_eq!(phrase_problem("don't do that"), None);
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
