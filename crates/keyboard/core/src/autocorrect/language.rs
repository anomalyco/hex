//! What autocorrect reads of a keyboard language (`KeyboardLanguage`): its code, spelling
//! dictionaries, letter rows (for the neighbouring-key cost), and its own word lists.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::text;

/// Each letter's (row, column), by the letter's composed form.
pub type KeyCoordinates = HashMap<String, (f64, f64)>;

/// `KeyboardLanguage`, by its raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    English,
    Norwegian,
    Spanish,
    German,
    Italian,
    French,
    FrenchCanadian,
    FrenchSwiss,
    Portuguese,
    Polish,
    Czech,
    Romanian,
    Russian,
    Ukrainian,
    Greek,
    SerbianLatin,
}

/// `KeyboardLetterLayout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LetterLayout {
    Own,
    Qwerty,
    Qwertz,
    Azerty,
    Dvorak,
}

impl LetterLayout {
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "own" => Self::Own,
            "qwerty" => Self::Qwerty,
            "qwertz" => Self::Qwertz,
            "azerty" => Self::Azerty,
            "dvorak" => Self::Dvorak,
            _ => return None,
        })
    }
}

impl Language {
    pub const ALL: [Language; 16] = [
        Self::English,
        Self::Norwegian,
        Self::Spanish,
        Self::German,
        Self::Italian,
        Self::French,
        Self::FrenchCanadian,
        Self::FrenchSwiss,
        Self::Portuguese,
        Self::Polish,
        Self::Czech,
        Self::Romanian,
        Self::Russian,
        Self::Ukrainian,
        Self::Greek,
        Self::SerbianLatin,
    ];

    /// `rawValue`.
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Norwegian => "nb",
            Self::Spanish => "es",
            Self::German => "de",
            Self::Italian => "it",
            Self::French => "fr",
            Self::FrenchCanadian => "fr-CA",
            Self::FrenchSwiss => "fr-CH",
            Self::Portuguese => "pt",
            Self::Polish => "pl",
            Self::Czech => "cs",
            Self::Romanian => "ro",
            Self::Russian => "ru",
            Self::Ukrainian => "uk",
            Self::Greek => "el",
            Self::SerbianLatin => "sr-Latn",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.code() == code)
    }

    /// `baseLanguage`: the code without its region (Apple's three French layouts are one
    /// language).
    pub fn base_language(self) -> &'static str {
        let code = self.code();
        code.split('-').next().unwrap_or(code)
    }

    /// `UITextChecker` language codes to try, best first.
    pub fn spell_check_candidates(self) -> &'static [&'static str] {
        match self {
            Self::English => &["en_US", "en"],
            Self::Norwegian => &["nb", "nb_NO", "no", "no_NO"],
            Self::Spanish => &["es", "es_ES", "es_MX"],
            Self::German => &["de", "de_DE"],
            Self::Italian => &["it", "it_IT"],
            Self::French => &["fr", "fr_FR", "fr_CA"],
            Self::FrenchCanadian => &["fr_CA", "fr", "fr_FR"],
            Self::FrenchSwiss => &["fr_CH", "fr", "fr_FR"],
            Self::Portuguese => &["pt_BR", "pt", "pt_PT"],
            Self::Polish => &["pl", "pl_PL"],
            Self::Czech => &["cs", "cs_CZ"],
            Self::Romanian => &["ro", "ro_RO"],
            Self::Russian => &["ru", "ru_RU"],
            Self::Ukrainian => &["uk", "uk_UA"],
            Self::Greek => &["el", "el_GR"],
            Self::SerbianLatin => &["sr_Latn", "sr_Latn_RS", "sr-Latn"],
        }
    }

    /// The first candidate the device has a dictionary for (`available`), or `None`.
    pub fn spell_check_language<S: AsRef<str>>(self, available: &[S]) -> Option<&'static str> {
        self.spell_check_candidates()
            .iter()
            .copied()
            .find(|c| available.iter().any(|a| text::eq(a.as_ref(), c)))
    }

    /// Whether the built-in English contractions and typo list apply.
    pub fn uses_english_fixes(self) -> bool {
        self == Self::English
    }

    /// Two-letter words common enough that a two-letter non-word one neighbouring key from
    /// one of them is fixed ("tk" -> "to"), commonest first.
    pub fn common_two_letter_words(self) -> &'static [&'static str] {
        match self {
            Self::English => &[
                "to", "of", "in", "it", "is", "be", "as", "at", "so", "we", "he", "by", "or", "on",
                "do", "if", "me", "my", "up", "an", "go", "no", "us", "am",
            ],
            Self::Spanish => &[
                "de", "la", "el", "en", "no", "se", "lo", "es", "un", "su", "me", "te", "ya", "mi",
                "tu", "yo", "le", "si", "al", "ha", "ni",
            ],
            Self::Italian => &[
                "di", "il", "la", "in", "un", "si", "le", "lo", "ha", "ho", "ma", "da", "mi", "ci",
                "ti", "se", "ne", "no", "io", "tu", "su", "al",
            ],
            _ => &[],
        }
    }

    /// Unaccented spellings space puts the accent back on (`accentRestorations`).
    pub fn accent_restoration(self, lower: &str) -> Option<&'static str> {
        if self != Self::Italian {
            return None;
        }
        Some(match text::nfc(lower).as_ref() {
            "perche" => "perché",
            "poiche" => "poiché",
            "affinche" => "affinché",
            "benche" => "benché",
            "finche" => "finché",
            "piu" => "più",
            "gia" => "già",
            "puo" => "può",
            "cioe" => "cioè",
            "percio" => "perciò",
            "citta" => "città",
            "societa" => "società",
            "universita" => "università",
            "qualita" => "qualità",
            "liberta" => "libertà",
            "verita" => "verità",
            "realta" => "realtà",
            "attivita" => "attività",
            "possibilita" => "possibilità",
            "difficolta" => "difficoltà",
            "opportunita" => "opportunità",
            "caffe" => "caffè",
            "giu" => "giù",
            "lunedi" => "lunedì",
            "martedi" => "martedì",
            "mercoledi" => "mercoledì",
            "giovedi" => "giovedì",
            "venerdi" => "venerdì",
            _ => return None,
        })
    }

    /// Accented forms offered in the strip, never put in by space (`accentSuggestions`).
    pub fn accent_suggestion(self, lower: &str) -> Option<&'static str> {
        if self != Self::Italian {
            return None;
        }
        match text::nfc(lower).as_ref() {
            "e" => Some("è"),
            "ce" => Some("c'è"),
            _ => None,
        }
    }

    /// The layouts offered for this language, its own first.
    pub fn letter_layouts(self) -> &'static [LetterLayout] {
        use LetterLayout::*;
        match self {
            Self::English => &[Qwerty, Azerty, Qwertz, Dvorak],
            Self::German => &[Own, Qwertz, Qwerty, Azerty],
            Self::Spanish => &[Own, Qwerty, Azerty, Qwertz],
            Self::French => &[Azerty, Qwerty, Qwertz],
            Self::FrenchCanadian => &[Qwerty, Azerty, Qwertz],
            Self::FrenchSwiss => &[Qwertz, Azerty, Qwerty],
            Self::Italian | Self::Portuguese => &[Qwerty, Azerty, Qwertz],
            Self::Polish | Self::Romanian => &[Qwerty, Qwertz, Azerty],
            Self::Czech => &[Own, Qwerty],
            Self::SerbianLatin => &[Qwertz, Qwerty, Azerty],
            Self::Norwegian | Self::Russian | Self::Ukrainian | Self::Greek => &[Own],
        }
    }

    /// `letters` where this language offers it, else its own layout.
    pub fn offered(self, letters: Option<LetterLayout>) -> LetterLayout {
        match letters {
            Some(l) if self.letter_layouts().contains(&l) => l,
            _ => self.letter_layouts()[0],
        }
    }

    fn own_letter_rows(self) -> [&'static str; 3] {
        match self {
            Self::English
            | Self::Italian
            | Self::FrenchCanadian
            | Self::Portuguese
            | Self::Polish
            | Self::Romanian => ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            Self::French => ["azertyuiop", "qsdfghjklm", "wxcvbn"],
            Self::FrenchSwiss | Self::SerbianLatin => ["qwertzuiop", "asdfghjkl", "yxcvbnm"],
            Self::Norwegian => ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"],
            Self::Spanish => ["qwertyuiop", "asdfghjklñ", "zxcvbnm"],
            Self::German => ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"],
            Self::Russian => ["йцукенгшщзх", "фывапролджэ", "ячсмитьбю"],
            Self::Ukrainian => ["йцукенгшщзхї", "фівапролджєʼ", "ячсмитьбюґ"],
            Self::Greek => ["ερτυθιοπ\u{B4}", "ασδφγηξκλ", "ζχψωβνμ"],
            Self::Czech => ["qwertzuiop", "asdfghjkl\u{B4}", "yxcvbnm\u{2C7}"],
        }
    }

    /// The three letter rows on `letters` (or the language's own where it doesn't offer it).
    pub fn letter_rows(self, letters: LetterLayout) -> [&'static str; 3] {
        match self.offered(Some(letters)) {
            LetterLayout::Own => self.own_letter_rows(),
            LetterLayout::Qwerty if self == Self::Czech => {
                ["qwertyuiop", "asdfghjkl\u{B4}", "zxcvbnm\u{2C7}"]
            }
            LetterLayout::Qwerty => ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            LetterLayout::Qwertz => ["qwertzuiop", "asdfghjkl", "yxcvbnm"],
            LetterLayout::Azerty => ["azertyuiop", "qsdfghjklm", "wxcvbn"],
            LetterLayout::Dvorak => ["pyfgcrl", "aoeuidhtns", "qjkxbmwvz"],
        }
    }

    /// Each letter's (row, column) on `letters`, rows shifted as the keys are
    /// (`keyCoordinates`), by the letter's composed form.
    pub fn key_coordinates(self, letters: Option<LetterLayout>) -> KeyCoordinates {
        let letters = self.offered(letters);
        let offsets: [f64; 3] = match letters {
            LetterLayout::Azerty => [0.0, 0.0, 2.0],
            LetterLayout::Dvorak => [1.85, 0.0, 1.33],
            LetterLayout::Own if matches!(self, Self::Ukrainian | Self::Greek | Self::Czech) => {
                [0.0, 0.0, 1.0]
            }
            LetterLayout::Qwerty if self == Self::Czech => [0.0, 0.0, 1.0],
            _ => [0.0, 0.5, 1.5],
        };
        let mut map = HashMap::new();
        for (row, letters) in self.letter_rows(letters).iter().enumerate() {
            for (column, c) in letters.chars().enumerate() {
                let key = text::nfc(text::first(&c.to_string())).into_owned();
                map.insert(key, (row as f64, column as f64 + offsets[row]));
            }
        }
        map
    }

    /// `keyCoordinates` for the layout `letters` resolves to, made once per process.
    pub fn cached_key_coordinates(self, letters: Option<LetterLayout>) -> &'static KeyCoordinates {
        static ALL: OnceLock<HashMap<(Language, LetterLayout), KeyCoordinates>> = OnceLock::new();
        let all = ALL.get_or_init(|| {
            let mut out = HashMap::new();
            for language in Language::ALL {
                for &layout in language.letter_layouts() {
                    out.insert((language, layout), language.key_coordinates(Some(layout)));
                }
            }
            out
        });
        &all[&(self, self.offered(letters))]
    }

    /// Common sentence-initial words (`AutocorrectEngine.sentenceStarters`).
    pub fn sentence_starters(self) -> &'static [&'static str] {
        match self {
            Self::English => &["I", "the", "I'm"],
            Self::Spanish => &["yo", "el", "la"],
            Self::German => &["ich", "das", "die"],
            Self::French | Self::FrenchCanadian | Self::FrenchSwiss => &["je", "le", "la"],
            Self::Italian => &["io", "il", "la"],
            Self::Portuguese => &["eu", "o", "a"],
            Self::Polish => &["ja", "to", "nie"],
            Self::Czech => &["to", "já", "ale"],
            Self::Romanian => &["eu", "nu", "am"],
            Self::Norwegian => &["jeg", "det", "vi"],
            Self::Russian => &["я", "это", "мы"],
            Self::Ukrainian => &["я", "це", "ми"],
            Self::Greek => &["θα", "δεν", "το"],
            Self::SerbianLatin => &["ja", "to", "da"],
        }
    }

    /// Common first words of a clause, after a comma (`clauseStarters`).
    pub fn clause_starters(self) -> &'static [&'static str] {
        match self {
            Self::English => &["I", "but", "and"],
            Self::Spanish => &["que", "pero", "y"],
            Self::German => &["und", "aber", "ich"],
            Self::French | Self::FrenchCanadian | Self::FrenchSwiss => &["et", "mais", "je"],
            Self::Italian => &["e", "ma", "che"],
            Self::Portuguese => &["e", "mas", "que"],
            Self::Polish => &["a", "ale", "że"],
            Self::Czech => &["a", "ale", "že"],
            Self::Romanian => &["și", "dar", "că"],
            Self::Norwegian => &["og", "men", "jeg"],
            Self::Russian => &["и", "но", "что"],
            Self::Ukrainian => &["і", "але", "що"],
            Self::Greek => &["και", "αλλά", "που"],
            Self::SerbianLatin => &["i", "ali", "da"],
        }
    }

    /// The language's commonest words, for next-word slots the model can't fill.
    pub fn common_words(self) -> &'static [&'static str] {
        match self {
            Self::English => &["the", "to", "and", "a", "I", "of"],
            Self::Spanish => &["de", "que", "y", "la", "el", "en"],
            Self::German => &["und", "die", "der", "ich", "ist", "das"],
            Self::French | Self::FrenchCanadian | Self::FrenchSwiss => {
                &["de", "la", "et", "le", "je", "à"]
            }
            Self::Italian => &["di", "e", "che", "la", "il", "a"],
            Self::Portuguese => &["de", "que", "e", "o", "a", "do"],
            Self::Polish => &["nie", "i", "w", "się", "na", "to"],
            Self::Czech => &["a", "se", "je", "v", "na", "to"],
            Self::Romanian => &["de", "și", "în", "nu", "să", "la"],
            Self::Norwegian => &["og", "i", "det", "er", "på", "som"],
            Self::Russian => &["и", "в", "не", "на", "я", "что"],
            Self::Ukrainian => &["і", "в", "не", "на", "що", "я"],
            Self::Greek => &["και", "να", "το", "θα", "δεν", "η"],
            Self::SerbianLatin => &["i", "je", "da", "u", "se", "na"],
        }
    }
}
