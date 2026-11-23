//! What autocorrect decides (`hk_autocorrect_suggestions`, `_replacement_on_space`,
//! `_revision`, `_merge`, …) and what it answers about words.

use std::ffi::c_char;
use std::ptr;

use hex_keyboard_core::autocorrect::{Suggestion, SuggestionKind};

use super::{
    BAD_ARGUMENT, FAILED, HkAutocorrect, OK, c_string, field, on_core, strings, write_text,
};
use crate::model::HkModel;
use crate::tap::{free_string, optional_utf8, taps};
use crate::utf8;

/// One slot of the strip: `kind` 0 keeps what was typed, 1 the best replacement, 2 the best
/// replacement space puts in, 3 the runner-up, 4 a word that can follow.
#[repr(C)]
#[derive(Debug)]
pub struct HkSuggestion {
    pub display_text: *mut c_char,
    pub replacement: *mut c_char,
    pub kind: u8,
}

#[repr(C)]
#[derive(Debug)]
pub struct HkSuggestions {
    pub items: *mut HkSuggestion,
    pub count: usize,
}

/// Strings for the host, released with `hk_string_list_free`.
#[repr(C)]
#[derive(Debug)]
pub struct HkStringList {
    pub items: *mut *mut c_char,
    pub count: usize,
}

const EMPTY_LIST: HkStringList = HkStringList {
    items: ptr::null_mut(),
    count: 0,
};

fn kind(kind: SuggestionKind) -> u8 {
    match kind {
        SuggestionKind::KeepLiteral => 0,
        SuggestionKind::Primary {
            auto_replace: false,
        } => 1,
        SuggestionKind::Primary { auto_replace: true } => 2,
        SuggestionKind::Secondary => 3,
        SuggestionKind::NextWord => 4,
    }
}

fn c_suggestions(list: Vec<Suggestion>) -> Option<HkSuggestions> {
    let mut items = Vec::with_capacity(list.len());
    for s in list {
        let display_text = c_string(s.display_text)?;
        let Some(replacement) = c_string(s.replacement) else {
            // SAFETY: just made by c_string.
            unsafe { free_string(display_text) };
            return None;
        };
        items.push(HkSuggestion {
            display_text,
            replacement,
            kind: kind(s.kind),
        });
    }
    let items = items.into_boxed_slice();
    let count = items.len();
    Some(HkSuggestions {
        items: Box::into_raw(items).cast(),
        count,
    })
}

pub(crate) fn c_list(list: Vec<String>) -> Option<HkStringList> {
    let mut items = Vec::with_capacity(list.len());
    for s in list {
        match c_string(s) {
            Some(p) => items.push(p),
            None => {
                for p in items {
                    // SAFETY: made by c_string above.
                    unsafe { free_string(p) };
                }
                return None;
            }
        }
    }
    let items = items.into_boxed_slice();
    let count = items.len();
    Some(HkStringList {
        items: Box::into_raw(items).cast(),
        count,
    })
}

/// Writes `list` to `out`, or FAILED with `out` empty.
///
/// # Safety
/// `out` is writable.
pub(crate) unsafe fn write_list(out: *mut HkStringList, list: Vec<String>) -> i32 {
    match c_list(list) {
        Some(list) => {
            // SAFETY: writable, as the caller promises.
            unsafe { out.write(list) };
            OK
        }
        None => FAILED,
    }
}

/// The strip for the text before the cursor (`before` NULL when the host doesn't know it).
/// `field` 0 text, 1 URL, 2 email; `autocorrection_off` nonzero for a field that turns it off;
/// `prediction_model` the typing model for predictions (NULL: none); `capitalizes_next` nonzero
/// when shift would turn itself on. Returns 0 and fills `out` (release it with
/// `hk_suggestions_free`), else nonzero with `out` empty.
///
/// # Safety
/// `ac` is NULL or live; `before` NULL or NUL-terminated; `prediction_model` NULL or live;
/// `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_suggestions(
    ac: *mut HkAutocorrect,
    before: *const c_char,
    field_kind: u8,
    autocorrection_off: u8,
    prediction_model: *const HkModel,
    capitalizes_next: u8,
    out: *mut HkSuggestions,
) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe {
        out.write(HkSuggestions {
            items: ptr::null_mut(),
            count: 0,
        })
    };
    // SAFETY: as the caller promises.
    let Ok(before) = (unsafe { optional_utf8(before) }) else {
        return BAD_ARGUMENT;
    };
    // SAFETY: NULL or live, as the caller promises.
    let model = unsafe { prediction_model.as_ref() }.map(|m| m.model.clone());
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            let list = c.suggestions(
                before,
                field(field_kind),
                autocorrection_off != 0,
                model.as_deref(),
                capitalizes_next != 0,
            );
            match c_suggestions(list) {
                Some(list) => {
                    out.write(list);
                    OK
                }
                None => FAILED,
            }
        })
    }
}

/// # Safety
/// `list` is NULL or filled by `hk_autocorrect_suggestions`, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_suggestions_free(list: *mut HkSuggestions) {
    // SAFETY: NULL or filled by hk_autocorrect_suggestions, as the caller promises.
    let Some(list) = (unsafe { list.as_mut() }) else {
        return;
    };
    if !list.items.is_null() {
        // SAFETY: a boxed slice of `count` items from c_suggestions.
        let items = unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(list.items, list.count)) };
        for item in items.iter() {
            // SAFETY: made by c_string.
            unsafe {
                free_string(item.display_text);
                free_string(item.replacement);
            }
        }
    }
    list.items = ptr::null_mut();
    list.count = 0;
}

/// # Safety
/// `list` is NULL or filled by an `hk_*` call, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_string_list_free(list: *mut HkStringList) {
    // SAFETY: NULL or filled by an hk_* call, as the caller promises.
    let Some(list) = (unsafe { list.as_mut() }) else {
        return;
    };
    if !list.items.is_null() {
        // SAFETY: a boxed slice of `count` strings from c_list.
        let items = unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(list.items, list.count)) };
        for &item in items.iter() {
            // SAFETY: made by c_string.
            unsafe { free_string(item) };
        }
    }
    *list = EMPTY_LIST;
}

/// Writes a pair of strings (both NULL for none).
///
/// # Safety
/// `first` and `second` are writable.
unsafe fn write_pair(
    first: *mut *mut c_char,
    second: *mut *mut c_char,
    pair: Option<(String, String)>,
) -> i32 {
    let (a, b) = match pair {
        None => (None, None),
        Some((a, b)) => (Some(a), Some(b)),
    };
    // SAFETY: writable, as the caller promises.
    let status = unsafe { write_text(first, a) };
    if status != OK {
        // SAFETY: as above.
        unsafe { second.write(ptr::null_mut()) };
        return status;
    }
    // SAFETY: as above.
    let status = unsafe { write_text(second, b) };
    if status != OK {
        // SAFETY: written just above.
        unsafe {
            free_string(*first);
            first.write(ptr::null_mut());
        }
    }
    status
}

/// What space replaces (`replacementOnSpace`): returns 0 and writes what was typed and its
/// replacement (both NULL to keep it, release each with `hk_string_free`), else nonzero.
///
/// # Safety
/// `ac` is NULL or live; `before` NULL or NUL-terminated; `original` and `replacement` NULL or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_replacement_on_space(
    ac: *mut HkAutocorrect,
    before: *const c_char,
    field_kind: u8,
    autocorrection_off: u8,
    original: *mut *mut c_char,
    replacement: *mut *mut c_char,
) -> i32 {
    if original.is_null() || replacement.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe {
        original.write(ptr::null_mut());
        replacement.write(ptr::null_mut());
    }
    // SAFETY: as the caller promises.
    let Ok(before) = (unsafe { optional_utf8(before) }) else {
        return BAD_ARGUMENT;
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            let fix = c.replacement_on_space(before, field(field_kind), autocorrection_off != 0);
            write_pair(original, replacement, fix)
        })
    }
}

/// Your text replacement whose shortcut ends `before`: as `_replacement_on_space`.
///
/// # Safety
/// As `hk_autocorrect_replacement_on_space`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_text_replacement(
    ac: *mut HkAutocorrect,
    before: *const c_char,
    typed: *mut *mut c_char,
    replacement: *mut *mut c_char,
) -> i32 {
    if typed.is_null() || replacement.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe {
        typed.write(ptr::null_mut());
        replacement.write(ptr::null_mut());
    }
    // SAFETY: as the caller promises.
    let Ok(before) = (unsafe { optional_utf8(before) }) else {
        return BAD_ARGUMENT;
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            write_pair(typed, replacement, c.text_replacement(before))
        })
    }
}

/// Runs `body` with `out` cleared to NULL, writing its string.
///
/// # Safety
/// `ac` is NULL or live; `out` NULL or writable.
unsafe fn text_call(
    ac: *mut HkAutocorrect,
    out: *mut *mut c_char,
    body: impl FnOnce(&mut hex_keyboard_core::autocorrect::Autocorrect) -> Result<Option<String>, i32>,
) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(ptr::null_mut()) };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| match body(c) {
            Ok(text) => write_text(out, text),
            Err(status) => status,
        })
    }
}

/// The word before the one being finished, revised now that `next` follows it
/// (`revision(of:before:next:)`): returns 0 and writes it (NULL to leave it).
///
/// # Safety
/// `ac` is NULL or live; `word`, `before` and `next` NUL-terminated; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_revision(
    ac: *mut HkAutocorrect,
    word: *const c_char,
    before: *const c_char,
    next: *const c_char,
    field_kind: u8,
    autocorrection_off: u8,
    out: *mut *mut c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe { (utf8(word), utf8(before), utf8(next)) };
    // SAFETY: as the caller promises.
    unsafe {
        text_call(ac, out, |c| {
            let (Some(word), Some(before), Some(next)) = args else {
                return Err(BAD_ARGUMENT);
            };
            Ok(c.revision(
                word,
                before,
                next,
                field(field_kind),
                autocorrection_off != 0,
            ))
        })
    }
}

/// One word in place of `first`, a space tapped as a letter and the word being finished
/// (whose taps `hk_autocorrect_set_taps` gave): returns 0 and writes it (NULL to keep both).
///
/// # Safety
/// `ac` is NULL or live; the strings NUL-terminated; `first_taps` holds `count` pairs; `out`
/// NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_merge(
    ac: *mut HkAutocorrect,
    first: *const c_char,
    first_taps: *const f64,
    count: usize,
    space_x: f64,
    space_y: f64,
    second: *const c_char,
    before: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe {
        (
            utf8(first),
            taps(first_taps, count),
            utf8(second),
            utf8(before),
        )
    };
    // SAFETY: as the caller promises.
    unsafe {
        text_call(ac, out, |c| {
            let (Some(first), Some(first_taps), Some(second), Some(before)) = args else {
                return Err(BAD_ARGUMENT);
            };
            Ok(c.merge(first, &first_taps, (space_x, space_y), second, before))
        })
    }
}

/// The word of `word`'s group that the words around it make clearly likelier
/// (`contextChoice`); `previous` and `next` NULL for none.
///
/// # Safety
/// As `hk_autocorrect_revision`; `previous` and `next` may be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_context_choice(
    ac: *mut HkAutocorrect,
    word: *const c_char,
    previous: *const c_char,
    next: *const c_char,
    starts_sentence: u8,
    out: *mut *mut c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe { (utf8(word), optional_utf8(previous), optional_utf8(next)) };
    // SAFETY: as the caller promises.
    unsafe {
        text_call(ac, out, |c| {
            let (Some(word), Ok(previous), Ok(next)) = args else {
                return Err(BAD_ARGUMENT);
            };
            Ok(c.context_choice(word, previous, next, starts_sentence != 0))
        })
    }
}

/// A string question about `word` (and `other`, by `which`): 0 `withoutStrayCapital(word,
/// typed: other)`, 1 `withoutAddedCapitals(word, typed: other)`, 2 `vocabularyTerm(for: word)`,
/// 3 the language's tag, 4 the partner's, 5 the last fix's language (`lastFixLanguage`), 6
/// the last revision's (`lastRevisionLanguage`). Returns 0 and writes it (NULL for none).
///
/// # Safety
/// `ac` is NULL or live; `word` and `other` NULL or NUL-terminated; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_text(
    ac: *mut HkAutocorrect,
    which: u8,
    word: *const c_char,
    other: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let args = unsafe { (optional_utf8(word), optional_utf8(other)) };
    // SAFETY: as the caller promises.
    unsafe {
        text_call(ac, out, |c| {
            let (Ok(word), Ok(other)) = args else {
                return Err(BAD_ARGUMENT);
            };
            fn need(w: Option<&str>) -> Result<&str, i32> {
                w.ok_or(BAD_ARGUMENT)
            }
            Ok(match which {
                0 => Some(c.without_stray_capital(need(word)?, need(other)?)),
                1 => c.without_added_capitals(need(word)?, need(other)?),
                2 => c.vocabulary_term(need(word)?),
                3 => Some(c.language().to_owned()),
                4 => c.partner_language().map(str::to_owned),
                5 => c.last_fix_language().map(str::to_owned),
                6 => c.last_revision_language().map(str::to_owned),
                _ => return Err(BAD_ARGUMENT),
            })
        })
    }
}

/// A yes-or-no question about `word` (and `other`, by `which`): 0 `mayOffer(word, after:
/// other)`, 1 `knowsAsTyped(word)`, 2 `isOwnSpelling(word)`, 3 `isOwnCapitals(word, typed:
/// other)`, 4 `hasDictionary`, 5 `hasWords`, 6 the checker's `isWordValid(word)`. Returns 0 or
/// 1, or -1 on a bad argument.
///
/// # Safety
/// `ac` is NULL or live; `word` and `other` NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_is(
    ac: *mut HkAutocorrect,
    which: u8,
    word: *const c_char,
    other: *const c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let (Ok(word), Ok(other)) = (unsafe { optional_utf8(word) }, unsafe {
        optional_utf8(other)
    }) else {
        return -1;
    };
    // SAFETY: as the caller promises.
    let Some(c) = (unsafe { super::core(ac) }) else {
        return -1;
    };
    crate::guard(-1, || {
        let answer = match (which, word) {
            (0, Some(w)) => c.may_offer(w, other),
            (1, Some(w)) => c.knows_as_typed(w),
            (2, Some(w)) => c.is_own_spelling(w),
            (3, Some(w)) => c.is_own_capitals(w, other.unwrap_or("")),
            (4, _) => c.has_dictionary(),
            (5, _) => c.has_words(),
            (6, Some(w)) => c.is_word_valid(w),
            _ => return -1,
        };
        i32::from(answer)
    })
}

/// Which language the words of `before` are in (`languageLean(_:before:commonFrom:)`): returns
/// 0 and writes it (above zero the partner's), else nonzero (also without a partner).
///
/// # Safety
/// `ac` is NULL or live; `before` NUL-terminated; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_language_lean(
    ac: *mut HkAutocorrect,
    before: *const c_char,
    common_from: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: as the caller promises.
    let Some(before) = (unsafe { utf8(before) }) else {
        return BAD_ARGUMENT;
    };
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| match c.language_lean(before, common_from) {
            Some(lean) => {
                out.write(lean);
                OK
            }
            None => BAD_ARGUMENT,
        })
    }
}

/// Bumped whenever your own words, the language or Suggest swear words change
/// (`ownWordsRevision`).
///
/// # Safety
/// `ac` is NULL or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_own_words_revision(ac: *mut HkAutocorrect) -> u64 {
    // SAFETY: as the caller promises.
    let Some(core) = (unsafe { super::core(ac) }) else {
        return 0;
    };
    crate::guard(0, || core.own_words_revision())
}

/// A list: `which` 0 your words for Swipe typing (`ownWords`), 1 the words never offered
/// (`offensiveWords.blocked`), 2 the strip's words without capitals you didn't type
/// (`withoutAddedCapitals(items, typed: word)`), 3 a swipe's readings with your words in
/// capitals never first (`readingsOfferingOwnCapitals(items, capsLock: flag, model:)`).
/// Returns 0 and fills `out` (release it with `hk_string_list_free`), else nonzero.
///
/// # Safety
/// `ac` is NULL or live; `items` holds `count` strings; `word` NULL or NUL-terminated; `model`
/// NULL or live; `out` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_list(
    ac: *mut HkAutocorrect,
    which: u8,
    items: *const *const c_char,
    count: usize,
    word: *const c_char,
    flag: u8,
    model: *const HkModel,
    out: *mut HkStringList,
) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(EMPTY_LIST) };
    // SAFETY: as the caller promises.
    let (Some(items), Ok(word)) = (unsafe { strings(items, count) }, unsafe {
        optional_utf8(word)
    }) else {
        return BAD_ARGUMENT;
    };
    let items: Vec<String> = items.into_iter().map(str::to_owned).collect();
    // SAFETY: NULL or live, as the caller promises.
    let model = unsafe { model.as_ref() }.map(|m| m.model.clone());
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            let list = match which {
                0 => c.own_words(),
                1 => c.blocked_words(),
                2 => c.without_added_capitals_pool(items, word.unwrap_or("")),
                3 => match &model {
                    Some(m) => c.readings_offering_own_capitals(items, flag != 0, m),
                    None => return BAD_ARGUMENT,
                },
                _ => return BAD_ARGUMENT,
            };
            write_list(out, list)
        })
    }
}
