//! The dictation rules' C interface as Swift calls it: the committed parity fixture through
//! `hk_dictation_*`, the trap case, and the pointers and bytes it refuses.

use std::ffi::{CStr, c_void};
use std::ptr;

use hex_keyboard::*;
use serde_json::Value;

const FIXTURE: &str = include_str!("../../core/tests/fixtures/dictation-parity.jsonl");

fn s(text: &str) -> HkStr {
    HkStr {
        bytes: text.as_ptr().cast(),
        length: text.len(),
    }
}

/// The core's text as a String, released.
fn take(mut text: HkText) -> String {
    let out = if text.bytes.is_null() {
        String::new()
    } else {
        // SAFETY: the core wrote `length` bytes of UTF-8 there.
        let bytes = unsafe { std::slice::from_raw_parts(text.bytes.cast::<u8>(), text.length) };
        String::from_utf8(bytes.to_vec()).unwrap()
    };
    unsafe { hk_text_free(&mut text) };
    out
}

fn empty() -> HkText {
    HkText {
        bytes: ptr::null_mut(),
        length: 0,
    }
}

/// Owned strings behind an `HkLanguage`.
struct Lang {
    identifier: String,
    code: Option<String>,
    region: Option<String>,
}

impl Lang {
    fn from(case: &Value) -> Option<Lang> {
        let field = |name: &str| case.get(name).and_then(Value::as_str).map(str::to_owned);
        Some(Lang {
            identifier: field("lang")?,
            code: field("code"),
            region: field("region"),
        })
    }

    fn c(&self) -> HkLanguage {
        let optional = |text: &Option<String>| {
            text.as_deref().map_or(
                HkStr {
                    bytes: ptr::null(),
                    length: 0,
                },
                s,
            )
        };
        HkLanguage {
            identifier: s(&self.identifier),
            code: optional(&self.code),
            region: optional(&self.region),
        }
    }
}

type Stage = unsafe extern "C" fn(HkStr, *const HkLanguage, *mut HkText) -> i32;

fn stage(f: Stage, text: &str, language: *const HkLanguage) -> String {
    let mut out = empty();
    assert_eq!(
        unsafe { f(s(text), language, &mut out) },
        HK_DICTATION_OK,
        "{text:?}"
    );
    take(out)
}

unsafe extern "C" fn in_dictionary(context: *mut c_void, word: HkStr) -> u8 {
    // SAFETY: the test passes a `Vec<String>` as context; the core a valid word.
    let words = unsafe { &*context.cast::<Vec<String>>() };
    let word = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(
            word.bytes.cast::<u8>(),
            word.length,
        ))
    }
    .unwrap();
    u8::from(words.iter().any(|w| w == word))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

fn run(case: &Value) -> Option<Value> {
    let lang = Lang::from(case);
    let language = lang.as_ref().map(Lang::c);
    let language_ptr = language
        .as_ref()
        .map_or(ptr::null(), |l| l as *const HkLanguage);
    let text = case.get("text").and_then(Value::as_str).unwrap_or("");
    let out = match case["op"].as_str()? {
        "format" => {
            let style = ["polished", "casual", "notes", "verbatim"]
                .iter()
                .position(|&n| n == case["style"])
                .unwrap();
            let options = HkFormatOptions {
                style: style as u32,
                lists: case["lists"].as_bool()? as u8,
                punctuation: case["punct"].as_bool()? as u8,
                ends_with_period: case["period"].as_bool()? as u8,
            };
            let mut out = empty();
            assert_eq!(
                unsafe { hk_dictation_format(s(text), language_ptr, &options, &mut out) },
                HK_DICTATION_OK
            );
            Value::from(take(out))
        }
        "spoken_punctuation" => {
            Value::from(stage(hk_dictation_spoken_punctuation, text, language_ptr))
        }
        "spoken_lists" => Value::from(stage(hk_dictation_spoken_lists, text, language_ptr)),
        "cleanup_rules" => Value::from(stage(hk_dictation_cleanup_rules, text, language_ptr)),
        "spoken_symbols" => Value::from(stage(hk_dictation_spoken_symbols, text, language_ptr)),
        "spoken_counts" => Value::from(stage(hk_dictation_spoken_counts, text, language_ptr)),
        "french_spacing" => Value::from(stage(hk_dictation_french_spacing, text, language_ptr)),
        "cleanup_chain" => {
            let symbols = stage(hk_dictation_spoken_symbols, text, language_ptr);
            let counted = stage(hk_dictation_spoken_counts, &symbols, language_ptr);
            Value::from(stage(hk_dictation_cleanup_rules, &counted, language_ptr))
        }
        "reconstruct" => {
            let words = strings(&case["words"]);
            let words: Vec<HkStr> = words.iter().map(|w| s(w)).collect();
            let deletes: Vec<u8> = case["deletes"]
                .as_array()?
                .iter()
                .map(|d| d.as_bool().unwrap() as u8)
                .collect();
            let mut out = empty();
            let status = unsafe {
                hk_dictation_reconstruct(
                    words.as_ptr(),
                    words.len(),
                    deletes.as_ptr(),
                    deletes.len(),
                    &mut out,
                )
            };
            assert_eq!(status, HK_DICTATION_OK);
            Value::from(take(out))
        }
        "insertion" => {
            let mut words = case.get("words").map(strings);
            let before = case.get("before").and_then(Value::as_str).map(s);
            let after = case.get("after").and_then(Value::as_str).map(s);
            let (callback, context): (HkIsWord, *mut c_void) = match words.as_mut() {
                Some(words) => (Some(in_dictionary), (words as *mut Vec<String>).cast()),
                None => (None, ptr::null_mut()),
            };
            let mut out = empty();
            let status = unsafe {
                hk_dictation_insertion_text(
                    s(text),
                    before.as_ref().map_or(ptr::null(), |b| b as *const HkStr),
                    after.as_ref().map_or(ptr::null(), |a| a as *const HkStr),
                    callback,
                    context,
                    &mut out,
                )
            };
            assert_eq!(status, HK_DICTATION_OK);
            Value::from(take(out))
        }
        "continuing_sentence" => {
            let mut words = strings(&case["words"]);
            let mut out = empty();
            let context = (&mut words as *mut Vec<String>).cast();
            let status = unsafe {
                hk_dictation_continuing_sentence(s(text), Some(in_dictionary), context, &mut out)
            };
            assert_eq!(status, HK_DICTATION_OK);
            Value::from(take(out))
        }
        "starts_sentence" => {
            let mut out = 7;
            assert_eq!(
                unsafe { hk_dictation_starts_sentence(&s(text), &mut out) },
                HK_DICTATION_OK
            );
            Value::from(out != 0)
        }
        "dropping_final_full_stop" => {
            let mut out = empty();
            assert_eq!(
                unsafe { hk_dictation_dropping_final_full_stop(s(text), &mut out) },
                HK_DICTATION_OK
            );
            Value::from(take(out))
        }
        "words" => {
            let mut list = HkTextList {
                items: ptr::null_mut(),
                count: 0,
            };
            assert_eq!(
                unsafe { hk_dictation_words(s(text), &mut list) },
                HK_DICTATION_OK
            );
            let items = if list.count == 0 {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(list.items, list.count) }
            };
            let words: Vec<String> = items
                .iter()
                .map(|t| {
                    String::from_utf8(
                        unsafe { std::slice::from_raw_parts(t.bytes.cast::<u8>(), t.length) }
                            .to_vec(),
                    )
                    .unwrap()
                })
                .collect();
            unsafe { hk_text_list_free(&mut list) };
            Value::from(words)
        }
        "lowercases_continuations" => match language.as_ref() {
            None => Value::Null,
            Some(l) => {
                let mut out = 0;
                assert_eq!(
                    unsafe { hk_dictation_lowercases_continuations(l, &mut out) },
                    HK_DICTATION_OK
                );
                Value::from(out != 0)
            }
        },
        "supports" => {
            let mut out = 0;
            assert_eq!(
                unsafe { hk_dictation_cleanup_supports(language_ptr, &mut out) },
                HK_DICTATION_OK
            );
            Value::from(out != 0)
        }
        "filler_pattern" => {
            let pattern = unsafe { hk_dictation_filler_pattern(language_ptr) };
            if pattern.is_null() {
                Value::Null
            } else {
                Value::from(unsafe { CStr::from_ptr(pattern) }.to_str().unwrap())
            }
        }
        _ => return None,
    };
    Some(out)
}

#[test]
fn the_committed_fixture_through_the_c_interface() {
    let mut cases = 0;
    for line in FIXTURE.lines() {
        let case: Value = serde_json::from_str(line).unwrap();
        let Some(got) = run(&case) else { continue };
        assert_eq!(got, case["out"], "{case}");
        cases += 1;
    }
    assert!(cases > 3_000, "{cases} cases");
}

#[test]
fn back_to_back_addresses_report_the_trap() {
    let english = Lang {
        identifier: "en-US".into(),
        code: Some("en".into()),
        region: Some("US".into()),
    };
    let mut out = empty();
    let status = unsafe {
        hk_dictation_spoken_symbols(s("a at sign b.com at sign c.com"), &english.c(), &mut out)
    };
    assert_eq!(status, HK_DICTATION_OVERLAPPING_ADDRESSES);
    assert!(out.bytes.is_null() && out.length == 0, "out left empty");
}

#[test]
fn text_with_nul_and_empty_text_go_through() {
    assert_eq!(
        stage(hk_dictation_spoken_lists, "a\0b", ptr::null()),
        "a\0b"
    );
    assert_eq!(stage(hk_dictation_spoken_lists, "", ptr::null()), "");
    let mut out = empty();
    let none = HkStr {
        bytes: ptr::null(),
        length: 0,
    };
    assert_eq!(
        unsafe { hk_dictation_bare(none, &mut out) },
        HK_DICTATION_OK
    );
    assert_eq!(take(out), "");
}

#[test]
fn refuses_bad_pointers_bytes_and_options() {
    let mut out = empty();
    unsafe {
        let null_with_bytes = HkStr {
            bytes: ptr::null(),
            length: 3,
        };
        assert_eq!(
            hk_dictation_spoken_counts(null_with_bytes, ptr::null(), &mut out),
            HK_DICTATION_INVALID
        );
        let invalid = [0xff_u8, 0xfe];
        let not_utf8 = HkStr {
            bytes: invalid.as_ptr().cast(),
            length: 2,
        };
        assert_eq!(
            hk_dictation_spoken_counts(not_utf8, ptr::null(), &mut out),
            HK_DICTATION_INVALID
        );
        assert_eq!(
            hk_dictation_spoken_counts(s("x"), ptr::null(), ptr::null_mut()),
            HK_DICTATION_INVALID
        );
        assert_eq!(
            hk_dictation_format(s("x"), ptr::null(), ptr::null(), &mut out),
            HK_DICTATION_INVALID
        );
        let options = HkFormatOptions {
            style: 9,
            lists: 0,
            punctuation: 0,
            ends_with_period: 1,
        };
        assert_eq!(
            hk_dictation_format(s("x"), ptr::null(), &options, &mut out),
            HK_DICTATION_INVALID
        );
        assert_eq!(
            hk_dictation_french_spacing_with(s("x"), 7, &mut out),
            HK_DICTATION_INVALID
        );
        assert_eq!(
            hk_dictation_continuing_sentence(s("X"), None, ptr::null_mut(), &mut out),
            HK_DICTATION_INVALID
        );
        assert_eq!(
            hk_dictation_lowercases_continuations(ptr::null(), &mut 0),
            HK_DICTATION_INVALID
        );
        assert_eq!(
            hk_dictation_reconstruct(ptr::null(), 2, ptr::null(), 0, &mut out),
            HK_DICTATION_INVALID
        );
        assert!(out.bytes.is_null());
        hk_text_free(ptr::null_mut());
        hk_text_free(&mut out);
        hk_text_list_free(ptr::null_mut());
    }
}

#[test]
fn french_conventions() {
    let french = |region: Option<&str>| Lang {
        identifier: "fr".into(),
        code: Some("fr".into()),
        region: region.map(str::to_owned),
    };
    unsafe {
        assert_eq!(hk_dictation_french_convention(&french(None).c()), 0);
        assert_eq!(hk_dictation_french_convention(&french(Some("CA")).c()), 1);
        assert_eq!(hk_dictation_french_convention(&french(Some("CH")).c()), 2);
        assert_eq!(hk_dictation_french_convention(ptr::null()), -1);
    }
    let mut out = empty();
    assert_eq!(
        unsafe { hk_dictation_french_spacing_with(s("Quoi ?"), 1, &mut out) },
        HK_DICTATION_OK
    );
    assert_eq!(take(out), "Quoi?");
}
