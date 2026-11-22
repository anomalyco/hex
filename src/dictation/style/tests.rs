use super::*;

/// The four styles' output for one dictation, in the order Polished,
/// Casual, Notes, Verbatim.
fn styled(text: &str, language: &str) -> [String; 4] {
    DictationStyle::ALL.map(|style| format(text, style, language))
}

#[test]
fn empty_dictations_stay_empty() {
    for style in DictationStyle::ALL {
        assert_eq!(format("", style, "en"), "");
        assert_eq!(format("  \n\t ", style, "en"), "");
    }
}

#[test]
fn verbatim_collapses_horizontal_spaces_only() {
    assert_eq!(
        format("  um keep   raw words  ", DictationStyle::Verbatim, "en"),
        "um keep raw words"
    );
    assert_eq!(
        format("first  line\nsecond   line", DictationStyle::Verbatim, "en"),
        "first line\nsecond line"
    );
}

/// Ordinary short dictations as Apple Speech writes them read differently in
/// each style.
#[test]
fn styles_differ_on_ordinary_short_dictation() {
    assert_eq!(
        styled("Sounds good, see you at 5.", "en"),
        [
            "Sounds good, see you at 5.",
            "sounds good, see you at 5",
            "Sounds good, see you at 5",
            "Sounds good, see you at 5.",
        ]
    );
    assert_eq!(
        styled("Um, buy milk. Call the dentist. Book the flights.", "en"),
        [
            "Buy milk. Call the dentist. Book the flights.",
            "buy milk. call the dentist. book the flights",
            "- Buy milk\n- Call the dentist\n- Book the flights",
            "Um, buy milk. Call the dentist. Book the flights.",
        ]
    );
    assert_eq!(
        styled("I'll be there in 10.", "en"),
        [
            "I'll be there in 10.",
            "i'll be there in 10",
            "I'll be there in 10",
            "I'll be there in 10.",
        ]
    );
    assert_eq!(
        styled("i'm running late start without me", "en"),
        [
            "I'm running late start without me.",
            "i'm running late start without me",
            "I'm running late start without me",
            "i'm running late start without me",
        ]
    );
}

/// Every style but Verbatim keeps an address whole: no capital after its
/// dots, none on it at a sentence start, and no full stop after an address
/// said alone.
#[test]
fn styles_keep_an_address_whole() {
    assert_eq!(
        format("john@gmail.com", DictationStyle::Polished, "en"),
        "john@gmail.com"
    );
    assert_eq!(
        format("send it to john@gmail.com", DictationStyle::Polished, "en"),
        "Send it to john@gmail.com."
    );
    assert_eq!(
        format(
            "john@gmail.com is mine. call me",
            DictationStyle::Polished,
            "en"
        ),
        "john@gmail.com is mine. Call me."
    );
    assert_eq!(
        format("Mail john@gmail.com", DictationStyle::Casual, "en"),
        "mail john@gmail.com"
    );
    assert_eq!(
        format(
            "bullet john@gmail.com bullet call Ravi",
            DictationStyle::Notes,
            "en"
        ),
        "- john@gmail.com\n- Call Ravi"
    );
    assert_eq!(
        format("Hello. world! ok? yes", DictationStyle::Polished, "en"),
        "Hello. World! Ok? Yes."
    );
}

/// The styles trim each language's own hesitation sounds, never a word ("e"
/// is "and" in Spanish, "eh" is "anyway" in German).
#[test]
fn styles_trim_spanish_and_german_hesitations() {
    assert_eq!(
        format(
            "Mmm, creo que sí, eh, pero no estoy seguro.",
            DictationStyle::Polished,
            "es-ES"
        ),
        "Creo que sí, pero no estoy seguro."
    );
    assert_eq!(
        format(
            "Eh, ¿qué tal? Padre e hijo.",
            DictationStyle::Polished,
            "es-ES"
        ),
        "¿Qué tal? Padre e hijo."
    );
    assert_eq!(
        format("Äh, das ist eh egal.", DictationStyle::Polished, "de-DE"),
        "Das ist eh egal."
    );
    assert_eq!(
        format("Eh, nos vemos.", DictationStyle::Verbatim, "es-ES"),
        "Eh, nos vemos."
    );
}

/// "er" is Norwegian for "is"; English trims it as a filler, Norwegian must
/// not. Portuguese "um" is "a", and a language without a filler list trims
/// nothing.
#[test]
fn filler_trimming_follows_the_dictation_language() {
    assert_eq!(
        format("det er bra, eh, vi ses", DictationStyle::Polished, "nb-NO"),
        "Det er bra, vi ses."
    );
    assert_eq!(
        format("er I think so", DictationStyle::Polished, "en-US"),
        "I think so."
    );
    assert_eq!(
        format("um café, por favor", DictationStyle::Casual, "pt-BR"),
        "um café, por favor"
    );
    assert_eq!(
        format("um let's go", DictationStyle::Polished, "en"),
        "Let's go."
    );
    // "auto" and an empty language say nothing, so they are not English:
    // no fillers go, and the English-only rules stay out.
    for unknown in ["auto", "", "AUTO"] {
        assert_eq!(
            format("um let's go", DictationStyle::Polished, unknown),
            "Um let's go.",
            "{unknown:?}"
        );
        assert_eq!(
            format("i think it's gonna work", DictationStyle::Polished, unknown),
            "I think it's gonna work.",
            "{unknown:?}"
        );
    }
    assert!(is_english("en"));
    assert!(is_english("en-US"));
    assert!(is_english("en_GB"));
    assert!(!is_english("auto"));
    assert!(!is_english(""));
    assert!(!is_english("de"));
}

/// The filler patterns count every leading vowel, so a repeated one still
/// matches the iOS alternation.
#[test]
fn repeated_filler_vowels_are_trimmed() {
    assert_eq!(
        format("Eeh, hola.", DictationStyle::Polished, "es-ES"),
        "Hola."
    );
    assert_eq!(
        format("Eem, hola.", DictationStyle::Polished, "es-ES"),
        "Hola."
    );
    assert_eq!(
        format("Ääh, das ist gut.", DictationStyle::Polished, "de-DE"),
        "Das ist gut."
    );
    assert_eq!(
        format("Øøh, det er bra.", DictationStyle::Polished, "nb-NO"),
        "Det er bra."
    );
}

/// A bare apostrophe after "i" keeps Swift's forbidden lookahead, so it is
/// neither Polished's pronoun "I" nor Casual's "i".
#[test]
fn a_bare_apostrophe_after_i_blocks_recasing() {
    assert_eq!(format("say i'", DictationStyle::Polished, "en"), "Say i'.");
    assert_eq!(format("say I'", DictationStyle::Casual, "en"), "say I'");
    assert_eq!(
        format("i'm ready", DictationStyle::Polished, "en"),
        "I'm ready."
    );
}

/// Polished writes full sentences: "I", a capital and an ending for each
/// paragraph, in Chinese with "。" (never a second mark after one).
#[test]
fn polished_writes_full_sentences() {
    assert_eq!(
        format(
            "erm i think i'll go, it's fine",
            DictationStyle::Polished,
            "en"
        ),
        "I think I'll go, it's fine."
    );
    assert_eq!(
        format("first line\nsecond line", DictationStyle::Polished, "en"),
        "First line.\nSecond line."
    );
    assert_eq!(
        format("Shopping list:", DictationStyle::Polished, "en"),
        "Shopping list:"
    );
    assert_eq!(
        format("好的，明天见。", DictationStyle::Polished, "zh-CN"),
        "好的，明天见。"
    );
    assert_eq!(
        format("好的，明天见", DictationStyle::Polished, "zh-CN"),
        "好的，明天见。"
    );
    // "i" is English only.
    assert_eq!(
        format("i ragazzi", DictationStyle::Polished, "it-IT"),
        "I ragazzi."
    );
    assert_eq!(
        format("dimmi i nomi", DictationStyle::Polished, "it-IT"),
        "Dimmi i nomi."
    );
}

/// Casual texts: sentences may start lowercase, "I" is "i", an opening
/// interjection loses its comma, and a short message its final full stop.
/// Acronyms, names inside a sentence and words with inner capitals keep
/// their case.
#[test]
fn casual_texts() {
    assert_eq!(
        format(
            "Yeah, I'll text John about the iPhone order tomorrow.",
            DictationStyle::Casual,
            "en"
        ),
        "yeah i'll text John about the iPhone order tomorrow"
    );
    assert_eq!(
        format("I think I can, i.e. maybe.", DictationStyle::Casual, "en"),
        "i think i can, i.e. maybe"
    );
    assert_eq!(
        format(
            "OK, can you send me the deck?",
            DictationStyle::Casual,
            "en"
        ),
        "ok can you send me the deck?"
    );
    assert_eq!(
        format("NASA launched it. Wow!", DictationStyle::Casual, "en"),
        "NASA launched it. wow!"
    );
    assert_eq!(
        format("iPhone is charged.", DictationStyle::Casual, "en"),
        "iPhone is charged"
    );
    // Longer than a short message: four sentences, or more than one line,
    // keep the full stop.
    assert_eq!(
        format("One. Two. Three. Four.", DictationStyle::Casual, "en"),
        "one. two. three. four."
    );
    assert_eq!(
        format("Hi.\nSee you.", DictationStyle::Casual, "en"),
        "hi.\nsee you."
    );
    // Interjections are English; Chinese drops its final "。".
    assert_eq!(
        format("Sí, claro.", DictationStyle::Casual, "es-ES"),
        "sí, claro"
    );
    assert_eq!(
        format("好的，明天见。", DictationStyle::Casual, "zh-CN"),
        "好的，明天见"
    );
    assert_eq!(
        format("One. Two. Three.", DictationStyle::Casual, "en"),
        "one. two. three"
    );
}

/// Notes writes concise lines, and a list when there are several items.
#[test]
fn notes_list_several_items_concisely() {
    assert_eq!(
        format(
            "Call the dentist tomorrow at 9.",
            DictationStyle::Notes,
            "en"
        ),
        "Call the dentist tomorrow at 9"
    );
    assert_eq!(
        format("Can you send the deck?", DictationStyle::Notes, "en"),
        "Can you send the deck?"
    );
    assert_eq!(
        format("Yeah, I'll call Ravi.", DictationStyle::Notes, "en"),
        "I'll call Ravi"
    );
    assert_eq!(
        format(
            "So, first, buy milk. And then call the dentist.",
            DictationStyle::Notes,
            "en"
        ),
        "- Buy milk\n- Call the dentist"
    );
    assert_eq!(
        format(
            "Shopping list: eggs, milk, and bread.",
            DictationStyle::Notes,
            "en"
        ),
        "Shopping list:\n- Eggs\n- Milk\n- Bread"
    );
    // No list from an abbreviation's full stop or a colon before a sentence.
    assert_eq!(
        format(
            "Dr. Patel called. Call her back.",
            DictationStyle::Notes,
            "en"
        ),
        "- Dr. Patel called\n- Call her back"
    );
    assert_eq!(
        format("Note: the build is green.", DictationStyle::Notes, "en"),
        "Note: the build is green"
    );
    // Spoken markers decide the lines when there are any; Chinese splits at
    // "。".
    assert_eq!(
        format(
            "todo bullet ship it. Then tag it bullet tell Ravi",
            DictationStyle::Notes,
            "en"
        ),
        "Todo\n- Ship it. Then tag it\n- Tell Ravi"
    );
    assert_eq!(
        format("买牛奶。给妈妈打电话。", DictationStyle::Notes, "zh-CN"),
        "- 买牛奶\n- 给妈妈打电话"
    );
}

/// A colon list splits the last "and" and "or" by the separator's own
/// length, so "or" never eats a byte of the next item.
#[test]
fn notes_splits_a_colon_list_on_or_without_losing_bytes() {
    assert_eq!(
        format(
            "Shopping list: eggs, milk or bread.",
            DictationStyle::Notes,
            "en"
        ),
        "Shopping list:\n- Eggs\n- Milk\n- Bread"
    );
    assert_eq!(
        format("List: a, b, x or é", DictationStyle::Notes, "en"),
        "List:\n- A\n- B\n- X\n- É"
    );
}

/// Spoken "new line", "new paragraph", and "bullet" markers become lines in
/// Notes.
#[test]
fn notes_hears_spoken_line_and_bullet_markers() {
    assert_eq!(
        format(
            "Groceries new line milk new paragraph then eggs",
            DictationStyle::Notes,
            "en"
        ),
        "Groceries\nMilk\n\nEggs"
    );
}

/// The first pass of the shared decision: every style leaves a dictation
/// that is already one clean sentence alone, apart from case and its final
/// stop.
#[test]
fn an_already_clean_sentence_only_changes_case_and_stop() {
    assert_eq!(
        styled("We meet at noon.", "en"),
        [
            "We meet at noon.",
            "we meet at noon",
            "We meet at noon",
            "We meet at noon.",
        ]
    );
}

/// Reads the iOS fields' spelling without values the app itself could never
/// produce, such as a lone blank line from a bullet at the start.
#[test]
fn spoken_bullets_at_the_start_do_not_leave_a_blank_line() {
    assert_eq!(
        format("bullet ship it bullet tag it", DictationStyle::Notes, "en"),
        "- Ship it\n- Tag it"
    );
}
