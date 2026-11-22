//! The main rules, with examples from the Swift tests (ios/HEXMobileTests/
//! TranscriptCleanupTests.swift); `tests/dictation_parity.rs` holds the full comparison.

use super::*;

fn language(identifier: &str, code: &str, region: Option<&str>) -> Language {
    Language {
        identifier: identifier.to_owned(),
        code: Some(code.to_owned()),
        region: region.map(str::to_owned),
    }
}

fn styled(text: &str, style: Style, language: Option<&Language>) -> String {
    format(
        text,
        &FormatOptions {
            style,
            language,
            lists: false,
            punctuation: false,
            ends_with_period: true,
        },
    )
}

#[test]
fn polished_trims_fillers_and_ends_sentences() {
    assert_eq!(
        styled("erm i think i'll go, it's fine", Style::Polished, None),
        "I think I'll go, it's fine."
    );
    assert_eq!(
        styled("Hello. world! ok? yes", Style::Polished, None),
        "Hello. World! Ok? Yes."
    );
    assert_eq!(
        styled("first line\nsecond line", Style::Polished, None),
        "First line.\nSecond line."
    );
    assert_eq!(
        styled("john@gmail.com", Style::Polished, None),
        "john@gmail.com"
    );
    let chinese = language("zh-CN", "zh", Some("CN"));
    assert_eq!(
        styled("好的，明天见", Style::Polished, Some(&chinese)),
        "好的，明天见。"
    );
    let spanish = language("es-ES", "es", Some("ES"));
    assert_eq!(
        styled(
            "Mmm, creo que sí, eh, pero no estoy seguro.",
            Style::Polished,
            Some(&spanish)
        ),
        "Creo que sí, pero no estoy seguro."
    );
}

#[test]
fn casual_texts() {
    assert_eq!(
        styled(
            "Yeah, I'll text John about the iPhone order tomorrow.",
            Style::Casual,
            None
        ),
        "yeah i'll text John about the iPhone order tomorrow"
    );
    assert_eq!(
        styled("NASA launched it. Wow!", Style::Casual, None),
        "NASA launched it. wow!"
    );
    assert_eq!(
        styled("One. Two. Three. Four.", Style::Casual, None),
        "one. two. three. four."
    );
}

#[test]
fn notes_list_several_items() {
    assert_eq!(
        styled(
            "So, first, buy milk. And then call the dentist.",
            Style::Notes,
            None
        ),
        "- Buy milk\n- Call the dentist"
    );
    assert_eq!(
        styled("Shopping list: eggs, milk, and bread.", Style::Notes, None),
        "Shopping list:\n- Eggs\n- Milk\n- Bread"
    );
    assert_eq!(
        styled(
            "todo bullet ship it. Then tag it bullet tell Ravi",
            Style::Notes,
            None
        ),
        "Todo\n- Ship it. Then tag it\n- Tell Ravi"
    );
}

#[test]
fn verbatim_keeps_the_words_and_period_setting_drops_full_stops() {
    assert_eq!(
        styled("um keep  raw words", Style::Verbatim, None),
        "um keep raw words"
    );
    let options = FormatOptions {
        style: Style::Polished,
        language: None,
        lists: false,
        punctuation: false,
        ends_with_period: false,
    };
    assert_eq!(
        format("Sounds good. See you then", &options),
        "Sounds good. See you then"
    );
    assert_eq!(
        dropping_final_full_stop("We met in the U.S."),
        "We met in the U.S."
    );
}

#[test]
fn spoken_lists_and_punctuation() {
    let english = language("en-US", "en", Some("US"));
    assert_eq!(
        spoken_lists(
            "Here is my shopping list. First, eggs. Second, milk, third, bread.",
            Some(&english)
        ),
        "Here is my shopping list.\n1. Eggs\n2. Milk\n3. Bread"
    );
    assert_eq!(
        spoken_lists("1. Eggs. 2. Milk.", Some(&english)),
        "1. Eggs\n2. Milk"
    );
    assert_eq!(
        spoken_punctuation(
            "I came semicolon, I saw semicolon, I conquered.",
            Some(&english)
        ),
        "I came; I saw; I conquered."
    );
    assert_eq!(
        spoken_punctuation(
            "She said open quote hello there close quote and left.",
            Some(&english)
        ),
        "She said “hello there” and left."
    );
    assert_eq!(
        spoken_punctuation("It is John Apostrophe S Carr.", Some(&english)),
        "It is John's Carr."
    );
}

#[test]
fn cleanup_rules_symbols_and_counts() {
    let spanish = language("es-ES", "es", Some("ES"));
    assert_eq!(
        cleanup_rules("Eh, nos vemos a las 3, digo, a las 4.", Some(&spanish)),
        "Nos vemos a las 4."
    );
    assert_eq!(
        cleanup_rules("Y, este, fuimos al cine.", Some(&spanish)),
        "Y fuimos al cine."
    );
    let german = language("de-DE", "de", Some("DE"));
    assert_eq!(
        cleanup_rules("Wir treffen uns um 3, nein, um 4.", Some(&german)),
        "Wir treffen uns um 4."
    );
    let english = language("en-IN", "en", Some("IN"));
    assert_eq!(
        spoken_symbols("My email is John at the rategmail.com.", Some(&english)),
        "My email is john@gmail.com."
    );
    assert_eq!(
        spoken_symbols("Type the at sign, then your name", Some(&english)),
        "Type the @, then your name"
    );
    assert_eq!(
        spoken_counts("Testing 1234, can you hear me?", Some(&english)),
        "Testing 1, 2, 3, 4, can you hear me?"
    );
    assert_eq!(
        spoken_counts("One, two, three, go!", Some(&english)),
        "1, 2, 3, go!"
    );
}

#[test]
fn french_spacing_by_region() {
    let nbsp = '\u{A0}';
    let thin = '\u{202F}';
    let canada = language("fr-CA", "fr", Some("CA"));
    assert_eq!(
        french_spacing("ça va ? Oui ! Voici : «tout»", Some(&canada)),
        format!("ça va? Oui! Voici{nbsp}: «{nbsp}tout{nbsp}»")
    );
    let france = language("fr-FR", "fr", Some("FR"));
    assert_eq!(
        french_spacing("Il m'a dit «Tu viens?»", Some(&france)),
        format!("Il m'a dit «{nbsp}Tu viens{thin}?{nbsp}»")
    );
    assert_eq!(french_spacing("10:30", Some(&france)), "10:30");
    assert_eq!(french_convention(None), None);
}

#[test]
fn insertion_joins_the_text_around_the_cursor() {
    assert_eq!(
        insertion_text("world.", Some("Hello"), None, None),
        " world."
    );
    assert_eq!(
        insertion_text("world.", Some("Hello "), None, None),
        "world."
    );
    assert_eq!(insertion_text("Hello", None, Some("world"), None), "Hello ");
    let words = |word: &str| ["that's", "the"].contains(&word);
    assert_eq!(
        insertion_text("That's it.", Some("and"), None, Some(&words)),
        " that's it."
    );
    assert!(starts_sentence(Some("Done. ")));
    assert!(!starts_sentence(Some("Note: ")));
    let german = language("de-DE", "de", Some("DE"));
    assert!(!lowercases_continuations(&german));
}

#[test]
fn reconstruction_repairs_commas_and_capitals() {
    let words = ["Can", "you,", "um,", "send", "it?"];
    assert_eq!(
        reconstruct(&words, &[false, false, true, false, false]),
        "Can you send it?"
    );
    let words = ["Meet", "at", "5.", "Wait,", "no.", "At", "6."];
    assert_eq!(
        reconstruct(&words, &[false, false, true, true, true, true, false]),
        "Meet at 6."
    );
}

#[test]
fn swift_string_semantics() {
    // Canonical equivalence, Characters and CoreFoundation's fold.
    assert!(text::same("e\u{301}", "é"));
    assert!(!text::has_prefix("e\u{301}x", "e"));
    assert_eq!(text::count("a\r\nb"), 3);
    assert_eq!(fold::fold("Schließende Ñ ﬁn"), "schliessende n fin");
    assert_eq!(fold::fold("Ꭰ ꭰ ि"), "ꭰ ꭰ ि");
    assert_eq!(words(" a\u{A0}b\n c "), ["a", "b", "c"]);
}
