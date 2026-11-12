//! Labs › Screen vocabulary: the uncommon names and terms visible in the
//! focused window, as temporary vocabulary for one dictation.
//!
//! The macOS reader that collects the window's text lives in
//! [`crate::accessibility`]; [`extract`] is pure so every host's tests can
//! check it. Nothing here saves, logs, or sends a term.

use std::collections::{HashMap, HashSet};

/// Most terms one capture adds.
pub const MAX_TERMS: usize = 24;
/// Shortest and longest token a term may be, in characters.
const MIN_CHARS: usize = 3;
const MAX_CHARS: usize = 32;
/// Longest acronym kept: API, GPT, NVIDIA.
const MAX_ACRONYM_CHARS: usize = 6;

/// Terms in `text` that look like names or technical terms, most frequent
/// first and then by first appearance, each once, and never one in `known`
/// (case-insensitive).
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(crate) fn extract(text: &str, known: &[String]) -> Vec<String> {
    let known: HashSet<String> = known.iter().map(|term| term.to_lowercase()).collect();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut slots: HashMap<String, usize> = HashMap::new();
    for (index, token) in tokens(text).into_iter().enumerate() {
        if !(MIN_CHARS..=MAX_CHARS).contains(&token.text.chars().count())
            || !looks_like_a_term(&token)
        {
            continue;
        }
        let key = token.text.to_lowercase();
        if known.contains(&key) {
            continue;
        }
        match slots.get(&key) {
            Some(&slot) => candidates[slot].count += 1,
            None => {
                slots.insert(key.clone(), candidates.len());
                candidates.push(Candidate {
                    term: token.text.into(),
                    count: 1,
                    first: index,
                });
            }
        }
    }
    candidates.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then(left.first.cmp(&right.first))
    });
    candidates
        .into_iter()
        .take(MAX_TERMS)
        .map(|candidate| candidate.term)
        .collect()
}

/// The accessible text of the focused window of the application in front,
/// when Labs › Screen vocabulary is on. Never Hex's own window.
#[cfg(target_os = "macos")]
pub(crate) fn from_focused_window() -> Option<Vec<String>> {
    let pid = crate::dictation::context::frontmost_pid()?;
    let text = crate::accessibility::focused_window_text(pid)?;
    let terms = extract(&text, &crate::dictation::vocabulary::terms());
    (!terms.is_empty()).then_some(terms)
}

struct Candidate {
    term: String,
    count: usize,
    first: usize,
}

struct Token<'a> {
    text: &'a str,
    /// Whether only whitespace separates it from the start of its line or a
    /// sentence-ending mark, where a capitalized word is usually just the
    /// first word.
    sentence_start: bool,
}

/// The words in `text`, keeping `.`, `-`, and `_` inside a word such as
/// `Node.js`, `gpt-4o`, or `my_variable`, and trimming them at its edges.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut begin = None;
    for (index, character) in text.char_indices() {
        if is_word_char(character) {
            begin.get_or_insert(index);
            continue;
        }
        if let Some(start) = begin.take() {
            push_token(&mut tokens, text, start, index);
        }
    }
    if let Some(start) = begin {
        push_token(&mut tokens, text, start, text.len());
    }
    tokens
}

fn push_token<'a>(tokens: &mut Vec<Token<'a>>, text: &'a str, start: usize, end: usize) {
    let raw = &text[start..end];
    let trimmed = raw.trim_matches(|character: char| matches!(character, '.' | '-' | '_'));
    if trimmed.is_empty() {
        return;
    }
    let start = start + (trimmed.as_ptr() as usize - raw.as_ptr() as usize);
    tokens.push(Token {
        text: trimmed,
        sentence_start: at_a_sentence_start(text, start),
    });
}

fn is_word_char(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '.' | '-' | '_')
}

fn at_a_sentence_start(text: &str, start: usize) -> bool {
    for character in text[..start].chars().rev() {
        if matches!(character, '\n' | '\r') {
            return true;
        }
        if character.is_whitespace() {
            continue;
        }
        return matches!(character, '.' | '!' | '?');
    }
    true
}

/// Whether a word is more than an ordinary lowercase word or a capitalized
/// one at the start of a sentence: an acronym, a mixed-case name, a letter
/// with digits, or a capitalized word away from a sentence's start.
fn looks_like_a_term(token: &Token<'_>) -> bool {
    let characters: Vec<char> = token.text.chars().collect();
    let letters = characters
        .iter()
        .filter(|character| character.is_alphabetic())
        .count();
    if letters == 0 {
        return false;
    }
    // gpt4o, v2, Qwen3, gpt-4o.
    if characters
        .iter()
        .any(|character| character.is_ascii_digit())
    {
        return true;
    }
    // API, GPT, NVIDIA.
    if characters
        .iter()
        .filter(|character| character.is_uppercase())
        .count()
        == letters
        && (2..=MAX_ACRONYM_CHARS).contains(&letters)
    {
        return true;
    }
    // PostgreSQL, OpenCode, iPhone.
    if characters
        .iter()
        .skip(1)
        .any(|character| character.is_uppercase())
    {
        return true;
    }
    // Qdrant, away from the start of a sentence.
    characters[0].is_uppercase() && !token.sentence_start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(text: &str) -> Vec<String> {
        extract(text, &[])
    }

    #[test]
    fn qdrant_mid_sentence_is_kept_and_ordinary_words_are_not() {
        assert_eq!(
            terms("We store vectors in Qdrant, not a quadrant. It is fast."),
            ["Qdrant"]
        );
    }

    #[test]
    fn a_capitalized_word_only_at_a_sentence_start_is_dropped() {
        assert!(terms("Qdrant is a vector database. The database stores vectors.").is_empty());
        assert!(
            terms("Loading\nQdrant").is_empty(),
            "a line break starts one"
        );
    }

    #[test]
    fn mixed_case_acronyms_and_digits_are_kept() {
        assert_eq!(
            terms("PostgreSQL and OpenCode ship gpt4o through the API for iPhone."),
            ["PostgreSQL", "OpenCode", "gpt4o", "API", "iPhone"]
        );
    }

    #[test]
    fn the_most_frequent_term_comes_first_then_the_earliest() {
        assert_eq!(
            terms("Use Qdrant here. Then OpenCode calls Qdrant again."),
            ["Qdrant", "OpenCode"]
        );
    }

    #[test]
    fn internal_punctuation_stays_in_the_term() {
        assert_eq!(
            terms("We run Node.js and call gpt-4o, not my_variable."),
            ["Node.js", "gpt-4o"]
        );
        assert_eq!(terms("Use Node.js."), ["Node.js"]);
    }

    #[test]
    fn one_term_is_listed_once_in_the_spelling_first_seen() {
        assert_eq!(
            terms("Use Qdrant. Later QDRANT and Qdrant stay."),
            ["Qdrant"]
        );
    }

    #[test]
    fn the_list_stops_at_the_cap() {
        let mut text = String::new();
        for index in 0..MAX_TERMS + 6 {
            text.push_str(&format!("uses Term{index} here. "));
        }
        let terms = terms(&text);
        assert_eq!(terms.len(), MAX_TERMS);
        assert_eq!(terms.first().map(String::as_str), Some("Term0"));
        assert_eq!(terms.last().map(String::as_str), Some("Term23"));
    }

    #[test]
    fn known_terms_are_left_out_in_any_case() {
        let known = vec!["qdrant".into(), "OPENCODE".into()];
        assert_eq!(
            extract("We use Qdrant and OpenCode, plus PostgreSQL.", &known),
            ["PostgreSQL"]
        );
    }

    #[test]
    fn a_long_word_is_not_a_term() {
        let long = format!(
            "We call Supercalifragilisticexpialidocious{} now.",
            "x".repeat(4)
        );
        assert!(terms(&long).is_empty());
    }
}
