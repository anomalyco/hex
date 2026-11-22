//! Notes: concise lines, a list when several items are said, and the
//! English connectives and colon list the style drops.

use std::sync::LazyLock;

use regex::Regex;

use super::spacing::{
    capitalize_first_letter, collapse_horizontal_spaces, ends_sentence,
    fix_spacing_before_punctuation,
};

const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "st", "jr", "sr", "prof", "vs", "etc", "inc", "ltd", "approx", "e.g",
    "i.e",
];

/// Words that open a spoken note but add nothing to a written one.
const NOTE_CONNECTIVES: &[&str] = &[
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

/// Of those, the ones that may also open a note without a comma after them.
const BARE_NOTE_CONNECTIVES: &[&str] = &["and then", "and also", "and", "also", "then", "plus"];

static NEW_PARAGRAPH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bnew paragraph\b[,.]?\s*").unwrap());
static NEW_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bnew line\b[,.]?\s*").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|\s+)(?:bullet point|next bullet|bullet)\b[:,-]?\s*").unwrap()
});

pub(super) fn notes(text: String, english: bool) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut working = text;
    for (regex, replacement) in [
        (&*NEW_PARAGRAPH, "\n\n"),
        (&*NEW_LINE, "\n"),
        (&*BULLET, "\n- "),
    ] {
        working = regex.replace_all(&working, replacement).into_owned();
    }
    let mut lines: Vec<String> = working.split('\n').map(str::to_owned).collect();
    // Without spoken markers, one dictation of several items becomes a list.
    if lines.len() == 1 {
        let line = fix_spacing_before_punctuation(&collapse_horizontal_spaces(&lines[0]));
        let sentences = note_sentences(&line);
        if sentences.len() >= 2 {
            lines = sentences
                .into_iter()
                .map(|sentence| format!("- {sentence}"))
                .collect();
        } else if let Some((intro, items)) = colon_list(&line, english) {
            lines = std::iter::once(format!("{intro}:"))
                .chain(items.into_iter().map(|item| format!("- {item}")))
                .collect();
        }
    }
    lines
        .into_iter()
        .map(|line| {
            let line = collapse_horizontal_spaces(&line);
            match line.strip_prefix("- ") {
                Some(body) => {
                    let note = concise_note(body.trim(), english);
                    if note.is_empty() {
                        String::new()
                    } else {
                        format!("- {note}")
                    }
                }
                None => concise_note(&line, english),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// One note: no opening "so," "okay," "and" or "then" (English), no final
/// full stop, and a capital first letter.
fn concise_note(line: &str, english: bool) -> String {
    let mut note = fix_spacing_before_punctuation(line);
    if english {
        note = dropping_connectives(&note);
    }
    if matches!(note.chars().last(), Some('.' | '。')) && !note.ends_with("...") {
        note.pop();
    }
    capitalize_first_letter(note.trim())
}

fn dropping_connectives(line: &str) -> String {
    let mut note = line.to_owned();
    let mut changed = true;
    while changed {
        changed = false;
        let lowered = note.to_lowercase();
        for connective in NOTE_CONNECTIVES {
            if !lowered.starts_with(connective) {
                continue;
            }
            let Some(rest) = note.get(connective.len()..) else {
                continue;
            };
            let next = if let Some(rest) = rest.strip_prefix(", ") {
                rest
            } else if let Some(rest) = rest.strip_prefix(' ')
                && BARE_NOTE_CONNECTIVES.contains(connective)
            {
                rest
            } else {
                continue;
            };
            // Never the whole note, and never into a closing mark ("And?").
            if !next
                .chars()
                .next()
                .is_some_and(|first| first.is_alphabetic() || first.is_numeric())
            {
                continue;
            }
            note = next.to_owned();
            changed = true;
            break;
        }
    }
    note
}

/// `line`'s sentences, each with its mark; one sentence or none returns
/// `line`.
fn note_sentences(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut sentences = Vec::new();
    let mut start = 0;
    for index in 0..chars.len() {
        if !ends_sentence(&chars, index) {
            continue;
        }
        if chars[index] == '.' {
            let mut word_start = index;
            while word_start > start && !chars[word_start - 1].is_whitespace() {
                word_start -= 1;
            }
            let word: String = chars[word_start..index]
                .iter()
                .collect::<String>()
                .to_lowercase();
            // "Dr." or an initial ("J. K.") ends nothing.
            if ABBREVIATIONS.contains(&word.as_str())
                || (word.chars().count() == 1
                    && word.chars().next().is_some_and(char::is_alphabetic))
            {
                continue;
            }
        }
        let sentence: String = chars[start..=index]
            .iter()
            .collect::<String>()
            .trim()
            .to_owned();
        if !sentence.is_empty() {
            sentences.push(sentence);
        }
        start = index + 1;
    }
    let rest: String = chars[start..].iter().collect::<String>().trim().to_owned();
    if !rest.is_empty() {
        sentences.push(rest);
    }
    if sentences.is_empty() {
        vec![line.to_owned()]
    } else {
        sentences
    }
}

/// "Shopping list: eggs, milk and bread." -> ("Shopping list", ["eggs",
/// "milk", "bread"]): three or more short items (four words at most) after
/// a colon. English splits the last "and" or "or" off too; other languages
/// take commas only.
fn colon_list(line: &str, english: bool) -> Option<(String, Vec<String>)> {
    let colon = line.find(": ")?;
    let intro = line[..colon].trim().to_owned();
    let mut body = line[colon + 2..].trim().to_owned();
    if intro.is_empty() || body.contains(": ") {
        return None;
    }
    if matches!(body.chars().last(), Some('.' | '。')) {
        body.pop();
    }
    let mut items: Vec<String> = body
        .split(", ")
        .map(|item| item.trim().to_owned())
        .collect();
    if english && let Some(last) = items.pop() {
        let mut tail = last;
        for joiner in ["and ", "or "] {
            if tail.to_lowercase().starts_with(joiner) {
                tail = tail[joiner.len()..].to_owned();
            }
        }
        match tail.find(" and ").or_else(|| tail.find(" or ")) {
            Some(split) => {
                items.push(tail[..split].trim().to_owned());
                items.push(tail[split + 5..].trim().to_owned());
            }
            None => items.push(tail),
        }
    }
    let word_counts: Vec<usize> = items
        .iter()
        .map(|item| item.split_whitespace().count())
        .collect();
    let punctuation = items.iter().any(|item| {
        item.chars()
            .any(|character| ".!?。！？".contains(character))
    });
    (items.len() >= 3 && word_counts.iter().all(|count| (1..=4).contains(count)) && !punctuation)
        .then_some((intro, items))
}
