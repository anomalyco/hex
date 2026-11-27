//! The cleanup tagger's tokenizer: BERT uncased WordPiece.
//!
//! It matches Hugging Face's `BertTokenizer` as the tagger was trained and
//! exported with it, and the iOS `CleanupTokenizer` id for id: special
//! tokens are split out first, then `BertNormalizer` (clean text, space
//! around CJK ideographs, NFD accent stripping, lowercasing),
//! `BertPreTokenizer` (whitespace and punctuation), and greedy
//! longest-match WordPiece with `##` continuations.
//!
//! Hugging Face's character classes come from Unicode 8.0 tables, so
//! punctuation, nonspacing marks, and control or format characters assigned
//! later are treated as ordinary characters, exactly as the trained model
//! saw them.

use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

pub(super) const PAD: i32 = 0;
pub(super) const UNKNOWN: i32 = 100;
pub(super) const CLS: i32 = 101;
pub(super) const SEP: i32 = 102;
const SPECIAL_TOKENS: [(&str, i32); 5] = [
    ("[PAD]", PAD),
    ("[UNK]", UNKNOWN),
    ("[CLS]", CLS),
    ("[SEP]", SEP),
    ("[MASK]", 103),
];
/// The tagger's vocabulary: `bert-base-uncased`'s.
const VOCABULARY_SIZE: usize = 30_522;
const MAX_CHARACTERS_PER_WORD: usize = 100;

/// Hugging Face's category tables stop at Unicode 8.0: a character assigned
/// later is in none of them.
static PUNCTUATION: LazyLock<Regex> = LazyLock::new(|| class(r"\p{P}"));
static NONSPACING_MARK: LazyLock<Regex> = LazyLock::new(|| class(r"\p{Mn}"));
static CONTROL: LazyLock<Regex> = LazyLock::new(|| class(r"[\p{Cc}\p{Cf}\p{Co}]"));

fn class(category: &str) -> Regex {
    Regex::new(&format!(r"^[{category}&&\p{{Age:8.0}}]$"))
        .expect("a Unicode class is a valid pattern")
}

fn is_in(class: &Regex, ch: char) -> bool {
    class.is_match(ch.encode_utf8(&mut [0; 4]))
}

/// The vocabulary file does not have one token per id.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WrongVocabularySize {
    pub(crate) lines: usize,
}

impl fmt::Display for WrongVocabularySize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the cleanup vocabulary has {} tokens, not {VOCABULARY_SIZE}",
            self.lines
        )
    }
}

impl std::error::Error for WrongVocabularySize {}

/// Turns words into the ids the tagger was trained on.
#[derive(Clone, Debug)]
pub(crate) struct Tokenizer {
    vocabulary: HashMap<String, i32>,
}

impl Tokenizer {
    /// `text` is `vocab.txt`: one token per line, and the line number is its
    /// id. A file of any other length belongs to another model.
    pub(crate) fn from_vocabulary(text: &str) -> Result<Self, WrongVocabularySize> {
        let mut lines: Vec<&str> = text.split('\n').collect();
        if lines.last() == Some(&"") {
            lines.pop();
        }
        if lines.len() != VOCABULARY_SIZE {
            return Err(WrongVocabularySize { lines: lines.len() });
        }
        let mut vocabulary = HashMap::with_capacity(lines.len());
        for (id, line) in (0..).zip(lines) {
            // A Windows checkout writes the file with CRLF.
            let token = line.strip_suffix('\r').unwrap_or(line);
            vocabulary.entry(token.to_owned()).or_insert(id);
        }
        Ok(Self { vocabulary })
    }

    /// WordPiece ids for one whitespace-separated word, without [CLS] or
    /// [SEP].
    pub(super) fn encode(&self, word: &str) -> Vec<i32> {
        let mut ids = Vec::new();
        let mut text = String::new();
        let mut rest = word;
        while let Some(first) = rest.chars().next() {
            let special = SPECIAL_TOKENS
                .iter()
                .find(|(token, _)| first == '[' && rest.starts_with(token));
            match special {
                Some((token, id)) => {
                    self.append_text(&text, &mut ids);
                    text.clear();
                    ids.push(*id);
                    rest = &rest[token.len()..];
                }
                None => {
                    text.push(first);
                    rest = &rest[first.len_utf8()..];
                }
            }
        }
        self.append_text(&text, &mut ids);
        ids
    }

    fn append_text(&self, text: &str, ids: &mut Vec<i32>) {
        for piece in pre_tokenize(&normalize(text)) {
            ids.extend(self.word_piece(&piece));
        }
    }

    fn word_piece(&self, piece: &[char]) -> Vec<i32> {
        if piece.len() > MAX_CHARACTERS_PER_WORD {
            return vec![UNKNOWN];
        }
        let mut ids = Vec::new();
        let mut key = String::new();
        let mut start = 0;
        while start < piece.len() {
            let mut end = piece.len();
            let found = loop {
                if end == start {
                    break None;
                }
                key.clear();
                if start > 0 {
                    key.push_str("##");
                }
                key.extend(&piece[start..end]);
                if let Some(&id) = self.vocabulary.get(&key) {
                    break Some(id);
                }
                end -= 1;
            };
            let Some(id) = found else {
                return vec![UNKNOWN];
            };
            ids.push(id);
            start = end;
        }
        ids
    }
}

/// `BertNormalizer`.
fn normalize(text: &str) -> Vec<char> {
    let mut cleaned = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '\0' || ch == '\u{FFFD}' || is_control(ch) {
            continue;
        }
        if ch.is_whitespace() {
            cleaned.push(' ');
        } else if is_chinese_character(ch) {
            cleaned.extend([' ', ch, ' ']);
        } else {
            cleaned.push(ch);
        }
    }
    cleaned
        .nfd()
        .filter(|&ch| !is_in(&NONSPACING_MARK, ch))
        .flat_map(char::to_lowercase)
        .collect()
}

/// `BertPreTokenizer`.
fn pre_tokenize(text: &[char]) -> Vec<Vec<char>> {
    let mut pieces = Vec::new();
    let mut current = Vec::new();
    for &ch in text {
        if ch.is_whitespace() || is_punctuation(ch) {
            if !current.is_empty() {
                pieces.push(std::mem::take(&mut current));
            }
            if !ch.is_whitespace() {
                pieces.push(vec![ch]);
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

fn is_control(ch: char) -> bool {
    !matches!(ch, '\t' | '\n' | '\r') && is_in(&CONTROL, ch)
}

fn is_punctuation(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_punctuation()
    } else {
        is_in(&PUNCTUATION, ch)
    }
}

fn is_chinese_character(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B920..=0x2CEAF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
}
