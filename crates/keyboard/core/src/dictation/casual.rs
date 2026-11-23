//! Casual (TranscriptFormatter.swift, `casualMessage`): texting. Sentences may start
//! lowercase, English "i", no comma after an opening "yeah", and no final period on a short
//! message.

use super::formatter::{
    collapse_horizontal_spaces, ends_sentence, fix_spacing_before_punctuation, is_address,
    letters_of, recasing_pronoun_i,
};
use super::text as t;

const INTERJECTIONS: [&str; 19] = [
    "yeah", "yes", "yep", "yup", "no", "nope", "nah", "ok", "okay", "oh", "hey", "hi", "hello",
    "so", "well", "sure", "alright", "cool", "thanks",
];

pub(super) fn message(text: &str, english: bool) -> String {
    let mut message = fix_spacing_before_punctuation(&collapse_horizontal_spaces(text));
    if english {
        message = dropping_interjection_commas(&message);
    }
    message = lowercasing_sentence_starts(&message);
    if english {
        message = recasing_pronoun_i(&message, 'I', 'i');
    }
    // A short message drops its final full stop; a question or an exclamation keeps its mark.
    let chars = t::chars(&message);
    let sentences = (0..chars.len())
        .filter(|&index| ends_sentence(&chars, index))
        .count();
    let ends_with_stop = chars
        .last()
        .is_some_and(|c| t::is(c, '.') || t::is(c, '。'));
    if !t::contains(&message, "\n") && sentences <= 3 && ends_with_stop {
        t::remove_last(&mut message);
    }
    t::trim_whitespaces_and_newlines(&message).to_owned()
}

/// "Yeah, sounds good. Ok, see you." → "Yeah sounds good. Ok see you.".
fn dropping_interjection_commas(text: &str) -> String {
    let mut words: Vec<String> = t::components(text, " ")
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut at_start = true;
    for index in 0..words.len() {
        let word = words[index].clone();
        if at_start
            && t::has_suffix(&word, ",")
            && index + 1 < words.len()
            && INTERJECTIONS
                .iter()
                .any(|i| t::same(i, &t::lowercased(t::drop_last(&word))))
        {
            t::remove_last(&mut words[index]);
        }
        let mut chars = t::chars(&word);
        at_start = t::contains(&word, "\n") || {
            let count = chars.len();
            chars.push(" ");
            count > 0 && ends_sentence(&chars, count - 1)
        };
    }
    words.join(" ")
}

/// Lowercases the first letter of each sentence; acronyms, inner capitals ("iPhone") and
/// addresses keep theirs, and a list item ("1. Eggs", "- Eggs") its capital.
fn lowercasing_sentence_starts(text: &str) -> String {
    let mut chars = t::owned_chars(text);
    let mut at_start = true;
    let mut index = 0;
    while index < chars.len() {
        if (index == 0 || t::is_newline(&chars[index - 1]))
            && let Some(marker) = list_marker_length(&chars, index)
        {
            index += marker;
            at_start = false;
            continue;
        }
        let c = chars[index].clone();
        if at_start && t::is_letter(&c) {
            let mut end = index;
            while end < chars.len()
                && (t::is_letter(&chars[end])
                    || t::is(&chars[end], '\'')
                    || t::is(&chars[end], '’'))
            {
                end += 1;
            }
            let word = t::join(&chars[index..end]);
            if !keeps_capital(&word) && !is_address(&chars, index) {
                let lowered = if t::same(&word, "OK") {
                    "ok".to_owned()
                } else {
                    t::lowercased_first(&word)
                };
                let lowered = t::owned_chars(&lowered);
                let count = lowered.len();
                chars.splice(index..end, lowered);
                end = index + count;
            }
            at_start = false;
            index = end;
            continue;
        }
        if t::is_newline(&c) || ends_sentence(&chars, index) {
            at_start = true;
        } else if !t::is_whitespace(&c) && !t::is_one_of(&c, "\"“'‘(¿¡”’)]") {
            at_start = false;
        }
        index += 1;
    }
    t::join(&chars)
}

/// The length of a list marker ("1. ", "- ") at `index`, if one starts there.
fn list_marker_length(chars: &[String], index: usize) -> Option<usize> {
    if index + 1 < chars.len()
        && (t::is(&chars[index], '-') || t::is(&chars[index], '•'))
        && t::is(&chars[index + 1], ' ')
    {
        return Some(2);
    }
    let mut end = index;
    while end < chars.len()
        && end - index < 2
        && t::is_ascii(&chars[end])
        && t::is_number(&chars[end])
    {
        end += 1;
    }
    if end > index
        && end + 1 < chars.len()
        && t::is(&chars[end], '.')
        && t::is(&chars[end + 1], ' ')
    {
        Some(end + 2 - index)
    } else {
        None
    }
}

fn keeps_capital(word: &str) -> bool {
    if t::same(word, "OK") {
        return false;
    }
    let letters = letters_of(word);
    if t::count(&letters) >= 2 && t::same(&letters, &t::uppercased(&letters)) {
        return true;
    }
    t::chars(&letters)
        .iter()
        .skip(1)
        .any(|c| t::is_uppercase(c))
}
