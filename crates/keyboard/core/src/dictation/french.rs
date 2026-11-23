//! `FrenchSpacing` (ios/HEXShared/FrenchSpacing.swift): French typography's spaces before
//! `? ! ; :` and inside `« »`, the dictation pipeline's last step in every style but Verbatim.
//! France: a narrow no-break space before `? ! ;`, a no-break space before `:` and inside
//! `« »`; Canada: none before `? ! ;`; Switzerland: narrow ones inside `« »` too. A mark is
//! spaced only where it ends a word, so times, URLs, addresses, emoticons and code stay.

use super::Language;
use super::text as t;

const NARROW_NO_BREAK_SPACE: &str = "\u{202F}";
const NO_BREAK_SPACE: &str = "\u{A0}";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Convention {
    France,
    Canada,
    Switzerland,
}

/// The convention for `language`, or none when it isn't French.
pub fn convention(language: Option<&Language>) -> Option<Convention> {
    let language = language?;
    if language.code.as_deref() != Some("fr") {
        return None;
    }
    Some(match language.region.as_deref() {
        Some("CA") => Convention::Canada,
        Some("CH") => Convention::Switzerland,
        _ => Convention::France,
    })
}

/// `text` with French spacing when `language` is French; otherwise unchanged.
pub fn french_spacing(text: &str, language: Option<&Language>) -> String {
    match convention(language) {
        Some(convention) => apply(text, convention),
        None => text.to_owned(),
    }
}

pub fn apply(text: &str, convention: Convention) -> String {
    let chars = t::chars(text);
    if !chars.iter().any(|c| t::is_one_of(c, "?!;:«»")) {
        return text.to_owned();
    }
    let before_question = if convention == Convention::Canada {
        None
    } else {
        Some(NARROW_NO_BREAK_SPACE)
    };
    let inside_guillemets = if convention == Convention::Switzerland {
        NARROW_NO_BREAK_SPACE
    } else {
        NO_BREAK_SPACE
    };

    let mut out: Vec<&str> = Vec::with_capacity(chars.len() + 8);
    // Spaces read but not yet written: a mark after them may replace them.
    let mut pending: Vec<&str> = Vec::new();
    let mut in_code = false;
    let mut after_opening_guillemet = false;

    for (index, &c) in chars.iter().enumerate() {
        let next = chars.get(index + 1).copied();
        if t::is(c, '`') {
            in_code = !in_code;
            after_opening_guillemet = false;
            out.append(&mut pending);
            out.push(c);
            continue;
        }
        if in_code {
            out.append(&mut pending);
            out.push(c);
            continue;
        }
        if is_horizontal_space(c) {
            pending.push(c);
            continue;
        }
        if after_opening_guillemet {
            after_opening_guillemet = false;
            if !t::is_newline(c) && !t::is(c, '»') {
                pending = vec![inside_guillemets];
            }
        }
        if t::is_one_of(c, "?!;:") {
            let space = if ends_word(c, next) {
                space_before(c, &out, &pending, before_question)
            } else {
                None
            };
            match space {
                Some(space) => {
                    pending.clear();
                    out.extend(space);
                }
                None => out.append(&mut pending),
            }
            out.push(c);
        } else if t::is(c, '«') {
            out.append(&mut pending);
            out.push(c);
            after_opening_guillemet = true;
        } else if t::is(c, '»') {
            if out
                .last()
                .is_some_and(|last| !t::is_newline(last) && !t::is(last, '«'))
            {
                pending = vec![inside_guillemets];
            }
            out.append(&mut pending);
            out.push(c);
        } else {
            out.append(&mut pending);
            out.push(c);
        }
    }
    if !after_opening_guillemet {
        out.append(&mut pending);
    }
    t::join(&out)
}

fn is_horizontal_space(c: &str) -> bool {
    t::is(c, ' ')
        || t::is(c, '\t')
        || t::is(c, '\u{A0}')
        || t::is(c, '\u{202F}')
        || t::is(c, '\u{2009}')
}

/// Whether `mark` ends a word here, rather than sitting inside a time, a URL or an emoticon.
fn ends_word(mark: &str, next: Option<&str>) -> bool {
    let Some(next) = next else { return true };
    if (t::is(mark, ':') || t::is(mark, ';')) && t::is_one_of(next, ")(][|") {
        return false;
    }
    t::is_whitespace(next) || t::is_one_of(next, "?!.…,»”\"’')]}")
}

/// The space to write before `mark` (`Some(None)` for none), or none to leave what is there.
fn space_before(
    mark: &str,
    out: &[&str],
    pending: &[&str],
    question: Option<&'static str>,
) -> Option<Option<&'static str>> {
    let last = *out.last()?;
    if t::is_newline(last) || t::is_one_of(last, "(«“[{") {
        return None;
    }
    if t::is_one_of(mark, "?!") && t::is_one_of(last, "?!") {
        return Some(None);
    }
    if t::is_one_of(last, ";:") {
        return None;
    }
    if pending.is_empty() {
        let start = out
            .iter()
            .rposition(|c| t::is_whitespace(c))
            .map_or(0, |p| p + 1);
        let word = t::join(&out[start..]);
        if t::contains(&word, "://")
            || t::contains(&word, "@")
            || t::has_prefix(&t::lowercased(&word), "www.")
        {
            return None;
        }
    }
    Some(if t::is(mark, ':') {
        Some(NO_BREAK_SPACE)
    } else {
        question
    })
}
