//! `SpokenCleanupRules.spokenSymbols` and `spokenCounts` (SpokenCleanupRules.swift): English
//! "at the rate" and "at sign" as "@" (joining an address said around them), and a spoken
//! count ("one, two, three", "test 123") in digits. Both run in every style but Verbatim,
//! with Clean up what I say on or off.

use super::cleanup::{CLOSING_MARKS, bare, ends_sentence};
use super::{Language, is_english, text as t};

struct Token<'a> {
    word: &'a str,
    /// Byte offsets in the text.
    start: usize,
    end: usize,
}

/// The text's whitespace-separated words and where they are.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;
    for (index, c) in unicode_segmentation::UnicodeSegmentation::grapheme_indices(text, true) {
        if t::is_whitespace(c) {
            if let Some(begin) = start.take() {
                tokens.push(Token {
                    word: &text[begin..index],
                    start: begin,
                    end: index,
                });
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(begin) = start {
        tokens.push(Token {
            word: &text[begin..],
            start: begin,
            end: text.len(),
        });
    }
    tokens
}

fn lower(token: Option<&Token>) -> Option<String> {
    token.map(|token| t::lowercased(token.word))
}

fn is(word: Option<String>, wanted: &str) -> bool {
    word.is_some_and(|word| t::same(&word, wanted))
}

/// `word` without the closing marks after it.
fn core(word: &str) -> &str {
    let chars = t::chars(word);
    let mut end = chars.len();
    while end > 0 && t::is_one_of(chars[end - 1], CLOSING_MARKS) {
        end -= 1;
    }
    let bytes: usize = chars[..end].iter().map(|c| c.len()).sum();
    &word[..bytes]
}

/// Two addresses said back to back, the second's local part the first's domain ("a at sign
/// b.com at sign c.com"): their edits overlap, where Swift's `spokenSymbols` traps
/// (`text[cursor..<start]` with the cursor past the start). What to write instead is the
/// owner's decision; until then the iOS wrapper traps as before.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OverlappingAddresses;

/// English `text` with "@" for its spoken names, or `text` itself; an error where the Swift
/// original traps.
pub fn spoken_symbols(
    text: &str,
    language: Option<&Language>,
) -> Result<String, OverlappingAddresses> {
    if !is_english(language) || !t::lowercased(text).contains("at") {
        return Ok(text.to_owned());
    }
    let tokens = splitting_glued_domains(tokens(text));
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let Some((marker_end, ambiguous)) = symbol_marker(&tokens, index) else {
            index += 1;
            continue;
        };
        let next = marker_end + 1;
        if ambiguous && next < tokens.len() && t::same(&bare(tokens[next].word), "of") {
            index = next;
            continue;
        }
        if let Some((labels, domain_end, trailing)) = address_domain(&tokens, next) {
            let mut start = index;
            let mut local: Vec<&str> = Vec::new();
            if let Some(part) = index
                .checked_sub(1)
                .and_then(|i| local_part(tokens[i].word))
            {
                local.push(part);
                start = index - 1;
                // "john dot smith at the rate…": the parts before join with dots too.
                while start >= 2 && is(lower(tokens.get(start - 1)), "dot") {
                    let Some(part) = local_part(tokens[start - 2].word) else {
                        break;
                    };
                    local.insert(0, part);
                    start -= 2;
                }
            }
            let address = t::lowercased(&format!("{}@{}", local.join("."), labels.join(".")));
            edits.push((
                tokens[start].start,
                tokens[domain_end].end,
                address + trailing,
            ));
            index = domain_end + 1;
        } else if !ambiguous
            && !t::same(
                &bare(tokens.get(next).map_or("", |token| token.word)),
                "language",
            )
        {
            // "good at sign language" is no symbol.
            let last = t::chars(tokens[marker_end].word);
            let keep = last
                .iter()
                .rev()
                .take_while(|c| t::is_one_of(c, CLOSING_MARKS))
                .count();
            let trailing = t::join(&last[last.len() - keep..]);
            edits.push((
                tokens[index].start,
                tokens[marker_end].end,
                format!("@{trailing}"),
            ));
            index = marker_end + 1;
        } else {
            index = marker_end + 1;
        }
    }
    splice(text, &edits)
}

/// The edits in order, or an error for an edit that starts inside the one before.
fn splice(text: &str, edits: &[(usize, usize, String)]) -> Result<String, OverlappingAddresses> {
    if edits.is_empty() {
        return Ok(text.to_owned());
    }
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end, replacement) in edits {
        if *start < cursor {
            return Err(OverlappingAddresses);
        }
        result.push_str(&text[cursor..*start]);
        result.push_str(replacement);
        cursor = *end;
    }
    result.push_str(&text[cursor..]);
    Ok(result)
}

/// Apple's `SpeechTranscriber` glues a spoken domain to the word before it ("John at the
/// rategmail.com."): this splits "rate" or "sign" off again.
fn splitting_glued_domains(tokens: Vec<Token<'_>>) -> Vec<Token<'_>> {
    let mut result = Vec::with_capacity(tokens.len());
    for (index, token) in tokens.iter().enumerate() {
        let lowered = t::lowercased(token.word);
        let before = index.checked_sub(1).map(|i| t::lowercased(tokens[i].word));
        let prefix = if is(before.clone(), "the")
            && is(
                index.checked_sub(2).map(|i| t::lowercased(tokens[i].word)),
                "at",
            )
            && t::has_prefix(&lowered, "rate")
        {
            Some("rate")
        } else if is(before, "at") && t::has_prefix(&lowered, "sign") {
            Some("sign")
        } else {
            None
        };
        let cut = prefix.map(|p| t::prefix(token.word, t::count(p)).len());
        match cut {
            Some(cut) if is_domain(&token.word[cut..]) => {
                result.push(Token {
                    word: &token.word[..cut],
                    start: token.start,
                    end: token.start + cut,
                });
                result.push(Token {
                    word: &token.word[cut..],
                    start: token.start + cut,
                    end: token.end,
                });
            }
            _ => result.push(Token {
                word: token.word,
                start: token.start,
                end: token.end,
            }),
        }
    }
    result
}

/// A written domain, maybe with punctuation after it: "gmail.com.", "co.in".
fn is_domain(word: &str) -> bool {
    let labels = t::split_on(core(word), '.', true);
    let Some(top) = labels.last() else {
        return false;
    };
    labels.len() >= 2
        && t::count(top) >= 2
        && t::chars(top).iter().all(|c| t::is_letter(c))
        && labels.iter().all(|label| is_domain_label(label))
}

/// "at the rate" (ambiguous) or "at sign" / "at-sign" at `index`, and where it ends.
fn symbol_marker(tokens: &[Token], index: usize) -> Option<(usize, bool)> {
    let word = |offset: usize| lower(tokens.get(index + offset));
    if t::same(&bare(tokens[index].word), "at-sign") {
        return Some((index, false));
    }
    if !is(word(0), "at") {
        return None;
    }
    if word(1).is_some_and(|sign| t::same(&bare(&sign), "sign")) {
        return Some((index + 1, false));
    }
    if is(word(1), "the") && word(2).is_some_and(|rate| t::same(&bare(&rate), "rate")) {
        return Some((index + 2, true));
    }
    None
}

/// A domain spoken from `index`: labels joined by "dot" or already by ".", at least two,
/// ending in a top-level domain of letters. Its labels, last token and closing marks.
fn address_domain<'a>(
    tokens: &[Token<'a>],
    index: usize,
) -> Option<(Vec<&'a str>, usize, &'a str)> {
    let mut labels: Vec<&str> = Vec::new();
    let mut position = index;
    let mut trailing = "";
    while let Some(token) = tokens.get(position) {
        let core = core(token.word);
        let parts = t::split_on(core, '.', true);
        if core.is_empty() || !parts.iter().all(|part| is_domain_label(part)) {
            return None;
        }
        labels.extend(parts);
        trailing = &token.word[core.len()..];
        let continues = trailing.is_empty()
            && is(lower(tokens.get(position + 1)), "dot")
            && tokens.get(position + 2).is_some_and(|following| {
                is_domain_label(
                    t::split_on(core_owned(following.word).as_str(), '.', false)
                        .first()
                        .copied()
                        .unwrap_or(""),
                )
            });
        if !continues {
            break;
        }
        position += 2;
    }
    let top = labels.last()?;
    if labels.len() < 2 || t::count(top) < 2 || !t::chars(top).iter().all(|c| t::is_letter(c)) {
        return None;
    }
    Some((labels, position, trailing))
}

fn core_owned(word: &str) -> String {
    core(word).to_owned()
}

fn is_domain_label(label: &str) -> bool {
    let chars = t::chars(label);
    !chars.is_empty()
        && chars.len() <= 63
        && !t::is(chars[0], '-')
        && !t::is(chars[chars.len() - 1], '-')
        && chars
            .iter()
            .all(|c| (t::is_letter_or_number(c) || t::is(c, '-')) && t::is_ascii(c))
}

/// The word before "@" when it can start an address.
fn local_part(word: &str) -> Option<&str> {
    let chars = t::chars(word);
    let ok = !chars.is_empty()
        && t::is_letter_or_number(chars[chars.len() - 1])
        && t::is_letter_or_number(chars[0])
        && chars
            .iter()
            .all(|c| (t::is_letter_or_number(c) || t::is_one_of(c, "._-+")) && t::is_ascii(c));
    ok.then_some(word)
}

const COUNT_WORDS: [&str; 20] = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
];

/// English `text` with a spoken count in digits, or `text` itself.
pub fn spoken_counts(text: &str, language: Option<&Language>) -> String {
    if !is_english(language) {
        return text.to_owned();
    }
    let tokens = tokens(text);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if let Some(digits) = joined_count(&tokens, index) {
            edits.push((tokens[index].start, tokens[index].end, digits));
            index += 1;
            continue;
        }
        let mut count: Vec<(usize, &str)> = Vec::new();
        while let Some(number) = tokens
            .get(index + count.len())
            .and_then(|token| count_number(token.word))
        {
            if number.0 != count.len() + 1 {
                break;
            }
            count.push(number);
            // Only a comma or a space comes between two numbers of a count.
            if !number.1.is_empty() && !t::same(number.1, ",") {
                break;
            }
        }
        // "zero, one, two, three" and "twenty one two three" are no count from one.
        let previous = index
            .checked_sub(1)
            .map_or(String::new(), |i| bare(tokens[i].word));
        let follows_number = count_number(&previous).is_some()
            || t::same(&previous, "zero")
            || (!previous.is_empty() && t::chars(&previous).iter().all(|c| t::is_number(c)));
        if count.len() < 3 || follows_number {
            index += 1;
            continue;
        }
        let last = index + count.len() - 1;
        let written: Vec<String> = count
            .iter()
            .enumerate()
            .map(|(offset, (value, trailing))| {
                format!(
                    "{value}{}",
                    if offset == count.len() - 1 {
                        trailing
                    } else {
                        ","
                    }
                )
            })
            .collect();
        edits.push((tokens[index].start, tokens[last].end, written.join(" ")));
        index = last + 1;
    }
    // A count's edits never overlap: each starts after the one before ends.
    splice(text, &edits).unwrap_or_else(|_| text.to_owned())
}

/// A count's number and the marks after it: one of `COUNT_WORDS`, or 1 to 20 in digits.
fn count_number(word: &str) -> Option<(usize, &str)> {
    let core = core(word);
    let trailing = &word[core.len()..];
    let lowered = t::lowercased(core);
    if let Some(position) = COUNT_WORDS.iter().position(|w| t::same(w, &lowered)) {
        return Some((position + 1, trailing));
    }
    let chars = t::chars(core);
    if chars.len() > 2 || chars.first().is_some_and(|c| t::is(c, '0')) {
        return None;
    }
    if !chars.iter().all(|c| t::is_ascii(c) && t::is_number(c)) {
        return None;
    }
    core.parse().ok().map(|value| (value, trailing))
}

/// "123." after "test" or "testing" as "1, 2, 3.", when nothing but its marks or the end
/// follows it.
fn joined_count(tokens: &[Token], index: usize) -> Option<String> {
    let before = tokens.get(index.checked_sub(1)?)?;
    let lead = bare(before.word);
    if !(t::same(&lead, "test") || t::same(&lead, "testing")) || ends_sentence(before.word) {
        return None;
    }
    let word = tokens[index].word;
    let core = core(word);
    let trailing = &word[core.len()..];
    if t::count(core) < 3
        || !t::has_prefix("123456789", core)
        || (trailing.is_empty() && index != tokens.len() - 1)
    {
        return None;
    }
    let digits: Vec<&str> = t::chars(core);
    Some(digits.join(", ") + trailing)
}
