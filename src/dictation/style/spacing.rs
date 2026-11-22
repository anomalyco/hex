//! Spacing, punctuation, and capitalization shared by the four styles: the
//! iPhone app's rules, without its speech-only or insertion handling.

fn is_horizontal(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Each line without runs of spaces or tabs, and without the spaces at its
/// ends; the lines stay separate.
pub(super) fn collapse_horizontal_spaces(text: &str) -> String {
    let lines = text
        .split('\n')
        .map(|line| {
            let mut collapsed = String::with_capacity(line.len());
            let mut after_space = false;
            for character in line.chars() {
                if is_horizontal(character) {
                    if !after_space {
                        collapsed.push(' ');
                    }
                    after_space = true;
                } else {
                    collapsed.push(character);
                    after_space = false;
                }
            }
            collapsed.trim_matches(is_horizontal).to_owned()
        })
        .collect::<Vec<_>>();
    lines.join("\n").trim().to_owned()
}

/// The text without whitespace before `, . ; : ! ?`, and without a leading
/// run of `, ; :`. Whitespace before an emoticon such as `:-)` stays.
pub(super) fn fix_spacing_before_punctuation(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut tightened = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_whitespace() {
            let mut end = index;
            while chars.get(end).is_some_and(|c| c.is_whitespace()) {
                end += 1;
            }
            if chars
                .get(end)
                .is_some_and(|&next| matches!(next, ',' | '.' | ';' | ':' | '!' | '?'))
                && !is_emoticon(&chars, end)
            {
                index = end;
                continue;
            }
            tightened.extend(chars[index..end].iter());
            index = end;
            continue;
        }
        tightened.push(chars[index]);
        index += 1;
    }
    let mut start = 0;
    if tightened
        .first()
        .is_some_and(|&character| matches!(character, ',' | ';' | ':'))
    {
        while tightened
            .get(start)
            .is_some_and(|&character| matches!(character, ',' | ';' | ':'))
        {
            start += 1;
        }
        while tightened.get(start).is_some_and(|c| c.is_whitespace()) {
            start += 1;
        }
    }
    tightened[start..]
        .iter()
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Whether the punctuation at `index` starts an emoticon, where the space
/// before it stays: `:-)`, `;)`.
fn is_emoticon(chars: &[char], index: usize) -> bool {
    match chars.get(index + 1) {
        Some('(' | ')') => true,
        Some('-') => chars
            .get(index + 2)
            .is_some_and(|&character| matches!(character, '(' | ')')),
        _ => false,
    }
}

/// A full stop unless `line` already ends a sentence (or a list's colon),
/// is empty, or is a lone address ("john@gmail.com"), which is no sentence
/// to end.
pub(super) fn ending_sentence(line: &str) -> String {
    let Some(last) = line.chars().last() else {
        return line.to_owned();
    };
    if sentence_closers().contains(&last)
        || (line.contains('@') && !line.chars().any(char::is_whitespace))
    {
        return line.to_owned();
    }
    let stop = if uses_ideographic_full_stop(last) {
        '。'
    } else {
        '.'
    };
    format!("{line}{stop}")
}

/// What may end a sentence already: its mark, or a closing quote or bracket
/// after one. A line ending in a comma or semicolon ("Hi Sarah,") is no
/// sentence.
fn sentence_closers() -> &'static [char] {
    &[
        '.', '!', '?', '…', '"', '”', ')', ']', ':', ',', ';', '。', '！', '？', '」', '』', '）',
    ]
}

/// Marks that end a sentence. A full stop counts only before a space, a line
/// break, or the end ("gmail.com" and "e.g." end nothing); the ideographic
/// ones count anywhere.
pub(super) fn ends_sentence(chars: &[char], index: usize) -> bool {
    let Some(&character) = chars.get(index) else {
        return false;
    };
    if "。！？".contains(character) {
        return true;
    }
    if !".!?…".contains(character) {
        return false;
    }
    chars.get(index + 1).is_none_or(|next| next.is_whitespace())
}

/// Chinese and Japanese end a sentence with "。", not ".".
fn uses_ideographic_full_stop(character: char) -> bool {
    let scalar = character as u32;
    (0x4E00..=0x9FFF).contains(&scalar)
        || (0x3400..=0x4DBF).contains(&scalar)
        || (0x3040..=0x30FF).contains(&scalar)
        || (0xF900..=0xFAFF).contains(&scalar)
}

/// The first letter of `word` capitalized, the rest as given.
pub(super) fn capitalized_lower(word: &str) -> String {
    let mut capitalized = String::with_capacity(word.len());
    if let Some(first) = word.chars().next() {
        capitalized.extend(first.to_uppercase());
        capitalized.push_str(&word[first.len_utf8()..]);
    }
    capitalized
}

/// Whether the text after `before` starts a sentence: none (or only spaces),
/// a line break, or `.` `!` `?` `…` (closing quotes or brackets after them
/// count too).
fn starts_sentence(before: &str) -> bool {
    for character in before.chars().rev() {
        if character == '\n' {
            return true;
        }
        if character.is_whitespace() || "\"”’')]»".contains(character) {
            continue;
        }
        return ".!?…。".contains(character);
    }
    true
}

/// Whether the next word starts a sentence, or a bulleted item ("- ").
pub(super) fn starts_sentence_or_item(after: &str) -> bool {
    if starts_sentence(after) {
        return true;
    }
    let line = after.split('\n').next_back().unwrap_or_default();
    let marker = line.trim_matches(char::is_whitespace);
    marker == "-" || marker == "•"
}

/// English "i", "i'm", "i'll", "i've", "i'd" as `to`, never "i.e." or an
/// address.
pub(super) fn recasing_pronoun_i(text: &str, from: char, to: char) -> String {
    if !text.contains(from) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = chars.clone();
    for index in 0..chars.len() {
        if chars[index] != from {
            continue;
        }
        let before_ok = index == 0
            || !(chars[index - 1].is_alphabetic()
                || chars[index - 1].is_numeric()
                || matches!(chars[index - 1], '@' | '.' | '\'' | '’'));
        if !before_ok {
            continue;
        }
        let mut after = index + 1;
        if matches!(chars.get(after), Some('\'' | '’')) {
            let suffix: String = chars[after + 1..].iter().take(2).collect();
            let length = if suffix.starts_with('m') || suffix.starts_with('d') {
                1
            } else if suffix.starts_with("ll") || suffix.starts_with("ve") {
                2
            } else {
                // A bare apostrophe is in Swift's forbidden lookahead set, so
                // "i'" is no pronoun to recase.
                continue;
            };
            after += 1 + length;
        }
        let after_ok = chars.get(after).is_none_or(|&next| {
            !(next.is_alphabetic() || next.is_numeric() || matches!(next, '@' | '\'' | '’'))
                && !(next == '.' && chars.get(after + 1).is_some_and(|c| c.is_alphabetic()))
        });
        if after_ok {
            out[index] = to;
        }
    }
    out.into_iter().collect()
}

/// Capitalizes the first letter of each sentence: at the start, after a line
/// break, or after ".", "!" or "?" and a space. A full stop inside a word
/// ("gmail.com", "e.g.") starts nothing, and an address keeps its case.
pub(super) fn capitalize_sentence_starts(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let mut capitalize_next = true;
    let mut ended_sentence = false;
    for index in 0..chars.len() {
        let character = chars[index];
        if capitalize_next && character.is_alphabetic() {
            if !is_address(&chars, index)
                && let Some(upper) = character.to_uppercase().next()
            {
                chars[index] = upper;
            }
            capitalize_next = false;
        } else if character == '\n' {
            capitalize_next = true;
            ended_sentence = false;
        } else if ".!?".contains(character) {
            ended_sentence = true;
            capitalize_next = false;
        } else if character.is_whitespace() {
            if ended_sentence {
                capitalize_next = true;
                ended_sentence = false;
            }
        } else if "\"”’')]".contains(character) && ended_sentence {
            continue;
        } else if !"\"“(¿¡".contains(character) {
            capitalize_next = false;
            ended_sentence = false;
        }
    }
    chars.into_iter().collect()
}

/// Lowercases the first letter of each sentence, as people text. Keeps
/// acronyms ("NASA", though "OK" becomes "ok"), words with capitals inside
/// ("iPhone", "McDonald's"), and addresses.
pub(super) fn lowercasing_sentence_starts(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let mut at_start = true;
    let mut index = 0;
    while index < chars.len() {
        // A list item ("1. Eggs", "- Eggs") keeps its capital.
        if (index == 0 || chars[index - 1] == '\n')
            && let Some(marker) = list_marker_length(&chars, index)
        {
            index += marker;
            at_start = false;
            continue;
        }
        let character = chars[index];
        if at_start && character.is_alphabetic() {
            let mut end = index;
            while end < chars.len()
                && (chars[end].is_alphabetic() || matches!(chars[end], '\'' | '’'))
            {
                end += 1;
            }
            let word: String = chars[index..end].iter().collect();
            if !keeps_capital(&word) && !is_address(&chars, index) {
                let lowered = if word == "OK" {
                    "ok".to_owned()
                } else {
                    lowercase_first(&word)
                };
                let length = lowered.chars().count();
                chars.splice(index..end, lowered.chars());
                index += length;
            } else {
                index = end;
            }
            at_start = false;
            continue;
        }
        if character == '\n' || ends_sentence(&chars, index) {
            at_start = true;
        } else if !character.is_whitespace() && !"\"“'‘(¿¡”’)]".contains(character) {
            at_start = false;
        }
        index += 1;
    }
    chars.into_iter().collect()
}

fn lowercase_first(word: &str) -> String {
    let mut lowered = String::with_capacity(word.len());
    if let Some(first) = word.chars().next() {
        lowered.extend(first.to_lowercase());
        lowered.push_str(&word[first.len_utf8()..]);
    }
    lowered
}

/// The length of a list marker ("1. ", "- ") at `index`, if one starts
/// there.
fn list_marker_length(chars: &[char], index: usize) -> Option<usize> {
    if chars
        .get(index)
        .is_some_and(|&character| matches!(character, '-' | '•'))
        && chars.get(index + 1) == Some(&' ')
    {
        return Some(2);
    }
    let mut end = index;
    while end < chars.len() && end - index < 2 && chars[end].is_ascii_digit() {
        end += 1;
    }
    if end > index && chars.get(end) == Some(&'.') && chars.get(end + 1) == Some(&' ') {
        return Some(end + 2 - index);
    }
    None
}

fn keeps_capital(word: &str) -> bool {
    if word == "OK" {
        return false;
    }
    let letters: String = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.chars().count() >= 2 && letters.to_uppercase() == letters {
        return true;
    }
    letters.chars().skip(1).any(|c| c.is_uppercase())
}

/// Whether the word at `index` is an address ("john@gmail.com"), whose case
/// stays.
fn is_address(chars: &[char], index: usize) -> bool {
    let mut end = index;
    while end < chars.len() && !chars[end].is_whitespace() {
        if chars[end] == '@' {
            return true;
        }
        end += 1;
    }
    false
}

/// Capitalizes `text`'s first letter, unless it starts an address
/// ("john@gmail.com"), whose case stays.
pub(super) fn capitalize_first_letter(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let Some(index) = chars.iter().position(|character| character.is_alphabetic()) else {
        return text.to_owned();
    };
    if is_address(&chars, index) {
        return text.to_owned();
    }
    let mut capitalized = String::with_capacity(text.len());
    capitalized.extend(&chars[..index]);
    capitalized.extend(chars[index].to_uppercase());
    capitalized.extend(&chars[index + 1..]);
    capitalized
}
