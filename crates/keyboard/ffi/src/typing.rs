//! What a swipe types: `hk_swipe_words_*`, `hk_swipe_rank`, `hk_swipe_typed*`,
//! `hk_swipe_needs_space` and `hk_swipe_is_slid_tap`.

use std::ffi::{CString, c_char, c_void};
use std::ptr;

use hex_keyboard_core::swipe::Candidate;
use hex_keyboard_core::swipe::typing::{
    self, AutoCapitalization, Shift, SwipeContext, SwipeWords, Typed,
};

use crate::swipe::HkSwipeDecoder;
use crate::{guard, slice, utf8};

/// `HkIsWordValid`: nonzero when the host's dictionary knows the word.
pub type HkIsWordValid = Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> u8>;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkTextReplacement {
    pub shortcut: *const c_char,
    pub replacement: *const c_char,
}

pub struct HkSwipeWords {
    pub(crate) words: SwipeWords,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkSwipeContext {
    pub before: *const c_char,
    pub shift: u8,
    pub shift_held: u8,
    pub auto_capitalization: u8,
    pub auto_capitalizes: u8,
    pub autocorrection: u8,
}

#[repr(C)]
#[derive(Debug)]
pub struct HkSwipeTyped {
    pub space: u8,
    pub word: *mut c_char,
    pub alternatives: *mut *mut c_char,
    pub alternative_count: usize,
}

impl HkSwipeTyped {
    const EMPTY: Self = Self {
        space: 0,
        word: ptr::null_mut(),
        alternatives: ptr::null_mut(),
        alternative_count: 0,
    };
}

const OK: i32 = 0;
/// A NULL, malformed or non-UTF-8 argument.
const BAD_ARGUMENT: i32 = 1;
/// The core panicked.
const FAILED: i32 = 2;

pub(crate) fn shift(value: u8) -> Option<Shift> {
    match value {
        0 => Some(Shift::Off),
        1 => Some(Shift::On),
        2 => Some(Shift::Locked),
        _ => None,
    }
}

pub(crate) fn auto_capitalization(value: u8) -> Option<AutoCapitalization> {
    match value {
        0 => Some(AutoCapitalization::None),
        1 => Some(AutoCapitalization::Words),
        2 => Some(AutoCapitalization::Sentences),
        3 => Some(AutoCapitalization::AllCharacters),
        _ => None,
    }
}

/// UTF-8 strings from `count` C strings at `pointer`; None when one is NULL or not UTF-8.
///
/// # Safety
/// `pointer` is NULL with `count` 0, or holds `count` NUL-terminated strings.
unsafe fn strings<'a>(pointer: *const *const c_char, count: usize) -> Option<Vec<&'a str>> {
    // SAFETY: as the caller promises.
    let items = unsafe { slice(pointer, count) }?;
    // SAFETY: each NUL-terminated, as the caller promises.
    items.iter().map(|&s| unsafe { utf8(s) }).collect()
}

/// The host's `HkIsWordValid` as the core asks it.
fn checker(is_word_valid: HkIsWordValid, context: *mut c_void) -> impl FnMut(&str) -> bool {
    move |word: &str| {
        let (Some(is_word_valid), Ok(word)) = (is_word_valid, CString::new(word)) else {
            // No callback, or a word with a NUL in it, which no dictionary has.
            return false;
        };
        // SAFETY: the host's callback with its own context, during the call it was passed to.
        unsafe { is_word_valid(context, word.as_ptr()) != 0 }
    }
}

/// # Safety
/// Each pointer is NULL or valid for its count, every string NUL-terminated.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn hk_swipe_words_new(
    language: *const c_char,
    has_dictionary: u8,
    replacements: *const HkTextReplacement,
    replacement_count: usize,
    own: *const *const c_char,
    own_count: usize,
    vocabulary: *const *const c_char,
    vocabulary_count: usize,
) -> *mut HkSwipeWords {
    guard(ptr::null_mut(), || {
        // SAFETY: valid for their counts and NUL-terminated, as the caller promises.
        let parts = unsafe {
            (
                utf8(language),
                slice(replacements, replacement_count),
                strings(own, own_count),
                strings(vocabulary, vocabulary_count),
            )
        };
        let (Some(language), Some(replacements), Some(own), Some(vocabulary)) = parts else {
            return ptr::null_mut();
        };
        let replacements = replacements.iter().map(|r| {
            // SAFETY: NUL-terminated, as the caller promises.
            let (shortcut, replacement) = unsafe { (utf8(r.shortcut)?, utf8(r.replacement)?) };
            Some((shortcut.to_owned(), replacement.to_owned()))
        });
        let Some(replacements) = replacements.collect::<Option<Vec<_>>>() else {
            return ptr::null_mut();
        };
        let words = SwipeWords::new(
            language,
            has_dictionary != 0,
            replacements,
            own.into_iter().map(str::to_owned).collect(),
            vocabulary.into_iter().map(str::to_owned).collect(),
        );
        Box::into_raw(Box::new(HkSwipeWords { words }))
    })
}

/// # Safety
/// `words` is NULL or from `hk_swipe_words_new`, and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_words_free(words: *mut HkSwipeWords) {
    if words.is_null() {
        return;
    }
    // SAFETY: made by Box::into_raw in `hk_swipe_words_new`, not freed before.
    guard((), || drop(unsafe { Box::from_raw(words) }));
}

/// # Safety
/// `decoder` and `words` are NULL or live; `texts`, `costs` and `order` are NULL with `count`
/// 0 or hold `count` items (`texts` NUL-terminated strings, `order` writable).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn hk_swipe_rank(
    decoder: *const HkSwipeDecoder,
    words: *const HkSwipeWords,
    texts: *const *const c_char,
    costs: *const f64,
    count: usize,
    is_word_valid: HkIsWordValid,
    context: *mut c_void,
    order: *mut usize,
) -> i32 {
    let ranked = guard(Err(FAILED), || {
        // SAFETY: NULL or live, valid for `count`, as the caller promises.
        let (Some(decoder), Some(words), Some(texts), Some(costs)) = (unsafe {
            (
                decoder.as_ref(),
                words.as_ref(),
                strings(texts, count),
                slice(costs, count),
            )
        }) else {
            return Err(BAD_ARGUMENT);
        };
        if count > 0 && (order.is_null() || !order.is_aligned()) {
            return Err(BAD_ARGUMENT);
        }
        let candidates: Vec<Candidate> = texts
            .iter()
            .zip(costs)
            .map(|(&text, &cost)| Candidate {
                text: text.to_owned(),
                cost,
            })
            .collect();
        let mut check = checker(is_word_valid, context);
        Ok(decoder
            .decoder
            .ranked(&candidates, &words.words, &mut check))
    });
    match ranked {
        Ok(ranked) => {
            for (i, index) in ranked.into_iter().enumerate().take(count) {
                // SAFETY: `count` writable items (checked non-NULL and aligned above).
                unsafe { order.add(i).write(index) };
            }
            OK
        }
        Err(status) => status,
    }
}

/// The context a C caller passes, as the core takes it; None when it's malformed.
///
/// # Safety
/// `context.before` is NULL or NUL-terminated.
unsafe fn context<'a>(context: &HkSwipeContext) -> Option<SwipeContext<'a>> {
    let before = if context.before.is_null() {
        None
    } else {
        // SAFETY: non-NULL, so NUL-terminated, as the caller promises.
        Some(unsafe { utf8(context.before) }?)
    };
    Some(SwipeContext {
        before,
        shift: shift(context.shift)?,
        shift_held: context.shift_held != 0,
        auto_capitalization: auto_capitalization(context.auto_capitalization)?,
        auto_capitalizes: context.auto_capitalizes != 0,
        autocorrection: context.autocorrection != 0,
    })
}

/// `typed` as C strings; None when a text holds a NUL.
fn c_typed(typed: Typed) -> Option<HkSwipeTyped> {
    let word = CString::new(typed.word).ok()?;
    let alternatives: Vec<CString> = typed
        .alternatives
        .into_iter()
        .map(CString::new)
        .collect::<Result<_, _>>()
        .ok()?;
    let count = alternatives.len();
    let alternatives: Box<[*mut c_char]> =
        alternatives.into_iter().map(CString::into_raw).collect();
    Some(HkSwipeTyped {
        space: u8::from(typed.space),
        word: word.into_raw(),
        alternatives: if count == 0 {
            ptr::null_mut()
        } else {
            Box::into_raw(alternatives).cast()
        },
        alternative_count: count,
    })
}

/// # Safety
/// `words` and `context` are NULL or valid; `readings` holds `reading_count` NUL-terminated
/// strings; `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_typed(
    words: *const HkSwipeWords,
    readings: *const *const c_char,
    reading_count: usize,
    context: *const HkSwipeContext,
    is_word_valid: HkIsWordValid,
    callback_context: *mut c_void,
    out: *mut HkSwipeTyped,
) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(HkSwipeTyped::EMPTY) };
    let status = guard(Err(FAILED), || {
        // SAFETY: NULL or valid, as the caller promises.
        let (Some(words), Some(readings), Some(state)) = (unsafe {
            (
                words.as_ref(),
                strings(readings, reading_count),
                context.as_ref(),
            )
        }) else {
            return Err(BAD_ARGUMENT);
        };
        // SAFETY: `before` NULL or NUL-terminated, as the caller promises.
        let state = unsafe { self::context(state) }.ok_or(BAD_ARGUMENT)?;
        let mut check = checker(is_word_valid, callback_context);
        let typed =
            typing::typed(&readings, &state, &words.words, &mut check).ok_or(BAD_ARGUMENT)?;
        c_typed(typed).ok_or(FAILED)
    });
    match status {
        Ok(typed) => {
            // SAFETY: non-NULL and writable (checked and written above).
            unsafe { out.write(typed) };
            OK
        }
        Err(status) => status,
    }
}

/// # Safety
/// `typed` is NULL or filled by `hk_swipe_typed` and not freed since (an empty one is fine).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_typed_free(typed: *mut HkSwipeTyped) {
    // SAFETY: NULL or valid, as the caller promises.
    let Some(typed) = (unsafe { typed.as_mut() }) else {
        return;
    };
    let taken = std::mem::replace(typed, HkSwipeTyped::EMPTY);
    guard((), || {
        if !taken.word.is_null() {
            // SAFETY: made by CString::into_raw in `c_typed`, not freed before.
            drop(unsafe { CString::from_raw(taken.word) });
        }
        if taken.alternatives.is_null() {
            return;
        }
        let items = ptr::slice_from_raw_parts_mut(taken.alternatives, taken.alternative_count);
        // SAFETY: a boxed slice of `alternative_count` CString pointers made in `c_typed`.
        let items = unsafe { Box::from_raw(items) };
        for &item in items.iter() {
            // SAFETY: as above.
            drop(unsafe { CString::from_raw(item) });
        }
    });
}

/// # Safety
/// `before` is NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_needs_space(before: *const c_char) -> u8 {
    guard(0, || {
        if before.is_null() {
            return u8::from(typing::needs_space(None));
        }
        // SAFETY: non-NULL, so NUL-terminated, as the caller promises.
        match unsafe { utf8(before) } {
            Some(before) => u8::from(typing::needs_space(Some(before))),
            None => 0,
        }
    })
}

/// # Safety
/// `points` is NULL with `point_count` 0 or holds `point_count` pairs; `key` and
/// `first_reading` are NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_is_slid_tap(
    points: *const f64,
    point_count: usize,
    pitch: f64,
    key: *const c_char,
    first_reading: *const c_char,
) -> u8 {
    guard(0, || {
        // SAFETY: as the caller promises.
        let Some(points) = point_count
            .checked_mul(2)
            .and_then(|n| unsafe { slice(points, n) })
        else {
            return 0;
        };
        let optional = |s: *const c_char| -> Option<Option<&str>> {
            // SAFETY: NULL or NUL-terminated, as the caller promises.
            if s.is_null() {
                Some(None)
            } else {
                unsafe { utf8(s) }.map(Some)
            }
        };
        let (Some(key), Some(first_reading)) = (optional(key), optional(first_reading)) else {
            return 0;
        };
        let path: Vec<(f64, f64)> = points
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[x, y]| (x, y))
            .collect();
        u8::from(typing::is_slid_tap(&path, pitch, key, first_reading))
    })
}
