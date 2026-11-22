//! Reading the text before the cursor (the word being typed, the word before it, a
//! sentence's start) and casing a fix like the word typed: `AutocorrectEngine`'s static
//! helpers.

use super::language::Language;
use super::text;

/// The word right before the caret (`currentToken(in:)`): letters, with apostrophes between
/// letters; `None` inside URLs, handles, paths or numbers.
pub fn current_token(before_input: Option<&str>) -> Option<String> {
    let before = before_input?;
    if before.is_empty() {
        return None;
    }
    let chars = text::chars(before);
    let end = chars.len();
    let mut start = end;
    while start > 0 {
        let prev = start - 1;
        let ch = chars[prev];
        if text::is_letter(ch)
            || (ch == "'" && start < end && prev > 0 && text::is_letter(chars[prev - 1]))
        {
            start = prev;
        } else {
            break;
        }
    }
    if start >= end {
        return None;
    }
    if start > 0 {
        let boundary = chars[start - 1];
        if ["@", "/", ".", "#", "_", "-", ":"].contains(&boundary) || text::is_number(boundary) {
            return None;
        }
    }
    let token: String = chars[start..end].concat();
    text::is_letter(text::first(&token)).then_some(token)
}

/// The empty field, or after a sentence's end and a space, or a new line (`isSentenceStart`).
pub fn is_sentence_start(before_input: Option<&str>, language: Language) -> bool {
    let Some(before) = before_input else {
        return true;
    };
    let last = text::last(before);
    if last.is_empty() {
        return true;
    }
    if text::is_newline(last) {
        return true;
    }
    if last != " " {
        return false;
    }
    let trimmed = before.trim_end_matches(' ');
    let written = text::last(trimmed);
    if written.is_empty() {
        return true;
    }
    text::is_newline(written)
        || written == "."
        || written == "!"
        || written == "?"
        || (language == Language::Greek && text::eq(written, ";"))
}

/// Marks after which a sentence goes on (`midSentenceMarks`).
pub fn is_mid_sentence_mark(c: &str) -> bool {
    [
        ",", ";", ":", ")", "\"", "'", "\u{201D}", "\u{2019}", "\u{2014}",
    ]
    .iter()
    .any(|m| text::eq(c, m))
}

/// The word before `token` in `before`, lowercased, when only spaces separate them
/// (`previousWord`).
pub fn previous_word(before: &str, token: &str) -> Option<String> {
    let rest = if text::has_suffix(before, token) {
        text::drop_last(before, text::count(token))
    } else {
        before
    };
    if text::last(rest) != " " {
        return None;
    }
    let rest = rest.trim_end_matches(' ');
    let chars = text::chars(rest);
    let mut start = chars.len();
    while start > 0 && (text::is_letter(chars[start - 1]) || chars[start - 1] == "'") {
        start -= 1;
    }
    let word: String = chars[start..].concat();
    (!word.is_empty()).then(|| text::lowercased(&word))
}

/// Whether `token`, at the end of `before`, starts a sentence (`startsSentence`).
pub fn starts_sentence(before: &str, token: &str) -> bool {
    let rest = if text::has_suffix(before, token) {
        text::drop_last(before, text::count(token))
    } else {
        before
    };
    should_capitalize_sentences(Some(rest), true, Language::English)
}

/// `AutoCapitalization.shouldCapitalize(mode: .sentences, …)`.
pub fn should_capitalize_sentences(
    before: Option<&str>,
    has_text: bool,
    language: Language,
) -> bool {
    let Some(before) = before else {
        return !has_text;
    };
    let last = text::last(before);
    if last.is_empty() || text::is_newline(last) {
        return true;
    }
    if !text::is_whitespace(last) {
        return false;
    }
    let mut chars = text::chars(before);
    while let Some(&c) = chars.last() {
        if text::is_whitespace(c) && !text::is_newline(c) {
            chars.pop();
        } else {
            break;
        }
    }
    let Some(&last_written) = chars.last() else {
        return true;
    };
    if text::is_newline(last_written) {
        return true;
    }
    const CLOSERS: [&str; 6] = ["\"", "'", ")", "]", "\u{201D}", "\u{2019}"];
    while let Some(&c) = chars.last() {
        if CLOSERS.iter().any(|m| text::eq(c, m)) {
            chars.pop();
        } else {
            break;
        }
    }
    let Some(&ender) = chars.last() else {
        return false;
    };
    [".", "!", "?"].iter().any(|m| text::eq(ender, m))
        || (language == Language::Greek && (text::eq(ender, ";") || text::eq(ender, "\u{37E}")))
}

/// `target` cased as `typed` was typed (`matchCase`).
pub fn match_case(typed: &str, target: &str) -> String {
    if typed.is_empty() || target.is_empty() {
        return target.to_owned();
    }
    if text::count(typed) >= 2
        && text::eq(typed, &text::uppercased(typed))
        && !text::eq(typed, &text::lowercased(typed))
    {
        return text::uppercased(target);
    }
    let typed_capital = text::is_uppercase(text::first(typed));
    if !text::eq(target, &text::lowercased(target)) {
        if typed_capital && text::is_lowercase(text::first(target)) {
            return text::capitalised_first(target);
        }
        return target.to_owned();
    }
    if typed_capital {
        return text::capitalised_first(target);
    }
    target.to_owned()
}

/// Whether `token` has a vowel, accents aside (`hasVowel`).
pub fn has_vowel(token: &str) -> bool {
    let folded = text::without_diacritics(&text::lowercased(token));
    text::chars(&folded).iter().any(|c| {
        ["a", "e", "i", "o", "u", "y"]
            .iter()
            .any(|v| text::eq(c, v))
    })
}

/// The words a vocabulary adds: each term, and each word of a phrase (`vocabularyWords`).
pub fn vocabulary_words<S: AsRef<str>>(terms: &[S]) -> Vec<String> {
    let mut words = Vec::new();
    for term in terms {
        let term = term.as_ref();
        words.push(term.to_owned());
        if text::contains_char(term, " ") {
            words.extend(text::split_on(term, " ").into_iter().map(str::to_owned));
        }
    }
    words
}

/// What space puts in for `token`, a vocabulary word (`vocabularySpelling`).
pub fn vocabulary_spelling(token: &str, term: &str) -> Option<String> {
    let spelled = match_case(token, term);
    (!text::eq(&spelled, token)).then_some(spelled)
}
