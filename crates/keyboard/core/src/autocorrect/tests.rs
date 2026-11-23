//! A few of `AutocorrectEngineTests`' cases, with a dictionary of their own. The parity test
//! (`tests/autocorrect_parity`) proves the port against the Swift; these say what it does.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::*;

/// A deterministic dictionary, as the iOS tests' `StubChecker`.
#[derive(Default)]
struct Stub {
    valid: HashSet<String>,
    capitalised: HashSet<String>,
    everything: bool,
    guesses: HashMap<String, Vec<String>>,
    completions: HashMap<String, Vec<String>>,
    asked: RefCell<usize>,
}

impl Stub {
    fn with(valid: &[&str], guesses: &[(&str, &[&str])]) -> Box<Self> {
        Box::new(Self {
            valid: valid.iter().map(|s| (*s).to_owned()).collect(),
            guesses: guesses
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
                .collect(),
            ..Self::default()
        })
    }
}

impl TextChecker for Stub {
    fn is_word_valid(&self, word: &str) -> bool {
        *self.asked.borrow_mut() += 1;
        self.everything
            || self.valid.contains(&text::lowercased(word))
            || self.capitalised.contains(word)
    }
    fn guesses(&self, word: &str) -> Vec<String> {
        self.guesses
            .get(&text::lowercased(word))
            .cloned()
            .unwrap_or_default()
    }
    fn completions(&self, prefix: &str) -> Vec<String> {
        self.completions
            .get(&text::lowercased(prefix))
            .cloned()
            .unwrap_or_default()
    }
    fn use_language(&mut self, _code: &str) {}
}

/// QWERTY's letters, rows shifted as the keys are.
fn qwerty() -> KeyGrid {
    KeyGrid::new(&qwerty_cells())
}

fn qwerty_cells() -> Vec<KeyCell> {
    let mut cells = Vec::new();
    for (row, (letters, offset)) in [("qwertyuiop", 0.0), ("asdfghjkl", 0.5), ("zxcvbnm", 1.5)]
        .into_iter()
        .enumerate()
    {
        for (column, letter) in letters.chars().enumerate() {
            cells.push(KeyCell {
                letter,
                row: row as f64,
                column: column as f64 + offset,
            });
        }
    }
    cells
}

fn engine(checker: Box<Stub>) -> Autocorrect {
    let bundled = Arc::new(Bundled {
        never_suggest: NeverSuggestList::new(
            "*\tswear\tfuck\n*\tslur\tfaggot\nen\tswear\tblow job\n",
        ),
        german_capitals: None,
    });
    let mut e = Autocorrect::new(bundled, checker);
    e.set_keys("en", &qwerty_cells());
    e
}

fn fix(e: &mut Autocorrect, before: &str) -> Option<String> {
    e.replacement_on_space(Some(before), FieldKind::Text, false)
        .map(|f| f.1)
}

const AVAILABLE: [&str; 3] = ["en_US", "fr_FR", "it"];

#[test]
fn fixes_contractions_slips_and_text_replacements() {
    let mut e = engine(Stub::with(&["the", "from", "form", "well"], &[]));
    e.load_lexicon_entries(&[("omw", "On my way!"), ("Kit", "Kit")]);
    for (typed, expected) in [
        ("i", "I"),
        ("dont", "don't"),
        ("Dont", "Don't"),
        ("DONT", "DON'T"),
        ("teh", "the"),
        ("iphnoe", "iPhone"),
        ("omw", "On my way!"),
        ("Omw", "On my way!"),
    ] {
        assert_eq!(fix(&mut e, typed).as_deref(), Some(expected), "{typed}");
    }
    for typed in ["the", "well", "API", "HexKey", "thx", "brb", "hello@teh"] {
        assert_eq!(fix(&mut e, typed), None, "{typed}");
    }
    assert_eq!(fix(&mut e, "say (omw"), Some("On my way!".to_owned()));
    assert_eq!(fix(&mut e, "xomw"), None);
}

#[test]
fn the_strip_keeps_what_you_typed_first() {
    let mut e = engine(Stub::with(&["the", "they"], &[]));
    let strip = e.suggestions(Some("teh"), FieldKind::Text, false, None, false);
    assert_eq!(strip[0].kind, SuggestionKind::KeepLiteral);
    assert_eq!(strip[0].display_text, "\u{201C}teh\u{201D}");
    assert_eq!(strip[1].replacement, "the");
    assert_eq!(
        strip[1].kind,
        SuggestionKind::Primary { auto_replace: true }
    );
    let starters = e.suggestions(Some("Done. "), FieldKind::Text, false, None, true);
    let words: Vec<&str> = starters.iter().map(|s| s.replacement.as_str()).collect();
    assert_eq!(words, ["I", "The", "I'm"]);
}

#[test]
fn a_language_without_a_dictionary_corrects_nothing() {
    let mut e = engine(Stub::with(&["the"], &[]));
    e.set_language("sr-Latn", &AVAILABLE);
    assert!(!e.has_dictionary());
    assert_eq!(fix(&mut e, "teh"), None);
}

#[test]
fn italian_restores_accents_and_never_corrects_into_english() {
    let mut e = engine(Stub::with(&["perché", "è", "e"], &[]));
    e.set_language("it", &AVAILABLE);
    assert_eq!(fix(&mut e, "perche").as_deref(), Some("perché"));
    assert_eq!(fix(&mut e, "i"), None);
    let strip = e.suggestions(Some("e"), FieldKind::Text, false, None, false);
    assert!(strip.iter().any(|s| s.replacement == "è"));
}

#[test]
fn bilingual_autocorrect_keeps_both_languages_words() {
    let english = Stub::with(
        &["the", "see", "you", "meeting"],
        &[("mercu", &["mercy"]), ("meetign", &["meeting"])],
    );
    let mut french = engine(Stub::with(
        &["je", "pense", "que", "dont", "est", "merci"],
        &[("mercu", &["merci"])],
    ));
    french.set_language("fr", &AVAILABLE);
    french.set_partner(Some("en"), &AVAILABLE, english);
    assert_eq!(french.partner_language(), Some("en"));
    assert_eq!(fix(&mut french, "the"), None, "an English word stays");
    assert_eq!(fix(&mut french, "dont"), None, "French \"dont\" stays");
    assert_eq!(fix(&mut french, "meetign").as_deref(), Some("meeting"));
    assert_eq!(fix(&mut french, "mercu").as_deref(), Some("merci"));
    assert_eq!(fix(&mut french, "see you mercu").as_deref(), Some("mercy"));
    assert_eq!(french.last_fix_language(), Some("en"));
}

#[test]
fn a_partner_dictionary_that_takes_every_word_is_ignored() {
    let mut english = engine(Stub::with(&["the", "just"], &[("jest", &["just"])]));
    let mut broken = Stub::with(&[], &[]);
    broken.everything = true;
    english.set_partner(Some("fr"), &AVAILABLE, broken);
    assert_eq!(fix(&mut english, "jest").as_deref(), Some("just"));
}

#[test]
fn offensive_words_are_never_offered_unless_your_own() {
    let mut e = engine(Stub::with(&[], &[]));
    assert!(!e.may_offer("fuck", None));
    assert!(!e.may_offer("job", Some("blow")));
    assert!(e.may_offer("job", None));
    e.set_allows_swearing(true);
    assert!(e.may_offer("fuck", None));
    assert!(!e.may_offer("faggot", None));
    e.load_lexicon_entries(&[("faggot", "faggot")]);
    assert!(e.may_offer("Faggot", None));
}

#[test]
fn a_kept_word_is_left_alone_for_the_session() {
    let mut e = engine(Stub::with(&["the"], &[]));
    assert_eq!(fix(&mut e, "teh").as_deref(), Some("the"));
    e.suppress_and_keep("Teh");
    assert_eq!(fix(&mut e, "teh"), None);
    e.set_language("en", &AVAILABLE);
    assert_eq!(
        fix(&mut e, "teh").as_deref(),
        Some("the"),
        "a new language starts over"
    );
}

#[test]
fn reading_the_text_before_the_cursor() {
    assert_eq!(current_token(Some("hello wor")).as_deref(), Some("wor"));
    assert_eq!(current_token(Some("don't")).as_deref(), Some("don't"));
    assert_eq!(current_token(Some("@handle")), None);
    assert_eq!(current_token(Some("3rd")), None);
    assert_eq!(previous_word("see you tom", "tom").as_deref(), Some("you"));
    assert_eq!(previous_word("Done. tom", "tom"), None);
    assert!(is_sentence_start(
        Some("Hi! "),
        LanguageConfig::for_tag("en")
    ));
    assert!(is_sentence_start(
        Some("Τι κάνεις; "),
        LanguageConfig::for_tag("el")
    ));
    assert!(!is_sentence_start(
        Some("Τι κάνεις; "),
        LanguageConfig::for_tag("en")
    ));
    assert_eq!(match_case("Teh", "the"), "The");
    assert_eq!(match_case("TEH", "the"), "THE");
    assert_eq!(match_case("im", "I'm"), "I'm");
    assert_eq!(
        single_edit_keyboard_cost("teh", "the", LanguageConfig::for_tag("en"), &qwerty()),
        Some(0.6)
    );
    assert_eq!(
        single_edit_keyboard_cost("zolw", "żółw", LanguageConfig::for_tag("pl"), &qwerty()),
        Some(0.25)
    );
}

#[test]
fn an_unknown_language_keeps_what_was_typed() {
    let mut e = engine(Stub::with(&["the"], &[]));
    e.set_language("xx", &["xx"]);
    assert!(!e.has_dictionary());
    assert_eq!(fix(&mut e, "teh"), None);
    assert_eq!(LanguageConfig::for_tag("xx").id, "");
}

#[test]
fn french_fixes_both_sides_of_an_elision() {
    let mut e = engine(Stub::with(&["homme", "il"], &[("hpmme", &["homme"])]));
    e.set_language("fr-CA", &AVAILABLE);
    assert_eq!(fix(&mut e, "l'hpmme").as_deref(), Some("l'homme"));
    assert_eq!(fix(&mut e, "k'homme").as_deref(), Some("l'homme"));
    assert_eq!(fix(&mut e, "l'"), None);
}
