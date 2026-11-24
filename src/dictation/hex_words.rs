//! Hex words files (`.hexwords`): the shared vocabulary and the Global
//! mode's replacements as one file a person carries between Hex on the
//! desktop, Hex for iPhone, and Hex for Android. The format is
//! `docs/specs/hex-words.md`; `tests/fixtures/hex-words` holds the cases
//! every Hex must pass.
//!
//! A file is something a stranger can send, so [`read`] trusts none of it:
//! it refuses a file that isn't a words file and skips, with a reason, every
//! entry that breaks a rule. Nothing applies until the person reviews the
//! file and presses Add, which runs [`merge`]; [`Undo`] keeps both lists
//! from before it. [`export`] writes the desktop's own lists.

use std::fmt;

mod check;
mod clean;
mod merge;
mod read;
mod undo;
mod write;

#[cfg(test)]
mod shared_cases_tests;

pub(crate) use merge::{Clash, Limits, Lists, Merged, Parts, merge};
pub(crate) use read::{OpenError, open, read};
pub(crate) use undo::Undo;
pub(crate) use write::{Export, export};

/// The name the save dialog offers.
pub(crate) const FILE_NAME: &str = "hex-words.hexwords";
/// The extension Open… filters on, without its dot.
pub(crate) const EXTENSION: &str = "hexwords";
/// The largest file a reader accepts, in bytes.
pub(crate) const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

const FORMAT: &str = "hex.words";
/// The version this reader knows and every writer writes.
const VERSION: u64 = 1;
/// `{` and `[` levels outside strings; the top object is level 1.
const MAX_DEPTH: usize = 8;
const MAX_TERMS: usize = 500;
/// Characters (code points) in a term or a spelling.
const MAX_TERM_CHARS: usize = 64;
const MAX_SPELLINGS: usize = 16;
/// Words in a spelling or a heard phrase.
const MAX_WORDS: usize = 8;
const MAX_REPLACEMENTS: usize = 1000;
const MAX_HEARD_CHARS: usize = 128;
const MAX_WRITE_CHARS: usize = 1000;
const MAX_APP_CHARS: usize = 64;
const MAX_APP_VERSION_CHARS: usize = 32;

/// Why a whole file was refused. Nothing of a refused file is kept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    TooLarge,
    NotUtf8,
    Empty,
    TooDeep,
    DuplicateKey,
    NotJson,
    NotAnObject,
    NotHexWords,
    BadVersion,
}

impl Refusal {
    /// The spec's stable name, for tests and logs.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::TooLarge => "tooLarge",
            Self::NotUtf8 => "notUTF8",
            Self::Empty => "empty",
            Self::TooDeep => "tooDeep",
            Self::DuplicateKey => "duplicateKey",
            Self::NotJson => "notJSON",
            Self::NotAnObject => "notAnObject",
            Self::NotHexWords => "notHexWords",
            Self::BadVersion => "badVersion",
        }
    }
}

/// Why one part of a file was skipped while the rest was read.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum Reason {
    BadPart,
    BadEntry,
    BadField,
    UnknownField,
    HiddenCharacters,
    NoLetters,
    TooLong,
    TooManyWords,
    Duplicate,
    SameAsTerm,
    OverLimit,
}

impl Reason {
    /// The spec's stable name, for tests and logs.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::BadPart => "badPart",
            Self::BadEntry => "badEntry",
            Self::BadField => "badField",
            Self::UnknownField => "unknownField",
            Self::HiddenCharacters => "hiddenCharacters",
            Self::NoLetters => "noLetters",
            Self::TooLong => "tooLong",
            Self::TooManyWords => "tooManyWords",
            Self::Duplicate => "duplicate",
            Self::SameAsTerm => "sameAsTerm",
            Self::OverLimit => "overLimit",
        }
    }
}

/// A place in a file, written as the spec writes paths:
/// `vocabulary[3].soundsLike[0]`, `replacements[0].learned`, `madeBy.app`,
/// `colors`. A key named here comes from the file, so a place is shown and
/// compared, never logged.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Place {
    /// A top-level key: a part, `madeBy`, or an unknown field.
    Field(String),
    /// A key inside `madeBy`.
    MadeBy(String),
    Term(usize),
    TermField(usize, String),
    Spelling(usize, usize),
    Replacement(usize),
    ReplacementField(usize, String),
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Field(key) => write!(f, "{key}"),
            Self::MadeBy(key) => write!(f, "madeBy.{key}"),
            Self::Term(index) => write!(f, "vocabulary[{index}]"),
            Self::TermField(index, key) => write!(f, "vocabulary[{index}].{key}"),
            Self::Spelling(term, index) => write!(f, "vocabulary[{term}].soundsLike[{index}]"),
            Self::Replacement(index) => write!(f, "replacements[{index}]"),
            Self::ReplacementField(index, key) => write!(f, "replacements[{index}].{key}"),
        }
    }
}

/// One part of a file that was skipped, and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Skip {
    pub(crate) place: Place,
    pub(crate) reason: Reason,
}

/// A term as read: cleaned, with its spellings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Term {
    pub(crate) term: String,
    pub(crate) sounds_like: Vec<String>,
}

/// A replacement as read: cleaned. `learned` is the file's claim that a
/// phone learned it; merging drops it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Replacement {
    pub(crate) heard: String,
    pub(crate) write: String,
    pub(crate) learned: bool,
}

/// Which Hex the file says wrote it: shown as its claim, never trusted.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct MadeBy {
    pub(crate) app: Option<String>,
    pub(crate) version: Option<String>,
}

/// A file that was read: what it holds that this Hex accepts, and every
/// part it skipped.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct WordsFile {
    pub(crate) version: u64,
    pub(crate) made_by: MadeBy,
    pub(crate) vocabulary: Vec<Term>,
    pub(crate) replacements: Vec<Replacement>,
    pub(crate) skipped: Vec<Skip>,
}

impl WordsFile {
    /// Whether a newer Hex wrote it: the fields this one doesn't know were
    /// skipped as unknown.
    pub(crate) fn newer(&self) -> bool {
        self.version > VERSION
    }

    /// Whether it holds nothing to import.
    pub(crate) fn is_empty(&self) -> bool {
        self.vocabulary.is_empty() && self.replacements.is_empty()
    }
}
