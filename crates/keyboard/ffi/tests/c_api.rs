//! The C interface as Swift calls it, on the English model the iOS keyboard ships.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::ptr;

use hex_keyboard::*;

fn en_model_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ios/HEXKeyboard/TypingModel/en.hextm")
}

fn c_path(path: &std::path::Path) -> CString {
    CString::new(path.to_str().unwrap()).unwrap()
}

fn open_en() -> *mut HkModel {
    let model = unsafe { hk_model_open(c_path(&en_model_path()).as_ptr()) };
    assert!(!model.is_null());
    model
}

#[test]
fn the_abi_version_is_the_headers() {
    assert_eq!(hk_abi_version(), HK_ABI_VERSION);
}

#[test]
fn opens_the_english_model_by_path() {
    let model = open_en();
    assert_eq!(unsafe { hk_model_word_count(model) }, 86_133);
    unsafe { hk_model_free(model) };
}

#[test]
fn opens_the_english_model_inside_a_larger_file() {
    // As an APK holds it: at an offset that isn't page aligned, with bytes after it.
    let bytes = std::fs::read(en_model_path()).unwrap();
    let dir = std::env::temp_dir().join(format!("hex-keyboard-ffi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let packed = dir.join("packed.bin");
    let mut contents = vec![0x5a; 4_099];
    contents.extend_from_slice(&bytes);
    contents.extend_from_slice(&[0xa5; 77]);
    std::fs::write(&packed, &contents).unwrap();

    let file = File::open(&packed).unwrap();
    let (fd, length) = (file.as_raw_fd(), bytes.len() as i64);
    let model = unsafe { hk_model_open_fd(fd, 4_099, length) };
    drop(file); // the map outlives the descriptor
    assert!(!model.is_null());
    assert_eq!(unsafe { hk_model_word_count(model) }, 86_133);
    unsafe { hk_model_free(model) };

    let file = File::open(&packed).unwrap();
    let fd = file.as_raw_fd();
    unsafe {
        // Past the end of the file, at the wrong offset, and nonsense ranges.
        assert!(hk_model_open_fd(fd, 4_099, length + 78).is_null());
        assert!(hk_model_open_fd(fd, 4_098, length).is_null());
        assert!(hk_model_open_fd(fd, -1, length).is_null());
        assert!(hk_model_open_fd(fd, 4_099, 0).is_null());
        assert!(hk_model_open_fd(fd, i64::MAX, i64::MAX).is_null());
        assert!(hk_model_open_fd(-1, 4_099, length).is_null());
        // A descriptor that isn't open: the dup fails (EBADF), nothing is mapped.
        assert!(hk_model_open_fd(1_000_000, 4_099, length).is_null());
    }
    drop(file);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_that_isnt_a_model_opens_nothing() {
    let dir = std::env::temp_dir().join(format!("hex-keyboard-ffi-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("bad.hextm");
    std::fs::write(&bad, b"HXTM but not really a typing model at all").unwrap();
    let empty = dir.join("empty.hextm");
    std::fs::write(&empty, b"").unwrap();
    unsafe {
        assert!(hk_model_open(c_path(&bad).as_ptr()).is_null());
        assert!(hk_model_open(c_path(&empty).as_ptr()).is_null());
        assert!(hk_model_open(c_path(&dir.join("missing.hextm")).as_ptr()).is_null());
        assert!(hk_model_open(c_path(&dir).as_ptr()).is_null());
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn null_arguments_are_refused() {
    unsafe {
        assert!(hk_model_open(ptr::null()).is_null());
        assert_eq!(hk_model_word_count(ptr::null()), 0);
        hk_model_free(ptr::null_mut());
        hk_swipe_default_parameters(ptr::null_mut());
        hk_swipe_free(ptr::null_mut());
        hk_swipe_reading_free(ptr::null_mut());

        let parameters = HkSwipeParameters::default();
        assert!(
            hk_swipe_new(
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
            hk_swipe_new(
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
        // Items promised at NULL.
        assert!(
            hk_swipe_new(
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
        assert!(
            hk_swipe_new(
                model,
                &parameters,
                ptr::null(),
                0,
                ptr::null(),
                1,
                ptr::null(),
                0
            )
            .is_null()
        );
        assert!(
            hk_swipe_new(
                model,
                &parameters,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                3
            )
            .is_null()
        );
        let extra = [HkExtraWord {
            text: ptr::null(),
            cost: 7.5,
            contraction: 1,
        }];
        assert!(
            hk_swipe_new(
                model,
                &parameters,
                ptr::null(),
                0,
                ptr::null(),
                0,
                extra.as_ptr(),
                1
            )
            .is_null()
        );
        hk_model_free(model);

        let keys = HkKeys {
            centres: ptr::null(),
            centre_count: 0,
            pitch_width: 40.0,
            pitch_height: 50.0,
        };
        let mut out = HkSwipeReading {
            candidates: ptr::NonNull::dangling().as_ptr(),
            candidate_count: 9,
            scored: 9,
        };
        assert_ne!(
            hk_swipe_read(
                ptr::null(),
                ptr::null(),
                0,
                ptr::null(),
                &keys,
                ptr::null(),
                5,
                &mut out
            ),
            0
        );
        assert!(out.candidates.is_null());
        assert_eq!((out.candidate_count, out.scored), (0, 0));
        assert_ne!(
            hk_swipe_read(
                ptr::null(),
                ptr::null(),
                0,
                ptr::null(),
                &keys,
                ptr::null(),
                5,
                ptr::null_mut()
            ),
            0
        );
        hk_swipe_reading_free(&mut out);
    }
}

#[test]
fn a_panic_in_the_core_never_crosses_the_boundary() {
    // Whatever the core does with zeroed parameters, the call returns (a decoder or NULL)
    // instead of unwinding into the caller or aborting.
    unsafe {
        let mut parameters = HkSwipeParameters::default();
        hk_swipe_default_parameters(&mut parameters);
        let model = open_en();
        let decoder = hk_swipe_new(
            model,
            &HkSwipeParameters::default(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
        );
        hk_swipe_free(decoder);
        hk_model_free(model);
    }
}

/// The English letter keys of the 390 pt iPhone layout (`KeyboardLayout(width: 390)`), whose
/// pitch is 38.3 × 54 pt: each row's keys one pitch apart, the middle row in half a key.
fn english_390_keys() -> Vec<HkKeyCentre> {
    let rows = [
        ("qwertyuiop", 22.65, 67.5),
        ("asdfghjkl", 41.8, 121.5),
        ("zxcvbnm", 80.1, 175.5),
    ];
    let mut centres = Vec::new();
    for (letters, first, y) in rows {
        for (i, c) in letters.chars().enumerate() {
            centres.push(HkKeyCentre {
                letter: c as u32,
                x: first + 38.3 * i as f64,
                y,
            });
        }
    }
    centres
}

fn centre(keys: &[HkKeyCentre], c: char) -> (f64, f64) {
    let key = keys.iter().find(|k| k.letter == c as u32).unwrap();
    (key.x, key.y)
}

/// A path straight from letter to letter of `word`, 8 points a leg.
fn path(keys: &[HkKeyCentre], word: &str) -> Vec<f64> {
    let stops: Vec<_> = word.chars().map(|c| centre(keys, c)).collect();
    let mut points = Vec::new();
    for leg in stops.windows(2) {
        for step in 0..8 {
            let t = step as f64 / 8.0;
            points.extend([
                leg[0].0 + (leg[1].0 - leg[0].0) * t,
                leg[0].1 + (leg[1].1 - leg[0].1) * t,
            ]);
        }
    }
    let last = stops.last().unwrap();
    points.extend([last.0, last.1]);
    points
}

#[test]
fn the_default_parameters_are_the_cores() {
    let mut parameters = HkSwipeParameters::default();
    unsafe { hk_swipe_default_parameters(&mut parameters) };
    assert_eq!(
        parameters,
        HkSwipeParameters::from(&hex_keyboard_core::swipe::Parameters::default())
    );
    assert_eq!(parameters.samples, 32);
}

#[test]
fn contraction_costs_are_the_cores() {
    unsafe {
        let base = hk_swipe_contraction_cost();
        assert_eq!(base, 7.5);
        assert_eq!(hk_swipe_own_word_cost(), 11.0);
        let cost =
            |word: &str| hk_swipe_cost_of_contraction(CString::new(word).unwrap().as_ptr(), base);
        assert_eq!(cost("I'm"), 6.0, "the commonest, in any case");
        assert_eq!(cost("DON'T"), 6.0);
        assert_eq!(cost("we're"), 7.5);
        assert_eq!(cost("ma'am"), 9.5);
        assert_eq!(cost(""), 9.5);
        assert_eq!(hk_swipe_cost_of_contraction(ptr::null(), base), 9.5);
        let bad_utf8 = [0xffu8, 0];
        assert_eq!(
            hk_swipe_cost_of_contraction(bad_utf8.as_ptr().cast(), 1.0),
            3.0
        );
        assert_eq!(
            hk_swipe_cost_of_contraction(CString::new("can't").unwrap().as_ptr(), 1.0),
            -0.5
        );
    }
}

#[test]
fn reads_a_swipe_through_the_c_interface() {
    unsafe {
        let model = open_en();
        let mut parameters = HkSwipeParameters::default();
        hk_swipe_default_parameters(&mut parameters);
        let own = CString::new("hexkb").unwrap();
        let extra = [HkExtraWord {
            text: own.as_ptr(),
            cost: 11.0,
            contraction: 0,
        }];
        let decoder = hk_swipe_new(
            model,
            &parameters,
            ptr::null(),
            0,
            ptr::null(),
            0,
            extra.as_ptr(),
            1,
        );
        hk_model_free(model); // the decoder keeps its own reference
        assert!(!decoder.is_null());

        let centres = english_390_keys();
        let keys = HkKeys {
            centres: centres.as_ptr(),
            centre_count: centres.len(),
            pitch_width: 38.3,
            pitch_height: 54.0,
        };
        let points = path(&centres, "hello");
        let mut out = HkSwipeReading {
            candidates: ptr::null_mut(),
            candidate_count: 0,
            scored: 0,
        };
        let status = hk_swipe_read(
            decoder,
            points.as_ptr(),
            points.len() / 2,
            ptr::null(),
            &keys,
            ptr::null(),
            5,
            &mut out,
        );
        assert_eq!(status, 0);
        assert!(out.candidate_count > 0 && out.candidate_count <= 5);
        assert!(out.scored >= out.candidate_count);
        let candidates = std::slice::from_raw_parts(out.candidates, out.candidate_count);
        let texts: Vec<&str> = candidates
            .iter()
            .map(|c| CStr::from_ptr(c.text).to_str().unwrap())
            .collect();
        assert_eq!(texts.first(), Some(&"hello"), "{texts:?}");
        assert!(candidates.windows(2).all(|c| c[0].cost <= c[1].cost));
        hk_swipe_reading_free(&mut out);
        assert!(out.candidates.is_null());
        hk_swipe_reading_free(&mut out); // freeing twice through the struct is harmless

        // With times, after a word, and refusing malformed input.
        let times: Vec<f64> = (0..points.len() / 2).map(|i| i as f64 * 0.01).collect();
        let previous = CString::new("say").unwrap();
        let status = hk_swipe_read(
            decoder,
            points.as_ptr(),
            points.len() / 2,
            times.as_ptr(),
            &keys,
            previous.as_ptr(),
            3,
            &mut out,
        );
        assert_eq!(status, 0);
        assert!(out.candidate_count <= 3);
        hk_swipe_reading_free(&mut out);
        let bad_letter = [HkKeyCentre {
            letter: 0xD800,
            x: 0.0,
            y: 0.0,
        }];
        let bad_keys = HkKeys {
            centres: bad_letter.as_ptr(),
            centre_count: 1,
            pitch_width: 40.0,
            pitch_height: 50.0,
        };
        assert_ne!(
            hk_swipe_read(
                decoder,
                points.as_ptr(),
                points.len() / 2,
                ptr::null(),
                &bad_keys,
                ptr::null(),
                3,
                &mut out
            ),
            0
        );
        assert!(out.candidates.is_null());
        let bad_utf8 = [0xffu8, 0];
        assert_ne!(
            hk_swipe_read(
                decoder,
                points.as_ptr(),
                points.len() / 2,
                ptr::null(),
                &keys,
                bad_utf8.as_ptr().cast(),
                3,
                &mut out
            ),
            0
        );
        hk_swipe_free(decoder);
    }
}

/// The test dictionary: "monday" only capitalised, "bell" in both cases.
unsafe extern "C" fn is_word_valid(
    _context: *mut std::ffi::c_void,
    word: *const std::ffi::c_char,
) -> u8 {
    let word = unsafe { CStr::from_ptr(word) }.to_str().unwrap();
    u8::from(matches!(word, "Monday" | "bell" | "Bell" | "the"))
}

#[test]
fn types_a_swipe_through_the_c_interface() {
    unsafe {
        let (shortcut, replacement) = (
            CString::new("omw").unwrap(),
            CString::new("On my way!").unwrap(),
        );
        let replacements = [HkTextReplacement {
            shortcut: shortcut.as_ptr(),
            replacement: replacement.as_ptr(),
        }];
        let own = CString::new("Bell").unwrap();
        let own = [own.as_ptr()];
        let en = CString::new("en").unwrap();
        let words = hk_swipe_words_new(
            en.as_ptr(),
            1,
            replacements.as_ptr(),
            1,
            own.as_ptr(),
            1,
            ptr::null(),
            0,
        );
        assert!(!words.is_null());

        let texts: Vec<CString> = ["monday", "omw", "bell"]
            .iter()
            .map(|t| CString::new(*t).unwrap())
            .collect();
        let readings: Vec<*const std::ffi::c_char> = texts.iter().map(|t| t.as_ptr()).collect();
        let before = CString::new("see you").unwrap();
        let context = HkSwipeContext {
            before: before.as_ptr(),
            shift: 0,
            shift_held: 0,
            auto_capitalization: 2,
            auto_capitalizes: 1,
            autocorrection: 1,
        };
        let mut typed = HkSwipeTyped {
            space: 0,
            word: ptr::null_mut(),
            alternatives: ptr::null_mut(),
            alternative_count: 0,
        };
        let status = hk_swipe_typed(
            words,
            readings.as_ptr(),
            3,
            &context,
            Some(is_word_valid),
            ptr::null_mut(),
            &mut typed,
        );
        assert_eq!(status, 0);
        assert_eq!(typed.space, 1, "after \"see you\"");
        assert_eq!(
            CStr::from_ptr(typed.word).to_str().unwrap(),
            "Monday",
            "a name the dictionary knows capitalised"
        );
        let alternatives: Vec<&str> =
            std::slice::from_raw_parts(typed.alternatives, typed.alternative_count)
                .iter()
                .map(|&a| CStr::from_ptr(a).to_str().unwrap())
                .collect();
        assert_eq!(
            alternatives,
            ["On my way!", "bell"],
            "a replacement, and your own word in lowercase mid-sentence"
        );
        hk_swipe_typed_free(&mut typed);
        assert!(typed.word.is_null());
        assert_ne!(
            hk_swipe_typed(
                words,
                readings.as_ptr(),
                0,
                &context,
                Some(is_word_valid),
                ptr::null_mut(),
                &mut typed
            ),
            0
        );

        let monday = CString::new("monday").unwrap();
        assert_eq!(
            hk_swipe_reads_as_name(words, monday.as_ptr(), Some(is_word_valid), ptr::null_mut()),
            1
        );
        let open = CString::new("(").unwrap();
        assert_eq!(hk_swipe_needs_space(open.as_ptr()), 0);
        assert_eq!(hk_swipe_needs_space(before.as_ptr()), 1);
        assert_eq!(hk_swipe_needs_space(ptr::null()), 0);
        let (l, lol, a, r#as) = (
            CString::new("l").unwrap(),
            CString::new("lol").unwrap(),
            CString::new("a").unwrap(),
            CString::new("as").unwrap(),
        );
        assert_eq!(hk_swipe_starts_and_ends_with(lol.as_ptr(), l.as_ptr()), 1);
        let points = [100.0, 100.0, 110.0, 104.0, 101.0, 100.0];
        assert_eq!(
            hk_swipe_is_slid_tap(points.as_ptr(), 3, 38.3, a.as_ptr(), r#as.as_ptr()),
            1,
            "\"a\" slid, read as \"as\""
        );
        assert_eq!(
            hk_swipe_is_slid_tap(points.as_ptr(), 3, 38.3, l.as_ptr(), lol.as_ptr()),
            0,
            "\"lol\" comes back to l"
        );
        assert_eq!(
            hk_swipe_is_slid_tap(points.as_ptr(), 3, 38.3, ptr::null(), r#as.as_ptr()),
            0,
            "lifted on another key"
        );

        let model = open_en();
        let mut parameters = HkSwipeParameters::default();
        hk_swipe_default_parameters(&mut parameters);
        let decoder = hk_swipe_new(
            model,
            &parameters,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
        );
        hk_model_free(model);
        let (kent, many) = (CString::new("kent").unwrap(), CString::new("many").unwrap());
        let candidates = [kent.as_ptr(), many.as_ptr()];
        let mut order = [0usize; 2];
        unsafe extern "C" fn knows_kent(
            _context: *mut std::ffi::c_void,
            word: *const std::ffi::c_char,
        ) -> u8 {
            u8::from(matches!(
                unsafe { CStr::from_ptr(word) }.to_str().unwrap(),
                "Kent" | "many"
            ))
        }
        let status = hk_swipe_rank(
            decoder,
            words,
            candidates.as_ptr(),
            [10.0, 12.0].as_ptr(),
            2,
            Some(knows_kent),
            ptr::null_mut(),
            order.as_mut_ptr(),
        );
        assert_eq!(status, 0);
        assert_eq!(order, [1, 0], "a rare name costs 4 more: \"many\" first");
        hk_swipe_free(decoder);
        hk_swipe_words_free(words);
    }
}

#[test]
fn the_english_contractions_come_through_the_c_interface() {
    let mut count = 0usize;
    let words = unsafe { hk_swipe_english_contractions(&mut count) };
    assert!(!words.is_null());
    let words: Vec<&str> = unsafe { std::slice::from_raw_parts(words, count) }
        .iter()
        .map(|&w| unsafe { CStr::from_ptr(w) }.to_str().unwrap())
        .collect();
    assert_eq!(words, hex_keyboard_core::swipe::ENGLISH_CONTRACTIONS);
    assert_eq!(words[..3], ["I'd", "I'll", "I'm"]);
    assert_eq!(
        unsafe { hk_swipe_english_contractions(ptr::null_mut()) },
        unsafe { hk_swipe_english_contractions(&mut count) }
    );
}
