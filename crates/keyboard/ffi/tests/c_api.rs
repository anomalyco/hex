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
