//! Spatial autocorrect's tap decoder (`hk_tap_*`) and the letter keys it reads taps against,
//! made from the host's layout (`hk_layout_*`).

use std::collections::HashMap;
use std::ffi::{CString, c_char};
use std::ptr;

use hex_keyboard_core::decoder::{
    Decision, KeyStats, Keys, LayoutKey, Parameters, Rect, Split, TypingDecoder, UserWords, offsets,
};
use hex_keyboard_core::never_suggest::NeverSuggest;

use crate::model::HkModel;
use crate::swipe::{HkCandidate, HkKeyCentre, HkKeys};
use crate::{guard, slice, utf8};

const OK: i32 = 0;
/// A NULL, malformed or non-UTF-8 argument.
const BAD_ARGUMENT: i32 = 1;
/// The core panicked.
const FAILED: i32 = 2;

/// `decoder::Parameters`, field for field, laid out as the header's struct.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HkTapParameters {
    pub sigma_x: f64,
    pub sigma_y: f64,
    pub lm_weight: f64,
    pub deletion_cost: f64,
    pub insertion_cost: f64,
    pub transposition_cost: f64,
    pub space_omission_cost: f64,
    pub space_tap_cost: f64,
    pub space_tap_weight: f64,
    pub merge_reach: f64,
    pub max_edits: usize,
    pub beam: usize,
    pub auto_margin: f64,
    pub valid_margin: f64,
    pub unknown_valid_cost: f64,
    pub unknown_cost: f64,
    pub max_tap_cost: f64,
    pub splits: u8,
    pub accent_cost: f64,
    pub one_letter_word_cost: f64,
    pub user_word_cost: f64,
    pub accent_margin: f64,
}

/// Both directions, field for field (`splits` apart): a field missing on either side fails
/// to compile.
macro_rules! convert_parameters {
    ($($field:ident),* $(,)?) => {
        impl From<&Parameters> for HkTapParameters {
            fn from(p: &Parameters) -> Self {
                Self { $($field: p.$field,)* splits: u8::from(p.splits) }
            }
        }

        impl From<&HkTapParameters> for Parameters {
            fn from(p: &HkTapParameters) -> Self {
                Self { $($field: p.$field,)* splits: p.splits != 0 }
            }
        }
    };
}

convert_parameters!(
    sigma_x,
    sigma_y,
    lm_weight,
    deletion_cost,
    insertion_cost,
    transposition_cost,
    space_omission_cost,
    space_tap_cost,
    space_tap_weight,
    merge_reach,
    max_edits,
    beam,
    auto_margin,
    valid_margin,
    unknown_valid_cost,
    unknown_cost,
    max_tap_cost,
    accent_cost,
    one_letter_word_cost,
    user_word_cost,
    accent_margin,
);

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HkRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl From<HkRect> for Rect {
    fn from(r: HkRect) -> Self {
        Rect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

impl From<Rect> for HkRect {
    fn from(r: Rect) -> Self {
        HkRect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkTapKeys {
    pub keys: HkKeys,
    pub bottom_row_y: f64,
    pub has_space: u8,
    pub space: HkRect,
}

pub struct HkTapDecoder {
    decoder: TypingDecoder,
}

#[repr(C)]
#[derive(Debug)]
pub struct HkTapDecision {
    pub decided: u8,
    pub replacement: *mut c_char,
    pub readings: *mut HkCandidate,
    pub reading_count: usize,
    pub literal_cost: f64,
}

impl HkTapDecision {
    const EMPTY: Self = Self {
        decided: 0,
        replacement: ptr::null_mut(),
        readings: ptr::null_mut(),
        reading_count: 0,
        literal_cost: 0.0,
    };
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HkLayoutKey {
    pub character: *const c_char,
    pub space: u8,
    pub frame: HkRect,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HkKeyStats {
    pub present: u8,
    pub count: u64,
    pub dx: f64,
    pub dy: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HkKeyOffset {
    pub letter: u32,
    pub dx: f64,
    pub dy: f64,
}

/// # Safety
/// `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_tap_default_parameters(out: *mut HkTapParameters) {
    if out.is_null() {
        return;
    }
    let parameters = guard(HkTapParameters::default(), || {
        (&Parameters::default()).into()
    });
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(parameters) };
}

/// # Safety
/// `language` is NULL or NUL-terminated; `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_tap_tuned_parameters(
    language: *const c_char,
    out: *mut HkTapParameters,
) {
    if out.is_null() {
        return;
    }
    // SAFETY: NULL or NUL-terminated, as the caller promises.
    let language = unsafe { utf8(language) }.unwrap_or("");
    let parameters = guard(HkTapParameters::default(), || {
        (&Parameters::tuned(language)).into()
    });
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(parameters) };
}

/// # Safety
/// `model` and `parameters` are NULL or valid; each array is NULL or holds its count of
/// items (`never_pairs` twice `never_pair_count`); each user word is NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_tap_new(
    model: *const HkModel,
    parameters: *const HkTapParameters,
    user_words: *const *const c_char,
    user_word_count: usize,
    never_words: *const u32,
    never_word_count: usize,
    never_pairs: *const u32,
    never_pair_count: usize,
) -> *mut HkTapDecoder {
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
                slice(user_words, user_word_count),
                slice(never_words, never_word_count),
                never_pair_count
                    .checked_mul(2)
                    .and_then(|n| slice(never_pairs, n)),
            )
        };
        let (Some(user_words), Some(never_words), Some(never_pairs)) = arrays else {
            return ptr::null_mut();
        };
        // SAFETY: each is NUL-terminated, as the caller promises.
        let Some(words) = user_words
            .iter()
            .map(|&w| unsafe { utf8(w) })
            .collect::<Option<Vec<&str>>>()
        else {
            return ptr::null_mut();
        };
        let mut decoder = TypingDecoder::new(model.model.clone(), Some(parameters.into()));
        decoder.user_words = UserWords::new(&words, &model.model);
        decoder.never_suggest = NeverSuggest {
            words: never_words.iter().map(|&w| w as usize).collect(),
            pairs: never_pairs
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[a, b]| (a as usize, b as usize))
                .collect(),
        };
        Box::into_raw(Box::new(HkTapDecoder { decoder }))
    })
}

/// # Safety
/// `decoder` is NULL or from `hk_tap_new`, and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_tap_free(decoder: *mut HkTapDecoder) {
    if decoder.is_null() {
        return;
    }
    // SAFETY: made by Box::into_raw in `hk_tap_new` and, the caller promises, not freed
    // before.
    guard((), || drop(unsafe { Box::from_raw(decoder) }));
}

/// The decoder's keys from the host's; None when a centre's letter isn't a Unicode scalar.
///
/// # Safety
/// `keys` is valid with `centre_count` centres.
pub(crate) unsafe fn keys(keys: &HkTapKeys) -> Option<Keys> {
    // SAFETY: `centres` holds `centre_count` items, as the caller promises.
    let centres = unsafe { slice(keys.keys.centres, keys.keys.centre_count) }?;
    let centres = centres
        .iter()
        .map(|c| Some((char::from_u32(c.letter)?, c.x, c.y)))
        .collect::<Option<Vec<_>>>()?;
    Some(Keys {
        centres,
        pitch: (keys.keys.pitch_width, keys.keys.pitch_height),
        bottom_row_y: keys.bottom_row_y,
        space: (keys.has_space != 0).then(|| keys.space.into()),
    })
}

/// Interleaved (x, y) taps.
///
/// # Safety
/// `taps` is NULL or holds `count` pairs.
pub(crate) unsafe fn taps(taps: *const f64, count: usize) -> Option<Vec<(f64, f64)>> {
    // SAFETY: holds `count` pairs, as the caller promises.
    let flat = unsafe { count.checked_mul(2).and_then(|n| slice(taps, n)) }?;
    Some(
        flat.as_chunks::<2>()
            .0
            .iter()
            .map(|&[x, y]| (x, y))
            .collect(),
    )
}

/// # Safety
/// `pointer` is NULL or NUL-terminated.
pub(crate) unsafe fn optional_utf8<'a>(pointer: *const c_char) -> Result<Option<&'a str>, i32> {
    if pointer.is_null() {
        return Ok(None);
    }
    // SAFETY: non-NULL, so NUL-terminated, as the caller promises.
    unsafe { utf8(pointer) }.map(Some).ok_or(BAD_ARGUMENT)
}

/// Frees what `c_string` made.
///
/// # Safety
/// `text` is NULL or from `CString::into_raw`, not freed before.
pub(crate) unsafe fn free_string(text: *mut c_char) {
    if !text.is_null() {
        // SAFETY: as the caller promises.
        drop(unsafe { CString::from_raw(text) });
    }
}

fn c_decision(decision: Option<Decision>) -> Option<HkTapDecision> {
    let Some(d) = decision else {
        return Some(HkTapDecision::EMPTY);
    };
    let texts: Option<Vec<CString>> = d
        .readings
        .iter()
        .map(|r| CString::new(r.text.as_str()).ok())
        .collect();
    let replacement = match d.replacement {
        Some(text) => Some(CString::new(text).ok()?),
        None => None,
    };
    let readings: Box<[HkCandidate]> = texts?
        .into_iter()
        .zip(&d.readings)
        .map(|(text, r)| HkCandidate {
            text: text.into_raw(),
            cost: r.cost,
        })
        .collect();
    let count = readings.len();
    Some(HkTapDecision {
        decided: 1,
        replacement: replacement.map_or(ptr::null_mut(), CString::into_raw),
        readings: if count == 0 {
            ptr::null_mut()
        } else {
            Box::into_raw(readings).cast()
        },
        reading_count: count,
        literal_cost: d.literal_cost,
    })
}

/// # Safety
/// `decoder` is NULL or live; `taps` holds `tap_count` pairs; `typed` and `previous` are NULL
/// or NUL-terminated; `keys` is NULL or valid; `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_tap_decide(
    decoder: *const HkTapDecoder,
    taps: *const f64,
    tap_count: usize,
    typed: *const c_char,
    previous: *const c_char,
    literal_is_valid: u8,
    keys: *const HkTapKeys,
    out: *mut HkTapDecision,
) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(HkTapDecision::EMPTY) };
    let status = guard(Err(FAILED), || {
        // SAFETY: NULL or valid, as the caller promises.
        let (Some(decoder), Some(keys), Some(typed)) = (
            unsafe { decoder.as_ref() },
            unsafe { keys.as_ref() },
            unsafe { utf8(typed) },
        ) else {
            return Err(BAD_ARGUMENT);
        };
        // SAFETY: as the caller promises (see above).
        let (taps, keys, previous) = unsafe {
            (
                self::taps(taps, tap_count),
                self::keys(keys),
                optional_utf8(previous)?,
            )
        };
        let (Some(taps), Some(keys)) = (taps, keys) else {
            return Err(BAD_ARGUMENT);
        };
        let decision = decoder
            .decoder
            .decide(&taps, typed, previous, literal_is_valid != 0, &keys);
        c_decision(decision).ok_or(FAILED)
    });
    match status {
        Ok(decision) => {
            // SAFETY: non-NULL and writable (checked and written above).
            unsafe { out.write(decision) };
            OK
        }
        Err(status) => status,
    }
}

/// # Safety
/// `decision` is NULL or filled by `hk_tap_decide` and not freed since (an empty one is
/// fine).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_tap_decision_free(decision: *mut HkTapDecision) {
    // SAFETY: NULL or valid, as the caller promises.
    let Some(decision) = (unsafe { decision.as_mut() }) else {
        return;
    };
    let taken = std::mem::replace(decision, HkTapDecision::EMPTY);
    guard((), || {
        // SAFETY: made by CString::into_raw in `c_decision`, not freed before.
        unsafe { free_string(taken.replacement) };
        if taken.readings.is_null() {
            return;
        }
        let readings = ptr::slice_from_raw_parts_mut(taken.readings, taken.reading_count);
        // SAFETY: a boxed slice of `reading_count` from `c_decision`, each text by
        // CString::into_raw, and (the caller promises) not freed before.
        let readings = unsafe { Box::from_raw(readings) };
        for reading in readings.iter() {
            // SAFETY: as above.
            unsafe { free_string(reading.text) };
        }
    });
}

/// # Safety
/// `decoder` is NULL or live; each taps array holds its count of pairs; the strings are NULL
/// or NUL-terminated (`previous` may be NULL); `keys` is NULL or valid; `out` is NULL or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_tap_merge(
    decoder: *const HkTapDecoder,
    first: *const c_char,
    first_taps: *const f64,
    first_tap_count: usize,
    space_x: f64,
    space_y: f64,
    second: *const c_char,
    second_taps: *const f64,
    second_tap_count: usize,
    previous: *const c_char,
    first_is_valid: u8,
    second_is_valid: u8,
    keys: *const HkTapKeys,
    out: *mut *mut c_char,
) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(ptr::null_mut()) };
    let status = guard(Err(FAILED), || {
        // SAFETY: NULL or valid, as the caller promises.
        let args = unsafe { (decoder.as_ref(), keys.as_ref(), utf8(first), utf8(second)) };
        let (Some(decoder), Some(keys), Some(first), Some(second)) = args else {
            return Err(BAD_ARGUMENT);
        };
        // SAFETY: as the caller promises (see above).
        let (first_taps, second_taps, keys, previous) = unsafe {
            (
                taps(first_taps, first_tap_count),
                taps(second_taps, second_tap_count),
                self::keys(keys),
                optional_utf8(previous)?,
            )
        };
        let (Some(first_taps), Some(second_taps), Some(keys)) = (first_taps, second_taps, keys)
        else {
            return Err(BAD_ARGUMENT);
        };
        let merged = decoder.decoder.merge(
            first,
            &first_taps,
            (space_x, space_y),
            second,
            &second_taps,
            previous,
            first_is_valid != 0,
            second_is_valid != 0,
            &keys,
        );
        match merged {
            Some(text) => Ok(CString::new(text).map_err(|_| FAILED)?.into_raw()),
            None => Ok(ptr::null_mut()),
        }
    });
    match status {
        Ok(text) => {
            // SAFETY: non-NULL and writable (checked and written above).
            unsafe { out.write(text) };
            OK
        }
        Err(status) => status,
    }
}

/// # Safety
/// `text` is NULL or from `hk_tap_merge`, and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_string_free(text: *mut c_char) {
    // SAFETY: as the caller promises.
    guard((), || unsafe { free_string(text) });
}

/// # Safety
/// As `hk_tap_decide`; `reading` is NULL or NUL-terminated; `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_tap_replaces(
    decoder: *const HkTapDecoder,
    reading: *const c_char,
    taps: *const f64,
    tap_count: usize,
    typed: *const c_char,
    previous: *const c_char,
    literal_is_valid: u8,
    keys: *const HkTapKeys,
    out: *mut i32,
) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    let status = guard(Err(FAILED), || {
        // SAFETY: NULL or valid, as the caller promises.
        let args = unsafe { (decoder.as_ref(), keys.as_ref(), utf8(reading), utf8(typed)) };
        let (Some(decoder), Some(keys), Some(reading), Some(typed)) = args else {
            return Err(BAD_ARGUMENT);
        };
        // SAFETY: as the caller promises (see above).
        let (taps, keys, previous) = unsafe {
            (
                self::taps(taps, tap_count),
                self::keys(keys),
                optional_utf8(previous)?,
            )
        };
        let (Some(taps), Some(keys)) = (taps, keys) else {
            return Err(BAD_ARGUMENT);
        };
        let replaces = decoder.decoder.replaces(
            reading,
            &taps,
            typed,
            previous,
            literal_is_valid != 0,
            &keys,
        );
        Ok(replaces.map_or(-1, i32::from))
    });
    match status {
        Ok(answer) => {
            // SAFETY: non-NULL and writable, as the caller promises.
            unsafe { out.write(answer) };
            OK
        }
        Err(status) => status,
    }
}

/// The host's keys as the core's; None when a character isn't UTF-8.
///
/// # Safety
/// Each key's `character` is NULL or NUL-terminated.
unsafe fn layout_keys(keys: &[HkLayoutKey]) -> Option<Vec<LayoutKey>> {
    keys.iter()
        .map(|k| {
            let character = if k.character.is_null() {
                None
            } else {
                // SAFETY: non-NULL, so NUL-terminated, as the caller promises.
                Some(unsafe { utf8(k.character) }?.to_owned())
            };
            Some(LayoutKey {
                character,
                space: k.space != 0,
                frame: k.frame.into(),
            })
        })
        .collect()
}

/// # Safety
/// `keys` and `stats` are NULL or hold `key_count` items; each key's `character` is NULL or
/// NUL-terminated; `out` is NULL or holds `capacity` items.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_layout_offsets(
    keys: *const HkLayoutKey,
    stats: *const HkKeyStats,
    key_count: usize,
    out: *mut HkKeyOffset,
    capacity: usize,
) -> usize {
    // SAFETY: each holds `key_count` items, as the caller promises.
    let arrays = unsafe { (slice(keys, key_count), slice(stats, key_count)) };
    let (Some(keys), Some(stats)) = arrays else {
        return 0;
    };
    // SAFETY: each character is NULL or NUL-terminated, as the caller promises.
    let Some(layout) = (unsafe { layout_keys(keys) }) else {
        return 0;
    };
    let found = guard(Vec::new(), || {
        // Each key's own stats, by its character, as the host's dictionary gives them.
        let by_character: HashMap<String, KeyStats> = layout
            .iter()
            .zip(stats)
            .filter(|(_, s)| s.present != 0)
            .filter_map(|(k, s)| {
                let stats = KeyStats {
                    count: s.count,
                    dx: s.dx,
                    dy: s.dy,
                };
                Some((k.character.clone()?, stats))
            })
            .collect();
        let mut found: Vec<(char, (f64, f64))> =
            offsets(&layout, &by_character).into_iter().collect();
        found.sort_by_key(|&(c, _)| c);
        found
    });
    if out.is_null() || found.len() > capacity {
        return 0;
    }
    for (i, &(letter, (dx, dy))) in found.iter().enumerate() {
        let offset = HkKeyOffset {
            letter: letter as u32,
            dx,
            dy,
        };
        // SAFETY: i < found.len() <= capacity, as the caller promises.
        unsafe { out.add(i).write(offset) };
    }
    found.len()
}

/// # Safety
/// `keys` holds `key_count` items with NULL or NUL-terminated characters; `split` is NULL or
/// holds 2 items; `offsets` holds `offset_count` items; `centres` holds `capacity` items;
/// `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_layout_tap_keys(
    keys: *const HkLayoutKey,
    key_count: usize,
    split: *const f64,
    pitch_width: f64,
    pitch_height: f64,
    offsets: *const HkKeyOffset,
    offset_count: usize,
    centres: *mut HkKeyCentre,
    capacity: usize,
    out: *mut HkTapKeys,
) -> i32 {
    if out.is_null() || centres.is_null() {
        return BAD_ARGUMENT;
    }
    // SAFETY: each array holds its count of items, as the caller promises.
    let arrays = unsafe { (slice(keys, key_count), slice(offsets, offset_count)) };
    let (Some(keys), Some(offsets)) = arrays else {
        return BAD_ARGUMENT;
    };
    let split = if split.is_null() {
        None
    } else {
        // SAFETY: non-NULL, so 2 items, as the caller promises.
        let s = unsafe { slice(split, 2) }.ok_or(()).ok();
        match s {
            Some(s) => Some(Split {
                middle: s[0],
                shift: s[1],
            }),
            None => return BAD_ARGUMENT,
        }
    };
    // SAFETY: each character is NULL or NUL-terminated, as the caller promises.
    let Some(layout) = (unsafe { layout_keys(keys) }) else {
        return BAD_ARGUMENT;
    };
    let Some(offsets) = offsets
        .iter()
        .map(|o| Some((char::from_u32(o.letter)?, (o.dx, o.dy))))
        .collect::<Option<HashMap<char, (f64, f64)>>>()
    else {
        return BAD_ARGUMENT;
    };
    let made = guard(None, || {
        Some(Keys::from_layout(
            &layout,
            split,
            (pitch_width, pitch_height),
            &offsets,
        ))
    });
    let Some(made) = made else {
        return FAILED;
    };
    if made.centres.len() > capacity {
        return BAD_ARGUMENT;
    }
    for (i, &(letter, x, y)) in made.centres.iter().enumerate() {
        let centre = HkKeyCentre {
            letter: letter as u32,
            x,
            y,
        };
        // SAFETY: i < centres.len() <= capacity, as the caller promises.
        unsafe { centres.add(i).write(centre) };
    }
    let tap_keys = HkTapKeys {
        keys: HkKeys {
            centres,
            centre_count: made.centres.len(),
            pitch_width: made.pitch.0,
            pitch_height: made.pitch.1,
        },
        bottom_row_y: made.bottom_row_y,
        has_space: u8::from(made.space.is_some()),
        space: made.space.map(HkRect::from).unwrap_or_default(),
    };
    // SAFETY: non-NULL and writable, as the caller promises.
    unsafe { out.write(tap_keys) };
    OK
}
