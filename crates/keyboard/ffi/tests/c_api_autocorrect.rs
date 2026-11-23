//! Autocorrect's C interface as Swift calls it, against the core called directly: the same
//! answers, and the dictionary asked the same questions in the same order through the
//! callback as the core asks its checker.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::rc::Rc;
use std::sync::Arc;

use hex_keyboard::*;
use hex_keyboard_core::autocorrect::{
    Autocorrect, Bundled, FieldKind, KeyCell, NeverSuggestList, SuggestionKind, TextChecker,
    TypingLearning,
};

const TSV: &str = "*\tswear\tfuck\n*\tslur\tfaggot\nen\tswear\tblow job\nfr\tswear\tmerde\n";

/// A small dictionary that logs every question.
struct Words {
    valid: Vec<&'static str>,
    log: Rc<RefCell<Vec<String>>>,
    tag: &'static str,
}

impl Words {
    fn new(tag: &'static str, valid: &[&'static str], log: &Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            valid: valid.to_vec(),
            log: log.clone(),
            tag,
        }
    }

    fn valid(&self, word: &str) -> bool {
        let answer =
            word.chars().count() == 1 || self.valid.contains(&word.to_lowercase().as_str());
        self.log
            .borrow_mut()
            .push(format!("{}|v|{word}|{answer}", self.tag));
        answer
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        let out: Vec<String> = self
            .valid
            .iter()
            .filter(|w| {
                w.len().abs_diff(word.len()) <= 1 && w.chars().next() == word.chars().next()
            })
            .map(|w| (*w).to_owned())
            .collect();
        self.log
            .borrow_mut()
            .push(format!("{}|g|{word}|{}", self.tag, out.join(",")));
        out
    }

    fn completions(&self, prefix: &str) -> Vec<String> {
        let out: Vec<String> = self
            .valid
            .iter()
            .filter(|w| w.starts_with(prefix) && **w != prefix)
            .map(|w| (*w).to_owned())
            .collect();
        self.log
            .borrow_mut()
            .push(format!("{}|c|{prefix}|{}", self.tag, out.join(",")));
        out
    }
}

impl TextChecker for Words {
    fn is_word_valid(&self, word: &str) -> bool {
        self.valid(word)
    }
    fn guesses(&self, word: &str) -> Vec<String> {
        Words::guesses(self, word)
    }
    fn completions(&self, prefix: &str) -> Vec<String> {
        Words::completions(self, prefix)
    }
    fn use_language(&mut self, code: &str) {
        self.log.borrow_mut().push(format!("{}|u|{code}", self.tag));
    }
}

fn s<'a>(p: *const c_char) -> &'a str {
    unsafe { CStr::from_ptr(p) }.to_str().unwrap()
}

unsafe extern "C" fn c_valid(context: *mut c_void, word: *const c_char) -> u8 {
    let words = unsafe { &*(context as *const Words) };
    u8::from(words.valid(s(word)))
}

unsafe extern "C" fn c_guesses(context: *mut c_void, word: *const c_char, out: *mut HkStrings) {
    let words = unsafe { &*(context as *const Words) };
    for g in words.guesses(s(word)) {
        let g = CString::new(g).unwrap();
        unsafe { hk_strings_push(out, g.as_ptr()) };
    }
}

unsafe extern "C" fn c_completions(context: *mut c_void, word: *const c_char, out: *mut HkStrings) {
    let words = unsafe { &*(context as *const Words) };
    for g in words.completions(s(word)) {
        let g = CString::new(g).unwrap();
        unsafe { hk_strings_push(out, g.as_ptr()) };
    }
}

unsafe extern "C" fn c_use(context: *mut c_void, code: *const c_char) {
    let words = unsafe { &*(context as *const Words) };
    words
        .log
        .borrow_mut()
        .push(format!("{}|u|{}", words.tag, s(code)));
}

thread_local! {
    static RELEASED: RefCell<usize> = const { RefCell::new(0) };
}

unsafe extern "C" fn c_release(context: *mut c_void) {
    drop(unsafe { Box::from_raw(context as *mut Words) });
    RELEASED.with(|r| *r.borrow_mut() += 1);
}

fn checker(words: Words) -> HkTextChecker {
    HkTextChecker {
        context: Box::into_raw(Box::new(words)).cast(),
        is_word_valid: Some(c_valid),
        guesses: Some(c_guesses),
        completions: Some(c_completions),
        use_language: Some(c_use),
        release: Some(c_release),
    }
}

fn c(text: &str) -> CString {
    CString::new(text).unwrap()
}

fn take(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = s(p).to_owned();
    unsafe { hk_string_free(p) };
    Some(text)
}

const ENGLISH: [&str; 9] = [
    "the", "they", "see", "you", "meeting", "mercy", "be", "hello", "help",
];
const FRENCH: [&str; 6] = ["je", "pense", "que", "merci", "est", "dont"];

fn qwerty() -> Vec<(char, f64, f64)> {
    let mut out = Vec::new();
    for (row, (letters, offset)) in [("qwertyuiop", 0.0), ("asdfghjkl", 0.5), ("zxcvbnm", 1.5)]
        .into_iter()
        .enumerate()
    {
        for (column, letter) in letters.chars().enumerate() {
            out.push((letter, row as f64, column as f64 + offset));
        }
    }
    out
}

#[test]
fn the_c_interface_answers_and_asks_as_the_core() {
    let available: Vec<CString> = ["en_US", "fr"].iter().map(|a| c(a)).collect();
    let available_ptrs: Vec<*const c_char> = available.iter().map(|a| a.as_ptr()).collect();
    let grid = qwerty();

    // Through C.
    let c_log = Rc::new(RefCell::new(Vec::new()));
    let tsv = c(TSV);
    let list = unsafe { hk_never_suggest_new(tsv.as_ptr()) };
    assert!(!list.is_null());
    let ac = unsafe {
        hk_autocorrect_new(
            list,
            ptr::null(),
            checker(Words::new("p", &ENGLISH, &c_log)),
        )
    };
    unsafe { hk_never_suggest_free(list) }; // the handle keeps its own reference
    assert!(!ac.is_null());
    let cells: Vec<HkKeyCentre> = grid
        .iter()
        .map(|&(letter, row, column)| HkKeyCentre {
            letter: letter as u32,
            x: column,
            y: row,
        })
        .collect();
    let en = c("en");
    let fr = c("fr");
    unsafe {
        assert_eq!(
            hk_autocorrect_set_language(ac, en.as_ptr(), available_ptrs.as_ptr(), 2),
            0
        );
        assert_eq!(
            hk_autocorrect_set_keys(ac, en.as_ptr(), cells.as_ptr(), cells.len()),
            0
        );
        assert_eq!(
            hk_autocorrect_set_partner(
                ac,
                fr.as_ptr(),
                available_ptrs.as_ptr(),
                2,
                checker(Words::new("x", &FRENCH, &c_log))
            ),
            0
        );
        let (inputs, outputs) = ([c("omw")], [c("On my way!")]);
        let (i, o) = ([inputs[0].as_ptr()], [outputs[0].as_ptr()]);
        assert_eq!(
            hk_autocorrect_load_lexicon(ac, i.as_ptr(), o.as_ptr(), 1),
            0
        );
        let (typo, fix) = (c("hrllo"), c("hello"));
        let learning = HkLearning {
            typos: [typo.as_ptr()].as_ptr(),
            fixes: [fix.as_ptr()].as_ptr(),
            typo_count: 1,
            rejected: ptr::null(),
            rejected_count: 0,
        };
        assert_eq!(hk_autocorrect_apply_learning(ac, &learning, ptr::null()), 0);
    }

    // Directly.
    let r_log = Rc::new(RefCell::new(Vec::new()));
    let bundled = Arc::new(Bundled {
        never_suggest: Arc::new(NeverSuggestList::new(TSV)),
        german_capitals: None,
    });
    let mut core = Autocorrect::new(bundled, Box::new(Words::new("p", &ENGLISH, &r_log)));
    core.set_language("en", &["en_US", "fr"]);
    let core_cells: Vec<KeyCell> = grid
        .iter()
        .map(|&(letter, row, column)| KeyCell {
            letter,
            row,
            column,
        })
        .collect();
    core.set_keys("en", &core_cells);
    core.set_partner(
        Some("fr"),
        &["en_US", "fr"],
        Box::new(Words::new("x", &FRENCH, &r_log)),
    );
    core.load_lexicon_entries(&[("omw", "On my way!")]);
    core.apply_learning(&TypingLearning::new(&[("hrllo", "hello")], &[]), None);

    for before in [
        "teh",
        "the",
        "dont",
        "mercu",
        "see you mercu",
        "omw",
        "hrllo",
        "i'm",
        "Done. ",
        "hel",
        "the ",
    ] {
        let b = c(before);
        let (mut original, mut replacement) = (ptr::null_mut(), ptr::null_mut());
        assert_eq!(
            unsafe {
                hk_autocorrect_replacement_on_space(
                    ac,
                    b.as_ptr(),
                    0,
                    0,
                    &mut original,
                    &mut replacement,
                )
            },
            0
        );
        let via_c = take(original).zip(take(replacement));
        assert_eq!(
            via_c,
            core.replacement_on_space(Some(before), FieldKind::Text, false),
            "space after {before:?}"
        );

        let mut strip = HkSuggestions {
            items: ptr::null_mut(),
            count: 0,
        };
        assert_eq!(
            unsafe { hk_autocorrect_suggestions(ac, b.as_ptr(), 0, 0, ptr::null(), 0, &mut strip) },
            0
        );
        let items = unsafe { std::slice::from_raw_parts(strip.items, strip.count) };
        let via_c: Vec<(String, u8)> = items
            .iter()
            .map(|i| (s(i.replacement).to_owned(), i.kind))
            .collect();
        unsafe { hk_suggestions_free(&mut strip) };
        let direct: Vec<(String, u8)> = core
            .suggestions(Some(before), FieldKind::Text, false, None, false)
            .into_iter()
            .map(|x| {
                let kind = match x.kind {
                    SuggestionKind::KeepLiteral => 0,
                    SuggestionKind::Primary {
                        auto_replace: false,
                    } => 1,
                    SuggestionKind::Primary { auto_replace: true } => 2,
                    SuggestionKind::Secondary => 3,
                    SuggestionKind::NextWord => 4,
                };
                (x.replacement, kind)
            })
            .collect();
        assert_eq!(via_c, direct, "strip for {before:?}");
    }
    let (word, before, next) = (c("Im"), c("Im"), c("going"));
    let mut out = ptr::null_mut();
    assert_eq!(
        unsafe {
            hk_autocorrect_revision(
                ac,
                word.as_ptr(),
                before.as_ptr(),
                next.as_ptr(),
                0,
                0,
                &mut out,
            )
        },
        0
    );
    assert_eq!(
        take(out),
        core.revision("Im", "Im", "going", FieldKind::Text, false)
    );
    let mut out = ptr::null_mut();
    assert_eq!(
        unsafe { hk_autocorrect_text(ac, 5, ptr::null(), ptr::null(), &mut out) },
        0
    );
    assert_eq!(take(out).as_deref(), core.last_fix_language());
    assert_eq!(
        unsafe { hk_autocorrect_is(ac, 0, c("fuck").as_ptr(), ptr::null()) },
        0
    );
    assert_eq!(
        unsafe { hk_autocorrect_is(ac, 1, c("merci").as_ptr(), ptr::null()) },
        i32::from(core.knows_as_typed("merci"))
    );
    assert_eq!(
        unsafe { hk_autocorrect_is(ac, 9, c("x").as_ptr(), ptr::null()) },
        -1
    );

    assert_eq!(
        *c_log.borrow(),
        *r_log.borrow(),
        "the same questions, in the same order"
    );
    assert!(c_log.borrow().len() > 20);

    let before_free = RELEASED.with(|r| *r.borrow());
    unsafe { hk_autocorrect_free(ac) };
    assert_eq!(
        RELEASED.with(|r| *r.borrow()) - before_free,
        2,
        "both checkers released"
    );
}

#[test]
fn bad_arguments_and_text_alone() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let ac = unsafe {
        hk_autocorrect_new(
            ptr::null(),
            ptr::null(),
            checker(Words::new("p", &ENGLISH, &log)),
        )
    };
    let mut out = ptr::null_mut();
    unsafe {
        assert_ne!(
            hk_autocorrect_set_language(ptr::null_mut(), c("en").as_ptr(), ptr::null(), 0),
            0
        );
        assert_ne!(
            hk_autocorrect_set_language(ac, ptr::null(), ptr::null(), 0),
            0
        );
        assert_ne!(
            hk_autocorrect_revision(ac, ptr::null(), ptr::null(), ptr::null(), 0, 0, &mut out),
            0
        );
        assert!(out.is_null());
        assert_eq!(
            hk_text(0, c("hello wor").as_ptr(), ptr::null(), &mut out),
            0
        );
        assert_eq!(take(out).as_deref(), Some("wor"));
        assert_eq!(hk_text(0, c("@handle").as_ptr(), ptr::null(), &mut out), 0);
        assert!(out.is_null());
        assert_eq!(
            hk_text_is(0, c("Τι κάνεις; ").as_ptr(), ptr::null(), c("el").as_ptr()),
            1
        );
        assert_eq!(
            hk_text_is(4, ptr::null(), ptr::null(), c("fr-CA").as_ptr()),
            1
        );
        assert_eq!(hk_text_is(4, ptr::null(), ptr::null(), c("xx").as_ptr()), 0);
        let cells: Vec<HkKeyCentre> = qwerty()
            .iter()
            .map(|&(letter, row, column)| HkKeyCentre {
                letter: letter as u32,
                x: column,
                y: row,
            })
            .collect();
        let mut cost = 0.0;
        assert_eq!(
            hk_edit_cost(
                c("teh").as_ptr(),
                c("the").as_ptr(),
                c("en").as_ptr(),
                cells.as_ptr(),
                cells.len(),
                &mut cost
            ),
            0
        );
        assert_eq!(cost, 0.6);
        assert_eq!(
            hk_adjacent_keys(
                c("q").as_ptr(),
                c("w").as_ptr(),
                cells.as_ptr(),
                cells.len()
            ),
            1
        );
        let mut words = HkStringList {
            items: ptr::null_mut(),
            count: 0,
        };
        assert_eq!(hk_language_words(c("pl").as_ptr(), 0, &mut words), 0);
        let starters: Vec<&str> = std::slice::from_raw_parts(words.items, words.count)
            .iter()
            .map(|&w| s(w))
            .collect();
        assert_eq!(starters, ["ja", "to", "nie"]);
        hk_string_list_free(&mut words);
        assert_eq!(hk_language_words(c("fr-CA").as_ptr(), 4, &mut words), 0);
        let codes: Vec<&str> = std::slice::from_raw_parts(words.items, words.count)
            .iter()
            .map(|&w| s(w))
            .collect();
        assert_eq!(codes, ["fr_CA", "fr", "fr_FR"]);
        hk_string_list_free(&mut words);
        assert_eq!(hk_language_words(c("it").as_ptr(), 5, &mut words), 0);
        let pairs: Vec<&str> = std::slice::from_raw_parts(words.items, words.count)
            .iter()
            .map(|&w| s(w))
            .collect();
        assert_eq!(pairs.len(), 56);
        assert!(pairs.as_chunks::<2>().0.contains(&["perche", "perché"]));
        hk_string_list_free(&mut words);
        assert_ne!(hk_language_words(c("it").as_ptr(), 9, &mut words), 0);
        hk_autocorrect_free(ac);
        hk_autocorrect_free(ptr::null_mut());
    }
}

#[test]
fn never_suggest_and_german_capitals() {
    let tsv = c(TSV);
    let list = unsafe { hk_never_suggest_new(tsv.as_ptr()) };
    let bytes = b"haus\t99\nleben\t60\nzeit\t40\n";
    let capitals = unsafe { hk_german_capitals_new(bytes.as_ptr(), bytes.len()) };
    unsafe {
        assert_eq!(
            hk_never_suggest_kind(list, c("en").as_ptr(), c("Fuck").as_ptr()),
            2
        );
        assert_eq!(
            hk_never_suggest_kind(list, c("fr").as_ptr(), c("merde").as_ptr()),
            2
        );
        assert_eq!(
            hk_never_suggest_kind(list, c("en").as_ptr(), c("merde").as_ptr()),
            0
        );
        assert_eq!(
            hk_never_suggest_blocks(
                list,
                c("en").as_ptr(),
                c("job").as_ptr(),
                0,
                c("blow").as_ptr()
            ),
            1
        );
        assert_eq!(
            hk_never_suggest_blocks(list, c("en").as_ptr(), c("fuck").as_ptr(), 1, ptr::null()),
            0
        );
        assert_eq!(hk_german_capitals_percent(capitals, c("Haus").as_ptr()), 99);
        assert_eq!(hk_german_capitals_percent(capitals, c("zeit").as_ptr()), 0);
        assert_eq!(
            hk_german_capitals_capitalises(capitals, c("leben").as_ptr(), c("das").as_ptr()),
            1
        );
        assert_eq!(
            hk_german_capitals_capitalises(capitals, c("leben").as_ptr(), c("zu").as_ptr()),
            0
        );
        assert_eq!(
            hk_german_capitals_capitalises(capitals, c("auto").as_ptr(), ptr::null()),
            2
        );
        assert!(hk_german_capitals_new(b"no newline".as_ptr(), 10).is_null());
        hk_german_capitals_free(capitals);
        hk_never_suggest_free(list);
    }
}
