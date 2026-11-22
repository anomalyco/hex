//! The typing model's lookups and predictions: the rest of `hk_model_*`.

use std::ffi::{c_char, c_void};
use std::ptr;

use hex_keyboard_core::model::{Model, SortedWords};

use crate::model::HkModel;
use crate::{guard, slice, utf8};

/// The model's sizes, language and alphabet size, laid out as the header's struct.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HkModelInfo {
    pub word_count: usize,
    pub pair_count: usize,
    pub byte_count: usize,
    pub language: [c_char; 8],
    pub code_count: usize,
    pub apostrophe_code: i32,
}

/// Whether a word (by index) may be offered: the host's filter.
pub type HkAllowWord = Option<unsafe extern "C" fn(context: *mut c_void, word: u32) -> u8>;

/// # Safety
/// `model` is NULL or a live handle.
unsafe fn model<'a>(model: *const HkModel) -> Option<&'a Model> {
    // SAFETY: NULL or a live handle, as the caller promises.
    unsafe { model.as_ref() }.map(|m| &*m.model)
}

/// # Safety
/// `pointer` is NULL or NUL-terminated.
unsafe fn optional_utf8<'a>(pointer: *const c_char) -> Result<Option<&'a str>, ()> {
    if pointer.is_null() {
        return Ok(None);
    }
    // SAFETY: non-NULL, so NUL-terminated, as the caller promises.
    unsafe { utf8(pointer) }.map(Some).ok_or(())
}

/// # Safety
/// `model` is NULL or live; `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_info(model: *const HkModel, out: *mut HkModelInfo) -> i32 {
    // SAFETY: NULL or live, as the caller promises.
    let (Some(model), false) = (unsafe { self::model(model) }, out.is_null()) else {
        return 1;
    };
    let info = guard(None, || {
        let mut language = [0 as c_char; 8];
        for (to, &from) in language
            .iter_mut()
            .zip(model.language().as_bytes().iter().take(7))
        {
            *to = from as c_char;
        }
        Some(HkModelInfo {
            word_count: model.word_count(),
            pair_count: model.bigram_count(),
            byte_count: model.byte_count(),
            language,
            code_count: model.code_count(),
            apostrophe_code: model.apostrophe_code().map_or(-1, i32::from),
        })
    });
    let Some(info) = info else {
        return 2;
    };
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(info) };
    0
}

/// # Safety
/// `model` is NULL or live; `characters` and `base_codes` are NULL or hold `capacity` items.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_alphabet(
    model: *const HkModel,
    characters: *mut u32,
    base_codes: *mut u8,
    capacity: usize,
) -> i32 {
    // SAFETY: NULL or live, as the caller promises.
    let Some(model) = (unsafe { self::model(model) }) else {
        return 1;
    };
    let count = model.code_count();
    if characters.is_null() || base_codes.is_null() || capacity < count {
        return 1;
    }
    guard(2, || {
        for code in 0..count {
            // SAFETY: code < count <= capacity, so inside both arrays, as the caller promises.
            unsafe {
                characters.add(code).write(model.character(code) as u32);
                base_codes.add(code).write(model.base_code(code));
            }
        }
        0
    })
}

/// # Safety
/// `model` is NULL or live; `length` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_word_codes(
    model: *const HkModel,
    word: usize,
    length: *mut usize,
) -> *const u8 {
    // SAFETY: NULL or live, as the caller promises.
    let (Some(model), false) = (unsafe { self::model(model) }, length.is_null()) else {
        return ptr::null();
    };
    let codes = guard(None, || {
        (word < model.word_count()).then(|| model.codes(word))
    });
    let (pointer, count) = codes.map_or((ptr::null(), 0), |c| (c.as_ptr(), c.len()));
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { length.write(count) };
    pointer
}

/// # Safety
/// `model` is NULL or live; `word` is NULL or NUL-terminated; `out` is NULL or holds
/// `capacity` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_encode(
    model: *const HkModel,
    word: *const c_char,
    out: *mut u8,
    capacity: usize,
) -> i64 {
    // SAFETY: NULL or valid, as the caller promises.
    let (Some(model), Some(word)) = (unsafe { self::model(model) }, unsafe { utf8(word) }) else {
        return -2;
    };
    let Some(codes) = guard(None, || Some(model.encode(word))) else {
        return -2;
    };
    let Some(codes) = codes else {
        return -1;
    };
    if codes.len() > capacity || (out.is_null() && !codes.is_empty()) {
        return -2;
    }
    // SAFETY: `out` holds `capacity` >= codes.len() bytes, as the caller promises.
    unsafe { ptr::copy_nonoverlapping(codes.as_ptr(), out, codes.len()) };
    codes.len() as i64
}

/// # Safety
/// `model` is NULL or live; `word` is NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_index(model: *const HkModel, word: *const c_char) -> i64 {
    // SAFETY: NULL or valid, as the caller promises.
    let (Some(model), Some(word)) = (unsafe { self::model(model) }, unsafe { utf8(word) }) else {
        return -1;
    };
    guard(None, || model.index(word)).map_or(-1, |i| i as i64)
}

/// # Safety
/// `model` is NULL or live; `codes` is NULL or holds `count` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_index_of_codes(
    model: *const HkModel,
    codes: *const u8,
    count: usize,
) -> i64 {
    // SAFETY: NULL or valid, as the caller promises.
    let (Some(model), Some(codes)) = (unsafe { self::model(model) }, unsafe {
        slice(codes, count)
    }) else {
        return -1;
    };
    guard(None, || model.index_of_codes(codes)).map_or(-1, |i| i as i64)
}

/// # Safety
/// `model` is NULL or live; `out_lo` and `out_hi` are NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_narrow(
    model: *const HkModel,
    lo: usize,
    hi: usize,
    depth: usize,
    code: i32,
    out_lo: *mut usize,
    out_hi: *mut usize,
) {
    if out_lo.is_null() || out_hi.is_null() {
        return;
    }
    // SAFETY: NULL or live, as the caller promises.
    let range = unsafe { self::model(model) }
        .filter(|m| lo <= hi && hi <= m.word_count())
        .and_then(|m| guard(None, || Some(m.narrow(lo, hi, depth, code))))
        .unwrap_or((lo, lo));
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe {
        out_lo.write(range.0);
        out_hi.write(range.1);
    }
}

/// # Safety
/// `model` is NULL or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_unigram_cost(model: *const HkModel, word: usize) -> f64 {
    // SAFETY: NULL or live, as the caller promises.
    unsafe { self::model(model) }
        .filter(|m| word < m.word_count())
        .and_then(|m| guard(None, || Some(m.unigram_cost(word))))
        .unwrap_or(f64::NAN)
}

/// Writes the cost and returns 1 when found, else 0 (or 0 on a bad argument).
///
/// # Safety
/// `out` is NULL or writable.
unsafe fn found(cost: Option<f64>, out: *mut f64) -> i32 {
    match cost {
        Some(cost) if !out.is_null() => {
            // SAFETY: non-NULL and writable, as the caller promises.
            unsafe { out.write(cost) };
            1
        }
        _ => 0,
    }
}

/// # Safety
/// `model` is NULL or live; `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_bigram_cost(
    model: *const HkModel,
    first: usize,
    second: usize,
    out: *mut f64,
) -> i32 {
    // SAFETY: NULL or live, as the caller promises.
    let cost = unsafe { self::model(model) }
        .filter(|m| second < m.word_count())
        .and_then(|m| guard(None, || m.bigram_cost(first, second)));
    // SAFETY: `out` is NULL or writable, as the caller promises.
    unsafe { found(cost, out) }
}

/// # Safety
/// `model` is NULL or live; `word` and `previous` are NULL or NUL-terminated; `out` is NULL
/// or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_cost(
    model: *const HkModel,
    word: *const c_char,
    previous: *const c_char,
    out: *mut f64,
) -> i32 {
    // SAFETY: NULL or valid, as the caller promises.
    let (Some(model), Some(word), Ok(previous)) = (
        unsafe { self::model(model) },
        unsafe { utf8(word) },
        unsafe { optional_utf8(previous) },
    ) else {
        return 0;
    };
    let cost = guard(None, || model.cost(word, previous));
    // SAFETY: `out` is NULL or writable, as the caller promises.
    unsafe { found(cost, out) }
}

/// # Safety
/// `model` is NULL or live; `first` and `second` are NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_has_pair(
    model: *const HkModel,
    first: *const c_char,
    second: *const c_char,
) -> i32 {
    // SAFETY: NULL or valid, as the caller promises.
    let (Some(model), Some(first), Some(second)) = (
        unsafe { self::model(model) },
        unsafe { utf8(first) },
        unsafe { utf8(second) },
    ) else {
        return 0;
    };
    i32::from(guard(false, || model.has_pair(first, second)))
}

/// Copies `words` (at most `capacity`, which the predictions never exceed) into `out`.
///
/// # Safety
/// `out` holds `capacity` items.
unsafe fn write_words(words: &[usize], out: *mut u32, capacity: usize) -> usize {
    let count = words.len().min(capacity);
    for (i, &w) in words.iter().take(count).enumerate() {
        // SAFETY: i < count <= capacity, as the caller promises.
        unsafe { out.add(i).write(w as u32) };
    }
    count
}

/// The host's filter as a closure; every word allowed without one.
fn allow(allowing: HkAllowWord, context: *mut c_void) -> impl FnMut(usize) -> bool {
    move |word| match allowing {
        // SAFETY: the host's function, called with the context it gave, as the header says.
        Some(allowing) => (unsafe { allowing(context, word as u32) }) != 0,
        None => true,
    }
}

/// # Safety
/// `model` is NULL or live; `previous` is NULL or NUL-terminated; `allowing` is NULL or a
/// function safe to call with `context`; `out` is NULL or holds `capacity` items.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_next_words(
    model: *const HkModel,
    previous: *const c_char,
    allowing: HkAllowWord,
    context: *mut c_void,
    out: *mut u32,
    capacity: usize,
) -> usize {
    // SAFETY: NULL or valid, as the caller promises.
    let (Some(model), Some(previous), false) = (
        unsafe { self::model(model) },
        unsafe { utf8(previous) },
        out.is_null(),
    ) else {
        return 0;
    };
    let words = guard(Vec::new(), || {
        model.next_word_indices(previous, allow(allowing, context))
    });
    // SAFETY: `out` holds `capacity` items, as the caller promises.
    unsafe { write_words(&words, out, capacity) }
}

/// # Safety
/// As `hk_model_next_words`; `prefix` is NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_completions(
    model: *const HkModel,
    prefix: *const c_char,
    previous: *const c_char,
    allowing: HkAllowWord,
    context: *mut c_void,
    out: *mut u32,
    capacity: usize,
) -> usize {
    // SAFETY: NULL or valid, as the caller promises.
    let (Some(model), Some(prefix), Ok(previous), false) = (
        unsafe { self::model(model) },
        unsafe { utf8(prefix) },
        unsafe { optional_utf8(previous) },
        out.is_null(),
    ) else {
        return 0;
    };
    let words = guard(Vec::new(), || {
        model.completion_indices(prefix, previous, allow(allowing, context))
    });
    // SAFETY: `out` holds `capacity` items, as the caller promises.
    unsafe { write_words(&words, out, capacity) }
}

/// # Safety
/// `model` is NULL or live; `prefix` is NULL or holds `count` bytes; `mass` is NULL or
/// holds `capacity` items.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_continuations(
    model: *const HkModel,
    prefix: *const u8,
    count: usize,
    mass: *mut f64,
    capacity: usize,
) -> i32 {
    // SAFETY: NULL or valid, as the caller promises.
    let (Some(model), Some(prefix)) = (unsafe { self::model(model) }, unsafe {
        slice(prefix, count)
    }) else {
        return 0;
    };
    if mass.is_null() || capacity < model.code_count() {
        return 0;
    }
    let Some(found) = guard(None, || model.continuations(prefix)) else {
        return 0;
    };
    // SAFETY: `mass` holds `capacity` >= code_count == found.len() items, as the caller
    // promises.
    unsafe { ptr::copy_nonoverlapping(found.as_ptr(), mass, found.len()) };
    1
}
