//! `TranscriptFormatter` (ios/HEXShared/TranscriptFormatter.swift): the style, and joining a
//! dictation with the text around the cursor. Casual is `casual.rs`, Notes `notes.rs`.

use super::text as t;
use super::{Language, Style, casual, lists, notes, punctuation, regex};

/// `text` in `style` (see `super::format`).
pub fn format(
    raw: &str,
    style: Style,
    language: Option<&Language>,
    lists: bool,
    punctuation: bool,
    ends_with_period: bool,
) -> String {
    let styled = formatted(raw, style, language, lists, punctuation);
    if ends_with_period || style == Style::Verbatim {
        return styled;
    }
    t::lines(&styled)
        .into_iter()
        .map(dropping_final_full_stop)
        .collect::<Vec<_>>()
        .join("\n")
}

fn formatted(
    raw: &str,
    style: Style,
    language: Option<&Language>,
    lists: bool,
    punctuation: bool,
) -> String {
    let trimmed = t::trim_whitespaces_and_newlines(raw);
    if trimmed.is_empty() {
        return String::new();
    }
    let fillers = filler_pattern(language);
    let english = super::is_english(language);
    if style == Style::Verbatim {
        return collapse_horizontal_spaces(trimmed);
    }
    let unspaced = writes_without_spaces(language);
    let stripped = strip_standalone_fillers(trimmed, fillers);
    let stripped = if unspaced {
        closing_unspaced_gaps(&stripped)
    } else {
        stripped
    };
    let cleaned = spoken(&stripped, lists, punctuation, language);
    match style {
        Style::Polished => polish_sentences(&cleaned, english, unspaced),
        Style::Casual => casual::message(&cleaned, english),
        _ => notes::format(&cleaned, english),
    }
}

/// Spoken lists, then spoken punctuation.
fn spoken(text: &str, lists: bool, punctuation: bool, language: Option<&Language>) -> String {
    let listed = if lists {
        lists::format(text, language)
    } else {
        text.to_owned()
    };
    if punctuation {
        punctuation::format(&listed, language)
    } else {
        listed
    }
}

/// `TranscriptFormatter.insertionText`: `formatted` joined to the text before and after the
/// cursor. `is_word` says whether a lowercase word is in the dictation language's
/// dictionary; none leaves the case alone. No space goes where either side is written without
/// spaces (`is_unspaced_script`), and a Chinese or Japanese dictation inserted into a sentence
/// that goes on after the cursor loses the "。" its style ended it with.
pub fn insertion_text(
    formatted: &str,
    before: Option<&str>,
    after: Option<&str>,
    is_word: Option<&dyn Fn(&str) -> bool>,
) -> String {
    if formatted.is_empty() {
        return String::new();
    }
    let mut output = formatted.to_owned();
    if let Some(is_word) = is_word
        && !starts_sentence(before)
    {
        output = continuing_sentence(&output, is_word);
    }
    if t::has_suffix(&output, "。")
        && let Some(next) = after.and_then(t::first)
        && !t::is_whitespace(next)
        && is_unspaced_script(next)
        && (t::is_one_of(next, UNSPACED_CLOSERS) || !starts_sentence(before))
    {
        t::remove_last(&mut output);
    }
    let last_before = before.and_then(t::last);
    match last_before {
        None => output = drop_leading_spaces(&output),
        Some(c) if t::is_whitespace(c) => output = drop_leading_spaces(&output),
        Some(c) => {
            let needs = !t::is_one_of(c, "([{\"“‘«¿¡")
                && !t::has_prefix(&output, " ")
                && !t::has_prefix(&output, "\n")
                && !is_closing_punctuation(t::first(&output))
                && !is_unspaced_script(c)
                && !t::first(&output).is_some_and(is_unspaced_script);
            if needs {
                output.insert(0, ' ');
            }
        }
    }
    if let Some(first) = after.and_then(t::first) {
        let needs = !t::is_whitespace(first)
            && !is_closing_punctuation(Some(first))
            && !t::has_suffix(&output, "\n")
            && !t::has_suffix(&output, " ")
            && !is_unspaced_script(first)
            && !t::last(&output).is_some_and(is_unspaced_script);
        if needs {
            output.push(' ');
        }
    }
    output
}

fn drop_leading_spaces(text: &str) -> String {
    let chars = t::chars(text);
    let skip = chars
        .iter()
        .take_while(|c| t::is(c, ' ') || t::is(c, '\t'))
        .count();
    t::join(&chars[skip..])
}

/// Whether text typed before the cursor leaves the next words at a sentence start.
pub fn starts_sentence(before: Option<&str>) -> bool {
    let Some(before) = before else { return true };
    for c in t::chars(before).into_iter().rev() {
        if t::is_newline(c) {
            return true;
        }
        if t::is_whitespace(c) || t::is_one_of(c, "\"”’')]»") {
            continue;
        }
        return t::is_one_of(c, ".!?…。！？");
    }
    true
}

/// Whether a Character belongs to text written without spaces between words (its first
/// scalar): Han, kana, bopomofo, and the CJK and full-width punctuation and forms. Not Hangul.
pub fn is_unspaced_script(c: &str) -> bool {
    let scalar = c.chars().next().map_or(0, |c| c as u32);
    matches!(
        scalar,
        0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3100..=0x312F
            | 0x31A0..=0x31BF
            | 0x31F0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE1F
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFF61..=0xFF9F
            | 0x20000..=0x3134F
    )
}

/// The languages written without spaces between words: their dictations lose the spaces
/// Apple's speech model puts beside Chinese characters (`closing_unspaced_gaps`).
pub fn writes_without_spaces(language: Option<&Language>) -> bool {
    let Some(language) = language else {
        return false;
    };
    let code = language.code.as_deref().unwrap_or(&language.identifier);
    ["zh", "ja", "yue", "wuu", "cmn"].contains(&code)
}

/// `text` without a run of spaces or tabs that has Chinese or Japanese on either side.
/// Spaces between two Latin words, and at a line's start or end, stay.
pub fn closing_unspaced_gaps(text: &str) -> String {
    let chars = t::chars(text);
    let mut result: Vec<&str> = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if !(t::is(c, ' ') || t::is(c, '\t')) {
            result.push(c);
            index += 1;
            continue;
        }
        let mut end = index;
        while end < chars.len() && (t::is(chars[end], ' ') || t::is(chars[end], '\t')) {
            end += 1;
        }
        let before = result.last().copied();
        let after = chars.get(end).copied();
        let unspaced =
            before.is_some_and(is_unspaced_script) || after.is_some_and(is_unspaced_script);
        let keep = match (before, after) {
            (Some(before), Some(after)) => {
                !unspaced || t::is_newline(before) || t::is_newline(after)
            }
            _ => true,
        };
        if keep {
            result.extend_from_slice(&chars[index..end]);
        }
        index = end;
    }
    t::join(&result)
}

/// Full-width marks that close or continue a clause, before which a dictation's "。" goes.
const UNSPACED_CLOSERS: &str = "，。！？、；：」』）】》〉…—．｡､";

/// German and Luxembourgish keep a dictation's case mid-sentence.
pub fn lowercases_continuations(language: &Language) -> bool {
    let code = language.code.as_deref().unwrap_or(&language.identifier);
    code != "de" && code != "lb"
}

/// Lowercases the first letter of words that continue a sentence, as typing would.
pub fn continuing_sentence(text: &str, is_word: &dyn Fn(&str) -> bool) -> String {
    let Some(first) = t::first(text) else {
        return text.to_owned();
    };
    if !t::is_uppercase(first) {
        return text.to_owned();
    }
    let chars = t::chars(text);
    let word = t::join(
        &chars
            .iter()
            .take_while(|c| t::is_letter(c) || t::is(c, '\'') || t::is(c, '’'))
            .collect::<Vec<_>>(),
    );
    if t::same(&word, "I") || t::has_prefix(&word, "I'") || t::has_prefix(&word, "I’") {
        return text.to_owned();
    }
    let letters = letters_of(&word);
    if t::count(&letters) >= 2 && t::same(&letters, &t::uppercased(&letters)) {
        return text.to_owned();
    }
    if t::chars(&letters)
        .iter()
        .skip(1)
        .any(|c| t::is_uppercase(c))
    {
        return text.to_owned();
    }
    if !is_word(&t::lowercased(&word)) {
        return text.to_owned();
    }
    t::lowercased(first) + &text[first.len()..]
}

/// `word.filter(\.isLetter)`.
pub(super) fn letters_of(word: &str) -> String {
    t::chars(word)
        .into_iter()
        .filter(|c| t::is_letter(c))
        .collect()
}

fn is_closing_punctuation(c: Option<&str>) -> bool {
    c.is_some_and(|c| t::is_one_of(c, ".,!?;:)]}”"))
}

/// The hesitation sounds a style trims, per language.
pub fn filler_pattern(language: Option<&Language>) -> Option<&'static str> {
    match super::code_or_english(language) {
        "en" => Some("um+|uh+|er+m*|ah+"),
        "nb" | "nn" | "no" | "da" | "sv" => Some("e+h+m*|ø+h+m*|ö+h+m*|hm+"),
        "es" => Some("e+h+m*|e+m+|m{2,}|hm+"),
        "de" => Some("ä+h+m*|ö+h+m*|hm+"),
        _ => None,
    }
}

fn strip_standalone_fillers(text: &str, fillers: Option<&str>) -> String {
    match fillers {
        None => collapse_horizontal_spaces(text),
        Some(fillers) => collapse_horizontal_spaces(&regex::strip_fillers(text, fillers)),
    }
}

fn polish_sentences(text: &str, english: bool, unspaced: bool) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut normalized = fix_spacing_before_punctuation(&collapse_horizontal_spaces(text));
    if english {
        normalized = expanding_informal_contractions(&normalized);
    }
    normalized = capitalize_sentence_starts(&normalized);
    if english {
        normalized = recasing_pronoun_i(&normalized, 'i', 'I');
    }
    t::lines(&normalized)
        .into_iter()
        .map(|line| ending_sentence(line, unspaced))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `line` with a full stop unless it already ends a sentence, is empty, or is a lone address. A
/// Chinese or Japanese line takes "。" even when it ends in a Latin word or a number, in a
/// dictation in that language (`unspaced`).
fn ending_sentence(line: &str, unspaced: bool) -> String {
    let Some(last) = t::last(line) else {
        return String::new();
    };
    if t::is_one_of(last, SENTENCE_CLOSERS) {
        return line.to_owned();
    }
    if t::contains(line, "@") && !t::chars(line).iter().any(|c| t::is_whitespace(c)) {
        return line.to_owned();
    }
    let ideographic = uses_ideographic_full_stop(last)
        || (unspaced && t::chars(line).into_iter().any(uses_ideographic_full_stop));
    let stop = if ideographic { "。" } else { "." };
    line.to_owned() + stop
}

/// `line` without the full stop that ends it; an ellipsis, an abbreviation's or an
/// initialism's stays.
pub fn dropping_final_full_stop(line: &str) -> String {
    let mut chars = t::chars(line);
    let mut stop = chars.len();
    while stop > 0 && t::is_one_of(chars[stop - 1], "\"”’')]」』）") {
        stop -= 1;
    }
    if stop == 0 || !t::is_one_of(chars[stop - 1], ".。．｡") {
        return line.to_owned();
    }
    let stop = stop - 1;
    if t::is(chars[stop], '.') {
        let mut start = stop;
        while start > 0 && !t::is_whitespace(chars[start - 1]) {
            start -= 1;
        }
        let word = t::lowercased(&t::join(&chars[start..stop]));
        let initials = t::split_on(&word, '.', true);
        let initialism = initials.len() >= 2
            && initials
                .iter()
                .all(|part| t::count(part) == 1 && t::first(part).is_some_and(t::is_letter));
        if t::has_suffix(&word, ".") || initialism || is_abbreviation(&word) {
            return line.to_owned();
        }
    }
    chars.remove(stop);
    t::join(&chars)
}

const SENTENCE_CLOSERS: &str = ".!?…\"”)]:,;。！？」』）";

/// Whether a full stop or another mark ends a sentence at `index`.
pub(super) fn ends_sentence<S: AsRef<str>>(chars: &[S], index: usize) -> bool {
    let c = chars[index].as_ref();
    if t::is_one_of(c, "。！？") {
        return true;
    }
    if !t::is_one_of(c, ".!?…") {
        return false;
    }
    index + 1 == chars.len() || t::is_whitespace(chars[index + 1].as_ref())
}

fn uses_ideographic_full_stop(c: &str) -> bool {
    let scalar = c.chars().next().map_or(0, |c| c as u32);
    matches!(scalar, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x3040..=0x30FF | 0xF900..=0xFAFF)
}

const INFORMAL_CONTRACTIONS: [(&str, &str); 5] = [
    ("gonna", "going to"),
    ("wanna", "want to"),
    ("gotta", "got to"),
    ("kinda", "kind of"),
    ("lemme", "let me"),
];

fn informal_expansion(word: &str) -> Option<&'static str> {
    INFORMAL_CONTRACTIONS
        .iter()
        .find(|(key, _)| t::same(key, word))
        .map(|(_, expansion)| *expansion)
}

/// "I'm gonna call" → "I'm going to call", never inside quotes or on a capitalized word
/// mid-sentence.
fn expanding_informal_contractions(text: &str) -> String {
    let lowered = t::lowercased(text);
    if !INFORMAL_CONTRACTIONS
        .iter()
        .any(|(key, _)| t::contains(&lowered, key))
    {
        return text.to_owned();
    }
    let mut output = String::new();
    let mut word = String::new();
    let mut straight = false;
    let mut curly = 0usize;
    let flush = |word: &mut String, output: &mut String, straight: bool, curly: usize| {
        let lower = t::lowercased(word);
        match informal_expansion(&lower) {
            Some(expansion) if !straight && curly == 0 => {
                if t::same(word, &lower) {
                    output.push_str(expansion);
                } else if t::same(word, &t::capitalized_first(&lower))
                    && starts_sentence_or_item(output)
                {
                    output.push_str(&t::capitalized_first(expansion));
                } else {
                    output.push_str(word);
                }
            }
            _ => output.push_str(word),
        }
        word.clear();
    };
    for c in t::chars(text) {
        if t::is_letter_or_number(c) || t::is(c, '\'') || t::is(c, '’') {
            word.push_str(c);
            continue;
        }
        flush(&mut word, &mut output, straight, curly);
        if t::is(c, '"') {
            straight = !straight;
        } else if t::is(c, '“') {
            curly += 1;
        } else if t::is(c, '”') {
            curly = curly.saturating_sub(1);
        }
        output.push_str(c);
    }
    flush(&mut word, &mut output, straight, curly);
    output
}

/// Whether the next word starts a sentence, or a bulleted item ("- ").
pub(super) fn starts_sentence_or_item(text: &str) -> bool {
    if starts_sentence(Some(text)) {
        return true;
    }
    let line = t::split_on(text, '\n', true).pop().unwrap_or("");
    let marker = t::trim_whitespaces(line);
    t::same(marker, "-") || t::same(marker, "•")
}

/// English "i", "i'm", "i'll", "i've", "i'd" as `to` (from `from`); never "i.e." or an address.
pub(super) fn recasing_pronoun_i(text: &str, from: char, to: char) -> String {
    if !t::contains(text, from.encode_utf8(&mut [0; 4])) {
        return text.to_owned();
    }
    regex::recase_pronoun(text, from, to)
}

/// Abbreviations whose full stop ends no sentence ("Dr. Patel").
pub(super) fn is_abbreviation(word: &str) -> bool {
    const ABBREVIATIONS: [&str; 15] = [
        "mr", "mrs", "ms", "dr", "st", "jr", "sr", "prof", "vs", "etc", "inc", "ltd", "approx",
        "e.g", "i.e",
    ];
    ABBREVIATIONS.iter().any(|a| t::same(a, word))
}

/// Runs of spaces and tabs as one space, each line trimmed, and the text trimmed.
pub(super) fn collapse_horizontal_spaces(text: &str) -> String {
    let lines: Vec<String> = t::lines(text)
        .into_iter()
        .map(|line| {
            let mut collapsed = String::with_capacity(line.len());
            let mut in_run = false;
            for c in line.chars() {
                if c == ' ' || c == '\t' {
                    if !in_run {
                        collapsed.push(' ');
                    }
                    in_run = true;
                } else {
                    collapsed.push(c);
                    in_run = false;
                }
            }
            t::trim_whitespaces(&collapsed).to_owned()
        })
        .collect();
    t::trim_whitespaces_and_newlines(&lines.join("\n")).to_owned()
}

/// No space before `, . ; : ! ?` (never before an emoticon), no mark opening the text.
pub(super) fn fix_spacing_before_punctuation(text: &str) -> String {
    t::trim_whitespaces_and_newlines(&regex::tighten_marks(text)).to_owned()
}

/// Capitalizes the first letter of each sentence; an address keeps its case.
fn capitalize_sentence_starts(text: &str) -> String {
    let mut chars = t::owned_chars(text);
    let mut capitalize_next = true;
    let mut ended_sentence = false;
    for index in 0..chars.len() {
        let c = chars[index].clone();
        if capitalize_next && t::is_letter(&c) {
            if !is_address(&chars, index) {
                let upper = t::uppercased(&c);
                if let Some(first) = t::first(&upper) {
                    chars[index] = first.to_owned();
                }
            }
            capitalize_next = false;
        } else if t::is(&c, '\n') {
            capitalize_next = true;
            ended_sentence = false;
        } else if t::is_one_of(&c, ".!?") {
            ended_sentence = true;
            capitalize_next = false;
        } else if t::is_whitespace(&c) {
            if ended_sentence {
                capitalize_next = true;
                ended_sentence = false;
            }
        } else if t::is_one_of(&c, "\"”’')]") && ended_sentence {
            continue;
        } else if !t::is_one_of(&c, "\"“(¿¡") {
            capitalize_next = false;
            ended_sentence = false;
        }
    }
    t::join(&chars)
}

/// Whether the word at `index` is an address ("john@gmail.com"), whose case stays.
pub(super) fn is_address<S: AsRef<str>>(chars: &[S], index: usize) -> bool {
    chars[index..]
        .iter()
        .map(AsRef::as_ref)
        .take_while(|c| !t::is_whitespace(c))
        .any(|c| t::is(c, '@'))
}

/// A capital first letter, unless it starts an address.
pub(super) fn capitalize_first_letter(text: &str) -> String {
    let mut chars = t::owned_chars(text);
    let Some(index) = chars.iter().position(|c| t::is_letter(c)) else {
        return text.to_owned();
    };
    if is_address(&t::chars(text), index) {
        return text.to_owned();
    }
    chars[index] = t::uppercased(&chars[index]);
    t::join(&chars)
}
