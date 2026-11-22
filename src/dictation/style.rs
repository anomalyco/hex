//! Labs › Dictation style: the iPhone app's four ways to write a dictation,
//! applied to a desktop dictation after its corrections and any rewrite and
//! before the mode's transformations. Every rule is punctuation, case, and
//! word-list work, with no model. Verbatim only collapses spaces, so the
//! model's words stay exactly as heard.
//!
//! English-only rules are the informal contractions, the pronoun "I", Casual's
//! interjections, and Notes' connectives and colon list. "auto" (or an empty
//! language) says nothing about the language, so it gets only the rules that
//! work in every language, and no fillers.

use crate::app_settings::DictationStyle;

mod notes;
mod spacing;

use self::notes::notes;
use self::spacing::{
    capitalize_sentence_starts, capitalized_lower, collapse_horizontal_spaces, ending_sentence,
    ends_sentence, fix_spacing_before_punctuation, lowercasing_sentence_starts, recasing_pronoun_i,
    starts_sentence_or_item,
};

/// `raw` in `style`, for a dictation in `language` (a BCP 47 tag, or
/// "auto"/empty while the language is unknown). The words are trimmed first,
/// and an empty dictation stays empty.
pub(crate) fn format(raw: &str, style: DictationStyle, language: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let fillers = filler_language(language);
    let english = is_english(language);
    match style {
        DictationStyle::Verbatim => collapse_horizontal_spaces(trimmed),
        DictationStyle::Polished => polished(strip_standalone_fillers(trimmed, fillers), english),
        DictationStyle::Casual => casual(strip_standalone_fillers(trimmed, fillers), english),
        DictationStyle::Notes => notes(strip_standalone_fillers(trimmed, fillers), english),
    }
}

/// The leading language subtag of `language`, lowercase; None while the
/// dictation's language is unknown ("auto" or empty).
fn primary_language(language: &str) -> Option<String> {
    let language = language.trim();
    if language.is_empty() || language.eq_ignore_ascii_case("auto") {
        return None;
    }
    let code = language
        .split(['-', '_'])
        .next()
        .unwrap_or(language)
        .to_ascii_lowercase();
    (!code.is_empty()).then_some(code)
}

/// Whether the English-only rules apply: the language is English, never
/// "auto" or empty, which say nothing about the language.
fn is_english(language: &str) -> bool {
    primary_language(language).as_deref() == Some("en")
}

/// The hesitation sounds a style trims, per language. English "er" is a word
/// in Norwegian, Danish, Swedish ("er" = is) and German, and "um" is one in
/// Portuguese, so each language trims only its own; a language without a list
/// trims nothing. Spanish trims "eh", "em" and "mmm" (never "e", which is
/// "and"), German "äh", "ähm" and "öhm" (never "eh", which is "anyway").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FillerLanguage {
    English,
    Nordic,
    Spanish,
    German,
}

fn filler_language(language: &str) -> Option<FillerLanguage> {
    match primary_language(language)?.as_str() {
        "en" => Some(FillerLanguage::English),
        "nb" | "nn" | "no" | "da" | "sv" => Some(FillerLanguage::Nordic),
        "es" => Some(FillerLanguage::Spanish),
        "de" => Some(FillerLanguage::German),
        _ => None,
    }
}

/// The lowercase form of `c`, which is what the case-insensitive patterns
/// compare.
fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// The end of a run of `expected` starting at `start`, one character or
/// more.
fn run(chars: &[char], start: usize, expected: char) -> usize {
    let mut end = start;
    while chars.get(end).is_some_and(|&c| lower(c) == expected) {
        end += 1;
    }
    end
}

/// Where the filler at `start` ends, if one does: the iOS per-language
/// patterns, matched case-insensitively.
fn filler_at(language: FillerLanguage, chars: &[char], start: usize) -> Option<usize> {
    use FillerLanguage::{English, German, Nordic, Spanish};
    let first = lower(*chars.get(start)?);
    let second = chars.get(start + 1).map(|&c| lower(c));
    match language {
        English => match (first, second?) {
            // um+
            ('u', 'm') => Some(run(chars, start + 2, 'm')),
            // uh+
            ('u', 'h') => Some(run(chars, start + 2, 'h')),
            // er+m*
            ('e', 'r') => Some(run(chars, run(chars, start + 2, 'r'), 'm')),
            // ah+
            ('a', 'h') => Some(run(chars, start + 2, 'h')),
            _ => None,
        },
        Nordic => match first {
            // e+h+m*
            'e' => {
                let after_e = run(chars, start, 'e');
                let after_h = run(chars, after_e, 'h');
                (after_h > after_e).then(|| run(chars, after_h, 'm'))
            }
            // ø+h+m*, ö+h+m*
            'ø' | 'ö' => {
                let after_h = run(chars, start + 1, 'h');
                (after_h > start + 1).then(|| run(chars, after_h, 'm'))
            }
            // hm+
            'h' => {
                let after_m = run(chars, start + 1, 'm');
                (after_m > start + 1).then_some(after_m)
            }
            _ => None,
        },
        Spanish => match first {
            // e+h+m*
            'e' if second == Some('h') => {
                let after_e = run(chars, start, 'e');
                let after_h = run(chars, after_e, 'h');
                (after_h > after_e).then(|| run(chars, after_h, 'm'))
            }
            // e+m+
            'e' => {
                let after_e = run(chars, start, 'e');
                let after_m = run(chars, after_e, 'm');
                (after_m > after_e).then_some(after_m)
            }
            // m{2,}
            'm' => {
                let after_m = run(chars, start, 'm');
                (after_m >= start + 2).then_some(after_m)
            }
            // hm+
            'h' => {
                let after_m = run(chars, start + 1, 'm');
                (after_m > start + 1).then_some(after_m)
            }
            _ => None,
        },
        German => match first {
            // ä+h+m*, ö+h+m*
            'ä' | 'ö' => {
                let after_h = run(chars, start + 1, 'h');
                (after_h > start + 1).then(|| run(chars, after_h, 'm'))
            }
            // hm+
            'h' => {
                let after_m = run(chars, start + 1, 'm');
                (after_m > start + 1).then_some(after_m)
            }
            _ => None,
        },
    }
}

/// `text` without its standalone fillers, with the spaces they leave closed
/// up. A filler at the start of the text or after whitespace or a comma, and
/// followed by whitespace or a comma or the end of a sentence, goes with its
/// comma and the spaces around it.
fn strip_standalone_fillers(text: &str, language: Option<FillerLanguage>) -> String {
    let Some(language) = language else {
        return collapse_horizontal_spaces(text);
    };
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if let Some(end) = filler_at(language, &chars, index) {
            let starts = index == 0 || chars[index - 1].is_whitespace() || chars[index - 1] == ',';
            let ends = chars
                .get(end)
                .is_none_or(|&next| next.is_whitespace() || matches!(next, ',' | '.' | '!' | '?'));
            if starts && ends {
                let mut after = end;
                if chars.get(after) == Some(&',') {
                    after += 1;
                }
                while chars.get(after).is_some_and(|c| c.is_whitespace()) {
                    after += 1;
                }
                out.push(' ');
                index = after;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    collapse_horizontal_spaces(&out)
}

fn polished(text: String, english: bool) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut normalized = fix_spacing_before_punctuation(&collapse_horizontal_spaces(&text));
    if english {
        normalized = expanding_informal_contractions(&normalized);
    }
    normalized = capitalize_sentence_starts(&normalized);
    if english {
        normalized = recasing_pronoun_i(&normalized, 'i', 'I');
    }
    normalized
        .split('\n')
        .map(ending_sentence)
        .collect::<Vec<_>>()
        .join("\n")
}

/// What iOS 27's speech models write for casual speech, and what Polished
/// writes instead.
const INFORMAL_CONTRACTIONS: &[(&str, &str)] = &[
    ("gonna", "going to"),
    ("wanna", "want to"),
    ("gotta", "got to"),
    ("kinda", "kind of"),
    ("lemme", "let me"),
];

/// "I'm gonna call" -> "I'm going to call", "Gonna rain." -> "Going to
/// rain.". Never inside quotes (he said "gonna win") or on a capitalized
/// word mid-sentence, which is a name or a title ("Gonna Fly Now"); a
/// capitalized one starting a sentence keeps its capital.
fn expanding_informal_contractions(text: &str) -> String {
    let lowered = text.to_lowercase();
    if !INFORMAL_CONTRACTIONS
        .iter()
        .any(|(word, _)| lowered.contains(*word))
    {
        return text.to_owned();
    }
    let mut output = String::with_capacity(text.len());
    let mut word = String::new();
    let mut in_straight_quote = false;
    let mut curly_quotes = 0;
    let flush =
        |word: &mut String, output: &mut String, in_straight_quote: bool, curly_quotes: usize| {
            let lower = word.to_lowercase();
            let expansion = INFORMAL_CONTRACTIONS
                .iter()
                .find(|(english, _)| *english == lower.as_str())
                .map(|(_, expansion)| *expansion);
            match expansion {
                Some(_) if in_straight_quote || curly_quotes > 0 => output.push_str(word),
                Some(expansion) if *word == lower => output.push_str(expansion),
                Some(expansion) if *word == capitalized_lower(&lower) => {
                    if starts_sentence_or_item(output) {
                        output.extend(expansion.chars().next().unwrap().to_uppercase());
                        output.push_str(&expansion[1..]);
                    } else {
                        output.push_str(word);
                    }
                }
                _ => output.push_str(word),
            }
            word.clear();
        };
    for character in text.chars() {
        if character.is_alphabetic() || character.is_numeric() || matches!(character, '\'' | '’')
        {
            word.push(character);
            continue;
        }
        flush(&mut word, &mut output, in_straight_quote, curly_quotes);
        match character {
            '"' => in_straight_quote = !in_straight_quote,
            '“' => curly_quotes += 1,
            '”' => curly_quotes = curly_quotes.saturating_sub(1),
            _ => {}
        }
        output.push(character);
    }
    flush(&mut word, &mut output, in_straight_quote, curly_quotes);
    output
}

/// Interjections that open a text without their comma in Casual ("yeah
/// sounds good").
const CASUAL_INTERJECTIONS: &[&str] = &[
    "yeah", "yes", "yep", "yup", "no", "nope", "nah", "ok", "okay", "oh", "hey", "hi", "hello",
    "so", "well", "sure", "alright", "cool", "thanks",
];

fn casual(text: String, english: bool) -> String {
    let mut message = fix_spacing_before_punctuation(&collapse_horizontal_spaces(&text));
    if english {
        message = dropping_interjection_commas(&message);
    }
    message = lowercasing_sentence_starts(&message);
    if english {
        message = recasing_pronoun_i(&message, 'I', 'i');
    }
    // A short message drops its final full stop; a question or an
    // exclamation keeps its mark.
    let chars: Vec<char> = message.chars().collect();
    let sentences = (0..chars.len())
        .filter(|&index| ends_sentence(&chars, index))
        .count();
    if !message.contains('\n')
        && sentences <= 3
        && matches!(message.chars().last(), Some('.' | '。'))
    {
        message.pop();
    }
    message.trim().to_owned()
}

/// "Yeah, sounds good. Ok, see you." -> "Yeah sounds good. Ok see you.": an
/// interjection at a sentence start loses its comma.
fn dropping_interjection_commas(text: &str) -> String {
    let mut words: Vec<String> = text.split(' ').map(str::to_owned).collect();
    let mut at_start = true;
    for index in 0..words.len() {
        let word = words[index].clone();
        if at_start
            && word.ends_with(',')
            && index + 1 < words.len()
            && CASUAL_INTERJECTIONS.contains(&word[..word.len() - 1].to_lowercase().as_str())
        {
            words[index].pop();
        }
        let chars: Vec<char> = word.chars().collect();
        let mut with_space = chars.clone();
        with_space.push(' ');
        at_start = word.contains('\n')
            || (!chars.is_empty() && ends_sentence(&with_space, chars.len() - 1));
    }
    words.join(" ")
}

#[cfg(test)]
mod tests;
