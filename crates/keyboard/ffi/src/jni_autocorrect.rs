//! Autocorrect's entry points for the Android IME (`KeyboardCore.autocorrect*`): the C
//! interface's `hk_autocorrect_*` with Kotlin's types. The spelling dictionary is a Kotlin
//! `KeyboardCore.TextChecker`, called back on the calling thread during the call that needs
//! it. A handle is used from one thread at a time. A failure returns 0, null or false and never
//! throws; a Java exception a callback raised is cleared and counts as no answer.
//!
//! R8 must keep `TextChecker`'s methods and `AutocorrectSuggestion`'s `(String, String, int)`
//! constructor, which these call by name, once a release build ships the core.

use std::ptr;
use std::sync::Arc;

use hex_keyboard_core::autocorrect::{
    Autocorrect, Bundled, FieldKind, KeyCell, NeverSuggestList, Suggestion, SuggestionKind,
    TextChecker, TypingLearning,
};
use jni::objects::{GlobalRef, JDoubleArray, JIntArray, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jdouble, jint, jlong, jobjectArray, jstring};
use jni::{JNIEnv, JavaVM};

use crate::autocorrect::HkAutocorrect;
use crate::guard;
use crate::jni::{Result, doubles, handle, ints, model_of, settled, string, strings};

/// A Kotlin `KeyboardCore.TextChecker`.
struct KotlinChecker {
    vm: JavaVM,
    checker: GlobalRef,
}

impl KotlinChecker {
    fn new(env: &mut JNIEnv, checker: &JObject) -> Option<Self> {
        if checker.is_null() {
            return None;
        }
        Some(Self {
            vm: env.get_java_vm().ok()?,
            checker: env.new_global_ref(checker).ok()?,
        })
    }

    /// Calls `method` with `word`; None (an exception cleared) when it fails.
    fn call<T>(
        &self,
        method: &str,
        signature: &str,
        word: &str,
        read: impl FnOnce(&mut JNIEnv, jni::objects::JValueOwned) -> Result<T>,
    ) -> Option<T> {
        let mut env = self.vm.get_env().ok()?;
        let answer = env
            .with_local_frame(8, |env| -> Result<T> {
                let word = env.new_string(word)?;
                let value =
                    env.call_method(&self.checker, method, signature, &[JValue::Object(&word)])?;
                read(env, value)
            })
            .ok();
        settled(&mut env, answer)
    }

    fn list(&self, method: &str, word: &str) -> Vec<String> {
        self.call(
            method,
            "(Ljava/lang/String;)[Ljava/lang/String;",
            word,
            |env, value| {
                let array = JObjectArray::from(value.l()?);
                if array.is_null() {
                    return Ok(Vec::new());
                }
                strings(env, &array)
            },
        )
        .unwrap_or_default()
    }
}

impl TextChecker for KotlinChecker {
    fn is_word_valid(&self, word: &str) -> bool {
        self.call("isWordValid", "(Ljava/lang/String;)Z", word, |_, v| v.z())
            .unwrap_or(false)
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        self.list("guesses", word)
    }

    fn completions(&self, prefix: &str) -> Vec<String> {
        self.list("completions", prefix)
    }

    fn use_language(&mut self, code: &str) {
        self.call("useLanguage", "(Ljava/lang/String;)V", code, |_, _| Ok(()));
    }
}

/// The core behind `handle`.
///
/// # Safety
/// `handle` is 0 or a live handle from `autocorrectNew` no other call is using.
unsafe fn core_of<'a>(handle: jlong) -> Option<&'a mut Autocorrect> {
    // SAFETY: 0 or a live HkAutocorrect pointer, as the caller promises.
    unsafe { (handle as *mut HkAutocorrect).as_mut() }.map(|a| &mut a.core)
}

/// Runs `body` on the core under the panic guard, clearing any exception it left.
///
/// # Safety
/// As `core_of`.
unsafe fn with<T>(
    env: &mut JNIEnv,
    handle: jlong,
    fallback: T,
    body: impl FnOnce(&mut JNIEnv, &mut Autocorrect) -> Option<T>,
) -> T {
    // SAFETY: as the caller promises.
    let Some(core) = (unsafe { core_of(handle) }) else {
        return fallback;
    };
    let value = guard(None, || body(env, core));
    settled(env, value).unwrap_or(fallback)
}

fn optional_string(env: &mut JNIEnv, value: &JString) -> Option<Option<String>> {
    if value.is_null() {
        return Some(None);
    }
    string(env, value).ok().map(Some)
}

fn java_string(env: &mut JNIEnv, text: Option<String>) -> jstring {
    match text {
        None => ptr::null_mut(),
        Some(text) => env
            .new_string(text)
            .map_or(ptr::null_mut(), JString::into_raw),
    }
}

fn java_strings(env: &mut JNIEnv, items: &[String]) -> Result<jobjectArray> {
    let count = jint::try_from(items.len()).unwrap_or(jint::MAX);
    let array = env.new_object_array(count, "java/lang/String", JObject::null())?;
    for (i, item) in (0..count).zip(items) {
        let text = env.new_string(item)?;
        env.set_object_array_element(&array, i, &text)?;
        env.delete_local_ref(text)?;
    }
    Ok(array.into_raw())
}

/// (x, y) pairs from an interleaved array; None for an odd length.
fn pairs(flat: &[f64]) -> Option<Vec<(f64, f64)>> {
    let (pairs, rest) = flat.as_chunks::<2>();
    rest.is_empty()
        .then(|| pairs.iter().map(|&[x, y]| (x, y)).collect())
}

fn field(field: jint) -> FieldKind {
    match field {
        1 => FieldKind::Url,
        2 => FieldKind::Email,
        _ => FieldKind::Text,
    }
}

/// A new autocorrect in English reading `neverSuggest` (the never-suggest list's text), or 0.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectNew(
    mut env: JNIEnv,
    _this: JObject,
    never_suggest: JString,
    checker: JObject,
) -> jlong {
    let made = guard(None, || {
        let tsv = string(&mut env, &never_suggest).ok()?;
        let checker = KotlinChecker::new(&mut env, &checker)?;
        let bundled = Arc::new(Bundled {
            never_suggest: Arc::new(NeverSuggestList::new(tsv)),
            german_capitals: None,
        });
        let core = Autocorrect::new(bundled, Box::new(checker));
        Some(handle(Box::into_raw(Box::new(HkAutocorrect { core }))))
    })
    .unwrap_or(0);
    settled(&mut env, made)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectFree(
    _env: JNIEnv,
    _this: JObject,
    ac: jlong,
) {
    // SAFETY: 0 or a handle from autocorrectNew, freed once, as the Kotlin caller promises.
    unsafe { crate::autocorrect::hk_autocorrect_free(ac as *mut HkAutocorrect) };
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectSetSettings(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    enabled: jboolean,
    allows_swearing: jboolean,
    adds_capitals: jboolean,
    learns_per_language: jboolean,
) {
    // SAFETY: 0 or a live handle, as the Kotlin caller promises.
    unsafe {
        with(&mut env, ac, (), |_, c| {
            c.set_enabled(enabled != 0);
            c.set_allows_swearing(allows_swearing != 0);
            c.set_adds_capitals(adds_capitals != 0);
            c.set_learns_per_language(learns_per_language != 0);
            Some(())
        })
    }
}

/// Follows the keyboard layout: `tag` the language, `available` the device's dictionaries.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectSetLanguage(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    tag: JString,
    available: JObjectArray,
) -> jboolean {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, 0, |env, c| {
            let (tag, available) = (string(env, &tag).ok()?, strings(env, &available).ok()?);
            c.set_language(&tag, &available);
            Some(1)
        })
    }
}

/// Bilingual autocorrect's second language (`tag` null: none) with its own dictionary.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectSetPartner(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    tag: JString,
    available: JObjectArray,
    checker: JObject,
) -> jboolean {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, 0, |env, c| {
            let tag = optional_string(env, &tag)?;
            let available = strings(env, &available).ok()?;
            match tag {
                None => c.set_partner(None, &available, Box::new(NoWords)),
                Some(tag) => {
                    let checker = KotlinChecker::new(env, &checker)?;
                    c.set_partner(Some(&tag), &available, Box::new(checker));
                }
            }
            Some(1)
        })
    }
}

/// The dictionary of no partner.
struct NoWords;

impl TextChecker for NoWords {
    fn is_word_valid(&self, _: &str) -> bool {
        false
    }
    fn guesses(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
    fn completions(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
    fn use_language(&mut self, _: &str) {}
}

/// The keys typed on: `layout`'s language tag and its letter grid, in step (a letter's code
/// point, column and row).
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectSetKeys(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    layout: JString,
    letters: JIntArray,
    columns: JDoubleArray,
    rows: JDoubleArray,
) -> jboolean {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, 0, |env, c| {
            let layout = string(env, &layout).ok()?;
            let (letters, columns, rows) = (
                ints(env, &letters).ok()?,
                doubles(env, &columns).ok()?,
                doubles(env, &rows).ok()?,
            );
            if columns.len() != letters.len() || rows.len() != letters.len() {
                return None;
            }
            let cells = letters
                .iter()
                .zip(columns.iter().zip(&rows))
                .map(|(&l, (&column, &row))| {
                    Some(KeyCell {
                        letter: char::from_u32(u32::try_from(l).ok()?)?,
                        row,
                        column,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            c.set_keys(&layout, &cells);
            Some(1)
        })
    }
}

/// Your text replacements and terms: typing `inputs[i]` gives `outputs[i]`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectLoadLexicon(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    inputs: JObjectArray,
    outputs: JObjectArray,
) -> jboolean {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, 0, |env, c| {
            let (inputs, outputs) = (strings(env, &inputs).ok()?, strings(env, &outputs).ok()?);
            if inputs.len() != outputs.len() {
                return None;
            }
            let entries: Vec<(String, String)> = inputs.into_iter().zip(outputs).collect();
            c.load_lexicon_entries(&entries);
            Some(1)
        })
    }
}

/// Hex's vocabulary (`which` 0) or the kept words of this language's list (1) or the
/// partner's (2), as `hk_autocorrect_set_words`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectSetWords(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    which: jint,
    words: JObjectArray,
) -> jboolean {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, 0, |env, c| {
            let words = strings(env, &words).ok()?;
            match which {
                0 => c.set_vocabulary(&words),
                1 => c.set_learned_words(&words),
                2 => c.set_partner_learned_words(&words),
                _ => return None,
            }
            Some(1)
        })
    }
}

fn learning(
    env: &mut JNIEnv,
    typos: &JObjectArray,
    fixes: &JObjectArray,
    rejected: &JObjectArray,
) -> Option<TypingLearning> {
    let (typos, fixes, rejected) = (
        strings(env, typos).ok()?,
        strings(env, fixes).ok()?,
        strings(env, rejected).ok()?,
    );
    if typos.len() != fixes.len() {
        return None;
    }
    let pairs: Vec<(String, String)> = typos.into_iter().zip(fixes).collect();
    Some(TypingLearning::new(&pairs, &rejected))
}

/// What Learn how I type learned; the partner's arrays are null to give it the same.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectApplyLearning(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    typos: JObjectArray,
    fixes: JObjectArray,
    rejected: JObjectArray,
    partner_typos: JObjectArray,
    partner_fixes: JObjectArray,
    partner_rejected: JObjectArray,
) -> jboolean {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, 0, |env, c| {
            let mine = learning(env, &typos, &fixes, &rejected)?;
            let theirs = if partner_typos.is_null() {
                None
            } else {
                Some(learning(
                    env,
                    &partner_typos,
                    &partner_fixes,
                    &partner_rejected,
                )?)
            };
            c.apply_learning(&mine, theirs.as_ref());
            Some(1)
        })
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectSuppressAndKeep(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    word: JString,
) {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, (), |env, c| {
            c.suppress_and_keep(&string(env, &word).ok()?);
            Some(())
        })
    }
}

/// Spatial autocorrect's model (`which` 0), the partner's (1) or the context model (2); 0 for
/// none.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectSetModel(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    which: jint,
    model: jlong,
) -> jboolean {
    // SAFETY: 0 or a live model handle, as the Kotlin caller promises.
    let model = unsafe { model_of(model) }.map(|m| m.model.clone());
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, 0, |_, c| {
            match which {
                0 => c.set_decoder(model),
                1 => c.set_partner_decoder(model),
                2 => c.set_context_model(model),
                _ => return None,
            }
            Some(1)
        })
    }
}

/// Where each letter of the word being typed was tapped, (x, y) interleaved; null when the
/// keys don't know.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectSetTaps(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    taps: JDoubleArray,
) {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, (), |env, c| {
            let taps = if taps.is_null() {
                None
            } else {
                let flat = doubles(env, &taps).ok()?;
                Some(pairs(&flat)?)
            };
            c.set_decoder_taps(taps);
            Some(())
        })
    }
}

fn kind(kind: SuggestionKind) -> jint {
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

fn java_suggestions(env: &mut JNIEnv, list: Vec<Suggestion>) -> Result<jobjectArray> {
    let class = env.find_class("ly/anoma/hex/typing/AutocorrectSuggestion")?;
    let count = jint::try_from(list.len()).unwrap_or(jint::MAX);
    let array = env.new_object_array(count, &class, JObject::null())?;
    for (i, s) in (0..count).zip(list) {
        let (display, replacement) = (
            env.new_string(&s.display_text)?,
            env.new_string(&s.replacement)?,
        );
        let item = env.new_object(
            &class,
            "(Ljava/lang/String;Ljava/lang/String;I)V",
            &[
                JValue::Object(&display),
                JValue::Object(&replacement),
                JValue::Int(kind(s.kind)),
            ],
        )?;
        env.set_object_array_element(&array, i, &item)?;
        for local in [JObject::from(display), JObject::from(replacement), item] {
            env.delete_local_ref(local)?;
        }
    }
    Ok(array.into_raw())
}

/// The strip for the text before the cursor (null when unknown); `field` 0 text, 1 URL, 2
/// email; `predictionModel` 0 for none. Null on a bad argument.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectSuggestions(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    before: JString,
    field_kind: jint,
    autocorrection_off: jboolean,
    prediction_model: jlong,
    capitalizes_next: jboolean,
) -> jobjectArray {
    // SAFETY: 0 or a live model handle, as the Kotlin caller promises.
    let model = unsafe { model_of(prediction_model) }.map(|m| m.model.clone());
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, ptr::null_mut(), |env, c| {
            let before = optional_string(env, &before)?;
            let list = c.suggestions(
                before.as_deref(),
                field(field_kind),
                autocorrection_off != 0,
                model.as_deref(),
                capitalizes_next != 0,
            );
            java_suggestions(env, list).ok()
        })
    }
}

/// What space puts in: [what was typed, its replacement], or null to keep it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectReplacementOnSpace(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    before: JString,
    field_kind: jint,
    autocorrection_off: jboolean,
) -> jobjectArray {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, ptr::null_mut(), |env, c| {
            let before = optional_string(env, &before)?;
            match c.replacement_on_space(
                before.as_deref(),
                field(field_kind),
                autocorrection_off != 0,
            ) {
                None => Some(ptr::null_mut()),
                Some((typed, fix)) => java_strings(env, &[typed, fix]).ok(),
            }
        })
    }
}

/// The word before the one being finished revised now that `next` follows it, or null.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectRevision(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    word: JString,
    before: JString,
    next: JString,
    field_kind: jint,
    autocorrection_off: jboolean,
) -> jstring {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, ptr::null_mut(), |env, c| {
            let (word, before, next) = (
                string(env, &word).ok()?,
                string(env, &before).ok()?,
                string(env, &next).ok()?,
            );
            let revised = c.revision(
                &word,
                &before,
                &next,
                field(field_kind),
                autocorrection_off != 0,
            );
            Some(java_string(env, revised))
        })
    }
}

/// One word for `first` (its taps interleaved), a space tapped as a letter and `second`, or
/// null to keep both.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectMerge(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    first: JString,
    first_taps: JDoubleArray,
    space_x: jdouble,
    space_y: jdouble,
    second: JString,
    before: JString,
) -> jstring {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, ptr::null_mut(), |env, c| {
            let (first, second, before) = (
                string(env, &first).ok()?,
                string(env, &second).ok()?,
                string(env, &before).ok()?,
            );
            let taps = pairs(&doubles(env, &first_taps).ok()?)?;
            let merged = c.merge(&first, &taps, (space_x, space_y), &second, &before);
            Some(java_string(env, merged))
        })
    }
}

/// Your text replacement whose shortcut ends `before`: [the shortcut as typed, what it gives],
/// or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectTextReplacement(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    before: JString,
) -> jobjectArray {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, ptr::null_mut(), |env, c| {
            let before = optional_string(env, &before)?;
            match c.text_replacement(before.as_deref()) {
                None => Some(ptr::null_mut()),
                Some((typed, fix)) => java_strings(env, &[typed, fix]).ok(),
            }
        })
    }
}

/// Whether `word` may be offered after `previous` (null: none).
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectMayOffer(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
    word: JString,
    previous: JString,
) -> jboolean {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, 0, |env, c| {
            let (word, previous) = (string(env, &word).ok()?, optional_string(env, &previous)?);
            Some(jboolean::from(c.may_offer(&word, previous.as_deref())))
        })
    }
}

/// The language of the fix space last decided on, or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_autocorrectLastFixLanguage(
    mut env: JNIEnv,
    _this: JObject,
    ac: jlong,
) -> jstring {
    // SAFETY: as above.
    unsafe {
        with(&mut env, ac, ptr::null_mut(), |env, c| {
            let language = c.last_fix_language().map(str::to_owned);
            Some(java_string(env, language))
        })
    }
}

/// The word right before the caret in `before`, or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_currentToken(
    mut env: JNIEnv,
    _this: JObject,
    before: JString,
) -> jstring {
    let token = guard(None, || {
        let before = optional_string(&mut env, &before)?;
        Some(java_string(
            &mut env,
            hex_keyboard_core::autocorrect::current_token(before.as_deref()),
        ))
    });
    settled(&mut env, token).unwrap_or(ptr::null_mut())
}

/// Whether the core has `tag`'s language manifest.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ly_anoma_hex_typing_KeyboardCore_languageKnown(
    mut env: JNIEnv,
    _this: JObject,
    tag: JString,
) -> jboolean {
    let known = guard(false, || {
        string(&mut env, &tag).is_ok_and(|tag| {
            hex_keyboard_core::autocorrect::LanguageConfig::for_tag(&tag).id == tag
        })
    });
    settled(&mut env, jboolean::from(known))
}
