//! The Android IME's entry points: `external fun`s of the Kotlin `object`
//! `ly.anoma.hex.typing.KeyboardCore`. Handles are the C interface's pointers as `Long`s (0
//! for none). A failure returns 0 or null and never throws: a Rust panic is caught, and a Java
//! exception a JNI call raised on the way (a null array, an allocation that failed, a class R8
//! renamed) is cleared before the entry point returns.
//!
//! Release builds don't ship the core yet. When one does, R8 must keep `KeyboardCore`'s
//! `external fun`s (their names are these functions' symbols), `SwipeReading`'s
//! `(String[], double[], int)` and `SwipeTyped`'s `(boolean, String, String[])` constructors,
//! which `swipe_reading` and `swipe_typed` call by name, and `WordChecker.isWordValid`.

use std::path::Path;
use std::ptr;

use hex_keyboard_core::swipe::typing::{self as core_typing, SwipeContext, SwipeWords, Typed};
use hex_keyboard_core::swipe::{
    CONTRACTION_COST, Candidate, ENGLISH_CONTRACTIONS, ExtraWord, OWN_WORD_COST, Reading,
    cost_of_contraction,
};
use jni::JNIEnv;
use jni::objects::{
    JBooleanArray, JDoubleArray, JIntArray, JObject, JObjectArray, JPrimitiveArray, JString, JValue,
};
use jni::sys::{jboolean, jdouble, jint, jlong, jobject};

use crate::model::{self, HkModel};
use crate::swipe::{self, HkKeyCentre, HkSwipeDecoder};
use crate::typing::{self, HkSwipeWords};
use crate::{HK_ABI_VERSION, guard};

type Result<T> = jni::errors::Result<T>;

/// The model behind a handle from `modelOpen*`, or None for 0.
///
/// # Safety
/// `handle` is 0 or a live handle from `modelOpen*`.
unsafe fn model_of<'a>(handle: jlong) -> Option<&'a HkModel> {
    // SAFETY: 0 or a live HkModel pointer, as the caller promises.
    unsafe { (handle as *const HkModel).as_ref() }
}

/// # Safety
/// `handle` is 0 or a live handle from `swipeNew`.
unsafe fn decoder_of<'a>(handle: jlong) -> Option<&'a HkSwipeDecoder> {
    // SAFETY: 0 or a live HkSwipeDecoder pointer, as the caller promises.
    unsafe { (handle as *const HkSwipeDecoder).as_ref() }
}

fn handle<T>(pointer: *mut T) -> jlong {
    pointer as jlong
}

/// `value`, once any Java exception a JNI call left pending is cleared: an entry point fails by
/// what it returns, never by throwing.
fn settled<T>(env: &mut JNIEnv, value: T) -> T {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    value
}

fn region<'a, T: jni::objects::TypeArray + Default + Clone>(
    env: &JNIEnv,
    array: &JPrimitiveArray<'a, T>,
    read: impl FnOnce(&JNIEnv, &JPrimitiveArray<'a, T>, &mut [T]) -> Result<()>,
) -> Result<Vec<T>> {
    let length = env.get_array_length(array)?;
    let mut items = vec![T::default(); usize::try_from(length).unwrap_or(0)];
    read(env, array, &mut items)?;
    Ok(items)
}

fn ints(env: &JNIEnv, array: &JIntArray) -> Result<Vec<jint>> {
    region(env, array, |env, a, out| {
        env.get_int_array_region(a, 0, out)
    })
}

fn doubles(env: &JNIEnv, array: &JDoubleArray) -> Result<Vec<jdouble>> {
    region(env, array, |env, a, out| {
        env.get_double_array_region(a, 0, out)
    })
}

fn booleans(env: &JNIEnv, array: &JBooleanArray) -> Result<Vec<jboolean>> {
    region(env, array, |env, a, out| {
        env.get_boolean_array_region(a, 0, out)
    })
}

fn string(env: &mut JNIEnv, value: &JString) -> Result<String> {
    Ok(env.get_string(value)?.into())
}

fn strings(env: &mut JNIEnv, array: &JObjectArray) -> Result<Vec<String>> {
    let length = env.get_array_length(array)?;
    let mut out = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    for i in 0..length {
        // One local reference at a time: the word lists run to thousands.
        let element = JString::from(env.get_object_array_element(array, i)?);
        out.push(string(env, &element)?);
        env.delete_local_ref(element)?;
    }
    Ok(out)
}

/// Word indices from Kotlin's signed ints; None when one is negative.
fn indices(items: Vec<jint>) -> Option<Vec<u32>> {
    items.into_iter().map(|i| u32::try_from(i).ok()).collect()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_abiVersion(
    _env: JNIEnv,
    _this: JObject,
) -> jint {
    HK_ABI_VERSION as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_modelOpen(
    mut env: JNIEnv,
    _this: JObject,
    path: JString,
) -> jlong {
    let model = guard(0, || {
        if path.is_null() {
            return 0;
        }
        match string(&mut env, &path) {
            Ok(path) => handle(model::open_path(Path::new(&path))),
            Err(_) => 0,
        }
    });
    settled(&mut env, model)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_modelOpenFd(
    _env: JNIEnv,
    _this: JObject,
    fd: jint,
    offset: jlong,
    length: jlong,
) -> jlong {
    handle(model::open_fd(fd, offset, length))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_modelFree(
    _env: JNIEnv,
    _this: JObject,
    model: jlong,
) {
    // SAFETY: 0 or a handle from modelOpen*, freed once, as the Kotlin caller promises.
    unsafe { model::hk_model_free(model as *mut HkModel) };
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_modelWordCount(
    _env: JNIEnv,
    _this: JObject,
    model: jlong,
) -> jlong {
    // SAFETY: 0 or a live handle from modelOpen*, as the Kotlin caller promises.
    unsafe { model::hk_model_word_count(model as *const HkModel) as jlong }
}

/// The decoder's word lists from Kotlin; None when they're malformed.
fn swipe_inputs(
    env: &mut JNIEnv,
    never_words: &JIntArray,
    never_pairs: &JIntArray,
    extra_texts: &JObjectArray,
    extra_costs: &JDoubleArray,
    extra_contractions: &JBooleanArray,
) -> Option<(Vec<u32>, Vec<u32>, Vec<ExtraWord>)> {
    let never_words = indices(ints(env, never_words).ok()?)?;
    let never_pairs = indices(ints(env, never_pairs).ok()?)?;
    let texts = strings(env, extra_texts).ok()?;
    let costs = doubles(env, extra_costs).ok()?;
    let contractions = booleans(env, extra_contractions).ok()?;
    if costs.len() != texts.len() || contractions.len() != texts.len() {
        return None;
    }
    let extra = texts
        .into_iter()
        .zip(costs)
        .zip(contractions)
        .map(|((text, cost), contraction)| ExtraWord {
            text,
            cost,
            contraction: contraction != 0,
        })
        .collect();
    Some((never_words, never_pairs, extra))
}

/// A decoder with the default parameters, or 0.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeNew(
    mut env: JNIEnv,
    _this: JObject,
    model: jlong,
    never_words: JIntArray,
    never_pairs: JIntArray,
    extra_texts: JObjectArray,
    extra_costs: JDoubleArray,
    extra_contractions: JBooleanArray,
) -> jlong {
    let decoder = guard(0, || {
        // SAFETY: 0 or a live handle from modelOpen*, as the Kotlin caller promises.
        let Some(model) = (unsafe { model_of(model) }) else {
            return 0;
        };
        let Some(parameters) = swipe::default_parameters() else {
            return 0;
        };
        let Some((never_words, never_pairs, extra)) = swipe_inputs(
            &mut env,
            &never_words,
            &never_pairs,
            &extra_texts,
            &extra_costs,
            &extra_contractions,
        ) else {
            return 0;
        };
        handle(swipe::new_decoder(
            model,
            parameters,
            &never_words,
            &never_pairs,
            extra,
        ))
    });
    settled(&mut env, decoder)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeFree(
    _env: JNIEnv,
    _this: JObject,
    decoder: jlong,
) {
    // SAFETY: 0 or a handle from swipeNew, freed once, as the Kotlin caller promises.
    unsafe { swipe::hk_swipe_free(decoder as *mut HkSwipeDecoder) };
}

/// `SwipeReading(texts, costs, scored)`.
fn swipe_reading<'local>(env: &mut JNIEnv<'local>, reading: Reading) -> Result<JObject<'local>> {
    let count = jint::try_from(reading.candidates.len()).unwrap_or(jint::MAX);
    let texts = env.new_object_array(count, "java/lang/String", JObject::null())?;
    for (i, candidate) in (0..count).zip(&reading.candidates) {
        let text = env.new_string(&candidate.text)?;
        env.set_object_array_element(&texts, i, &text)?;
        env.delete_local_ref(text)?;
    }
    let costs: Vec<jdouble> = reading.candidates.iter().map(|c| c.cost).collect();
    let cost_array = env.new_double_array(count)?;
    env.set_double_array_region(&cost_array, 0, &costs[..count as usize])?;
    let scored = jint::try_from(reading.scored).unwrap_or(jint::MAX);
    env.new_object(
        "ly/anoma/hex/typing/SwipeReading",
        "([Ljava/lang/String;[DI)V",
        &[
            JValue::Object(&texts),
            JValue::Object(&cost_array),
            JValue::Int(scored),
        ],
    )
}

/// The likeliest words for a swipe, or null. `points` interleaves (x, y); the key arrays
/// run in step.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeRead(
    mut env: JNIEnv,
    _this: JObject,
    decoder: jlong,
    points: JDoubleArray,
    times: JDoubleArray,
    key_letters: JIntArray,
    key_xs: JDoubleArray,
    key_ys: JDoubleArray,
    pitch_width: jdouble,
    pitch_height: jdouble,
    previous: JString,
    limit: jint,
) -> jobject {
    let reading = guard(ptr::null_mut(), || {
        // SAFETY: 0 or a live handle from swipeNew, as the Kotlin caller promises.
        let Some(decoder) = (unsafe { decoder_of(decoder) }) else {
            return ptr::null_mut();
        };
        let read = |env: &mut JNIEnv| -> Option<Reading> {
            let points = doubles(env, &points).ok()?;
            let times = if times.is_null() {
                None
            } else {
                Some(doubles(env, &times).ok()?)
            };
            let (letters, xs, ys) = (
                ints(env, &key_letters).ok()?,
                doubles(env, &key_xs).ok()?,
                doubles(env, &key_ys).ok()?,
            );
            if xs.len() != letters.len() || ys.len() != letters.len() {
                return None;
            }
            let centres = letters
                .iter()
                .zip(&xs)
                .zip(&ys)
                .map(|((&letter, &x), &y)| HkKeyCentre {
                    letter: letter as u32,
                    x,
                    y,
                });
            let keys = swipe::keys(centres, (pitch_width, pitch_height))?;
            let previous = if previous.is_null() {
                None
            } else {
                Some(string(env, &previous).ok()?)
            };
            let limit = usize::try_from(limit).ok()?;
            swipe::read(
                decoder,
                &points,
                times.as_deref(),
                &keys,
                previous.as_deref(),
                limit,
            )
        };
        let Some(reading) = read(&mut env) else {
            return ptr::null_mut();
        };
        swipe_reading(&mut env, reading).map_or(ptr::null_mut(), JObject::into_raw)
    });
    settled(&mut env, reading)
}

// MARK: - What a swipe types
//
// The Kotlin these entry points expect, in `ly.anoma.hex.typing` (beside `SwipeReading`):
//
//     /** Whether the dictionary knows a word; the core asks only while a call runs. */
//     fun interface WordChecker {
//         fun isWordValid(word: String): Boolean
//     }
//
//     /** What a finished swipe types: a space first or not, the word, the strip's others. */
//     data class SwipeTyped(val space: Boolean, val word: String, val alternatives: Array<String>)
//
// and in `object KeyboardCore`:
//
//     /**
//      * Your text replacements (`shortcuts` lowercased, in step with `replacements`) and words
//      * (`own`: other own spellings; `vocabulary`: your vocabulary's words), for the layout's
//      * `language`; 0 when malformed. Immutable: make a new one when any of them changes.
//      */
//     external fun swipeWordsNew(
//         language: String,
//         hasDictionary: Boolean,
//         shortcuts: Array<String>,
//         replacements: Array<String>,
//         own: Array<String>,
//         vocabulary: Array<String>,
//     ): Long
//
//     external fun swipeWordsFree(words: Long)
//
//     /** The order of a reading's candidates once rare names cost more; null when malformed. */
//     external fun swipeRank(decoder: Long, words: Long, texts: Array<String>, costs: DoubleArray, checker: WordChecker): IntArray?
//
//     /**
//      * What a swipe with these `readings` (best first, those that may be offered; at least one)
//      * types. `shift`: 0 off, 1 on, 2 caps lock. `autoCapitalization`: 0 none, 1 words,
//      * 2 sentences, 3 all characters (0 also with Auto-capitalization off). Null when malformed.
//      */
//     external fun swipeTyped(
//         words: Long,
//         readings: Array<String>,
//         before: String?,
//         shift: Int,
//         shiftHeld: Boolean,
//         autoCapitalization: Int,
//         autoCapitalizes: Boolean,
//         autocorrection: Boolean,
//         checker: WordChecker,
//     ): SwipeTyped?
//
//     external fun swipeNeedsSpace(before: String?): Boolean
//
//     /** The cost of English contraction `word` (any case), as `hk_swipe_cost_of_contraction` with the core's base. */
//     external fun swipeCostOfContraction(word: String): Double
//
//     /** The cost of one of your own words. */
//     external fun swipeOwnWordCost(): Double
//
//     /** The English contractions Swipe typing reads as common words, in iOS's order. */
//     external fun swipeEnglishContractions(): Array<String>
//
//     /** `points` interleaves (x, y); `key` is the character of the key it went down and lifted on. */
//     external fun swipeIsSlidTap(points: DoubleArray, pitch: Double, key: String?, firstReading: String?): Boolean
//
// A `WordChecker` that throws counts as "not a word": its exception is cleared at once, since no
// other JNI call may follow a pending one. R8 must keep `WordChecker.isWordValid` and
// `SwipeTyped`'s `(boolean, String, String[])` constructor too, once a release build ships this.

/// The words behind a handle from `swipeWordsNew`, or None for 0.
///
/// # Safety
/// `handle` is 0 or a live handle from `swipeWordsNew`.
unsafe fn words_of<'a>(handle: jlong) -> Option<&'a HkSwipeWords> {
    // SAFETY: 0 or a live HkSwipeWords pointer, as the caller promises.
    unsafe { (handle as *const HkSwipeWords).as_ref() }
}

fn optional_string(env: &mut JNIEnv, value: &JString) -> Option<Option<String>> {
    if value.is_null() {
        Some(None)
    } else {
        string(env, value).ok().map(Some)
    }
}

/// `checker.isWordValid(word)`, false when it throws (the exception cleared) or can't be asked.
fn is_word_valid(env: &mut JNIEnv, checker: &JObject, word: &str) -> bool {
    let answer = (|| -> Result<bool> {
        let word = env.new_string(word)?;
        let answer = env
            .call_method(
                checker,
                "isWordValid",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&word)],
            )?
            .z()?;
        env.delete_local_ref(word)?;
        Ok(answer)
    })();
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    answer.unwrap_or(false)
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeWordsNew(
    mut env: JNIEnv,
    _this: JObject,
    language: JString,
    has_dictionary: jboolean,
    shortcuts: JObjectArray,
    replacements: JObjectArray,
    own: JObjectArray,
    vocabulary: JObjectArray,
) -> jlong {
    let words = guard(0, || {
        let read = |env: &mut JNIEnv| -> Option<SwipeWords> {
            let language = string(env, &language).ok()?;
            let shortcuts = strings(env, &shortcuts).ok()?;
            let replacements = strings(env, &replacements).ok()?;
            if shortcuts.len() != replacements.len() {
                return None;
            }
            let own = strings(env, &own).ok()?;
            let vocabulary = strings(env, &vocabulary).ok()?;
            Some(SwipeWords::new(
                &language,
                has_dictionary != 0,
                shortcuts.into_iter().zip(replacements).collect(),
                own,
                vocabulary,
            ))
        };
        read(&mut env).map_or(0, |words| {
            handle(Box::into_raw(Box::new(HkSwipeWords { words })))
        })
    });
    settled(&mut env, words)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeWordsFree(
    _env: JNIEnv,
    _this: JObject,
    words: jlong,
) {
    // SAFETY: 0 or a handle from swipeWordsNew, freed once, as the Kotlin caller promises.
    unsafe { typing::hk_swipe_words_free(words as *mut HkSwipeWords) };
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeRank(
    mut env: JNIEnv,
    _this: JObject,
    decoder: jlong,
    words: jlong,
    texts: JObjectArray,
    costs: JDoubleArray,
    checker: JObject,
) -> jobject {
    let order = guard(ptr::null_mut(), || {
        // SAFETY: 0 or live handles, as the Kotlin caller promises.
        let (Some(decoder), Some(words)) =
            (unsafe { decoder_of(decoder) }, unsafe { words_of(words) })
        else {
            return ptr::null_mut();
        };
        let read = |env: &mut JNIEnv| -> Option<Vec<Candidate>> {
            let texts = strings(env, &texts).ok()?;
            let costs = doubles(env, &costs).ok()?;
            (texts.len() == costs.len()).then(|| {
                texts
                    .into_iter()
                    .zip(costs)
                    .map(|(text, cost)| Candidate { text, cost })
                    .collect()
            })
        };
        let Some(candidates) = read(&mut env) else {
            return ptr::null_mut();
        };
        let mut check = |word: &str| is_word_valid(&mut env, &checker, word);
        let order = decoder
            .decoder
            .ranked(&candidates, &words.words, &mut check);
        let order: Vec<jint> = order.into_iter().map(|i| i as jint).collect();
        let array = (|| -> Result<JObject> {
            let array = env.new_int_array(order.len() as jint)?;
            env.set_int_array_region(&array, 0, &order)?;
            Ok(array.into())
        })();
        array.map_or(ptr::null_mut(), JObject::into_raw)
    });
    settled(&mut env, order)
}

/// `SwipeTyped(space, word, alternatives)`.
fn swipe_typed<'local>(env: &mut JNIEnv<'local>, typed: Typed) -> Result<JObject<'local>> {
    let count = jint::try_from(typed.alternatives.len()).unwrap_or(jint::MAX);
    let alternatives = env.new_object_array(count, "java/lang/String", JObject::null())?;
    for (i, text) in (0..count).zip(&typed.alternatives) {
        let text = env.new_string(text)?;
        env.set_object_array_element(&alternatives, i, &text)?;
        env.delete_local_ref(text)?;
    }
    let word = env.new_string(&typed.word)?;
    env.new_object(
        "ly/anoma/hex/typing/SwipeTyped",
        "(ZLjava/lang/String;[Ljava/lang/String;)V",
        &[
            JValue::Bool(u8::from(typed.space)),
            JValue::Object(&word),
            JValue::Object(&alternatives),
        ],
    )
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeTyped(
    mut env: JNIEnv,
    _this: JObject,
    words: jlong,
    readings: JObjectArray,
    before: JString,
    shift: jint,
    shift_held: jboolean,
    auto_capitalization: jint,
    auto_capitalizes: jboolean,
    autocorrection: jboolean,
    checker: JObject,
) -> jobject {
    let typed = guard(ptr::null_mut(), || {
        // SAFETY: 0 or a live handle from swipeWordsNew, as the Kotlin caller promises.
        let Some(words) = (unsafe { words_of(words) }) else {
            return ptr::null_mut();
        };
        let (Some(readings), Some(before)) = (
            strings(&mut env, &readings).ok(),
            optional_string(&mut env, &before),
        ) else {
            return ptr::null_mut();
        };
        let (Some(shift), Some(auto_capitalization)) = (
            u8::try_from(shift).ok().and_then(typing::shift),
            u8::try_from(auto_capitalization)
                .ok()
                .and_then(typing::auto_capitalization),
        ) else {
            return ptr::null_mut();
        };
        let context = SwipeContext {
            before: before.as_deref(),
            shift,
            shift_held: shift_held != 0,
            auto_capitalization,
            auto_capitalizes: auto_capitalizes != 0,
            autocorrection: autocorrection != 0,
        };
        let readings: Vec<&str> = readings.iter().map(String::as_str).collect();
        let mut check = |word: &str| is_word_valid(&mut env, &checker, word);
        let Some(typed) = core_typing::typed(&readings, &context, &words.words, &mut check) else {
            return ptr::null_mut();
        };
        swipe_typed(&mut env, typed).map_or(ptr::null_mut(), JObject::into_raw)
    });
    settled(&mut env, typed)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeNeedsSpace(
    mut env: JNIEnv,
    _this: JObject,
    before: JString,
) -> jboolean {
    let needs = guard(0, || {
        optional_string(&mut env, &before).map_or(0, |before| {
            u8::from(core_typing::needs_space(before.as_deref()))
        })
    });
    settled(&mut env, needs)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeIsSlidTap(
    mut env: JNIEnv,
    _this: JObject,
    points: JDoubleArray,
    pitch: jdouble,
    key: JString,
    first_reading: JString,
) -> jboolean {
    let slid = guard(0, || {
        let (Some(points), Some(key), Some(first_reading)) = (
            doubles(&env, &points).ok(),
            optional_string(&mut env, &key),
            optional_string(&mut env, &first_reading),
        ) else {
            return 0;
        };
        if !points.len().is_multiple_of(2) {
            return 0;
        }
        let path: Vec<(f64, f64)> = points
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[x, y]| (x, y))
            .collect();
        u8::from(core_typing::is_slid_tap(
            &path,
            pitch,
            key.as_deref(),
            first_reading.as_deref(),
        ))
    });
    settled(&mut env, slid)
}

/// `hk_swipe_cost_of_contraction` with the core's base; a null or unreadable word is no
/// contraction.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeCostOfContraction(
    mut env: JNIEnv,
    _this: JObject,
    word: JString,
) -> jdouble {
    let none = CONTRACTION_COST + 2.0;
    let cost = guard(none, || {
        optional_string(&mut env, &word)
            .flatten()
            .map_or(none, |word| cost_of_contraction(&word, CONTRACTION_COST))
    });
    settled(&mut env, cost)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeOwnWordCost(
    _env: JNIEnv,
    _this: JObject,
) -> jdouble {
    OWN_WORD_COST
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_swipeEnglishContractions(
    mut env: JNIEnv,
    _this: JObject,
) -> jobject {
    let words = guard(ptr::null_mut(), || {
        let array = (|| -> Result<JObject> {
            let count = ENGLISH_CONTRACTIONS.len() as jint;
            let array = env.new_object_array(count, "java/lang/String", JObject::null())?;
            for (i, word) in (0..count).zip(ENGLISH_CONTRACTIONS) {
                let text = env.new_string(word)?;
                env.set_object_array_element(&array, i, &text)?;
                env.delete_local_ref(text)?;
            }
            Ok(array.into())
        })();
        array.map_or(ptr::null_mut(), JObject::into_raw)
    });
    settled(&mut env, words)
}
