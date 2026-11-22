//! The Android IME's entry points: `external fun`s of the Kotlin `object`
//! `ly.anoma.hex.typing.KeyboardCore`. Handles are the C interface's pointers as `Long`s (0
//! for none); a failure returns 0 or null, never throws from Rust.

use std::path::Path;
use std::ptr;

use hex_keyboard_core::swipe::{ExtraWord, Reading};
use jni::JNIEnv;
use jni::objects::{
    JBooleanArray, JDoubleArray, JIntArray, JObject, JObjectArray, JPrimitiveArray, JString, JValue,
};
use jni::sys::{jboolean, jdouble, jint, jlong, jobject};

use crate::model::{self, HkModel};
use crate::swipe::{self, HkKeyCentre, HkSwipeDecoder};
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
    guard(0, || {
        if path.is_null() {
            return 0;
        }
        match string(&mut env, &path) {
            Ok(path) => handle(model::open_path(Path::new(&path))),
            Err(_) => 0,
        }
    })
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
    guard(0, || {
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
    })
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
    guard(ptr::null_mut(), || {
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
    })
}
