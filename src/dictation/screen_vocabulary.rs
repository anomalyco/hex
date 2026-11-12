//! Labs › Screen vocabulary: the uncommon names and terms visible in the
//! focused window, as temporary vocabulary for one dictation.
//!
//! The macOS reader that collects the window's text lives in
//! [`crate::accessibility`]; [`extract`] is pure so every host's tests can
//! check it, and [`ScreenVocabulary`] is the bounded one-read state machine
//! `recognition::context_watch` starts. Nothing here saves a term or logs its
//! text.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
#[cfg(target_os = "macos")]
use std::thread;

/// Most terms one capture adds.
pub const MAX_TERMS: usize = 24;
/// Shortest non-acronym token and the longest token, in characters.
const MIN_CHARS: usize = 3;
const MAX_CHARS: usize = 32;
/// Shortest and longest all-caps acronym kept: AI, UI, API, GPT, NVIDIA.
const MIN_ACRONYM_CHARS: usize = 2;
const MAX_ACRONYM_CHARS: usize = 6;
/// Common window words and log levels that are capitalized only because they
/// are labels.
const STOPLIST: [&str; 25] = [
    "Recent", "Sidebar", "Settings", "Search", "Home", "File", "Edit", "View", "Window", "Help",
    "New", "Open", "Save", "Close", "Share", "More", "Today", "INFO", "DEBUG", "ERROR", "WARN",
    "TODO", "FIXME", "NOTE", "TRACE",
];
/// Unit suffixes that make a digit token a measurement, not a name: 100ms,
/// 1080p, 30x.
const UNIT_SUFFIXES: [&str; 32] = [
    "fps", "khz", "mhz", "ghz", "px", "kb", "mb", "gb", "tb", "ms", "hz", "db", "kw", "cm", "mm",
    "km", "kg", "mg", "ml", "ns", "us", "ps", "s", "x", "p", "m", "g", "h", "w", "v", "k", "b",
];

/// Terms in `text` that look like names or technical terms, most frequent
/// first and then by first appearance, each once, and never one in `known`
/// (case-insensitive).
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(crate) fn extract(text: &str, known: &[String]) -> Vec<String> {
    let known: HashSet<String> = known.iter().map(|term| term.to_lowercase()).collect();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut slots: HashMap<String, usize> = HashMap::new();
    for (index, token) in tokens(text).into_iter().enumerate() {
        let minimum = if acronym(token.text) {
            MIN_ACRONYM_CHARS
        } else {
            MIN_CHARS
        };
        if !(minimum..=MAX_CHARS).contains(&token.text.chars().count())
            || is_stopword(token.text)
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
/// when Labs › Screen vocabulary is on. Never Hex's own window. Only the
/// number of terms and the read time are logged, never the text or a term.
#[cfg(target_os = "macos")]
pub(crate) fn from_focused_window() -> Option<Vec<String>> {
    let started = std::time::Instant::now();
    let terms = crate::dictation::context::frontmost_pid()
        .and_then(crate::accessibility::focused_window_text)
        .map(|text| extract(&text, &crate::dictation::vocabulary::terms()))
        .unwrap_or_default();
    tracing::debug!(
        terms = terms.len(),
        read_ms = started.elapsed().as_millis(),
        "screen vocabulary read"
    );
    (!terms.is_empty()).then_some(terms)
}

struct Candidate {
    term: String,
    count: usize,
    first: usize,
}

/// One capture's screen vocabulary read: at most one waiting result and at
/// most one running reader. `recognition::context_watch` starts it as the
/// capture becomes real and takes it when the capture finishes.
#[derive(Default)]
pub(crate) struct ScreenVocabulary {
    read: Option<mpsc::Receiver<Vec<String>>>,
    /// A reader thread that has not finished yet: only one runs at a time.
    running: Arc<AtomicBool>,
}

impl ScreenVocabulary {
    /// Starts this capture's one read, if it has a pending capture, the lab
    /// is on, and the selected model would use the terms. Off, or for a model
    /// that ignores the vocabulary, nothing reads Accessibility at all.
    #[cfg(target_os = "macos")]
    pub(crate) fn start(&mut self, pending: bool) {
        let reading =
            crate::labs::screen_vocabulary() && crate::dictation::vocabulary::uses_terms();
        let Some(sender) = self.begin(pending, reading) else {
            return;
        };
        let running = self.running.clone();
        running.store(true, Ordering::Release);
        let spawned = thread::Builder::new()
            .name("screen-vocabulary".into())
            .spawn(move || {
                struct Done(Arc<AtomicBool>);
                impl Drop for Done {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Release);
                    }
                }
                let _done = Done(running);
                if let Some(terms) = from_focused_window() {
                    let _ = sender.send(terms);
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start the screen vocabulary reader");
            self.running.store(false, Ordering::Release);
            self.read = None;
        }
    }

    /// Claims the one read slot. `pending` is the capture's state and
    /// `reading` the lab and model gate; a read already waiting or a reader
    /// still running keeps this one from starting.
    fn begin(&mut self, pending: bool, reading: bool) -> Option<mpsc::Sender<Vec<String>>> {
        if !pending || !reading || self.read.is_some() || self.running.load(Ordering::Acquire) {
            return None;
        }
        let (sender, read) = mpsc::channel();
        self.read = Some(read);
        Some(sender)
    }

    /// This capture's terms, if the read finished before its run.
    pub(crate) fn take(&mut self) -> Vec<String> {
        self.read
            .take()
            .and_then(|read| read.try_recv().ok())
            .unwrap_or_default()
    }

    /// The capture ended without transcribing, or a new one started: a late
    /// read is discarded instead of reaching another dictation.
    pub(crate) fn cancel(&mut self) {
        self.read = None;
    }

    /// A read that answers at once, for the sequence tests.
    #[cfg(test)]
    fn read_now(&mut self, pending: bool, reading: bool, terms: &[&str]) -> bool {
        let Some(sender) = self.begin(pending, reading) else {
            return false;
        };
        let terms = terms.iter().map(|term| (*term).to_string()).collect();
        sender.send(terms).expect("the read's receiver is held");
        true
    }

    /// Marks a reader thread running or done, for the single-flight test.
    #[cfg(test)]
    fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Release);
    }
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

/// Whether a word is an all-caps acronym: AI, UI, API, GPT, NVIDIA.
fn acronym(text: &str) -> bool {
    let mut letters = 0;
    for character in text.chars() {
        if !character.is_alphabetic() || !character.is_uppercase() {
            return false;
        }
        letters += 1;
    }
    (MIN_ACRONYM_CHARS..=MAX_ACRONYM_CHARS).contains(&letters)
}

/// Whether a token is a common window label rather than a name.
fn is_stopword(text: &str) -> bool {
    STOPLIST.iter().any(|word| text.eq_ignore_ascii_case(word))
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
    // gpt4o, Qwen3, gpt-4o, ES2015. Hashes, ordinals, times, units, and random
    // tokens are not names.
    if characters
        .iter()
        .any(|character| character.is_ascii_digit())
    {
        return plausible_digits(&characters);
    }
    if acronym(token.text) {
        return true;
    }
    let uppercase = characters
        .iter()
        .filter(|character| character.is_uppercase())
        .count();
    // WARNING, DEPRECATED, CHANGELOG: all caps, but not an allowed acronym.
    if uppercase == letters {
        return false;
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

/// Whether a token's digits look like part of a name rather than a hash,
/// ordinal, clock time, measurement, or random id.
fn plausible_digits(characters: &[char]) -> bool {
    let length = characters.len();
    let digits = characters
        .iter()
        .filter(|character| character.is_ascii_digit())
        .count();
    let letters = characters
        .iter()
        .filter(|character| character.is_alphabetic())
        .count();
    if digits == 0 || letters == 0 {
        return false;
    }
    // A hexadecimal id or git short SHA: 353ad51d, cafe2, deadbeef2.
    if characters
        .iter()
        .all(|character| character.is_ascii_hexdigit())
    {
        return false;
    }
    let lower: String = characters.iter().flat_map(|c| c.to_lowercase()).collect();
    // 2nd, 10am, 5pm, 10.30am, 100ms, 1080p, 30x.
    if ordinal(&lower) || time_of_day(&lower) || unit(&lower) {
        return false;
    }
    // More than about two-thirds digits: 1080p, 1000000.
    if digits * 3 > length * 2 {
        return false;
    }
    let uppercase = characters.iter().any(|character| character.is_uppercase());
    // All lowercase (or caseless) with at least half digits: v10, x9y9z9,
    // abc123.
    if !uppercase && digits * 2 >= length {
        return false;
    }
    // A long random token mixing case and digits: aB3xY9zQ7wE1rT5.
    if length > 12 && uppercase && characters.iter().any(|character| character.is_lowercase()) {
        return false;
    }
    true
}

/// Whether `lower` is an ordinal such as 2nd or 10th.
fn ordinal(lower: &str) -> bool {
    ["st", "nd", "rd", "th"].iter().any(|suffix| {
        lower.strip_suffix(suffix).is_some_and(|prefix| {
            !prefix.is_empty() && prefix.chars().all(|character| character.is_ascii_digit())
        })
    })
}

/// Whether `lower` is a clock time such as 10am, 5pm, or 10.30am.
fn time_of_day(lower: &str) -> bool {
    ["am", "pm"].iter().any(|suffix| {
        lower.strip_suffix(suffix).is_some_and(|prefix| {
            !prefix.is_empty()
                && prefix
                    .chars()
                    .all(|character| character.is_ascii_digit() || matches!(character, '.' | ':'))
                && prefix.chars().any(|character| character.is_ascii_digit())
        })
    })
}

/// Whether `lower` is a measurement such as 100ms, 1080p, or 30x.
fn unit(lower: &str) -> bool {
    UNIT_SUFFIXES.iter().any(|suffix| {
        lower.strip_suffix(suffix).is_some_and(|prefix| {
            !prefix.is_empty()
                && prefix
                    .chars()
                    .all(|character| character.is_ascii_digit() || matches!(character, '.' | '-'))
                && prefix.chars().any(|character| character.is_ascii_digit())
        })
    })
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
    fn two_letter_acronyms_are_kept() {
        assert_eq!(terms("Use AI in the UI with the API."), ["AI", "UI", "API"]);
    }

    #[test]
    fn common_window_labels_are_not_terms() {
        assert!(
            terms(
                "Open the Settings window and search Recent files. File Edit View Help. New Save Close Share More Today Home Sidebar."
            )
            .is_empty()
        );
    }

    #[test]
    fn log_levels_and_markers_are_not_terms() {
        assert!(terms("Use INFO, DEBUG, ERROR, WARN, TODO, FIXME, NOTE, TRACE.").is_empty());
    }

    #[test]
    fn long_all_caps_words_are_not_terms() {
        assert!(terms("Use WARNING here. Then DEPRECATED and CHANGELOG follow.").is_empty());
        assert_eq!(
            terms("Use NVIDIA with PostgreSQL."),
            ["NVIDIA", "PostgreSQL"]
        );
    }

    #[test]
    fn hashes_ordinals_times_units_and_random_tokens_are_not_terms() {
        for token in [
            "353ad51d",
            "cafe2",
            "deadbeef2",
            "2nd",
            "10am",
            "5pm",
            "100ms",
            "3x",
            "1080p",
            "v2",
            "v10",
            "abc123",
            "aB3xY9zQ7wE1rT5",
            "123456",
        ] {
            assert!(terms(&format!("Use {token} here.")).is_empty(), "{token}");
        }
        for token in ["gpt4o", "Qwen3", "ES2015", "GPT4"] {
            assert_eq!(terms(&format!("Use {token} here.")), [token], "{token}");
        }
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

/// The pending-capture and single-flight half of the reader
/// `recognition::context_watch` starts.
#[cfg(test)]
mod context_watch {
    use super::ScreenVocabulary;

    /// begin -> intend -> cancel -> intend -> finish: a cancelled capture's
    /// read never reaches the next dictation, and an intend with no pending
    /// capture starts nothing.
    #[test]
    fn a_cancelled_capture_leaves_no_stale_screen_terms() {
        let mut screen = ScreenVocabulary::default();
        // begin: a press that is not a real capture yet.
        assert!(!screen.read_now(false, true, &["Stale"]));
        assert!(screen.take().is_empty());
        // intend: the first real capture reads its window.
        assert!(screen.read_now(true, true, &["Qdrant"]));
        // cancel: its read is dropped, and finish sees nothing stale.
        screen.cancel();
        assert!(screen.take().is_empty());
        // intend without a pending capture, such as a loop pass after finish.
        assert!(!screen.read_now(false, true, &["Stale"]));
        assert!(screen.take().is_empty());
        // The next capture's own read is the only one that reaches it.
        assert!(screen.read_now(true, true, &["Rune"]));
        assert_eq!(screen.take(), ["Rune"]);
    }

    #[test]
    fn one_screen_read_runs_at_a_time() {
        let mut screen = ScreenVocabulary::default();
        screen.set_running(true);
        assert!(
            !screen.read_now(true, true, &["OpenCode"]),
            "a running reader is not doubled"
        );
        screen.set_running(false);
        assert!(screen.read_now(true, true, &["Qdrant"]));
        assert!(
            !screen.read_now(true, true, &["OpenCode"]),
            "a waiting read is not replaced"
        );
        assert_eq!(screen.take(), ["Qdrant"]);
        assert!(
            !screen.read_now(true, false, &["OpenCode"]),
            "a model that ignores terms reads nothing"
        );
        assert!(screen.take().is_empty());
    }
}
