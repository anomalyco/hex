//! Swipe typing: `hk_swipe_*`.

use std::ffi::{CString, c_char};
use std::ptr;

use hex_keyboard_core::never_suggest::NeverSuggest;
use hex_keyboard_core::swipe::{
    CONTRACTION_COST, ENGLISH_CONTRACTIONS, ExtraWord, Keys, OWN_WORD_COST, Parameters, Reading,
    SwipeDecoder, cost_of_contraction,
};

use crate::model::HkModel;
use crate::{guard, slice, utf8};

/// `swipe::Parameters`, field for field, laid out as the header's struct.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HkSwipeParameters {
    pub samples: usize,
    pub start_radius: f64,
    pub end_radius: f64,
    pub max_start_keys: usize,
    pub max_end_keys: usize,
    pub near_radius: f64,
    pub min_length_ratio: f64,
    pub max_length_ratio: f64,
    pub start_sigma: f64,
    pub end_sigma: f64,
    pub start_stray_sigma: f64,
    pub end_stray_sigma: f64,
    pub stray: f64,
    pub location_weight: f64,
    pub shape_weight: f64,
    pub lm_weight: f64,
    pub shortlist: usize,
    pub prefilter: usize,
    pub band: f64,
    pub minimum_length: f64,
    pub contraction_missing_pair: f64,
    pub evidence_step: f64,
    pub turn_window: f64,
    pub turn_from: f64,
    pub turn_full: f64,
    pub dwell_window: f64,
    pub dwell_from: f64,
    pub dwell_full: f64,
    pub evidence_floor: f64,
    pub evidence_spacing: f64,
    pub evidence_guard: f64,
    pub evidence_sigma: f64,
    pub evidence_reach: f64,
    pub unexplained_cost: f64,
    pub straight_key_cost: f64,
    pub corner_key_cost: f64,
    pub straight_angle: f64,
}

/// Both directions, field for field: a field missing on either side fails to compile.
macro_rules! convert_parameters {
    ($($field:ident),* $(,)?) => {
        impl From<&Parameters> for HkSwipeParameters {
            fn from(p: &Parameters) -> Self {
                Self { $($field: p.$field),* }
            }
        }

        impl From<&HkSwipeParameters> for Parameters {
            fn from(p: &HkSwipeParameters) -> Self {
                Self { $($field: p.$field),* }
            }
        }
    };
}

convert_parameters!(
    samples,
    start_radius,
    end_radius,
    max_start_keys,
    max_end_keys,
    near_radius,
    min_length_ratio,
    max_length_ratio,
    start_sigma,
    end_sigma,
    start_stray_sigma,
    end_stray_sigma,
    stray,
    location_weight,
    shape_weight,
    lm_weight,
    shortlist,
    prefilter,
    band,
    minimum_length,
    contraction_missing_pair,
    evidence_step,
    turn_window,
    turn_from,
    turn_full,
    dwell_window,
    dwell_from,
    dwell_full,
    evidence_floor,
    evidence_spacing,
    evidence_guard,
    evidence_sigma,
    evidence_reach,
    unexplained_cost,
    straight_key_cost,
    corner_key_cost,
    straight_angle,
);

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkExtraWord {
    pub text: *const c_char,
    pub cost: f64,
    pub contraction: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HkKeyCentre {
    pub letter: u32,
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkKeys {
    pub centres: *const HkKeyCentre,
    pub centre_count: usize,
    pub pitch_width: f64,
    pub pitch_height: f64,
}

pub struct HkSwipeDecoder {
    pub(crate) decoder: SwipeDecoder,
}

#[repr(C)]
#[derive(Debug)]
pub struct HkCandidate {
    pub text: *mut c_char,
    pub cost: f64,
}

#[repr(C)]
#[derive(Debug)]
pub struct HkSwipeReading {
    pub candidates: *mut HkCandidate,
    pub candidate_count: usize,
    pub scored: usize,
}

impl HkSwipeReading {
    const EMPTY: Self = Self {
        candidates: ptr::null_mut(),
        candidate_count: 0,
        scored: 0,
    };
}

const OK: i32 = 0;
/// A NULL, malformed or non-UTF-8 argument.
const BAD_ARGUMENT: i32 = 1;
/// The core panicked.
const FAILED: i32 = 2;

pub(crate) fn default_parameters() -> Option<Parameters> {
    guard(None, || Some(Parameters::default()))
}

/// A decoder over `model`; `never_pairs` is interleaved (first, second). NULL if the core
/// panics.
pub(crate) fn new_decoder(
    model: &HkModel,
    parameters: Parameters,
    never_words: &[u32],
    never_pairs: &[u32],
    extra: Vec<ExtraWord>,
) -> *mut HkSwipeDecoder {
    if !never_pairs.len().is_multiple_of(2) {
        return ptr::null_mut();
    }
    let never_suggest = NeverSuggest {
        words: never_words.iter().map(|&w| w as usize).collect(),
        pairs: never_pairs
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[a, b]| (a as usize, b as usize))
            .collect(),
    };
    guard(ptr::null_mut(), || {
        let decoder = SwipeDecoder::new(model.model.clone(), parameters, never_suggest, extra);
        Box::into_raw(Box::new(HkSwipeDecoder { decoder }))
    })
}

/// The letter keys from their characters' scalars; None when one isn't a Unicode scalar.
pub(crate) fn keys(
    centres: impl IntoIterator<Item = HkKeyCentre>,
    pitch: (f64, f64),
) -> Option<Keys> {
    let centres = centres
        .into_iter()
        .map(|c| Some((char::from_u32(c.letter)?, c.x, c.y)))
        .collect::<Option<Vec<_>>>()?;
    Some(Keys { centres, pitch })
}

/// Reads a swipe; `points` is interleaved (x, y). None when the arguments don't match or the
/// core panics.
pub(crate) fn read(
    decoder: &HkSwipeDecoder,
    points: &[f64],
    times: Option<&[f64]>,
    keys: &Keys,
    previous: Option<&str>,
    limit: usize,
) -> Option<Reading> {
    if !points.len().is_multiple_of(2) || times.is_some_and(|t| t.len() != points.len() / 2) {
        return None;
    }
    let path: Vec<(f64, f64)> = points
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[x, y]| (x, y))
        .collect();
    guard(None, || {
        Some(decoder.decoder.read(&path, times, keys, previous, limit))
    })
}

/// # Safety
/// `out` is NULL or points to writable `HkSwipeParameters`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_default_parameters(out: *mut HkSwipeParameters) {
    if out.is_null() {
        return;
    }
    let parameters = default_parameters().map_or_else(HkSwipeParameters::default, |p| (&p).into());
    // SAFETY: non-NULL and writable, as the caller promises; written whole, never read
    unsafe { out.write(parameters) };
}

#[unsafe(no_mangle)]
pub extern "C" fn hk_swipe_contraction_cost() -> f64 {
    CONTRACTION_COST
}

#[unsafe(no_mangle)]
pub extern "C" fn hk_swipe_own_word_cost() -> f64 {
    OWN_WORD_COST
}

/// `swipe::ENGLISH_CONTRACTIONS` as C strings, made once.
struct Contractions {
    _texts: Vec<CString>,
    pointers: Vec<*const c_char>,
}

// SAFETY: built once and never changed; the pointers point into `_texts`, which it owns.
unsafe impl Sync for Contractions {}
// SAFETY: as above.
unsafe impl Send for Contractions {}

static CONTRACTIONS: std::sync::OnceLock<Contractions> = std::sync::OnceLock::new();

/// # Safety
/// `count` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_english_contractions(count: *mut usize) -> *const *const c_char {
    let contractions = guard(None, || {
        Some(CONTRACTIONS.get_or_init(|| {
            let texts: Vec<CString> = ENGLISH_CONTRACTIONS
                .iter()
                .map(|w| CString::new(*w).expect("no NUL in a contraction"))
                .collect();
            let pointers = texts.iter().map(|t| t.as_ptr()).collect();
            Contractions {
                _texts: texts,
                pointers,
            }
        }))
    });
    let (pointer, n) = contractions.map_or((ptr::null(), 0), |c| {
        (c.pointers.as_ptr(), c.pointers.len())
    });
    // SAFETY: NULL or writable, as the caller promises.
    if let Some(count) = unsafe { count.as_mut() } {
        *count = n;
    }
    pointer
}

/// `swipe::cost_of_contraction`; a NULL or non-UTF-8 `word` is no contraction (`base` + 2), as
/// is a panic.
///
/// # Safety
/// `word` is NULL or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_cost_of_contraction(word: *const c_char, base: f64) -> f64 {
    let none = base + 2.0;
    guard(none, || {
        // SAFETY: NULL or NUL-terminated, as the caller promises.
        unsafe { utf8(word) }.map_or(none, |word| cost_of_contraction(word, base))
    })
}

/// # Safety
/// `model` and `parameters` are NULL or valid; each array is NULL or holds its count of
/// items (`never_pairs` twice `never_pair_count`); each extra word's `text` is NULL or
/// NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_new(
    model: *const HkModel,
    parameters: *const HkSwipeParameters,
    never_words: *const u32,
    never_word_count: usize,
    never_pairs: *const u32,
    never_pair_count: usize,
    extra: *const HkExtraWord,
    extra_count: usize,
) -> *mut HkSwipeDecoder {
    guard(ptr::null_mut(), || {
        // SAFETY: NULL or valid, as the caller promises.
        let (Some(model), Some(parameters)) =
            (unsafe { model.as_ref() }, unsafe { parameters.as_ref() })
        else {
            return ptr::null_mut();
        };
        // SAFETY: each array holds its count of items, as the caller promises.
        let arrays = unsafe {
            (
                slice(never_words, never_word_count),
                never_pair_count
                    .checked_mul(2)
                    .and_then(|n| slice(never_pairs, n)),
                slice(extra, extra_count),
            )
        };
        let (Some(never_words), Some(never_pairs), Some(extra)) = arrays else {
            return ptr::null_mut();
        };
        let extra = extra.iter().map(|word| {
            // SAFETY: NULL or NUL-terminated, as the caller promises.
            let text = unsafe { utf8(word.text) }?;
            Some(ExtraWord {
                text: text.to_owned(),
                cost: word.cost,
                contraction: word.contraction != 0,
            })
        });
        let Some(extra) = extra.collect::<Option<Vec<_>>>() else {
            return ptr::null_mut();
        };
        new_decoder(model, parameters.into(), never_words, never_pairs, extra)
    })
}

/// # Safety
/// `decoder` is NULL or from `hk_swipe_new`, and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_free(decoder: *mut HkSwipeDecoder) {
    if decoder.is_null() {
        return;
    }
    // SAFETY: made by Box::into_raw in `new_decoder` and, the caller promises, not freed
    // before.
    guard((), || drop(unsafe { Box::from_raw(decoder) }));
}

/// The reading's candidates as C strings; None when a text holds a NUL.
fn c_reading(reading: Reading) -> Option<HkSwipeReading> {
    let candidates = reading
        .candidates
        .into_iter()
        .map(|c| {
            Some(HkCandidate {
                text: CString::new(c.text).ok()?.into_raw(),
                cost: c.cost,
            })
        })
        .collect::<Vec<_>>();
    let count = candidates.len();
    if candidates.iter().any(Option::is_none) {
        for text in candidates.into_iter().flatten().map(|c| c.text) {
            // SAFETY: made by CString::into_raw just above.
            drop(unsafe { CString::from_raw(text) });
        }
        return None;
    }
    let candidates: Box<[HkCandidate]> = candidates.into_iter().flatten().collect();
    Some(HkSwipeReading {
        candidates: if count == 0 {
            ptr::null_mut()
        } else {
            Box::into_raw(candidates).cast()
        },
        candidate_count: count,
        scored: reading.scored,
    })
}

/// # Safety
/// `decoder` is NULL or live; `points` holds `point_count` pairs; `times` is NULL or holds
/// `point_count` items; `keys` is NULL or valid with `centre_count` centres; `previous` is
/// NULL or NUL-terminated; `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_read(
    decoder: *const HkSwipeDecoder,
    points: *const f64,
    point_count: usize,
    times: *const f64,
    keys: *const HkKeys,
    previous: *const c_char,
    limit: usize,
    out: *mut HkSwipeReading,
) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(HkSwipeReading::EMPTY) };
    let status = guard(Err(FAILED), || {
        // SAFETY: NULL or valid, as the caller promises.
        let (Some(decoder), Some(keys)) = (unsafe { decoder.as_ref() }, unsafe { keys.as_ref() })
        else {
            return Err(BAD_ARGUMENT);
        };
        // SAFETY: each array holds its count of items, as the caller promises.
        let (points, centres) = unsafe {
            (
                point_count.checked_mul(2).and_then(|n| slice(points, n)),
                slice(keys.centres, keys.centre_count),
            )
        };
        let (Some(points), Some(centres)) = (points, centres) else {
            return Err(BAD_ARGUMENT);
        };
        let times = if times.is_null() {
            None
        } else {
            // SAFETY: non-NULL, so it holds `point_count` items, as the caller promises.
            Some(unsafe { slice(times, point_count) }.ok_or(BAD_ARGUMENT)?)
        };
        let previous = if previous.is_null() {
            None
        } else {
            // SAFETY: non-NULL, so NUL-terminated, as the caller promises.
            Some(unsafe { utf8(previous) }.ok_or(BAD_ARGUMENT)?)
        };
        let keys = self::keys(
            centres.iter().copied(),
            (keys.pitch_width, keys.pitch_height),
        )
        .ok_or(BAD_ARGUMENT)?;
        let reading = read(decoder, points, times, &keys, previous, limit).ok_or(FAILED)?;
        c_reading(reading).ok_or(FAILED)
    });
    match status {
        Ok(reading) => {
            // SAFETY: non-NULL and writable (checked and written above).
            unsafe { out.write(reading) };
            OK
        }
        Err(status) => status,
    }
}

/// # Safety
/// `reading` is NULL or filled by `hk_swipe_read` and not freed since (an empty one is fine).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_swipe_reading_free(reading: *mut HkSwipeReading) {
    // SAFETY: NULL or valid, as the caller promises.
    let Some(reading) = (unsafe { reading.as_mut() }) else {
        return;
    };
    let taken = std::mem::replace(reading, HkSwipeReading::EMPTY);
    if taken.candidates.is_null() {
        return;
    }
    guard((), || {
        let candidates = ptr::slice_from_raw_parts_mut(taken.candidates, taken.candidate_count);
        // SAFETY: made by Box::into_raw of a boxed slice of `candidate_count` in `c_reading`,
        // each text by CString::into_raw, and (the caller promises) not freed before.
        let candidates = unsafe { Box::from_raw(candidates) };
        for candidate in candidates.iter() {
            // SAFETY: as above.
            drop(unsafe { CString::from_raw(candidate.text) });
        }
    });
}
