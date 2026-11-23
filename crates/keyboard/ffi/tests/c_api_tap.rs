//! The tap decoder's and the model lookups' C interface as Swift calls it, on the English
//! model the iOS keyboard ships, against the core called directly.

use std::ffi::{CStr, CString, c_void};
use std::path::PathBuf;
use std::ptr;
use std::sync::Arc;

use hex_keyboard::*;
use hex_keyboard_core::decoder::{self, TypingDecoder};
use hex_keyboard_core::model::Model;

fn en_model_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ios/HEXKeyboard/TypingModel/en.hextm")
}

fn open_en() -> *mut HkModel {
    let path = CString::new(en_model_path().to_str().unwrap()).unwrap();
    let model = unsafe { hk_model_open(path.as_ptr()) };
    assert!(!model.is_null());
    model
}

fn core_en() -> Arc<Model> {
    Arc::new(Model::new(Box::new(std::fs::read(en_model_path()).unwrap())).unwrap())
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn word(model: *const HkModel, alphabet: &[u32], i: u32) -> String {
    let mut length = 0;
    let codes = unsafe { hk_model_word_codes(model, i as usize, &mut length) };
    let codes = unsafe { std::slice::from_raw_parts(codes, length) };
    codes
        .iter()
        .map(|&code| char::from_u32(alphabet[code as usize]).unwrap())
        .collect()
}

fn alphabet(model: *const HkModel) -> (HkModelInfo, Vec<u32>, Vec<u8>) {
    let mut info = HkModelInfo {
        word_count: 0,
        pair_count: 0,
        byte_count: 0,
        language: [0; 8],
        code_count: 0,
        apostrophe_code: 0,
    };
    assert_eq!(unsafe { hk_model_info(model, &mut info) }, 0);
    let mut characters = vec![0u32; info.code_count];
    let mut bases = vec![0u8; info.code_count];
    let status = unsafe {
        hk_model_alphabet(
            model,
            characters.as_mut_ptr(),
            bases.as_mut_ptr(),
            info.code_count,
        )
    };
    assert_eq!(status, 0);
    (info, characters, bases)
}

#[test]
fn reads_the_models_words_and_costs() {
    let model = open_en();
    let core = core_en();
    let (info, characters, bases) = alphabet(model);
    assert_eq!(info.word_count, 86_247);
    assert_eq!(info.pair_count, 417_182);
    assert_eq!(info.byte_count, core.byte_count());
    assert_eq!(
        unsafe { CStr::from_ptr(info.language.as_ptr()) }
            .to_str()
            .unwrap(),
        "en"
    );
    assert_eq!(info.code_count, core.code_count());
    assert_eq!(info.apostrophe_code, 1);
    for code in 0..info.code_count {
        assert_eq!(characters[code], core.character(code) as u32);
        assert_eq!(bases[code], core.base_code(code));
    }
    unsafe {
        let the = hk_model_index(model, c("the").as_ptr());
        assert_eq!(the, core.index("the").unwrap() as i64);
        assert_eq!(word(model, &characters, the as u32), "the");
        assert_eq!(hk_model_index(model, c("qzxv").as_ptr()), -1);
        let mut codes = [0u8; 16];
        assert_eq!(
            hk_model_encode(model, c("the").as_ptr(), codes.as_mut_ptr(), 16),
            3
        );
        assert_eq!(core.index_of_codes(&codes[..3]), Some(the as usize));
        assert_eq!(
            hk_model_encode(model, c("th3").as_ptr(), codes.as_mut_ptr(), 16),
            -1
        );
        assert_eq!(
            hk_model_encode(model, c("the").as_ptr(), codes.as_mut_ptr(), 2),
            -2
        );
        let mut length = 7;
        assert!(hk_model_word_codes(model, 86_247, &mut length).is_null());
        assert_eq!(length, 0);

        assert_eq!(
            hk_model_unigram_cost(model, the as usize),
            core.unigram_cost(the as usize)
        );
        assert!(hk_model_unigram_cost(model, 86_247).is_nan());
        let mut cost = 0.0;
        assert_eq!(
            hk_model_cost(model, c("the").as_ptr(), c("of").as_ptr(), &mut cost),
            1
        );
        assert_eq!(Some(cost), core.cost("the", Some("of")));
        assert_eq!(
            hk_model_cost(model, c("the").as_ptr(), ptr::null(), &mut cost),
            1
        );
        assert_eq!(Some(cost), core.cost("the", None));
        assert_eq!(
            hk_model_cost(model, c("qzxv").as_ptr(), ptr::null(), &mut cost),
            0
        );
        assert_eq!(
            hk_model_has_pair(model, c("of").as_ptr(), c("the").as_ptr()),
            1
        );
        assert_eq!(
            hk_model_has_pair(model, c("of").as_ptr(), c("qzxv").as_ptr()),
            0
        );

        hk_model_free(model);
    }
}

/// The host's filter: every word but those in the context's list.
unsafe extern "C" fn refuse(context: *mut c_void, word: u32) -> u8 {
    let (refused, asked) = unsafe { &mut *(context as *mut (Vec<u32>, Vec<u32>)) };
    asked.push(word);
    u8::from(!refused.contains(&word))
}

#[test]
fn predicts_as_the_core_does_with_the_hosts_filter() {
    let model = open_en();
    let core = core_en();
    let (_, characters, _) = alphabet(model);
    let mut out = [0u32; 3];
    unsafe {
        let n = hk_model_next_words(
            model,
            c("of").as_ptr(),
            None,
            ptr::null_mut(),
            out.as_mut_ptr(),
            3,
        );
        let all: Vec<String> = out[..n]
            .iter()
            .map(|&w| word(model, &characters, w))
            .collect();
        assert_eq!(all, core.next_words("of", |_| true));
        // Refusing the best one moves the others up, and the filter was asked about it.
        let best = out[0];
        let mut context = (vec![best], Vec::new());
        let n = hk_model_next_words(
            model,
            c("of").as_ptr(),
            Some(refuse),
            (&mut context as *mut (Vec<u32>, Vec<u32>)).cast(),
            out.as_mut_ptr(),
            3,
        );
        let filtered: Vec<String> = out[..n]
            .iter()
            .map(|&w| word(model, &characters, w))
            .collect();
        let first = all[0].clone();
        assert_eq!(filtered, core.next_words("of", |w| w != first));
        assert!(context.1.contains(&best));

        let n = hk_model_completions(
            model,
            c("th").as_ptr(),
            c("of").as_ptr(),
            None,
            ptr::null_mut(),
            out.as_mut_ptr(),
            2,
        );
        let completions: Vec<String> = out[..n]
            .iter()
            .map(|&w| word(model, &characters, w))
            .collect();
        assert_eq!(completions, core.completions("th", Some("of"), |_| true));
        assert_eq!(
            hk_model_completions(
                model,
                c("").as_ptr(),
                ptr::null(),
                None,
                ptr::null_mut(),
                out.as_mut_ptr(),
                2
            ),
            0
        );

        let prefix = core.encode("th").unwrap();
        let mut mass = vec![0.0; core.code_count()];
        assert_eq!(
            hk_model_continuations(model, prefix.as_ptr(), 2, mass.as_mut_ptr(), mass.len()),
            1
        );
        assert_eq!(Some(mass.clone()), core.continuations(&prefix));
        assert_eq!(
            hk_model_continuations(model, ptr::null(), 0, mass.as_mut_ptr(), mass.len()),
            1
        );
        assert_eq!(Some(mass.clone()), core.continuations(&[]));
        let none = core.encode("zzq").unwrap();
        assert_eq!(
            hk_model_continuations(model, none.as_ptr(), 3, mass.as_mut_ptr(), mass.len()),
            0
        );
        assert_eq!(
            hk_model_continuations(model, prefix.as_ptr(), 2, mass.as_mut_ptr(), 3),
            0
        );
        hk_model_free(model);
    }
}

/// QWERTY at a 39 × 54 pitch, rows offset half a column and one and a half.
fn qwerty() -> Vec<HkKeyCentre> {
    let mut centres = Vec::new();
    for (row, (letters, offset)) in [("qwertyuiop", 0.0), ("asdfghjkl", 0.5), ("zxcvbnm", 1.5)]
        .into_iter()
        .enumerate()
    {
        for (i, ch) in letters.chars().enumerate() {
            centres.push(HkKeyCentre {
                letter: ch as u32,
                x: 19.5 + 39.0 * (i as f64 + offset),
                y: 30.0 + 54.0 * row as f64,
            });
        }
    }
    centres
}

fn tap_keys(centres: &[HkKeyCentre]) -> HkTapKeys {
    HkTapKeys {
        keys: HkKeys {
            centres: centres.as_ptr(),
            centre_count: centres.len(),
            pitch_width: 39.0,
            pitch_height: 54.0,
        },
        bottom_row_y: 138.0,
        has_space: 1,
        space: HkRect {
            x: 100.0,
            y: 167.0,
            width: 190.0,
            height: 42.0,
        },
    }
}

fn core_keys(centres: &[HkKeyCentre]) -> decoder::Keys {
    decoder::Keys {
        centres: centres
            .iter()
            .map(|k| (char::from_u32(k.letter).unwrap(), k.x, k.y))
            .collect(),
        pitch: (39.0, 54.0),
        bottom_row_y: 138.0,
        space: Some(decoder::Rect {
            x: 100.0,
            y: 167.0,
            width: 190.0,
            height: 42.0,
        }),
    }
}

fn taps(centres: &[HkKeyCentre], word: &str) -> Vec<f64> {
    word.chars()
        .flat_map(|ch| {
            let k = centres.iter().find(|k| k.letter == ch as u32).unwrap();
            [k.x + 3.0, k.y - 2.0]
        })
        .collect()
}

fn new_decoder(model: *const HkModel, user: &[&str], never: &[u32]) -> *mut HkTapDecoder {
    let mut parameters = HkTapParameters::default();
    unsafe { hk_tap_tuned_parameters(c("en").as_ptr(), &mut parameters) };
    let user: Vec<CString> = user.iter().map(|w| c(w)).collect();
    let pointers: Vec<*const std::ffi::c_char> = user.iter().map(|w| w.as_ptr()).collect();
    let decoder = unsafe {
        hk_tap_new(
            model,
            &parameters,
            pointers.as_ptr(),
            pointers.len(),
            never.as_ptr(),
            never.len(),
            ptr::null(),
            0,
        )
    };
    assert!(!decoder.is_null());
    decoder
}

#[test]
fn the_parameters_are_the_cores() {
    let mut parameters = HkTapParameters::default();
    unsafe { hk_tap_default_parameters(&mut parameters) };
    assert_eq!(
        decoder::Parameters::from_c(&parameters),
        decoder::Parameters::default()
    );
    for language in ["en", "es", "de", ""] {
        unsafe { hk_tap_tuned_parameters(c(language).as_ptr(), &mut parameters) };
        assert_eq!(
            decoder::Parameters::from_c(&parameters),
            decoder::Parameters::tuned(language)
        );
    }
}

/// The C struct read back as the core's, field by field, for the test above.
trait FromC {
    fn from_c(p: &HkTapParameters) -> Self;
}

impl FromC for decoder::Parameters {
    fn from_c(p: &HkTapParameters) -> Self {
        decoder::Parameters {
            sigma_x: p.sigma_x,
            sigma_y: p.sigma_y,
            lm_weight: p.lm_weight,
            deletion_cost: p.deletion_cost,
            insertion_cost: p.insertion_cost,
            transposition_cost: p.transposition_cost,
            space_omission_cost: p.space_omission_cost,
            space_tap_cost: p.space_tap_cost,
            space_tap_weight: p.space_tap_weight,
            merge_reach: p.merge_reach,
            max_edits: p.max_edits,
            beam: p.beam,
            auto_margin: p.auto_margin,
            valid_margin: p.valid_margin,
            unknown_valid_cost: p.unknown_valid_cost,
            unknown_cost: p.unknown_cost,
            max_tap_cost: p.max_tap_cost,
            splits: p.splits != 0,
            accent_cost: p.accent_cost,
            one_letter_word_cost: p.one_letter_word_cost,
            user_word_cost: p.user_word_cost,
            accent_margin: p.accent_margin,
        }
    }
}

#[test]
fn decides_as_the_core_does() {
    let model = open_en();
    let core = TypingDecoder::new(core_en(), None);
    let centres = qwerty();
    let keys = tap_keys(&centres);
    let decoder = new_decoder(model, &[], &[]);
    for typed in ["teh", "hello", "seeyoutomorrow", "ofbthe", "xq"] {
        let flat = taps(&centres, typed);
        let mut out = HkTapDecision {
            decided: 9,
            replacement: ptr::null_mut(),
            readings: ptr::null_mut(),
            reading_count: 0,
            literal_cost: 0.0,
        };
        let status = unsafe {
            hk_tap_decide(
                decoder,
                flat.as_ptr(),
                flat.len() / 2,
                c(typed).as_ptr(),
                c("i").as_ptr(),
                0,
                &keys,
                &mut out,
            )
        };
        assert_eq!(status, 0);
        let pairs: Vec<(f64, f64)> = flat.chunks(2).map(|p| (p[0], p[1])).collect();
        let expected = core.decide(&pairs, typed, Some("i"), false, &core_keys(&centres));
        match expected {
            None => assert_eq!(out.decided, 0, "{typed}"),
            Some(d) => {
                assert_eq!(out.decided, 1);
                let replacement = (!out.replacement.is_null()).then(|| {
                    unsafe { CStr::from_ptr(out.replacement) }
                        .to_str()
                        .unwrap()
                        .to_owned()
                });
                assert_eq!(replacement, d.replacement, "{typed}");
                assert_eq!(out.literal_cost, d.literal_cost);
                let readings = if out.readings.is_null() {
                    &[][..]
                } else {
                    unsafe { std::slice::from_raw_parts(out.readings, out.reading_count) }
                };
                assert_eq!(readings.len(), d.readings.len());
                for (ours, theirs) in readings.iter().zip(&d.readings) {
                    assert_eq!(
                        unsafe { CStr::from_ptr(ours.text) }.to_str().unwrap(),
                        theirs.text
                    );
                    assert_eq!(ours.cost, theirs.cost);
                }
            }
        }
        unsafe { hk_tap_decision_free(&mut out) };
        assert!(out.readings.is_null() && out.replacement.is_null());
        unsafe { hk_tap_decision_free(&mut out) }; // twice is fine
    }
    // The model can't read a tap count that doesn't match.
    let flat = taps(&centres, "the");
    let mut out = HkTapDecision {
        decided: 9,
        replacement: ptr::null_mut(),
        readings: ptr::null_mut(),
        reading_count: 0,
        literal_cost: 0.0,
    };
    assert_eq!(
        unsafe {
            hk_tap_decide(
                decoder,
                flat.as_ptr(),
                3,
                c("th").as_ptr(),
                ptr::null(),
                0,
                &keys,
                &mut out,
            )
        },
        0
    );
    assert_eq!(out.decided, 0);

    let mut answer = 7;
    let teh = taps(&centres, "teh");
    unsafe {
        assert_eq!(
            hk_tap_replaces(
                decoder,
                c("the").as_ptr(),
                teh.as_ptr(),
                3,
                c("teh").as_ptr(),
                ptr::null(),
                0,
                &keys,
                &mut answer
            ),
            0
        );
        assert_eq!(answer, 1);
        assert_eq!(
            hk_tap_replaces(
                decoder,
                c("the end").as_ptr(),
                teh.as_ptr(),
                3,
                c("teh").as_ptr(),
                ptr::null(),
                0,
                &keys,
                &mut answer
            ),
            0
        );
        assert_eq!(answer, -1);
        hk_tap_free(decoder);
        hk_model_free(model);
    }
}

#[test]
fn merges_and_keeps_user_and_never_words() {
    let model = open_en();
    let core_model = core_en();
    let centres = qwerty();
    let keys = tap_keys(&centres);
    // "deli ate" with the space tapped high on the bar, under c.
    let c_key = centres.iter().find(|k| k.letter == 'c' as u32).unwrap();
    let (first, second) = (taps(&centres, "deli"), taps(&centres, "ate"));
    let decoder = new_decoder(model, &[], &[]);
    let mut merged = ptr::null_mut();
    let status = unsafe {
        hk_tap_merge(
            decoder,
            c("deli").as_ptr(),
            first.as_ptr(),
            4,
            c_key.x,
            170.0,
            c("ate").as_ptr(),
            second.as_ptr(),
            3,
            ptr::null(),
            1,
            1,
            &keys,
            &mut merged,
        )
    };
    assert_eq!(status, 0);
    let core = TypingDecoder::new(core_model.clone(), None);
    let pairs = |f: &[f64]| f.chunks(2).map(|p| (p[0], p[1])).collect::<Vec<_>>();
    let expected = core.merge(
        "deli",
        &pairs(&first),
        (c_key.x, 170.0),
        "ate",
        &pairs(&second),
        None,
        true,
        true,
        &core_keys(&centres),
    );
    let ours = (!merged.is_null()).then(|| {
        unsafe { CStr::from_ptr(merged) }
            .to_str()
            .unwrap()
            .to_owned()
    });
    assert_eq!(ours, expected);
    unsafe {
        hk_string_free(merged);
        hk_tap_free(decoder);
    }

    // A user word read for its slip, and a never-suggest word left out.
    let the = core_model.index("the").unwrap() as u32;
    let decoder = new_decoder(model, &["Kaitlyn"], &[the]);
    let mut core = TypingDecoder::new(core_model.clone(), None);
    core.user_words = decoder::UserWords::new(&["Kaitlyn"], &core_model);
    core.never_suggest.words.insert(the as usize);
    for typed in ["kaitlun", "teh"] {
        let flat = taps(&centres, typed);
        let mut out = HkTapDecision {
            decided: 0,
            replacement: ptr::null_mut(),
            readings: ptr::null_mut(),
            reading_count: 0,
            literal_cost: 0.0,
        };
        assert_eq!(
            unsafe {
                hk_tap_decide(
                    decoder,
                    flat.as_ptr(),
                    flat.len() / 2,
                    c(typed).as_ptr(),
                    ptr::null(),
                    0,
                    &keys,
                    &mut out,
                )
            },
            0
        );
        let expected = core
            .decide(&pairs(&flat), typed, None, false, &core_keys(&centres))
            .unwrap();
        let replacement = (!out.replacement.is_null()).then(|| {
            unsafe { CStr::from_ptr(out.replacement) }
                .to_str()
                .unwrap()
                .to_owned()
        });
        assert_eq!(replacement, expected.replacement, "{typed}");
        unsafe { hk_tap_decision_free(&mut out) };
    }
    unsafe {
        hk_tap_free(decoder);
        hk_model_free(model);
    }
}

#[test]
fn makes_keys_from_a_layout() {
    // Two rows of a split layout: g on both halves, a space bar on each.
    let names: Vec<CString> = ["a", "g", "g", "h", "."].iter().map(|s| c(s)).collect();
    let frame = |x: f64, y: f64, width: f64| HkRect {
        x,
        y,
        width,
        height: 40.0,
    };
    let mut keys: Vec<HkLayoutKey> = vec![
        HkLayoutKey {
            character: names[0].as_ptr(),
            space: 0,
            frame: frame(0.0, 0.0, 30.0),
        },
        HkLayoutKey {
            character: names[1].as_ptr(),
            space: 0,
            frame: frame(40.0, 0.0, 30.0),
        },
        HkLayoutKey {
            character: names[2].as_ptr(),
            space: 0,
            frame: frame(200.0, 0.0, 30.0),
        },
        HkLayoutKey {
            character: names[3].as_ptr(),
            space: 0,
            frame: frame(240.0, 50.0, 30.0),
        },
        HkLayoutKey {
            character: names[4].as_ptr(),
            space: 0,
            frame: frame(280.0, 50.0, 30.0),
        },
    ];
    keys.push(HkLayoutKey {
        character: ptr::null(),
        space: 1,
        frame: frame(10.0, 100.0, 60.0),
    });
    keys.push(HkLayoutKey {
        character: ptr::null(),
        space: 1,
        frame: frame(200.0, 100.0, 60.0),
    });
    let stats: Vec<HkKeyStats> = keys
        .iter()
        .enumerate()
        .map(|(i, _)| HkKeyStats {
            present: u8::from(i < 4),
            count: [40, 19, 60, 25][i.min(3)],
            dx: 4.0,
            dy: -2.0,
        })
        .collect();
    let mut offsets = vec![HkKeyOffset::default(); keys.len()];
    let n = unsafe {
        hk_layout_offsets(
            keys.as_ptr(),
            stats.as_ptr(),
            keys.len(),
            offsets.as_mut_ptr(),
            offsets.len(),
        )
    };

    let core_keys: Vec<decoder::LayoutKey> = keys
        .iter()
        .map(|k| decoder::LayoutKey {
            character: (!k.character.is_null()).then(|| {
                unsafe { CStr::from_ptr(k.character) }
                    .to_str()
                    .unwrap()
                    .to_owned()
            }),
            space: k.space != 0,
            frame: decoder::Rect {
                x: k.frame.x,
                y: k.frame.y,
                width: k.frame.width,
                height: k.frame.height,
            },
        })
        .collect();
    let core_stats = core_keys
        .iter()
        .zip(&stats)
        .filter(|(_, s)| s.present != 0)
        .map(|(k, s)| {
            (
                k.character.clone().unwrap(),
                decoder::KeyStats {
                    count: s.count,
                    dx: s.dx,
                    dy: s.dy,
                },
            )
        })
        .collect();
    let expected = decoder::offsets(&core_keys, &core_stats);
    assert_eq!(n, expected.len());
    for o in &offsets[..n] {
        assert_eq!(expected[&char::from_u32(o.letter).unwrap()], (o.dx, o.dy));
    }

    let split = [150.0, 120.0];
    let mut centres = vec![
        HkKeyCentre {
            letter: 0,
            x: 0.0,
            y: 0.0
        };
        keys.len()
    ];
    let mut out = tap_keys(&[]);
    let status = unsafe {
        hk_layout_tap_keys(
            keys.as_ptr(),
            keys.len(),
            split.as_ptr(),
            57.0,
            50.0,
            offsets.as_ptr(),
            n,
            centres.as_mut_ptr(),
            centres.len(),
            &mut out,
        )
    };
    assert_eq!(status, 0);
    let made = decoder::Keys::from_layout(
        &core_keys,
        Some(decoder::Split {
            middle: 150.0,
            shift: 120.0,
        }),
        (57.0, 50.0),
        &expected,
    );
    assert_eq!(out.keys.centre_count, made.centres.len());
    for (ours, theirs) in centres.iter().zip(&made.centres) {
        assert_eq!(
            (char::from_u32(ours.letter).unwrap(), ours.x, ours.y),
            *theirs
        );
    }
    assert_eq!(out.bottom_row_y, made.bottom_row_y);
    let space = made.space.unwrap();
    assert_eq!(
        (out.has_space, out.space.x, out.space.width),
        (1, space.x, space.width)
    );
    // Too small a buffer for the centres.
    assert_ne!(
        unsafe {
            hk_layout_tap_keys(
                keys.as_ptr(),
                keys.len(),
                ptr::null(),
                57.0,
                50.0,
                ptr::null(),
                0,
                centres.as_mut_ptr(),
                1,
                &mut out,
            )
        },
        0
    );
}

#[test]
fn null_arguments_are_refused() {
    unsafe {
        let mut info = std::mem::zeroed::<HkModelInfo>();
        assert_ne!(hk_model_info(ptr::null(), &mut info), 0);
        assert_ne!(
            hk_model_alphabet(ptr::null(), ptr::null_mut(), ptr::null_mut(), 0),
            0
        );
        assert_eq!(hk_model_index(ptr::null(), c("the").as_ptr()), -1);
        assert_eq!(
            hk_model_encode(ptr::null(), c("the").as_ptr(), ptr::null_mut(), 0),
            -2
        );
        assert!(hk_model_unigram_cost(ptr::null(), 0).is_nan());
        assert_eq!(
            hk_model_next_words(
                ptr::null(),
                c("of").as_ptr(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
                3
            ),
            0
        );
        hk_tap_default_parameters(ptr::null_mut());
        hk_tap_free(ptr::null_mut());
        hk_tap_decision_free(ptr::null_mut());
        hk_string_free(ptr::null_mut());
        let parameters = HkTapParameters::default();
        assert!(
            hk_tap_new(
                ptr::null(),
                &parameters,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0
            )
            .is_null()
        );
        let model = open_en();
        assert!(
            hk_tap_new(
                model,
                ptr::null(),
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0
            )
            .is_null()
        );
        assert!(
            hk_tap_new(
                model,
                &parameters,
                ptr::null(),
                2,
                ptr::null(),
                0,
                ptr::null(),
                0
            )
            .is_null()
        );
        let decoder = new_decoder(model, &[], &[]);
        let centres = qwerty();
        let keys = tap_keys(&centres);
        let mut out = std::mem::zeroed::<HkTapDecision>();
        assert_ne!(
            hk_tap_decide(
                decoder,
                ptr::null(),
                3,
                c("the").as_ptr(),
                ptr::null(),
                0,
                &keys,
                &mut out
            ),
            0
        );
        assert_ne!(
            hk_tap_decide(
                decoder,
                ptr::null(),
                0,
                ptr::null(),
                ptr::null(),
                0,
                &keys,
                &mut out
            ),
            0
        );
        assert_ne!(
            hk_tap_decide(
                decoder,
                ptr::null(),
                0,
                c("").as_ptr(),
                ptr::null(),
                0,
                ptr::null(),
                &mut out
            ),
            0
        );
        assert_ne!(
            hk_tap_decide(
                decoder,
                ptr::null(),
                0,
                c("").as_ptr(),
                ptr::null(),
                0,
                &keys,
                ptr::null_mut()
            ),
            0
        );
        let mut answer = 0;
        assert_ne!(
            hk_tap_replaces(
                ptr::null(),
                c("a").as_ptr(),
                ptr::null(),
                0,
                c("a").as_ptr(),
                ptr::null(),
                0,
                &keys,
                &mut answer
            ),
            0
        );
        let mut merged = ptr::null_mut();
        assert_ne!(
            hk_tap_merge(
                decoder,
                ptr::null(),
                ptr::null(),
                0,
                0.0,
                0.0,
                c("a").as_ptr(),
                ptr::null(),
                1,
                ptr::null(),
                0,
                0,
                &keys,
                &mut merged
            ),
            0
        );
        hk_tap_free(decoder);
        hk_model_free(model);
    }
}
