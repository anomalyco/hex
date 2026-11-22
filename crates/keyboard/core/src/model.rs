//! The typing model: a word list and word-pair language model in the HXTM v3 format that
//! `ios/scripts/build-typing-model.py` and `android/benchmark/typing/build-layout-model.py`
//! write. Read in place from the mapped file: nothing is copied or built per lookup.
//!
//! Ported from `TypingDecoderModel` (`ios/HEXShared/FullKeyboard/TypingDecoder.swift`).
//!
//! Layout, little-endian: a 28-byte header ("HXTM", version 3, word count, pair count, blob
//! bytes, alphabet size, 4 language bytes), the alphabet (per letter: u32 scalar, u8 code,
//! u8 base code, u16 0), then `u32[words + 1]` word offsets into the blob, `u8[words]`
//! unigram costs, padding to 4 bytes, `u32[words + 1]` pair rows, `u32[pairs]` second words
//! (sorted within each row), `u8[pairs]` pair costs, and the blob (one code per letter,
//! words sorted by their codes). Costs are -ln p in eighths of a nat.

use std::collections::HashMap;
use std::fmt;

use unicode_normalization::UnicodeNormalization;

const MAGIC: u32 = 0x4D54_5848; // "HXTM"
const VERSION: u32 = 3;
const HEADER: usize = 28;

/// Why a file isn't a model this reader can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    TooShort,
    NotHxtmV3,
    BadAlphabet,
    BadSize,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::TooShort => "shorter than an HXTM header",
            Self::NotHxtmV3 => "not an HXTM v3 file",
            Self::BadAlphabet => "the alphabet is malformed",
            Self::BadSize => "the sections don't add up to the file's size",
        };
        f.write_str(text)
    }
}

impl std::error::Error for ModelError {}

/// The bytes a model reads from: a memory map (the FFI's), or a vector (tests).
pub type ModelBytes = Box<dyn AsRef<[u8]> + Send + Sync>;

/// A word list sorted by letter codes, searchable letter by letter: the model's, or yours
/// (`TypingDecoderWords`).
pub trait SortedWords {
    fn word_count(&self) -> usize;
    fn length(&self, i: usize) -> usize;
    /// The word's letter code at `depth`, or -1 past its end.
    fn byte(&self, i: usize, depth: usize) -> i32;

    /// The words in `lo..hi` (which share their first `depth` codes) whose next code is
    /// `byte`.
    fn narrow(&self, lo: usize, hi: usize, depth: usize, byte: i32) -> (usize, usize) {
        let (mut a, mut b) = (lo, hi);
        while a < b {
            let mid = (a + b) / 2;
            if self.byte(mid, depth) < byte {
                a = mid + 1;
            } else {
                b = mid;
            }
        }
        let (mut c, mut d) = (a, hi);
        while c < d {
            let mid = (c + d) / 2;
            if self.byte(mid, depth) <= byte {
                c = mid + 1;
            } else {
                d = mid;
            }
        }
        (a, c)
    }
}

pub struct Model {
    bytes: ModelBytes,
    word_count: usize,
    bigram_count: usize,
    /// The language it was built for ("en", "es").
    language: String,
    /// Letter codes (1…) by character.
    codes: HashMap<char, u8>,
    /// Each code's character (index 0 unused) and its unaccented letter's code.
    characters: Vec<char>,
    base_codes: Vec<u8>,
    apostrophe: Option<u8>,
    offsets_at: usize,
    unigram_at: usize,
    pair_rows_at: usize,
    pair_words_at: usize,
    pair_costs_at: usize,
    blob_at: usize,
}

impl fmt::Debug for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Model")
            .field("language", &self.language)
            .field("words", &self.word_count)
            .field("pairs", &self.bigram_count)
            .finish()
    }
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
}

impl Model {
    /// Reads the header and alphabet; the rest is read in place on each lookup.
    pub fn new(bytes: ModelBytes) -> Result<Self, ModelError> {
        let data = (*bytes).as_ref();
        let size = data.len();
        if size < HEADER {
            return Err(ModelError::TooShort);
        }
        if u32_at(data, 0) != MAGIC as usize || u32_at(data, 4) != VERSION as usize {
            return Err(ModelError::NotHxtmV3);
        }
        let words = u32_at(data, 8);
        let bigrams = u32_at(data, 12);
        let blob = u32_at(data, 16);
        let letters = u32_at(data, 20);
        if letters >= 255 || HEADER + 8 * letters > size {
            return Err(ModelError::BadAlphabet);
        }
        let language_bytes: Vec<u8> = data[24..28].iter().copied().filter(|&b| b != 0).collect();
        let mut codes = HashMap::new();
        let mut characters = vec![' '; letters + 1];
        let mut base_codes = vec![0u8; letters + 1];
        for i in 0..letters {
            let at = HEADER + 8 * i;
            let scalar = char::from_u32(u32_at(data, at) as u32).ok_or(ModelError::BadAlphabet)?;
            let code = data[at + 4] as usize;
            let base = data[at + 5];
            if code < 1 || code > letters || base < 1 || base as usize > letters {
                return Err(ModelError::BadAlphabet);
            }
            codes.insert(scalar, code as u8);
            characters[code] = scalar;
            base_codes[code] = base;
        }
        let offsets_at = HEADER + 8 * letters;
        let unigram_at = offsets_at + 4 * (words + 1);
        let mut rows_at = unigram_at + words;
        rows_at += (4 - rows_at % 4) % 4;
        let pair_words_at = rows_at + 4 * (words + 1);
        let pair_costs_at = pair_words_at + 4 * bigrams;
        let blob_at = pair_costs_at + bigrams;
        if blob_at + blob != size {
            return Err(ModelError::BadSize);
        }
        let apostrophe = codes.get(&'\'').copied();
        Ok(Self {
            bytes,
            word_count: words,
            bigram_count: bigrams,
            language: String::from_utf8_lossy(&language_bytes).into_owned(),
            codes,
            characters,
            base_codes,
            apostrophe,
            offsets_at,
            unigram_at,
            pair_rows_at: rows_at,
            pair_words_at,
            pair_costs_at,
            blob_at,
        })
    }

    fn data(&self) -> &[u8] {
        (*self.bytes).as_ref()
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn bigram_count(&self) -> usize {
        self.bigram_count
    }

    pub fn byte_count(&self) -> usize {
        self.data().len()
    }

    /// The number of letter codes plus one (code 0 is unused): `characters.count` in Swift.
    pub fn code_count(&self) -> usize {
        self.characters.len()
    }

    /// The character of letter code `code` (1…).
    pub fn character(&self, code: usize) -> char {
        self.characters[code]
    }

    /// The code of `code`'s unaccented letter (its own when it has no accent).
    pub fn base_code(&self, code: usize) -> u8 {
        self.base_codes[code]
    }

    pub fn code_of(&self, c: char) -> Option<u8> {
        self.codes.get(&c).copied()
    }

    pub fn apostrophe_code(&self) -> Option<u8> {
        self.apostrophe
    }

    fn start(&self, i: usize) -> usize {
        u32_at(self.data(), self.offsets_at + 4 * i)
    }

    /// The word's letter codes, read in place.
    pub fn codes(&self, i: usize) -> &[u8] {
        let (s, e) = (self.start(i), self.start(i + 1));
        &self.data()[self.blob_at + s..self.blob_at + e]
    }

    pub fn word(&self, i: usize) -> String {
        self.codes(i)
            .iter()
            .map(|&c| self.characters[c as usize])
            .collect()
    }

    /// The word's letter codes, or `None` when it has a letter the model doesn't. Swift
    /// looks letters up by `Character`, which compares canonically equivalent spellings
    /// equal, so the word is composed (NFC) first.
    pub fn encode(&self, word: &str) -> Option<Vec<u8>> {
        word.nfc().map(|c| self.codes.get(&c).copied()).collect()
    }

    pub fn index_of_codes(&self, codes: &[u8]) -> Option<usize> {
        let (mut lo, mut hi) = (0, self.word_count);
        for (depth, &b) in codes.iter().enumerate() {
            (lo, hi) = self.narrow(lo, hi, depth, i32::from(b));
            if lo >= hi {
                return None;
            }
        }
        (lo < self.word_count && self.length(lo) == codes.len()).then_some(lo)
    }

    pub fn index(&self, word: &str) -> Option<usize> {
        self.index_of_codes(&self.encode(word)?)
    }

    /// -ln P(word), in nats.
    pub fn unigram_cost(&self, i: usize) -> f64 {
        f64::from(self.data()[self.unigram_at + i]) / 8.0
    }

    /// -ln P(b | a), in nats, when the pair is in the model.
    pub fn bigram_cost(&self, a: usize, b: usize) -> Option<f64> {
        if a >= self.word_count {
            return None;
        }
        let data = self.data();
        let mut lo = u32_at(data, self.pair_rows_at + 4 * a);
        let end = u32_at(data, self.pair_rows_at + 4 * (a + 1));
        if lo > end || end > self.bigram_count {
            return None;
        }
        let mut hi = end;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if u32_at(data, self.pair_words_at + 4 * mid) < b {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo < end && u32_at(data, self.pair_words_at + 4 * lo) == b {
            Some(f64::from(data[self.pair_costs_at + lo]) / 8.0)
        } else {
            None
        }
    }
}

impl SortedWords for Model {
    fn word_count(&self) -> usize {
        self.word_count
    }

    fn length(&self, i: usize) -> usize {
        self.start(i + 1) - self.start(i)
    }

    fn byte(&self, i: usize, depth: usize) -> i32 {
        let s = self.start(i);
        if depth < self.start(i + 1) - s {
            i32::from(self.data()[self.blob_at + s + depth])
        } else {
            -1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shipped model: `ios/HEXKeyboard/TypingModel/<language>.hextm`.
    pub(crate) fn bundled(language: &str) -> Model {
        let path = format!(
            "{}/../../../ios/HEXKeyboard/TypingModel/{language}.hextm",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        Model::new(Box::new(bytes)).expect("a valid model")
    }

    #[test]
    fn reads_the_english_model() {
        let model = bundled("en");
        assert_eq!(model.language(), "en");
        assert_eq!(model.word_count(), 86_247);
        assert_eq!(model.bigram_count(), 417_182);
        assert_eq!(model.apostrophe_code(), Some(1));
        assert_eq!(model.code_of('a'), Some(2));
    }

    #[test]
    fn every_word_round_trips() {
        for language in ["en", "es", "de"] {
            let model = bundled(language);
            for i in 0..model.word_count() {
                let word = model.word(i);
                assert_eq!(model.index(&word), Some(i), "{language} {word}");
            }
        }
    }

    #[test]
    fn decomposed_spellings_read_as_composed() {
        let model = bundled("es");
        assert_eq!(model.index("ma\u{F1}ana"), model.index("man\u{303}ana"));
        assert!(model.index("ma\u{F1}ana").is_some());
    }

    #[test]
    fn pairs_and_costs() {
        let model = bundled("en");
        let of = model.index("of").expect("of");
        let the = model.index("the").expect("the");
        assert!(model.bigram_cost(of, the).expect("of the") < model.unigram_cost(the) + 0.92);
        assert_eq!(model.bigram_cost(model.word_count(), the), None);
        assert_eq!(model.index("qzxv"), None);
    }

    #[test]
    fn rejects_other_files() {
        assert_eq!(
            Model::new(Box::new(vec![0u8; 10])).err(),
            Some(ModelError::TooShort)
        );
        assert_eq!(
            Model::new(Box::new(vec![0u8; 64])).err(),
            Some(ModelError::NotHxtmV3)
        );
    }
}
