//! The model's tokenizer: Hugging Face's byte-level BPE, read from the
//! model folder's `tokenizer.json`, with no tokenizer crate.
//!
//! The model was trained on words given one at a time
//! (`is_split_into_words`), and this tokenizer adds no space before a word,
//! so each word is encoded on its own: NFC, the tokenizer's added tokens cut
//! out, GPT-2's pre-tokenizer split (contractions, letters, digits, other
//! marks), then each piece's UTF-8 bytes as byte-level characters merged by
//! rank. Each word is read at its first sub-token, between [CLS] and [SEP].
//! `parity_tests` holds the ids to Python's `AutoTokenizer`.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

/// GPT-2's pre-tokenizer pattern without its whitespace branches: a word
/// holds no whitespace.
static PIECES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+")
        .expect("a fixed pattern compiles")
});

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    added_tokens: Vec<Added>,
    normalizer: Option<Typed>,
    pre_tokenizer: Option<PreTokenizer>,
    model: Model,
}

#[derive(Deserialize)]
struct Added {
    id: u32,
    content: String,
}

#[derive(Deserialize)]
struct Typed {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct PreTokenizer {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    add_prefix_space: bool,
    #[serde(default = "yes")]
    use_regex: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct Model {
    #[serde(rename = "type")]
    kind: String,
    vocab: HashMap<String, u32>,
    merges: Vec<Merge>,
}

/// Newer files write a merge as a pair, older ones as "a b".
#[derive(Deserialize)]
#[serde(untagged)]
enum Merge {
    Pair([String; 2]),
    Joined(String),
}

/// The tokenizer, loaded once with its model.
pub(crate) struct Bpe {
    vocab: HashMap<String, u32>,
    /// Each mergeable pair of ids: its rank, and the id it merges into.
    merges: HashMap<(u32, u32), (u32, u32)>,
    /// The id of each byte's byte-level character. A vocabulary may leave
    /// out bytes valid UTF-8 never holds (0xC0, 0xC1, 0xF5 and up).
    bytes: [Option<u32>; 256],
    /// Added tokens a word may hold, longest first.
    added: Vec<(String, u32)>,
    pub(crate) cls: u32,
    pub(crate) sep: u32,
}

/// One model run's worth of words: their ids between [CLS] and [SEP], and
/// where each word's first sub-token is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Window {
    /// The words of the line it covers.
    pub(crate) words: Range<usize>,
    pub(crate) input_ids: Vec<i32>,
    /// For each word, the position of its first sub-token in `input_ids`.
    pub(crate) first: Vec<usize>,
}

/// GPT-2's map from bytes to printable characters.
fn byte_chars() -> [char; 256] {
    let mut chars = ['\0'; 256];
    let mut next = 256_u32;
    for byte in 0..=255_u8 {
        let printable = matches!(byte, b'!'..=b'~' | 0xA1..=0xAC | 0xAE..=0xFF);
        chars[usize::from(byte)] = if printable {
            char::from(byte)
        } else {
            let ch = char::from_u32(next).unwrap_or('\0');
            next += 1;
            ch
        };
    }
    chars
}

impl Bpe {
    /// Reads a `tokenizer.json`, refusing any it would not encode the way
    /// the model was trained.
    pub(crate) fn from_json(json: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(json)
            .map_err(|error| format!("the cleanup tokenizer is unreadable: {error}"))?;
        let unsupported =
            |what: &str| Err(format!("the cleanup tokenizer's {what} is not supported"));
        if file.model.kind != "BPE" {
            return unsupported("model");
        }
        if file.normalizer.as_ref().is_some_and(|it| it.kind != "NFC") {
            return unsupported("normalizer");
        }
        match &file.pre_tokenizer {
            Some(pre) if pre.kind == "ByteLevel" && !pre.add_prefix_space && pre.use_regex => {}
            _ => return unsupported("pre-tokenizer"),
        }
        let vocab = file.model.vocab;
        let chars = byte_chars();
        let mut bytes = [None; 256];
        for (byte, ch) in chars.iter().enumerate() {
            bytes[byte] = vocab.get(&ch.to_string()).copied();
        }
        if !(0..0x80).all(|byte| bytes[byte].is_some()) {
            return Err("the cleanup tokenizer is missing a byte".into());
        }
        let mut merges = HashMap::with_capacity(file.model.merges.len());
        for (rank, merge) in file.model.merges.into_iter().enumerate() {
            let (left, right) = match merge {
                Merge::Pair([left, right]) => (left, right),
                Merge::Joined(joined) => match joined.split_once(' ') {
                    Some((left, right)) => (left.to_owned(), right.to_owned()),
                    None => continue,
                },
            };
            let merged = format!("{left}{right}");
            if let (Some(&left), Some(&right), Some(&merged)) =
                (vocab.get(&left), vocab.get(&right), vocab.get(&merged))
            {
                merges.entry((left, right)).or_insert((rank as u32, merged));
            }
        }
        let special = |name: &str| {
            file.added_tokens
                .iter()
                .find(|token| token.content == name)
                .map(|token| token.id)
                .ok_or_else(|| format!("the cleanup tokenizer has no {name}"))
        };
        let (cls, sep) = (special("[CLS]")?, special("[SEP]")?);
        let mut added: Vec<(String, u32)> = file
            .added_tokens
            .iter()
            .filter(|token| !token.content.trim().is_empty())
            .map(|token| (token.content.clone(), token.id))
            .collect();
        added.sort_by_key(|(content, _)| std::cmp::Reverse(content.len()));
        Ok(Self {
            vocab,
            merges,
            bytes,
            added,
            cls,
            sep,
        })
    }

    /// One word's ids.
    pub(crate) fn word(&self, word: &str) -> Vec<u32> {
        let word: String = word.nfc().collect();
        let mut ids = Vec::new();
        let mut rest = word.as_str();
        while !rest.is_empty() {
            // The leftmost added token, the longest where two start together.
            let found = self
                .added
                .iter()
                .filter_map(|(content, id)| {
                    rest.find(content.as_str()).map(|at| (at, content, *id))
                })
                .min_by_key(|(at, content, _)| (*at, std::cmp::Reverse(content.len())));
            let Some((at, content, id)) = found else {
                self.text(rest, &mut ids);
                break;
            };
            self.text(&rest[..at], &mut ids);
            ids.push(id);
            rest = &rest[at + content.len()..];
        }
        ids
    }

    fn text(&self, text: &str, ids: &mut Vec<u32>) {
        for piece in PIECES.find_iter(text) {
            self.piece(piece.as_str(), ids);
        }
    }

    /// Merges a piece's byte-level characters, the lowest rank first and
    /// the leftmost of equal ranks, as Hugging Face's BPE does.
    fn piece(&self, piece: &str, ids: &mut Vec<u32>) {
        let mut symbols: Vec<u32> = piece
            .bytes()
            .filter_map(|byte| self.bytes[usize::from(byte)])
            .collect();
        loop {
            let best = symbols
                .windows(2)
                .enumerate()
                .filter_map(|(at, pair)| {
                    self.merges
                        .get(&(pair[0], pair[1]))
                        .map(|&(rank, merged)| (rank, at, merged))
                })
                .min();
            let Some((_, at, merged)) = best else { break };
            symbols[at] = merged;
            symbols.remove(at + 1);
        }
        ids.extend(symbols);
    }

    /// The id of a whole token, for tests.
    #[cfg(test)]
    pub(crate) fn id(&self, token: &str) -> Option<u32> {
        self.vocab.get(token).copied()
    }

    /// A line's words in windows of whole words, each at most `max_tokens`
    /// long with [CLS] and [SEP]. A word too long for a window of its own
    /// is cut to fit.
    pub(crate) fn windows<S: AsRef<str>>(&self, words: &[S], max_tokens: usize) -> Vec<Window> {
        let room = max_tokens.saturating_sub(2).max(1);
        let mut windows = Vec::new();
        let mut current = Window {
            words: 0..0,
            input_ids: vec![self.cls as i32],
            first: Vec::new(),
        };
        for (index, word) in words.iter().enumerate() {
            let mut ids = self.word(word.as_ref());
            ids.truncate(room);
            if current.input_ids.len() - 1 + ids.len() > room && !current.first.is_empty() {
                current.input_ids.push(self.sep as i32);
                let start = index;
                windows.push(std::mem::replace(
                    &mut current,
                    Window {
                        words: start..start,
                        input_ids: vec![self.cls as i32],
                        first: Vec::new(),
                    },
                ));
            }
            current.first.push(current.input_ids.len());
            current.input_ids.extend(ids.iter().map(|&id| id as i32));
            current.words.end = index + 1;
        }
        if !current.first.is_empty() {
            current.input_ids.push(self.sep as i32);
            windows.push(current);
        }
        windows
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A byte-level tokenizer that knows "he", "hel", "hello", and "lo".
    pub(in super::super) fn tiny() -> Bpe {
        Bpe::from_json(&tiny_json(false)).unwrap()
    }

    fn tiny_json(prefix_space: bool) -> String {
        let mut vocab: serde_json::Map<String, serde_json::Value> = byte_chars()
            .iter()
            .enumerate()
            .map(|(id, ch)| (ch.to_string(), serde_json::json!(id)))
            .collect();
        for (offset, token) in ["he", "hel", "lo", "hello", "[CLS]", "[SEP]", "[MASK]"]
            .iter()
            .enumerate()
        {
            vocab.insert((*token).into(), serde_json::json!(256 + offset));
        }
        let json = serde_json::json!({
            "added_tokens": [
                {"id": 260, "content": "[CLS]"},
                {"id": 261, "content": "[SEP]"},
                {"id": 262, "content": "[MASK]"},
            ],
            "normalizer": {"type": "NFC"},
            "pre_tokenizer": {"type": "ByteLevel", "add_prefix_space": prefix_space, "use_regex": true},
            "model": {"type": "BPE", "vocab": vocab, "merges": [["h", "e"], "he l", ["l", "o"], ["hel", "lo"]]},
        });
        json.to_string()
    }

    #[test]
    fn a_word_merges_by_rank_and_splits_like_gpt2() {
        let bpe = tiny();
        assert_eq!(bpe.word("hello"), [259]);
        assert_eq!(bpe.word("helo"), [257, u32::from(b'o')]);
        // "'s" is a contraction, "," another piece, and "é" two bytes.
        let quote = u32::from(b'\'');
        assert_eq!(
            bpe.word("hello's,"),
            [259, quote, u32::from(b's'), u32::from(b',')]
        );
        // Each byte's character is its own id here.
        assert_eq!(bpe.word("e\u{301}"), [0xC3, 0xA9], "composed to é first");
        // An added token is cut out of a word.
        assert_eq!(bpe.word("hello[MASK]lo"), [259, 262, 258]);
        assert_eq!(bpe.id("hel"), Some(257));
    }

    #[test]
    fn a_long_line_is_read_in_windows_of_whole_words() {
        let bpe = tiny();
        let words = ["hello", "helo", "hello", "lo"];
        let windows = bpe.windows(&words, 5);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].words, 0..2);
        assert_eq!(windows[0].input_ids, [260, 259, 257, 111, 261]);
        assert_eq!(windows[0].first, [1, 2]);
        assert_eq!(windows[1].words, 2..4);
        assert_eq!(windows[1].input_ids, [260, 259, 258, 261]);
        assert_eq!(windows[1].first, [1, 2]);
        assert!(bpe.windows::<&str>(&[], 512).is_empty());
        // A word longer than a window is cut to fit one.
        let long = bpe.windows(&["helohelohelo"], 5);
        assert_eq!(long[0].input_ids.len(), 5);
    }

    #[test]
    fn a_tokenizer_the_model_was_not_trained_with_is_refused() {
        assert!(Bpe::from_json(&tiny_json(true)).is_err());
        assert!(Bpe::from_json("{}").is_err());
    }
}
