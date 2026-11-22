//! German nouns' share of capitalised uses (`GermanCapitals`, `de.capitals` beside the German
//! typing model): one `word<TAB>percent` line per word, lowercase. The host reads the file.

use std::collections::HashMap;

use super::text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GermanCapitals {
    percentages: HashMap<String, u8>,
}

impl GermanCapitals {
    /// A word the books write with a capital this often is a noun wherever it stands.
    pub const ALWAYS_SHARE: u8 = 95;
    /// From this share it is a noun after an article or determiner.
    pub const SOMETIMES_SHARE: u8 = 50;

    pub fn new(percentages: HashMap<String, u8>) -> Self {
        let percentages = percentages
            .into_iter()
            .map(|(k, v)| (text::nfc(&k).into_owned(), v))
            .collect();
        Self { percentages }
    }

    /// The file's lines; `None` when none is usable (`init?(url:)`).
    pub fn parse(contents: &str) -> Option<Self> {
        let mut percentages = HashMap::new();
        for line in text::split_on(contents, "\n") {
            let fields = text::split_keeping_empty(line, "\t");
            if fields.len() != 2 {
                continue;
            }
            let Ok(percent) = fields[1].parse::<u8>() else {
                continue;
            };
            if !(Self::SOMETIMES_SHARE..=100).contains(&percent) {
                continue;
            }
            percentages.insert(text::nfc(fields[0]).into_owned(), percent);
        }
        (!percentages.is_empty()).then_some(Self { percentages })
    }

    /// The share of the word's uses written with a capital (50–100), or `None`.
    pub fn percent(&self, word: &str) -> Option<u8> {
        self.percentages
            .get(text::nfc(&text::lowercased(word)).as_ref())
            .copied()
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
