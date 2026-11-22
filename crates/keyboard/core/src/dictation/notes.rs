//! Notes (TranscriptFormatter.swift, `formatNotes`): spoken "bullet" and "new line" markers,
//! else several sentences become a list (and a list after a colon becomes bullets), with no
//! opening "so", "yeah" or "and then" and no final periods.

use super::formatter::{
    capitalize_first_letter, collapse_horizontal_spaces, ends_sentence,
    fix_spacing_before_punctuation, is_abbreviation,
};
use super::{regex, text as t};

pub(super) fn format(text: &str, english: bool) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut working = regex::replace_spoken_break(text, "new paragraph", "\n\n");
    working = regex::replace_spoken_break(&working, "new line", "\n");
    working = regex::replace_spoken_bullets(&working);
    let mut lines: Vec<String> = t::lines(&working).into_iter().map(str::to_owned).collect();
    // Without spoken markers, one dictation of several items becomes a list.
    if lines.len() == 1 {
        let line = fix_spacing_before_punctuation(&collapse_horizontal_spaces(&lines[0]));
        let sentences = note_sentences(&line);
        if sentences.len() >= 2 {
            lines = sentences.iter().map(|s| format!("- {s}")).collect();
        } else if let Some((intro, items)) = colon_list(&line, english) {
            lines = std::iter::once(intro + ":")
                .chain(items.iter().map(|item| format!("- {item}")))
                .collect();
        }
    }
    let notes: Vec<String> = lines
        .iter()
        .map(|line| {
            let trimmed = collapse_horizontal_spaces(line);
            if t::has_prefix(&trimmed, "- ") {
                let body = t::trim_whitespaces(t::drop_first(&trimmed, 2));
                let note = concise_note(body, english);
                if note.is_empty() {
                    String::new()
                } else {
                    format!("- {note}")
                }
            } else {
                concise_note(&trimmed, english)
            }
        })
        .collect();
    t::trim_whitespaces_and_newlines(&notes.join("\n")).to_owned()
}

/// One note: no opening connective (English), no final full stop, a capital first letter.
fn concise_note(line: &str, english: bool) -> String {
    let mut note = fix_spacing_before_punctuation(line);
    if english {
        note = dropping_connectives(&note);
    }
    if t::last(&note).is_some_and(|c| t::is(c, '.') || t::is(c, '。'))
        && !t::has_suffix(&note, "...")
    {
        t::remove_last(&mut note);
    }
    capitalize_first_letter(t::trim_whitespaces(&note))
}

const CONNECTIVES: [&str; 24] = [
    "and then",
    "and also",
    "and",
    "also",
    "then",
    "plus",
    "so",
    "okay",
    "ok",
    "yeah",
    "oh",
    "well",
    "anyway",
    "alright",
    "basically",
    "first",
    "firstly",
    "second",
    "secondly",
    "third",
    "thirdly",
    "next",
    "finally",
    "lastly",
];
const BARE_CONNECTIVES: [&str; 6] = ["and then", "and also", "and", "also", "then", "plus"];

fn dropping_connectives(line: &str) -> String {
    let mut note = line.to_owned();
    let mut changed = true;
    while changed {
        changed = false;
        let lowered = t::lowercased(&note);
        for connective in CONNECTIVES {
            if !t::has_prefix(&lowered, connective) {
                continue;
            }
            let rest = t::drop_first(&note, connective.len());
            let next = if t::has_prefix(rest, ", ") {
                t::drop_first(rest, 2)
            } else if t::has_prefix(rest, " ") && BARE_CONNECTIVES.contains(&connective) {
                t::drop_first(rest, 1)
            } else {
                continue;
            };
            // Never the whole note, and never into a closing mark ("And?").
            if !t::first(next).is_some_and(t::is_letter_or_number) {
                continue;
            }
            note = next.to_owned();
            changed = true;
            break;
        }
    }
    note
}

/// `line`'s sentences, each with its mark; one sentence or none returns `[line]`.
fn note_sentences(line: &str) -> Vec<String> {
    let chars = t::chars(line);
    let mut sentences = Vec::new();
    let mut start = 0;
    for index in 0..chars.len() {
        if !ends_sentence(&chars, index) {
            continue;
        }
        if t::is(chars[index], '.') {
            let mut word_start = index;
            while word_start > start && !t::is_whitespace(chars[word_start - 1]) {
                word_start -= 1;
            }
            let word = t::lowercased(&t::join(&chars[word_start..index]));
            // "Dr." or an initial ("J. K.") ends nothing.
            if is_abbreviation(&word)
                || (t::count(&word) == 1 && t::first(&word).is_some_and(t::is_letter))
            {
                continue;
            }
        }
        let sentence = t::join(&chars[start..=index]);
        let sentence = t::trim_whitespaces(&sentence);
        if !sentence.is_empty() {
            sentences.push(sentence.to_owned());
        }
        start = index + 1;
    }
    let rest = t::join(&chars[start..]);
    let rest = t::trim_whitespaces(&rest);
    if !rest.is_empty() {
        sentences.push(rest.to_owned());
    }
    if sentences.is_empty() {
        vec![line.to_owned()]
    } else {
        sentences
    }
}

/// "Shopping list: eggs, milk and bread." → ("Shopping list", ["eggs", "milk", "bread"]).
fn colon_list(line: &str, english: bool) -> Option<(String, Vec<String>)> {
    let (colon_start, colon_end) = t::find(line, ": ")?;
    let intro = t::trim_whitespaces(&line[..colon_start]).to_owned();
    let mut body = t::trim_whitespaces(&line[colon_end..]).to_owned();
    if intro.is_empty() || t::contains(&body, ": ") {
        return None;
    }
    if t::last(&body).is_some_and(|c| t::is(c, '.') || t::is(c, '。')) {
        t::remove_last(&mut body);
    }
    let mut items: Vec<String> = t::components(&body, ", ")
        .into_iter()
        .map(|i| t::trim_whitespaces(i).to_owned())
        .collect();
    if english && let Some(last) = items.pop() {
        let mut tail = last;
        for joiner in ["and ", "or "] {
            if t::has_prefix(&t::lowercased(&tail), joiner) {
                tail = t::drop_first(&tail, joiner.len()).to_owned();
            }
        }
        match t::find(&tail, " and ").or_else(|| t::find(&tail, " or ")) {
            Some((lo, hi)) => {
                items.push(tail[..lo].to_owned());
                items.push(tail[hi..].to_owned());
            }
            None => items.push(tail),
        }
    }
    let short = items
        .iter()
        .all(|item| (1..=4).contains(&t::split_on(item, ' ', false).len()));
    let marked = items
        .iter()
        .any(|item| t::chars(item).iter().any(|c| t::is_one_of(c, ".!?。！？")));
    if items.len() < 3 || !short || marked {
        return None;
    }
    Some((intro, items))
}
