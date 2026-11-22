//! Dictation's text rules: `hk_dictation_*` (`hex_keyboard_core::dictation`).
//!
//! Text goes both ways as UTF-8 bytes with a length, never NUL-terminated, so a transcript
//! holding U+0000 survives. Each call writes its result to `out` and returns an
//! `HK_DICTATION_*` status; text the core writes is released with `hk_text_free` (or
//! `hk_text_list_free`).

use std::ffi::{CStr, c_char, c_void};
use std::ptr;

use hex_keyboard_core::dictation::{self, Convention, FormatOptions, Language, Style};

use crate::{guard, slice};

/// `HK_DICTATION_OK`: `out` holds the result.
pub const HK_DICTATION_OK: i32 = 0;
/// `HK_DICTATION_INVALID`: a NULL pointer where one is needed, text that isn't UTF-8, or an
/// unknown style or convention. `out` is left empty.
pub const HK_DICTATION_INVALID: i32 = 1;
/// `HK_DICTATION_FAILED`: the core failed (a panic, caught here). `out` is left empty.
pub const HK_DICTATION_FAILED: i32 = 2;
/// `HK_DICTATION_OVERLAPPING_ADDRESSES`: `hk_dictation_spoken_symbols` only, where the
/// Swift original traps. `out` is left empty.
pub const HK_DICTATION_OVERLAPPING_ADDRESSES: i32 = 3;

/// UTF-8 text the caller owns: `length` bytes at `bytes` (NULL when `length` is 0).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkStr {
    pub bytes: *const c_char,
    pub length: usize,
}

/// UTF-8 text the core wrote; release it with `hk_text_free`.
#[repr(C)]
#[derive(Debug)]
pub struct HkText {
    pub bytes: *mut c_char,
    pub length: usize,
}

/// Texts the core wrote; release them with `hk_text_list_free`.
#[repr(C)]
#[derive(Debug)]
pub struct HkTextList {
    pub items: *mut HkText,
    pub count: usize,
}

/// A dictation language as the host's locale reads it: its identifier, and the language code
/// and region (`bytes` NULL when there is none).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkLanguage {
    pub identifier: HkStr,
    pub code: HkStr,
    pub region: HkStr,
}

/// `hk_dictation_format`'s settings. `style`: 0 Polished, 1 Casual, 2 Notes, 3 Verbatim.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkFormatOptions {
    pub style: u32,
    pub lists: u8,
    pub punctuation: u8,
    pub ends_with_period: u8,
}

/// Whether a lowercase word is in the dictation language's dictionary; nonzero for yes.
/// Called on the calling thread, during the call.
pub type HkIsWord = Option<unsafe extern "C" fn(context: *mut c_void, word: HkStr) -> u8>;

impl HkText {
    const EMPTY: HkText = HkText {
        bytes: ptr::null_mut(),
        length: 0,
    };

    fn new(text: String) -> HkText {
        let bytes: Box<[u8]> = text.into_bytes().into_boxed_slice();
        let length = bytes.len();
        HkText {
            bytes: Box::into_raw(bytes).cast(),
            length,
        }
    }
}

/// The text at `text`, or None when its pointer is NULL with bytes or the bytes aren't UTF-8.
///
/// # Safety
/// A non-NULL `text.bytes` points to `text.length` bytes valid for `'a`.
unsafe fn read<'a>(text: HkStr) -> Option<&'a str> {
    // SAFETY: the caller's promise, passed on.
    let bytes = unsafe { slice(text.bytes.cast::<u8>(), text.length) }?;
    std::str::from_utf8(bytes).ok()
}

/// `Some(None)` for a NULL `text`, `Some(Some(…))` for its text, None when it's not UTF-8.
///
/// # Safety
/// `text` is NULL or points to an `HkStr` as `read` takes.
unsafe fn read_optional<'a>(text: *const HkStr) -> Option<Option<&'a str>> {
    if text.is_null() {
        return Some(None);
    }
    // SAFETY: non-NULL, and the caller promises an `HkStr`.
    unsafe { read(*text) }.map(Some)
}

/// The language at `language` (NULL for none, Swift's `nil`), or None when it's not UTF-8.
///
/// # Safety
/// `language` is NULL or points to an `HkLanguage` whose texts `read` takes.
unsafe fn language(language: *const HkLanguage) -> Option<Option<Language>> {
    if language.is_null() {
        return Some(None);
    }
    // SAFETY: non-NULL, and the caller promises an `HkLanguage`.
    let language = unsafe { *language };
    let field = |text: HkStr| -> Option<Option<String>> {
        if text.bytes.is_null() {
            return Some(None);
        }
        // SAFETY: the caller promises the field's bytes.
        unsafe { read(text) }.map(|t| Some(t.to_owned()))
    };
    Some(Some(Language {
        // SAFETY: as above.
        identifier: unsafe { read(language.identifier) }?.to_owned(),
        code: field(language.code)?,
        region: field(language.region)?,
    }))
}

/// Runs `body` under [`guard`] and writes what it gives to `out`.
///
/// # Safety
/// `out` is NULL or valid for a write of `T`.
unsafe fn answer<T>(out: *mut T, empty: T, body: impl FnOnce() -> Result<T, i32>) -> i32 {
    if out.is_null() {
        return HK_DICTATION_INVALID;
    }
    // SAFETY: `out` is non-NULL and, the caller promises, writable.
    unsafe { out.write(empty) };
    match guard(Err(HK_DICTATION_FAILED), body) {
        Ok(value) => {
            // SAFETY: as above.
            unsafe { out.write(value) };
            HK_DICTATION_OK
        }
        Err(status) => status,
    }
}

/// One text in, one out: the stages that take a language.
///
/// # Safety
/// As each `hk_dictation_*` taking `(text, language, out)`.
unsafe fn text_stage(
    text: HkStr,
    language_pointer: *const HkLanguage,
    out: *mut HkText,
    stage: impl FnOnce(&str, Option<&Language>) -> Result<String, i32>,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        answer(out, HkText::EMPTY, || {
            let text = read(text).ok_or(HK_DICTATION_INVALID)?;
            let language = language(language_pointer).ok_or(HK_DICTATION_INVALID)?;
            stage(text, language.as_ref()).map(HkText::new)
        })
    }
}

/// # Safety
/// `text` holds UTF-8 as `HkStr` says; `language` is NULL or an `HkLanguage`; `options`
/// points to an `HkFormatOptions`; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_format(
    text: HkStr,
    language: *const HkLanguage,
    options: *const HkFormatOptions,
    out: *mut HkText,
) -> i32 {
    if options.is_null() {
        return HK_DICTATION_INVALID;
    }
    // SAFETY: non-NULL, and the caller promises an `HkFormatOptions`.
    let options = unsafe { *options };
    let style = match options.style {
        0 => Style::Polished,
        1 => Style::Casual,
        2 => Style::Notes,
        3 => Style::Verbatim,
        _ => return HK_DICTATION_INVALID,
    };
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, language, out, |text, language| {
            Ok(dictation::format(
                text,
                &FormatOptions {
                    style,
                    language,
                    lists: options.lists != 0,
                    punctuation: options.punctuation != 0,
                    ends_with_period: options.ends_with_period != 0,
                },
            ))
        })
    }
}

/// # Safety
/// As `hk_dictation_format`, without options.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_spoken_lists(
    text: HkStr,
    language: *const HkLanguage,
    out: *mut HkText,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, language, out, |t, l| {
            Ok(dictation::spoken_lists(t, l))
        })
    }
}

/// # Safety
/// As `hk_dictation_spoken_lists`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_spoken_punctuation(
    text: HkStr,
    language: *const HkLanguage,
    out: *mut HkText,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, language, out, |t, l| {
            Ok(dictation::spoken_punctuation(t, l))
        })
    }
}

/// # Safety
/// As `hk_dictation_spoken_lists`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_spoken_symbols(
    text: HkStr,
    language: *const HkLanguage,
    out: *mut HkText,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, language, out, |t, l| {
            dictation::spoken_symbols(t, l).map_err(|_| HK_DICTATION_OVERLAPPING_ADDRESSES)
        })
    }
}

/// # Safety
/// As `hk_dictation_spoken_lists`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_spoken_counts(
    text: HkStr,
    language: *const HkLanguage,
    out: *mut HkText,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, language, out, |t, l| {
            Ok(dictation::spoken_counts(t, l))
        })
    }
}

/// # Safety
/// As `hk_dictation_spoken_lists`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_cleanup_rules(
    text: HkStr,
    language: *const HkLanguage,
    out: *mut HkText,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, language, out, |t, l| {
            Ok(dictation::cleanup_rules(t, l))
        })
    }
}

/// # Safety
/// As `hk_dictation_spoken_lists`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_french_spacing(
    text: HkStr,
    language: *const HkLanguage,
    out: *mut HkText,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, language, out, |t, l| {
            Ok(dictation::french_spacing(t, l))
        })
    }
}

/// `convention`: 0 France, 1 Canada, 2 Switzerland.
///
/// # Safety
/// `text` holds UTF-8 as `HkStr` says; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_french_spacing_with(
    text: HkStr,
    convention: u32,
    out: *mut HkText,
) -> i32 {
    let convention = match convention {
        0 => Convention::France,
        1 => Convention::Canada,
        2 => Convention::Switzerland,
        _ => return HK_DICTATION_INVALID,
    };
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, ptr::null(), out, |t, _| {
            Ok(dictation::french_spacing_with(t, convention))
        })
    }
}

/// The French spacing convention for `language` (0 France, 1 Canada, 2 Switzerland), or -1
/// when it isn't French (or the language isn't UTF-8).
///
/// # Safety
/// `language` is NULL or an `HkLanguage`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_french_convention(language: *const HkLanguage) -> i32 {
    // SAFETY: the caller's promise, passed on.
    let Some(language) = (unsafe { self::language(language) }) else {
        return -1;
    };
    guard(-1, || {
        match dictation::french_convention(language.as_ref()) {
            Some(Convention::France) => 0,
            Some(Convention::Canada) => 1,
            Some(Convention::Switzerland) => 2,
            None => -1,
        }
    })
}

/// # Safety
/// `text` holds UTF-8 as `HkStr` says; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_dropping_final_full_stop(
    text: HkStr,
    out: *mut HkText,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, ptr::null(), out, |t, _| {
            Ok(dictation::dropping_final_full_stop(t))
        })
    }
}

/// `SpokenCleanupRules.bare`: lowercased, without opening marks or trailing punctuation.
///
/// # Safety
/// `text` holds UTF-8 as `HkStr` says; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_bare(text: HkStr, out: *mut HkText) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe { text_stage(text, ptr::null(), out, |t, _| Ok(dictation::bare(t))) }
}

/// A host's `isWord` as a Rust closure.
struct Dictionary {
    is_word: unsafe extern "C" fn(*mut c_void, HkStr) -> u8,
    context: *mut c_void,
}

impl Dictionary {
    fn knows(&self, word: &str) -> bool {
        let word = HkStr {
            bytes: word.as_ptr().cast(),
            length: word.len(),
        };
        // SAFETY: the host gave this function and context for the call; `word` is valid
        // while it runs.
        unsafe { (self.is_word)(self.context, word) != 0 }
    }
}

/// `TranscriptFormatter.insertionText`. `before` and `after` are NULL for none; `is_word`
/// NULL leaves the case alone.
///
/// # Safety
/// `formatted` holds UTF-8 as `HkStr` says; `before` and `after` are NULL or `HkStr`s;
/// `is_word` is NULL or a function the core may call with `context` during the call; `out` is
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_insertion_text(
    formatted: HkStr,
    before: *const HkStr,
    after: *const HkStr,
    is_word: HkIsWord,
    context: *mut c_void,
    out: *mut HkText,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        answer(out, HkText::EMPTY, || {
            let formatted = read(formatted).ok_or(HK_DICTATION_INVALID)?;
            let before = read_optional(before).ok_or(HK_DICTATION_INVALID)?;
            let after = read_optional(after).ok_or(HK_DICTATION_INVALID)?;
            let dictionary = is_word.map(|is_word| Dictionary { is_word, context });
            let knows = |word: &str| dictionary.as_ref().is_some_and(|d| d.knows(word));
            let check: Option<&dyn Fn(&str) -> bool> = dictionary.as_ref().map(|_| &knows as _);
            Ok(HkText::new(dictation::insertion_text(
                formatted, before, after, check,
            )))
        })
    }
}

/// `TranscriptFormatter.continuingSentence`.
///
/// # Safety
/// `text` holds UTF-8 as `HkStr` says; `is_word` is a function the core may call with
/// `context` during the call; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_continuing_sentence(
    text: HkStr,
    is_word: HkIsWord,
    context: *mut c_void,
    out: *mut HkText,
) -> i32 {
    let Some(is_word) = is_word else {
        return HK_DICTATION_INVALID;
    };
    let dictionary = Dictionary { is_word, context };
    // SAFETY: the caller's promises, passed on.
    unsafe {
        text_stage(text, ptr::null(), out, |t, _| {
            Ok(dictation::continuing_sentence(t, &|word| {
                dictionary.knows(word)
            }))
        })
    }
}

/// `TranscriptFormatter.startsSentence(after:)`; `before` NULL for none.
///
/// # Safety
/// `before` is NULL or an `HkStr`; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_starts_sentence(before: *const HkStr, out: *mut u8) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        answer(out, 0, || {
            let before = read_optional(before).ok_or(HK_DICTATION_INVALID)?;
            Ok(u8::from(dictation::starts_sentence(before)))
        })
    }
}

/// `TranscriptFormatter.lowercasesContinuations(in:)`.
///
/// # Safety
/// `language` points to an `HkLanguage`; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_lowercases_continuations(
    language: *const HkLanguage,
    out: *mut u8,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        answer(out, 0, || match self::language(language) {
            Some(Some(language)) => Ok(u8::from(dictation::lowercases_continuations(&language))),
            _ => Err(HK_DICTATION_INVALID),
        })
    }
}

/// `SpokenCleanupRules.supports`.
///
/// # Safety
/// `language` is NULL or an `HkLanguage`; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_cleanup_supports(
    language: *const HkLanguage,
    out: *mut u8,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        answer(out, 0, || {
            let language = self::language(language).ok_or(HK_DICTATION_INVALID)?;
            Ok(u8::from(dictation::cleanup_supports(language.as_ref())))
        })
    }
}

/// `TranscriptFormatter.fillerPattern(for:)`: a NUL-terminated pattern the core keeps, or NULL
/// when the language trims no fillers (or isn't UTF-8).
///
/// # Safety
/// `language` is NULL or an `HkLanguage`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_filler_pattern(language: *const HkLanguage) -> *const c_char {
    // SAFETY: the caller's promise, passed on.
    let Some(language) = (unsafe { self::language(language) }) else {
        return ptr::null();
    };
    guard(ptr::null(), || {
        let pattern = dictation::filler_pattern(language.as_ref());
        FILLER_PATTERNS
            .iter()
            .find(|c| pattern.is_some_and(|p| c.to_str() == Ok(p)))
            .map_or(ptr::null(), |c| c.as_ptr())
    })
}

/// Every pattern `dictation::filler_pattern` gives, NUL-terminated for C (a test checks the
/// list is whole).
const FILLER_PATTERNS: [&CStr; 4] = [
    c"um+|uh+|er+m*|ah+",
    c"e+h+m*|ø+h+m*|ö+h+m*|hm+",
    c"e+h+m*|e+m+|m{2,}|hm+",
    c"ä+h+m*|ö+h+m*|hm+",
];

/// `CleanupReconstruction.reconstruct`: `words` and one delete flag per word (nonzero to
/// delete; fewer flags than words keep the rest).
///
/// # Safety
/// `words` points to `word_count` `HkStr`s (NULL when 0); `deletes` to `delete_count` bytes
/// (NULL when 0); `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_reconstruct(
    words: *const HkStr,
    word_count: usize,
    deletes: *const u8,
    delete_count: usize,
    out: *mut HkText,
) -> i32 {
    // SAFETY: the caller's promises, passed on.
    unsafe {
        answer(out, HkText::EMPTY, || {
            let words = slice(words, word_count).ok_or(HK_DICTATION_INVALID)?;
            let deletes = slice(deletes, delete_count).ok_or(HK_DICTATION_INVALID)?;
            let words: Vec<&str> = words
                .iter()
                .map(|&word| read(word))
                .collect::<Option<_>>()
                .ok_or(HK_DICTATION_INVALID)?;
            let deletes: Vec<bool> = deletes.iter().map(|&d| d != 0).collect();
            Ok(HkText::new(dictation::reconstruct(&words, &deletes)))
        })
    }
}

/// `CleanupTagger.words(in:)`: the whitespace-separated words, released with
/// `hk_text_list_free`.
///
/// # Safety
/// `text` holds UTF-8 as `HkStr` says; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_dictation_words(text: HkStr, out: *mut HkTextList) -> i32 {
    let empty = HkTextList {
        items: ptr::null_mut(),
        count: 0,
    };
    // SAFETY: the caller's promises, passed on.
    unsafe {
        answer(out, empty, || {
            let text = read(text).ok_or(HK_DICTATION_INVALID)?;
            let items: Box<[HkText]> = dictation::words(text)
                .into_iter()
                .map(HkText::new)
                .collect();
            let count = items.len();
            Ok(HkTextList {
                items: Box::into_raw(items).cast(),
                count,
            })
        })
    }
}

/// # Safety
/// `text` is NULL or an `HkText` the core wrote and nobody released; it's emptied.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_text_free(text: *mut HkText) {
    if text.is_null() {
        return;
    }
    // SAFETY: non-NULL, and the caller promises the core's `HkText`.
    let taken = unsafe { text.replace(HkText::EMPTY) };
    if taken.bytes.is_null() {
        return;
    }
    let bytes = ptr::slice_from_raw_parts_mut(taken.bytes.cast::<u8>(), taken.length);
    // SAFETY: made by Box::into_raw in `HkText::new` with this length, released once.
    guard((), || drop(unsafe { Box::from_raw(bytes) }));
}

/// # Safety
/// `list` is NULL or an `HkTextList` the core wrote and nobody released; it's emptied.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_text_list_free(list: *mut HkTextList) {
    if list.is_null() {
        return;
    }
    // SAFETY: non-NULL, and the caller promises the core's `HkTextList`.
    let taken = unsafe {
        list.replace(HkTextList {
            items: ptr::null_mut(),
            count: 0,
        })
    };
    if taken.items.is_null() {
        return;
    }
    let items = ptr::slice_from_raw_parts_mut(taken.items, taken.count);
    // SAFETY: made by Box::into_raw in `hk_dictation_words` with this count, released once;
    // each item is the core's `HkText`.
    guard((), || {
        // SAFETY: as above.
        let mut items = unsafe { Box::from_raw(items) };
        for item in items.iter_mut() {
            // SAFETY: each item is an `HkText` the core wrote, released once.
            unsafe { hk_text_free(item) };
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_filler_pattern_has_a_c_string() {
        for code in ["en", "nb", "nn", "no", "da", "sv", "es", "de", "fr"] {
            let language = Language {
                identifier: code.to_owned(),
                code: Some(code.to_owned()),
                region: None,
            };
            if let Some(pattern) = dictation::filler_pattern(Some(&language)) {
                assert!(
                    FILLER_PATTERNS.iter().any(|c| c.to_str() == Ok(pattern)),
                    "{pattern}"
                );
            }
        }
    }
}
