//! What autocorrect reads beside a handle: the never-suggest list (`hk_never_suggest_*`),
//! German's capitals (`hk_german_capitals_*`), and the questions about text and languages
//! that need no handle (`hk_text*`, `hk_edit_cost`, `hk_language_words`).

use std::ffi::{OsStr, c_char};
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::ptr;
use std::sync::Arc;

use hex_keyboard_core::autocorrect::{
    self as ac, GermanCapitals, KeyCell, KeyGrid, LanguageConfig, NeverSuggestList, OffensiveKind,
    lists,
};
use memmap2::Mmap;

use super::decide::{HkStringList, write_list};
use super::{BAD_ARGUMENT, FAILED, HkGermanCapitals, HkNeverSuggest, OK, write_text};
use crate::swipe::HkKeyCentre;
use crate::tap::optional_utf8;
use crate::{guard, slice, utf8};

/// The never-suggest list's text (`never-suggest.tsv`); NULL on a bad argument.
///
/// # Safety
/// `tsv` is NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_never_suggest_new(tsv: *const c_char) -> *mut HkNeverSuggest {
    // SAFETY: as the caller promises.
    let Some(tsv) = (unsafe { utf8(tsv) }) else {
        return ptr::null_mut();
    };
    guard(ptr::null_mut(), || {
        let list = Arc::new(NeverSuggestList::new(tsv));
        Box::into_raw(Box::new(HkNeverSuggest { list }))
    })
}

/// # Safety
/// `list` is NULL or from `hk_never_suggest_new`, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_never_suggest_free(list: *mut HkNeverSuggest) {
    if !list.is_null() {
        // SAFETY: from Box::into_raw, as the caller promises.
        guard((), || drop(unsafe { Box::from_raw(list) }));
    }
}

/// `language`'s kind of `word` (a spelling or two-word expression, any case): 0 none, 1 a
/// slur, 2 a swear word; -1 on a bad argument. `language` is the list's language column
/// ("en", "fr"; others read the shared `*` rows).
///
/// # Safety
/// `list` NULL or live; the strings NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_never_suggest_kind(
    list: *const HkNeverSuggest,
    language: *const c_char,
    word: *const c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe { (list.as_ref(), utf8(language), utf8(word)) };
    let (Some(list), Some(language), Some(word)) = args else {
        return -1;
    };
    guard(-1, || match list.list.bundled(language).kind(word) {
        None => 0,
        Some(OffensiveKind::Slur) => 1,
        Some(OffensiveKind::Swear) => 2,
    })
}

/// Whether `candidate` may not be offered in `language` (`OffensiveWords.blocks`): 1 or 0, -1
/// on a bad argument. `previous` NULL for none.
///
/// # Safety
/// `list` NULL or live; the strings NUL-terminated, `previous` may be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_never_suggest_blocks(
    list: *const HkNeverSuggest,
    language: *const c_char,
    candidate: *const c_char,
    allow_swearing: u8,
    previous: *const c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe {
        (
            list.as_ref(),
            utf8(language),
            utf8(candidate),
            optional_utf8(previous),
        )
    };
    let (Some(list), Some(language), Some(candidate), Ok(previous)) = args else {
        return -1;
    };
    guard(-1, || {
        let words = list.list.bundled(language);
        i32::from(words.blocks(candidate, allow_swearing != 0, previous))
    })
}

/// `language`'s spellings and expressions to block (`blocked(allowSwearing:)`), sorted. Returns
/// 0 and fills `out` (release it with `hk_string_list_free`), else nonzero.
///
/// # Safety
/// `list` NULL or live; `language` NUL-terminated; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_never_suggest_blocked(
    list: *const HkNeverSuggest,
    language: *const c_char,
    allow_swearing: u8,
    out: *mut HkStringList,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe { (list.as_ref(), utf8(language)) };
    let (Some(list), Some(language), false) = (args.0, args.1, out.is_null()) else {
        return BAD_ARGUMENT;
    };
    guard(FAILED, || {
        let mut words = list.list.bundled(language).blocked(allow_swearing != 0);
        words.sort();
        // SAFETY: non-NULL and writable, as the caller promises.
        unsafe { write_list(out, words) }
    })
}

/// A read-only map of a list file.
struct Mapped(Mmap);

impl AsRef<[u8]> for Mapped {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Maps `de.capitals` read-only; NULL when it can't be opened or isn't a list.
///
/// # Safety
/// `path` is NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_german_capitals_open(path: *const c_char) -> *mut HkGermanCapitals {
    if path.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: non-NULL, so NUL-terminated, as the caller promises.
    let path = OsStr::from_bytes(unsafe { std::ffi::CStr::from_ptr(path) }.to_bytes());
    guard(None, || {
        let file = File::open(path).ok()?;
        // SAFETY: a read-only shared map of a file shipped inside the app, never written while
        // mapped (as the models', `hk_model_open`).
        let map = unsafe { Mmap::map(&file) }.ok()?;
        let capitals = GermanCapitals::new(Box::new(Mapped(map)))?;
        Some(Box::into_raw(Box::new(HkGermanCapitals {
            capitals: Arc::new(capitals),
        })))
    })
    .unwrap_or(ptr::null_mut())
}

/// A copy of `count` bytes of a list (tests give their own); NULL when they aren't one.
///
/// # Safety
/// `bytes` holds `count` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_german_capitals_new(
    bytes: *const u8,
    count: usize,
) -> *mut HkGermanCapitals {
    // SAFETY: as the caller promises.
    let Some(bytes) = (unsafe { slice(bytes, count) }) else {
        return ptr::null_mut();
    };
    guard(None, || {
        let capitals = GermanCapitals::new(Box::new(bytes.to_vec()))?;
        Some(Box::into_raw(Box::new(HkGermanCapitals {
            capitals: Arc::new(capitals),
        })))
    })
    .unwrap_or(ptr::null_mut())
}

/// # Safety
/// `capitals` is NULL or from `hk_german_capitals_*`, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_german_capitals_free(capitals: *mut HkGermanCapitals) {
    if !capitals.is_null() {
        // SAFETY: from Box::into_raw, as the caller promises.
        guard((), || drop(unsafe { Box::from_raw(capitals) }));
    }
}

/// `word`'s share of capitalised uses (50 to 100), 0 when the list doesn't know it, -1 on a bad
/// argument (`percent(of:)`).
///
/// # Safety
/// `capitals` NULL or live; `word` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_german_capitals_percent(
    capitals: *const HkGermanCapitals,
    word: *const c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let (Some(capitals), Some(word)) = (unsafe { capitals.as_ref() }, unsafe { utf8(word) }) else {
        return -1;
    };
    guard(-1, || capitals.capitals.percent(word).map_or(0, i32::from))
}

/// Whether `word`, all lowercase, takes a capital after `previous` (NULL: none): 1 yes, 0 no,
/// 2 when the list doesn't know it (`capitalises(_:after:)` nil), -1 on a bad argument.
///
/// # Safety
/// `capitals` NULL or live; `word` NUL-terminated; `previous` NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_german_capitals_capitalises(
    capitals: *const HkGermanCapitals,
    word: *const c_char,
    previous: *const c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe { (capitals.as_ref(), utf8(word), optional_utf8(previous)) };
    let (Some(capitals), Some(word), Ok(previous)) = args else {
        return -1;
    };
    guard(-1, || match capitals.capitals.capitalises(word, previous) {
        Some(true) => 1,
        Some(false) => 0,
        None => 2,
    })
}

/// A string from text alone: `which` 0 the word before the caret (`currentToken(in: a)`, `a`
/// NULL for none), 1 the word before `b` in `a` (`previousWord`), 2 `b` cased as `a`
/// (`matchCase(of: a, onto: b)`), 3 your vocabulary's spelling for `a` (`vocabularySpelling(of:
/// a, as: b)`). Returns 0 and writes it (NULL for none), else nonzero.
///
/// # Safety
/// `a` and `b` NULL or NUL-terminated; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_text(
    which: u8,
    a: *const c_char,
    b: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(ptr::null_mut()) };
    // SAFETY: as the caller promises.
    let (Ok(a), Ok(b)) = (unsafe { optional_utf8(a) }, unsafe { optional_utf8(b) }) else {
        return BAD_ARGUMENT;
    };
    let text = guard(Err(FAILED), || match (which, a, b) {
        (0, a, _) => Ok(ac::current_token(a)),
        (1, Some(a), Some(b)) => Ok(ac::previous_word(a, b)),
        (2, Some(a), Some(b)) => Ok(Some(ac::match_case(a, b))),
        (3, Some(a), Some(b)) => Ok(ac::vocabulary_spelling(a, b)),
        _ => Err(BAD_ARGUMENT),
    });
    match text {
        // SAFETY: non-NULL and writable (written above).
        Ok(text) => unsafe { write_text(out, text) },
        Err(status) => status,
    }
}

/// A yes-or-no from text alone: `which` 0 a sentence starts after `a` in `language`
/// (`isSentenceStart`, `a` NULL for none), 1 `b` at the end of `a` starts one
/// (`startsSentence(before: a, token: b)`), 2 `a` has a vowel (`hasVowel`), 3 `a` is an
/// abbreviation a period after doesn't correct (`abbreviations`), 4 the core has `language`'s
/// manifest. Returns 1 or 0, -1 on a bad argument.
///
/// # Safety
/// `a`, `b` and `language` NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_text_is(
    which: u8,
    a: *const c_char,
    b: *const c_char,
    language: *const c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe { (optional_utf8(a), optional_utf8(b), optional_utf8(language)) };
    let (Ok(a), Ok(b), Ok(language)) = args else {
        return -1;
    };
    guard(-1, || {
        let answer = match (which, a, b, language) {
            (0, a, _, Some(l)) => ac::is_sentence_start(a, LanguageConfig::for_tag(l)),
            (1, Some(a), Some(b), _) => ac::starts_sentence(a, b),
            (2, Some(a), _, _) => ac::has_vowel(a),
            (3, Some(a), _, _) => lists::is_abbreviation(a),
            (4, _, _, Some(l)) => LanguageConfig::for_tag(l).id == l,
            _ => return -1,
        };
        i32::from(answer)
    })
}

/// The grid from `count` cells, x the column and y the row; None for a letter that isn't a
/// Unicode scalar.
///
/// # Safety
/// `cells` holds `count` items.
unsafe fn grid(cells: *const HkKeyCentre, count: usize) -> Option<KeyGrid> {
    // SAFETY: as the caller promises.
    let cells = unsafe { slice(cells, count) }?;
    let cells = cells
        .iter()
        .map(|c| {
            Some(KeyCell {
                letter: char::from_u32(c.letter)?,
                row: c.y,
                column: c.x,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(KeyGrid::new(&cells))
}

/// The keyboard cost of a one-edit slip from `source` to `target` in `language` on the grid
/// (`singleEditKeyboardCost`): returns 0 and writes it, NaN when they're further apart;
/// nonzero on a bad argument.
///
/// # Safety
/// The strings NUL-terminated; `cells` holds `count` items; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_edit_cost(
    source: *const c_char,
    target: *const c_char,
    language: *const c_char,
    cells: *const HkKeyCentre,
    count: usize,
    out: *mut f64,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe {
        (
            utf8(source),
            utf8(target),
            utf8(language),
            grid(cells, count),
        )
    };
    let (Some(source), Some(target), Some(language), Some(grid), false) =
        (args.0, args.1, args.2, args.3, out.is_null())
    else {
        return BAD_ARGUMENT;
    };
    guard(FAILED, || {
        let cost =
            ac::single_edit_keyboard_cost(source, target, LanguageConfig::for_tag(language), &grid);
        // SAFETY: non-NULL and writable, as the caller promises.
        unsafe { out.write(cost.unwrap_or(f64::NAN)) };
        OK
    })
}

/// Whether the keys of letters `a` and `b` neighbour on the grid (`areAdjacentKeys`): 1 or 0,
/// -1 on a bad argument.
///
/// # Safety
/// The strings NUL-terminated; `cells` holds `count` items.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_adjacent_keys(
    a: *const c_char,
    b: *const c_char,
    cells: *const HkKeyCentre,
    count: usize,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe { (utf8(a), utf8(b), grid(cells, count)) };
    let (Some(a), Some(b), Some(grid)) = args else {
        return -1;
    };
    guard(-1, || i32::from(ac::are_adjacent_keys(a, b, &grid)))
}

/// A language's words (`language` its tag): `which` 0 sentence starters, 1 clause starters, 2
/// its commonest words, 3 the English contractions Hex corrects to or offers (any language).
/// Returns 0 and fills `out` (release it with `hk_string_list_free`), else nonzero.
///
/// # Safety
/// `language` NUL-terminated; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_language_words(
    language: *const c_char,
    which: u8,
    out: *mut HkStringList,
) -> i32 {
    // SAFETY: as the caller promises.
    let (Some(language), false) = (unsafe { utf8(language) }, out.is_null()) else {
        return BAD_ARGUMENT;
    };
    guard(FAILED, || {
        let config = LanguageConfig::for_tag(language);
        let words: &[&str] = match which {
            0 => config.sentence_starters,
            1 => config.clause_starters,
            2 => config.common_words,
            3 => &lists::english_contractions(),
            _ => return BAD_ARGUMENT,
        };
        let words = words.iter().map(|w| (*w).to_owned()).collect();
        // SAFETY: non-NULL and writable, as the caller promises.
        unsafe { write_list(out, words) }
    })
}
