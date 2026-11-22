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

    /// Word `a`'s pair row: the range of its entries in the pair tables, or `None` when the
    /// file's row is malformed.
    fn pair_row(&self, a: usize) -> Option<(usize, usize)> {
        let data = self.data();
        let start = u32_at(data, self.pair_rows_at + 4 * a);
        let end = u32_at(data, self.pair_rows_at + 4 * (a + 1));
        (start <= end && end <= self.bigram_count).then_some((start, end))
    }

    /// Pair entry `link`'s second word.
    fn follower(&self, link: usize) -> usize {
        u32_at(self.data(), self.pair_words_at + 4 * link)
    }

    /// Pair entry `link`'s cost byte (eighths of a nat).
    fn pair_cost_byte(&self, link: usize) -> usize {
        usize::from(self.data()[self.pair_costs_at + link])
    }

    /// Word `i`'s unigram cost byte (eighths of a nat).
    fn unigram_byte(&self, i: usize) -> usize {
        usize::from(self.data()[self.unigram_at + i])
    }

    /// Whether the model has `second` following `first` as a pair.
    pub fn has_pair(&self, first: &str, second: &str) -> bool {
        match (self.index(first), self.index(second)) {
            (Some(a), Some(b)) => self.bigram_cost(a, b).is_some(),
            _ => false,
        }
    }

    /// -ln P(word | previous) with the decoder's backoff to the unigram (0.92 nats), or `None`
    /// when the model lacks the word (`cost(of:after:)`).
    pub fn cost(&self, word: &str, previous: Option<&str>) -> Option<f64> {
        let i = self.index(word)?;
        let backoff = 0.92 + self.unigram_cost(i);
        let pair = previous
            .and_then(|p| self.index(p))
            .and_then(|a| self.bigram_cost(a, i));
        Some(match pair {
            Some(pair) => pair.min(backoff),
            None => backoff,
        })
    }

    /// The three most probable allowed continuations of a completed word. Scans just its
    /// bigram row and keeps three entries, even for a word with thousands of followers
    /// (`nextWords(after:allowing:)`).
    pub fn next_words(
        &self,
        previous: &str,
        mut allowing: impl FnMut(&str) -> bool,
    ) -> Vec<String> {
        let Some(a) = self.index(previous) else {
            return Vec::new();
        };
        let Some((start, end)) = self.pair_row(a) else {
            return Vec::new();
        };
        let mut best: Vec<(usize, String)> = Vec::new();
        for i in start..end {
            let cost = self.pair_cost_byte(i);
            let b = self.follower(i);
            if b >= self.word_count {
                continue;
            }
            // Prefer the commoner word when two pair probabilities quantize to the same byte.
            let rank = cost * 256 + self.unigram_byte(b);
            if best.len() == 3 && rank >= best[2].0 {
                continue;
            }
            let text = self.word(b);
            if !allowing(&text) {
                continue;
            }
            let at = best.iter().position(|e| rank < e.0).unwrap_or(best.len());
            best.insert(at, (rank, text));
            best.truncate(3);
        }
        best.into_iter().map(|(_, text)| text).collect()
    }

    /// Completes a one- to three-letter prefix with the word frequencies, backed by the
    /// previous word's bigrams when there are any. Word indices and each bigram row share
    /// their sort order, so this walks only the prefix's range and makes strings only for
    /// contenders (`completions(of:after:allowing:)`).
    pub fn completions(
        &self,
        prefix: &str,
        previous: Option<&str>,
        mut allowing: impl FnMut(&str) -> bool,
    ) -> Vec<String> {
        let Some(codes) = self.encode(prefix) else {
            return Vec::new();
        };
        if !(1..=3).contains(&codes.len()) {
            return Vec::new();
        }
        let (mut lo, mut hi) = (0, self.word_count);
        for (depth, &code) in codes.iter().enumerate() {
            (lo, hi) = self.narrow(lo, hi, depth, i32::from(code));
            if lo == hi {
                return Vec::new();
            }
        }
        let (mut link, mut end) = (0, 0);
        if let Some(a) = previous.and_then(|p| self.index(p)) {
            let Some((start, row_end)) = self.pair_row(a) else {
                return Vec::new();
            };
            (link, end) = (start, row_end);
            let mut upper = end;
            while link < upper {
                let mid = (link + upper) / 2;
                if self.follower(mid) < lo {
                    link = mid + 1;
                } else {
                    upper = mid;
                }
            }
        }
        let mut best: Vec<(usize, String)> = Vec::new();
        let mut next = if link < end { self.follower(link) } else { hi };
        for i in lo..hi {
            let unigram = self.unigram_byte(i);
            let mut cost = unigram + 7; // 0.92 nats of unigram backoff, in eighths of a nat.
            if i == next {
                cost = cost.min(self.pair_cost_byte(link));
                link += 1;
                next = if link < end { self.follower(link) } else { hi };
            }
            let rank = cost * 256 + unigram;
            let length = self.length(i);
            if length <= codes.len()
                || length > codes.len() + 8
                || !(best.len() < 2 || rank < best[1].0)
            {
                continue;
            }
            let text = self.word(i);
            if !allowing(&text) {
                continue;
            }
            let at = best.iter().position(|e| rank < e.0).unwrap_or(best.len());
            best.insert(at, (rank, text));
            best.truncate(2);
        }
        best.into_iter().map(|(_, text)| text).collect()
    }

    /// How much of the word list's frequency continues `prefix` with each letter, by letter
    /// code (`code_count()` entries). `None` when no word starts with `prefix`, or one ends
    /// there and none goes on (`continuations(after:)`).
    pub fn continuations(&self, prefix: &[u8]) -> Option<Vec<f64>> {
        let (mut lo, mut hi) = (0, self.word_count);
        for (depth, &b) in prefix.iter().enumerate() {
            (lo, hi) = self.narrow(lo, hi, depth, i32::from(b));
            if lo >= hi {
                return None;
            }
        }
        let mut mass = vec![0.0; self.characters.len()];
        let depth = prefix.len();
        let apostrophe = i32::from(self.apostrophe.unwrap_or(0));
        let mut any = false;
        for i in lo..hi {
            let code = self.byte(i, depth);
            if code <= 0 || code == apostrophe {
                continue;
            }
            mass[code as usize] += frequency(self.unigram_byte(i));
            any = true;
        }
        any.then_some(mass)
    }
}

/// `exp(-b / 8)`: the probability of a word with unigram cost byte `b`.
pub fn frequency(b: usize) -> f64 {
    f64::from_bits(FREQUENCY[b])
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

/// `exp(-b / 8)` for each stored unigram cost byte `b` (cost = b / 8 nats), frozen as Darwin's
/// libm computes it (`TypingDecoderModel.frequency`), so every OS weighs letters alike.
#[rustfmt::skip]
const FREQUENCY: [u64; 256] = [
    0x3ff0000000000000, 0x3fec3d6a24ed8222, 0x3fe8ebef9eac820b, 0x3fe5fe4615e98e8f,
    0x3fe368b2fc6f960a, 0x3fe120dc934993e8, 0x3fde3b40ebefcd7e, 0x3fdaadde095dad4b,
    0x3fd78b56362cef38, 0x3fd4c71b2477ab20, 0x3fd25618372a584f, 0x3fd02e8620c7602c,
    0x3fcc8f87724b5c1d, 0x3fc93466da99ee64, 0x3fc63e397e022073, 0x3fc3a122b3a399d5,
    0x3fc152aaa3bf81cc, 0x3fbe9328b4c50f6c, 0x3fbafb718e8457f7, 0x3fb7cfcc2d36c512,
    0x3fb50385c094f425, 0x3fb28b6973646d82, 0x3fb05d93892fa010, 0x3face293844545b3,
    0x3fa97db0ccceb0af, 0x3fa67ee6d9ff847c, 0x3fa3da368521902d, 0x3fa1850986289556,
    0x3f9eec1018e4ff66, 0x3f9b49e6a61c3aa9, 0x3f981509354f0d29, 0x3f9540a009abb2e2,
    0x3f92c155b8213cf4, 0x3f908d29c294bb37, 0x3f8d369111221752, 0x3f89c7cfd9fd3c47,
    0x3f86c0504695c417, 0x3f8413f04e14ba09, 0x3f81b7fadf9e7099, 0x3f7f45f9ff8cd45a,
    0x3f7b993fe00d5376, 0x3f785b0f914b97b3, 0x3f757e6bfe8de51b, 0x3f72f7dec82ce360,
    0x3f70bd4a5aca7728, 0x3f6d8b82d70b7139, 0x3f6a12c66dcd95d0, 0x3f670277e69d2306,
    0x3f644e51f113d4d6, 0x3f61eb805a03296c, 0x3f5fa0e9586aebc7, 0x3f5be97fd3b2d5e4,
    0x3f58a1e18a954ffa, 0x3f55bceba3dade80, 0x3f532f066b724b5d, 0x3f50edf6e429613f,
    0x3f4de16b9c24a98f, 0x3f4a5e96faf18f91, 0x3f47455fe323fafd, 0x3f44895d5630dceb,
    0x3f421f9ba40f31d5, 0x3f3ffce11bb755d0, 0x3f3c3aa91ff0d498, 0x3f38e981713b5a6d,
    0x3f35fc21041027ad, 0x3f3366ce6f0a10d9, 0x3f311f30f59bf24b, 0x3f2e384e2ea1fd27,
    0x3f2aab43fb3940a1, 0x3f27890a6b80cb3c, 0x3f24c5146b08fdd6, 0x3f22544e715f68db,
    0x3f202cf22526545a, 0x3f1c8cbe6b4a2a02, 0x3f1931f19c066a3c, 0x3f163c0e2f9a8e2e,
    0x3f139f38a5498fe2, 0x3f1150fa2aabd864, 0x3f0e902d64e003bb, 0x3f0af8cfefa7b79c,
    0x3f07cd79b5647c9b, 0x3f05017922d4b0f7, 0x3f02899a7a835446, 0x3f005bfaf6e08e35,
    0x3efcdfc263f6a0ba, 0x3ef97b34688c4f39, 0x3ef67cb53ce7662b, 0x3ef3d846e5d21f34,
    0x3ef18354238f6764, 0x3eeee90c1d7b684d, 0x3eeb473d6087e981, 0x3ee812affcecdac2,
    0x3ee53e8d76780b04, 0x3ee2bf817d0b83b8, 0x3ee08b8c8c3ff7ab, 0x3edd33b7bff95c52,
    0x3ed9c54c3b43bc8b, 0x3ed6be184875fe35, 0x3ed411fb0da07713, 0x3ed1b64085373070,
    0x3ecf42ed3f68e690, 0x3ecb968edd81dd76, 0x3ec858af84b74310, 0x3ec57c53649338f0,
    0x3ec2f6053b981d98, 0x3ec0bba872f18a5b, 0x3ebd88a13d378348, 0x3eba103b7f9848e2,
    0x3eb7003974e945df, 0x3eb44c56ff1c44eb, 0x3eb1e9c0f95bc10d, 0x3eaf9dd3ba0d8e03,
    0x3eabe6c6fdb01612, 0x3ea89f7a95f389d7, 0x3ea5baccf1932cc0, 0x3ea32d277de79658,
    0x3ea0ec503d2696e8, 0x3e9dde81a18f2ae7, 0x3e9a5c04a7fea8dc, 0x3e97431aeb19a79f,
    0x3e94875ca227ec38, 0x3e921dd72e8b8a27, 0x3e8ff9c285574ba5, 0x3e8c37e85fb538f0,
    0x3e88e7138077168d, 0x3e85f9fc27c27ae6, 0x3e8364ea10e59277, 0x3e811d8581a1e756,
    0x3e7e355bbaee85cb, 0x3e7aa8aa2e092431, 0x3e7786beda27169f, 0x3e74c30de4307547,
    0x3e725284d838ef8c, 0x3e702b5e50eadd7d, 0x3e6c89f5a9d1f539, 0x3e692f7c9ad03786,
    0x3e6639e3175a689d, 0x3e639d4ec6b9f3d3, 0x3e614f49dbc5078c, 0x3e5e8d325f6b55c8,
    0x3e5af62e927c4664, 0x3e57cb27778b510b, 0x3e54ff6cb83daa3f, 0x3e5287cbaec88040,
    0x3e505a628c699fa1, 0x3e4cdcf189fb29d6, 0x3e4978b84259adeb, 0x3e467a83d6942bdf,
    0x3e43d65776d812bc, 0x3e41819eeb9db47d, 0x3e3ee6086d5aa206, 0x3e3b44945d63cb97,
    0x3e381056ff2c5772, 0x3e353c7b1702630c, 0x3e32bdad6f9f5835, 0x3e3089ef7e3732ce,
    0x3e2d30deb5f04c13, 0x3e29c2c8db4e7281, 0x3e26bbe081ba5d88, 0x3e241005fe0e234e,
    0x3e21b48655f37267, 0x3e1f3fe0cb68b20c, 0x3e1b93de1e27ca3b, 0x3e18564fb36f19dd,
    0x3e157a3afeed00ab, 0x3e12f42bdd31abcb, 0x3e10ba06b3d9c887, 0x3e0d85bfeb520f5e,
    0x3e0a0db0d0ddb3ec, 0x3e06fdfb3b3a9e2f, 0x3e044a5c3e94c788, 0x3e01e801c4554a85,
    0x3dff9abe68b14e81, 0x3dfbe40e6ba21a48, 0x3df89d13dd4a5b3a, 0x3df5b8ae74381863,
    0x3df32b48bf117da2, 0x3df0eaa9bf5b788d, 0x3deddb97efb94ee8, 0x3dea5972953f0f15,
    0x3de740d62bb76e50, 0x3de4855c201eceb3, 0x3de21c12e527b0c6, 0x3ddff6a43cd84879,
    0x3ddc3527e433fab1, 0x3dd8e4a5cc59cba3, 0x3dd5f7d780fb4f71, 0x3dd36305e1fd7f2e,
    0x3dd11bda375761f2, 0x3dce326990ce39e4, 0x3dcaa610a1c7025e, 0x3dc78473821a3a52,
    0x3dc4c1078fe9228a, 0x3dc250bb6bb291df, 0x3dc029caa4112409, 0x3dbc872d2ddbf5cb,
    0x3db92d07d6f15a44, 0x3db637b8353c67c4, 0x3db39b6517f01c8b, 0x3db14d99b706f257,
    0x3dae8a37a45fc32e, 0x3daaf38d76fb9c48, 0x3da7c8d573a59b12, 0x3da4fd6080cae2bb,
    0x3da285fd102f8a43, 0x3da058ca49c6f198, 0x3d9cda20f64c0557, 0x3d99763c5a30bf60,
    0x3d967852a7007e42, 0x3d93d468382eb41c, 0x3d917fe9de4f53c1, 0x3d8ee305087b5510,
    0x3d8b41eb9ca96642, 0x3d880dfe3c07cb77, 0x3d853a68eb45af36, 0x3d82bbd98fd84672,
    0x3d80885298767e9a, 0x3d7d2e05f2fff6f6, 0x3d79c045ba173f2d, 0x3d76b9a8f25d7b33,
    0x3d740e111f58fa5b, 0x3d71b2cc51cf0184, 0x3d6f3cd4a384c9f4, 0x3d6b912da1f88c45,
    0x3d6853f01d6d53ba, 0x3d657822cd9621de, 0x3d62f252acf50d0c, 0x3d60b8651d7f383a,
    0x3d5d82dee15411b1, 0x3d5a0b266197a626, 0x3d56fbbd398bb567, 0x3d544861af788a73,
    0x3d51e642baeb84a0, 0x3d4f97a9644eaad0, 0x3d4be1561d8241f9, 0x3d489aad6093eaef,
    0x3d45b6902bc47821, 0x3d43296a2eeb7336, 0x3d40e9036ac4012b, 0x3d3dd8ae869bfd5d,
    0x3d3a56e0c2ac7f75, 0x3d373e91a4f7c89d, 0x3d34835bd010a41b, 0x3d321a4ec7df581d,
    0x3d2ff3864232b407, 0x3d2c3267ad666608, 0x3d28e23854dd8f7a, 0x3d25f5b30fb56d04,
    0x3d236121e24d3bba, 0x3d211a2f16b8516c, 0x3d1e2f77b039ec9f, 0x3d1aa377566c862b,
    0x3d17822863549fce, 0x3d14bf016e2e1731, 0x3d124ef22bc7f5bc, 0x3d1028371e9550ce,
];

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
    fn the_frozen_frequencies_are_exp() {
        for b in 0..256 {
            let exact = (-(b as f64) / 8.0).exp();
            // Darwin's libm gives these bits; another libm may round the last place apart.
            assert!((frequency(b) - exact).abs() <= exact * 1e-15, "{b}");
        }
        assert_eq!(frequency(0), 1.0);
    }

    #[test]
    fn costs_pairs_and_predictions() {
        let model = bundled("en");
        assert!(model.has_pair("of", "the"));
        assert!(!model.has_pair("of", "qzxv"));
        let the = model.index("the").expect("the");
        assert_eq!(
            model.cost("the", None),
            Some(0.92 + model.unigram_cost(the))
        );
        assert!(model.cost("the", Some("of")) < model.cost("the", None));
        assert_eq!(model.cost("qzxv", None), None);
        assert_eq!(model.next_words("of", |_| true).len(), 3);
        assert!(model.next_words("qzxv", |_| true).is_empty());
        let completions = model.completions("th", None, |_| true);
        assert_eq!(completions.len(), 2);
        assert!(
            completions
                .iter()
                .all(|w| w.starts_with("th") && w.len() > 2)
        );
        assert!(model.completions("th", None, |_| false).is_empty());
        assert!(model.completions("", None, |_| true).is_empty());
        let mass = model
            .continuations(&model.encode("th").expect("th"))
            .expect("mass");
        assert_eq!(mass.len(), model.code_count());
        assert!(mass[usize::from(model.code_of('e').expect("e"))] > 0.0);
        assert_eq!(mass[usize::from(model.apostrophe_code().expect("'"))], 0.0);
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
