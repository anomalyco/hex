//! German nouns' share of capitalised uses (`GermanCapitals`, `de.capitals` beside the German
//! typing model): one `word<TAB>percent` line per word, lowercase. The host reads the file.

use super::text;
use crate::model::ModelBytes;

/// One of Hex's sorted word lists beside a typing model (`HexWordList`): one lowercase spelling
/// per line, alone or followed by a tab and a number, sorted by UTF-8 bytes, newline-terminated.
/// Searched in place by bisection, so a mapped file costs only the pages its lookups touch.
pub(crate) struct WordList {
    bytes: ModelBytes,
}

impl std::fmt::Debug for WordList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WordList({} bytes)", self.data().len())
    }
}

impl WordList {
    /// `None` when the bytes are empty or don't end in a newline.
    pub(crate) fn new(bytes: ModelBytes) -> Option<Self> {
        let data = (*bytes).as_ref();
        (data.last() == Some(&b'\n')).then_some(Self { bytes })
    }

    fn data(&self) -> &[u8] {
        (*self.bytes).as_ref()
    }

    /// The number after `word`'s tab, or `None` when the list lacks the word or its line has
    /// none (an empty number reads 0, as `HexWordList.value(of:)` does).
    pub(crate) fn value(&self, word: &str) -> Option<u32> {
        let (end, next) = self.line(word)?;
        if end >= next {
            return None;
        }
        let mut value: u32 = 0;
        for &b in &self.data()[end + 1..next] {
            if !b.is_ascii_digit() {
                return None;
            }
            value = value.wrapping_mul(10).wrapping_add(u32::from(b - b'0'));
        }
        Some(value)
    }

    /// Where `word`'s line ends (its tab or newline) and where its newline is.
    fn line(&self, word: &str) -> Option<(usize, usize)> {
        let key = word.as_bytes();
        let base = self.data();
        if key.is_empty() {
            return None;
        }
        let (mut lo, mut hi) = (0, base.len());
        while lo < hi {
            let mut start = (lo + hi) / 2;
            while start > lo && base[start - 1] != b'\n' {
                start -= 1;
            }
            let mut end = start;
            while base[end] != b'\n' && base[end] != b'\t' {
                end += 1;
            }
            let mut next = end;
            while base[next] != b'\n' {
                next += 1;
            }
            let mut order: isize = 0;
            let mut k = 0;
            while order == 0 && k < key.len() && start + k < end {
                order = isize::from(base[start + k]) - isize::from(key[k]);
                k += 1;
            }
            if order == 0 {
                order = (end - start) as isize - key.len() as isize;
            }
            if order == 0 {
                return Some((end, next));
            }
            if order < 0 {
                lo = next + 1;
            } else {
                hi = start;
            }
        }
        None
    }
}

/// German nouns' share of capitalised uses (`GermanCapitals`): `de.capitals`, read in place.
#[derive(Debug)]
pub struct GermanCapitals {
    list: WordList,
}

impl GermanCapitals {
    /// A word the books write with a capital this often is a noun wherever it stands.
    pub const ALWAYS_SHARE: u8 = 95;
    /// From this share it is a noun after an article or determiner.
    pub const SOMETIMES_SHARE: u8 = 50;

    /// The list's bytes (`init?(url:)`); `None` when they aren't a list.
    pub fn new(bytes: ModelBytes) -> Option<Self> {
        Some(Self {
            list: WordList::new(bytes)?,
        })
    }

    /// The share of the word's uses written with a capital (50–100), or `None`.
    pub fn percent(&self, word: &str) -> Option<u8> {
        let percent = self.list.value(&text::lowercased(word))?;
        (u32::from(Self::SOMETIMES_SHARE)..=100)
            .contains(&percent)
            .then_some(percent as u8)
    }

    /// Whether `word`, all lowercase, takes a capital; `None` when the list doesn't know it.
    pub fn capitalises(&self, word: &str, previous: Option<&str>) -> Option<bool> {
        if text::count(word) < 2 || !text::eq(word, &text::lowercased(word)) {
            return None;
        }
        let share = self.percent(word)?;
        if share < Self::SOMETIMES_SHARE {
            return None;
        }
        if share >= Self::ALWAYS_SHARE {
            return Some(true);
        }
        Some(previous.is_some_and(|p| DETERMINERS.contains(&text::nfc(p).as_ref())))
    }
}

/// Articles, determiners, their contractions and possessives (`GermanCapitals.determiners`).
pub const DETERMINERS: &[&str] = &[
    "der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "einem", "einer", "eines",
    "kein", "keine", "keinen", "keinem", "keiner", "keines", "mein", "meine", "meinen", "meinem",
    "meiner", "meines", "dein", "deine", "deinen", "deinem", "deiner", "deines", "sein", "seine",
    "seinen", "seinem", "seiner", "seines", "ihre", "ihren", "ihrem", "ihrer", "ihres", "unser",
    "unsere", "unseren", "unserem", "unserer", "unseres", "euer", "eure", "euren", "eurem",
    "eurer", "eures", "dies", "dieser", "diese", "dieses", "diesem", "diesen", "jeder", "jede",
    "jedes", "jedem", "jeden", "im", "in", "am", "zum", "zur", "vom", "beim", "ins", "ans",
];
