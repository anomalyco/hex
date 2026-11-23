//! Autocorrect (`hk_autocorrect_*`): one keyboard's suggestions and replace-on-space, its
//! spelling dictionary a host callback (`HkTextChecker`), its words, settings and keys handed
//! over as plain data. A handle is used from one thread at a time; the checker is called on
//! that thread, synchronously, during the call that needs it, and must not call back into the
//! handle.

mod decide;
mod lists;

use std::ffi::{CString, c_char, c_void};
use std::ptr;
use std::sync::Arc;

use hex_keyboard_core::autocorrect::{
    Autocorrect, Bundled, FieldKind, GermanCapitals, KeyCell, NeverSuggestList, TextChecker,
    TypingLearning,
};

use crate::model::HkModel;
use crate::swipe::HkKeyCentre;
use crate::tap::{HkTapKeys, keys, taps};
use crate::{guard, slice, utf8};

pub use decide::*;
pub use lists::*;

const OK: i32 = 0;
/// A NULL, malformed or non-UTF-8 argument.
const BAD_ARGUMENT: i32 = 1;
/// The core panicked, or a string it made holds a NUL.
const FAILED: i32 = 2;

/// Strings a checker callback answers with (`hk_strings_push`).
pub struct HkStrings {
    items: Vec<String>,
}

/// The host's spelling dictionary (`TextChecking`). Every function gets `context`; `release`
/// (NULL: nothing) is called once the core no longer needs the checker.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkTextChecker {
    pub context: *mut c_void,
    pub is_word_valid: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> u8>,
    pub guesses: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *mut HkStrings)>,
    pub completions: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *mut HkStrings)>,
    pub use_language: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// A checker the core calls back.
struct Callback(HkTextChecker);

impl Callback {
    /// The strings a list callback answers `word` with; none for a word with a NUL.
    fn list(
        &self,
        call: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *mut HkStrings)>,
        word: &str,
    ) -> Vec<String> {
        let (Some(call), Ok(word)) = (call, CString::new(word)) else {
            return Vec::new();
        };
        let mut out = HkStrings { items: Vec::new() };
        // SAFETY: the host promised `call` takes `context`, a NUL-terminated string valid for
        // the call, and the list, which it only passes to `hk_strings_push` during the call.
        unsafe { call(self.0.context, word.as_ptr(), &mut out) };
        out.items
    }
}

impl TextChecker for Callback {
    fn is_word_valid(&self, word: &str) -> bool {
        let (Some(call), Ok(word)) = (self.0.is_word_valid, CString::new(word)) else {
            return false;
        };
        // SAFETY: as `list`: the host's function with its context and a string for the call.
        unsafe { call(self.0.context, word.as_ptr()) != 0 }
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        self.list(self.0.guesses, word)
    }

    fn completions(&self, prefix: &str) -> Vec<String> {
        self.list(self.0.completions, prefix)
    }

    fn use_language(&mut self, code: &str) {
        if let (Some(call), Ok(code)) = (self.0.use_language, CString::new(code)) {
            // SAFETY: as `list`.
            unsafe { call(self.0.context, code.as_ptr()) };
        }
    }
}

impl Drop for Callback {
    fn drop(&mut self) {
        if let Some(release) = self.0.release {
            // SAFETY: the host's release for its context, called once, as the header says.
            unsafe { release(self.0.context) };
        }
    }
}

/// An `Autocorrect` handle.
pub struct HkAutocorrect {
    pub(crate) core: Autocorrect,
}

/// The never-suggest list (`never-suggest.tsv`), read once per language.
pub struct HkNeverSuggest {
    pub(crate) list: Arc<NeverSuggestList>,
}

/// German's nouns' capitals (`de.capitals`), read in place.
pub struct HkGermanCapitals {
    pub(crate) capitals: Arc<GermanCapitals>,
}

/// Adds `text` to a checker's answer.
///
/// # Safety
/// `list` is the list the callback was given, during the callback; `text` is NULL or
/// NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_strings_push(list: *mut HkStrings, text: *const c_char) {
    guard((), || {
        // SAFETY: as the caller promises.
        let (Some(list), Some(text)) = (unsafe { list.as_mut() }, unsafe { utf8(text) }) else {
            return;
        };
        list.items.push(text.to_owned());
    });
}

/// `count` strings, each NUL-terminated UTF-8; None if one isn't.
///
/// # Safety
/// `items` is NULL (with 0) or holds `count` NULL or NUL-terminated pointers.
pub(crate) unsafe fn strings<'a>(
    items: *const *const c_char,
    count: usize,
) -> Option<Vec<&'a str>> {
    // SAFETY: as the caller promises.
    let items = unsafe { slice(items, count) }?;
    // SAFETY: each is NULL or NUL-terminated, as the caller promises.
    items.iter().map(|&s| unsafe { utf8(s) }).collect()
}

/// A string for the host, or None when it holds a NUL.
pub(crate) fn c_string(text: String) -> Option<*mut c_char> {
    CString::new(text).ok().map(CString::into_raw)
}

/// Writes `value` (NULL for none) to `out`.
///
/// # Safety
/// `out` is writable.
pub(crate) unsafe fn write_text(out: *mut *mut c_char, value: Option<String>) -> i32 {
    let pointer = match value {
        None => ptr::null_mut(),
        Some(text) => match c_string(text) {
            Some(p) => p,
            None => return FAILED,
        },
    };
    // SAFETY: writable, as the caller promises.
    unsafe { out.write(pointer) };
    OK
}

/// The core behind `ac`, mutably.
///
/// # Safety
/// `ac` is NULL or a live handle no other call is using.
pub(crate) unsafe fn core<'a>(ac: *mut HkAutocorrect) -> Option<&'a mut Autocorrect> {
    // SAFETY: as the caller promises.
    unsafe { ac.as_mut() }.map(|a| &mut a.core)
}

/// Runs `body` on the core under the panic guard; BAD_ARGUMENT for a NULL handle.
///
/// # Safety
/// `ac` is NULL or a live handle no other call is using.
pub(crate) unsafe fn on_core(
    ac: *mut HkAutocorrect,
    body: impl FnOnce(&mut Autocorrect) -> i32,
) -> i32 {
    // SAFETY: as the caller promises.
    let Some(core) = (unsafe { core(ac) }) else {
        return BAD_ARGUMENT;
    };
    guard(FAILED, || body(core))
}

pub(crate) fn field(field: u8) -> FieldKind {
    match field {
        1 => FieldKind::Url,
        2 => FieldKind::Email,
        _ => FieldKind::Text,
    }
}

/// A new autocorrect. `never_suggest` is the never-suggest list (NULL: none); `german` the
/// bundled German capitals (NULL: none); `checker` its dictionary, which it releases on free.
/// NULL on a bad argument (the checker is released then too).
///
/// # Safety
/// `never_suggest` and `german` are NULL or live; `checker`'s functions take its context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_new(
    never_suggest: *const HkNeverSuggest,
    german: *const HkGermanCapitals,
    checker: HkTextChecker,
) -> *mut HkAutocorrect {
    let checker = Callback(checker);
    guard(ptr::null_mut(), || {
        // SAFETY: NULL or live, as the caller promises.
        let (list, german) = unsafe { (never_suggest.as_ref(), german.as_ref()) };
        let bundled = Arc::new(Bundled {
            never_suggest: list
                .map_or_else(|| Arc::new(NeverSuggestList::new("")), |l| l.list.clone()),
            german_capitals: german.map(|g| g.capitals.clone()),
        });
        let core = Autocorrect::new(bundled, Box::new(checker));
        Box::into_raw(Box::new(HkAutocorrect { core }))
    })
}

/// # Safety
/// `ac` is NULL or from `hk_autocorrect_new`, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_free(ac: *mut HkAutocorrect) {
    if ac.is_null() {
        return;
    }
    // SAFETY: from Box::into_raw in hk_autocorrect_new, as the caller promises.
    guard((), || drop(unsafe { Box::from_raw(ac) }));
}

/// The settings (`isEnabled`, `allowsSwearing`, `addsCapitals`, Learn each language
/// separately): 0 off, else on. Returns 0, else nonzero.
///
/// # Safety
/// `ac` is NULL or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_set_settings(
    ac: *mut HkAutocorrect,
    enabled: u8,
    allows_swearing: u8,
    adds_capitals: u8,
    learns_per_language: u8,
) -> i32 {
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            c.set_enabled(enabled != 0);
            c.set_allows_swearing(allows_swearing != 0);
            c.set_adds_capitals(adds_capitals != 0);
            c.set_learns_per_language(learns_per_language != 0);
            OK
        })
    }
}

/// Follows the keyboard layout: `tag` is the language's BCP 47 tag; `available` the device's
/// dictionaries (`UITextChecker.availableLanguages`).
///
/// # Safety
/// `ac` is NULL or live; `tag` NUL-terminated; `available` holds `count` strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_set_language(
    ac: *mut HkAutocorrect,
    tag: *const c_char,
    available: *const *const c_char,
    count: usize,
) -> i32 {
    // SAFETY: as the caller promises.
    let (Some(tag), Some(available)) = (unsafe { utf8(tag) }, unsafe { strings(available, count) })
    else {
        return BAD_ARGUMENT;
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            c.set_language(tag, &available);
            OK
        })
    }
}

/// Bilingual autocorrect's second language (`tag` NULL: none) with its own `checker`, which
/// the core releases when it doesn't keep it.
///
/// # Safety
/// As `hk_autocorrect_set_language`; `tag` may be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_set_partner(
    ac: *mut HkAutocorrect,
    tag: *const c_char,
    available: *const *const c_char,
    count: usize,
    checker: HkTextChecker,
) -> i32 {
    let checker = Callback(checker);
    // SAFETY: as the caller promises.
    let Some(available) = (unsafe { strings(available, count) }) else {
        return BAD_ARGUMENT;
    };
    // SAFETY: NULL or NUL-terminated, as the caller promises.
    let Ok(tag) = (unsafe { crate::tap::optional_utf8(tag) }) else {
        return BAD_ARGUMENT;
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            c.set_partner(tag, &available, Box::new(checker));
            OK
        })
    }
}

/// The keys typed on: `layout`, the layout's language tag, and the letter grid the host shows,
/// one `HkKeyCentre` a letter with x the column and y the row.
///
/// # Safety
/// `ac` is NULL or live; `layout` NUL-terminated; `cells` holds `count` items.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_set_keys(
    ac: *mut HkAutocorrect,
    layout: *const c_char,
    cells: *const HkKeyCentre,
    count: usize,
) -> i32 {
    // SAFETY: as the caller promises.
    let (Some(layout), Some(cells)) = (unsafe { utf8(layout) }, unsafe { slice(cells, count) })
    else {
        return BAD_ARGUMENT;
    };
    let Some(cells) = cells
        .iter()
        .map(|c| {
            Some(KeyCell {
                letter: char::from_u32(c.letter)?,
                row: c.y,
                column: c.x,
            })
        })
        .collect::<Option<Vec<_>>>()
    else {
        return BAD_ARGUMENT;
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            c.set_keys(layout, &cells);
            OK
        })
    }
}

/// Your `UILexicon` entries: `inputs[i]` typed gives `outputs[i]`.
///
/// # Safety
/// `ac` is NULL or live; both arrays hold `count` strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_load_lexicon(
    ac: *mut HkAutocorrect,
    inputs: *const *const c_char,
    outputs: *const *const c_char,
    count: usize,
) -> i32 {
    // SAFETY: as the caller promises.
    let (Some(inputs), Some(outputs)) = (unsafe { strings(inputs, count) }, unsafe {
        strings(outputs, count)
    }) else {
        return BAD_ARGUMENT;
    };
    let entries: Vec<(&str, &str)> = inputs.into_iter().zip(outputs).collect();
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            c.load_lexicon_entries(&entries);
            OK
        })
    }
}

/// Which words a `set_words` call sets.
const VOCABULARY: u8 = 0;
const LEARNED: u8 = 1;
const PARTNER_LEARNED: u8 = 2;

/// Hex's vocabulary (`HK_AUTOCORRECT_VOCABULARY`), or the kept words (`LearnedWordsStore.words`)
/// of this language's list (`_LEARNED`) or the partner's (`_PARTNER_LEARNED`), whenever they
/// change.
///
/// # Safety
/// `ac` is NULL or live; `words` holds `count` strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_set_words(
    ac: *mut HkAutocorrect,
    which: u8,
    words: *const *const c_char,
    count: usize,
) -> i32 {
    // SAFETY: as the caller promises.
    let Some(words) = (unsafe { strings(words, count) }) else {
        return BAD_ARGUMENT;
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| match which {
            VOCABULARY => {
                c.set_vocabulary(&words);
                OK
            }
            LEARNED => {
                c.set_learned_words(&words);
                OK
            }
            PARTNER_LEARNED => {
                c.set_partner_learned_words(&words);
                OK
            }
            _ => BAD_ARGUMENT,
        })
    }
}

/// What Learn how I type learned (`TypingLearning`): `typo_count` typos and their fixes, and
/// the words you turned down.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkLearning {
    pub typos: *const *const c_char,
    pub fixes: *const *const c_char,
    pub typo_count: usize,
    pub rejected: *const *const c_char,
    pub rejected_count: usize,
}

/// # Safety
/// The arrays hold their counts of strings.
unsafe fn learning(l: &HkLearning) -> Option<TypingLearning> {
    // SAFETY: as the caller promises.
    let (typos, fixes, rejected) = unsafe {
        (
            strings(l.typos, l.typo_count)?,
            strings(l.fixes, l.typo_count)?,
            strings(l.rejected, l.rejected_count)?,
        )
    };
    let pairs: Vec<(&str, &str)> = typos.into_iter().zip(fixes).collect();
    Some(TypingLearning::new(&pairs, &rejected))
}

/// Hands over what Learn how I type learned; with Learn each language separately on, the
/// partner takes `partner` (NULL: `mine`).
///
/// # Safety
/// `ac` is NULL or live; `mine` valid; `partner` NULL or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_apply_learning(
    ac: *mut HkAutocorrect,
    mine: *const HkLearning,
    partner: *const HkLearning,
) -> i32 {
    // SAFETY: NULL or valid, as the caller promises.
    let Some(mine) = (unsafe { mine.as_ref() }).and_then(|m| unsafe { learning(m) }) else {
        return BAD_ARGUMENT;
    };
    // SAFETY: as above.
    let theirs = match unsafe { partner.as_ref() } {
        None => None,
        // SAFETY: as above.
        Some(p) => match unsafe { learning(p) } {
            Some(l) => Some(l),
            None => return BAD_ARGUMENT,
        },
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            c.apply_learning(&mine, theirs.as_ref());
            OK
        })
    }
}

/// Keeps `word` as typed this session (`suppressAndKeep`); the host counts the keep.
///
/// # Safety
/// `ac` is NULL or live; `word` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_suppress_and_keep(
    ac: *mut HkAutocorrect,
    word: *const c_char,
) -> i32 {
    // SAFETY: as the caller promises.
    let Some(word) = (unsafe { utf8(word) }) else {
        return BAD_ARGUMENT;
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            c.suppress_and_keep(word);
            OK
        })
    }
}

/// Which model a `set_model` call sets.
const DECODER: u8 = 0;
const PARTNER_DECODER: u8 = 1;
const CONTEXT: u8 = 2;

/// Spatial autocorrect's model (`HK_AUTOCORRECT_DECODER`), the partner's (`_PARTNER_DECODER`),
/// or the layout language's word list and pairs (`contextModel`, `_CONTEXT`); `model` NULL for
/// none. The core keeps its own reference.
///
/// # Safety
/// `ac` is NULL or live; `model` NULL or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_set_model(
    ac: *mut HkAutocorrect,
    which: u8,
    model: *const HkModel,
) -> i32 {
    // SAFETY: NULL or live, as the caller promises.
    let model = unsafe { model.as_ref() }.map(|m| m.model.clone());
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            match which {
                DECODER => c.set_decoder(model),
                PARTNER_DECODER => c.set_partner_decoder(model),
                CONTEXT => c.set_context_model(model),
                _ => return BAD_ARGUMENT,
            }
            OK
        })
    }
}

/// The letter keys taps are read against (`decoderKeys`, `baseline` 0) or the centres read
/// without Adaptive touch (`decoderBaselineKeys`, 1); `keys` NULL for none.
///
/// # Safety
/// `ac` is NULL or live; `keys` NULL or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_set_decoder_keys(
    ac: *mut HkAutocorrect,
    baseline: u8,
    tap_keys: *const HkTapKeys,
) -> i32 {
    // SAFETY: NULL or valid, as the caller promises.
    let keys = match unsafe { tap_keys.as_ref() } {
        None => None,
        // SAFETY: valid, as the caller promises.
        Some(k) => match unsafe { keys(k) } {
            Some(k) => Some(k),
            None => return BAD_ARGUMENT,
        },
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            if baseline == 0 {
                c.set_decoder_keys(keys);
            } else {
                c.set_decoder_baseline_keys(keys);
            }
            OK
        })
    }
}

/// Where each letter of the word being typed was tapped: `count` (x, y) pairs interleaved, or
/// `taps` NULL when the keys don't know.
///
/// # Safety
/// `ac` is NULL or live; `taps` NULL or holds `count` pairs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_set_taps(
    ac: *mut HkAutocorrect,
    points: *const f64,
    count: usize,
) -> i32 {
    let points = if points.is_null() {
        None
    } else {
        // SAFETY: holds `count` pairs, as the caller promises.
        match unsafe { taps(points, count) } {
            Some(t) => Some(t),
            None => return BAD_ARGUMENT,
        }
    };
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            c.set_decoder_taps(points);
            OK
        })
    }
}

/// This engine's German capitals (`germanCapitals`), `capitals` NULL for none; until set, the
/// bundled ones `hk_autocorrect_new` was given.
///
/// # Safety
/// `ac` is NULL or live; `capitals` NULL or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_autocorrect_set_german_capitals(
    ac: *mut HkAutocorrect,
    capitals: *const HkGermanCapitals,
) -> i32 {
    // SAFETY: NULL or live, as the caller promises.
    let capitals = unsafe { capitals.as_ref() }.map(|c| c.capitals.clone());
    // SAFETY: as the caller promises.
    unsafe {
        on_core(ac, |c| {
            c.set_german_capitals(capitals);
            OK
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_and_list_kinds_are_the_headers() {
        let header = include_str!("../../include/hex_keyboard.h");
        for (name, value) in [
            ("HK_AUTOCORRECT_DECODER", DECODER),
            ("HK_AUTOCORRECT_PARTNER_DECODER", PARTNER_DECODER),
            ("HK_AUTOCORRECT_CONTEXT", CONTEXT),
            ("HK_AUTOCORRECT_VOCABULARY", VOCABULARY),
            ("HK_AUTOCORRECT_LEARNED", LEARNED),
            ("HK_AUTOCORRECT_PARTNER_LEARNED", PARTNER_LEARNED),
        ] {
            assert!(
                header.contains(&format!("#define {name} {value}")),
                "{name} is {value} in the header"
            );
        }
    }
}
