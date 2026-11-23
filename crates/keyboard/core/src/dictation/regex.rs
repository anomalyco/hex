//! The `NSRegularExpression` patterns the rules use, matched by hand with ICU's semantics:
//! `\s` is White_Space, `\p{L}`/`\p{N}` and `\b` read iOS's ICU sets, `^` is the start of
//! the text and `$` its end (or before a final line terminator, which `\s` covers wherever
//! `$` appears here), and case-insensitive letters fold only to their ASCII (or Ä, Ö, Ø)
//! pairs, the only folds the patterns' letters have.
//!
//! Every pattern is a fixed alternation of letter runs followed by a lookahead that refuses
//! a letter, so ICU's leftmost-first backtracking finds what a greedy run finds: giving a
//! letter back leaves a pattern letter next, which the lookahead refuses.

use super::unicode;

/// One letter repeated `min` to `max` times (`max` none: unbounded).
#[derive(Clone, Copy)]
struct Run {
    letter: char,
    min: usize,
    max: Option<usize>,
}

/// A filler pattern such as `um+|uh+|er+m*|ah+` as its alternatives.
fn alternatives(pattern: &str) -> Vec<Vec<Run>> {
    pattern
        .split('|')
        .map(|alternative| {
            let chars: Vec<char> = alternative.chars().collect();
            let mut runs = Vec::new();
            let mut index = 0;
            while index < chars.len() {
                let letter = chars[index];
                index += 1;
                let (min, max) = match chars.get(index) {
                    Some('+') => {
                        index += 1;
                        (1, None)
                    }
                    Some('*') => {
                        index += 1;
                        (0, None)
                    }
                    Some('{') => {
                        let close = chars[index..]
                            .iter()
                            .position(|&c| c == '}')
                            .map_or(chars.len(), |p| index + p);
                        let inside: String = chars[index + 1..close].iter().collect();
                        index = close + 1;
                        let (low, high) = inside.split_once(',').unwrap_or((&inside, &inside));
                        (low.parse().unwrap_or(1), high.parse().ok())
                    }
                    _ => (1, Some(1)),
                };
                runs.push(Run { letter, min, max });
            }
            runs
        })
        .collect()
}

/// Case-insensitive equality with a pattern's lowercase letter.
fn folds_to(c: char, letter: char) -> bool {
    if c == letter {
        return true;
    }
    let mut lower = c.to_lowercase();
    matches!((lower.next(), lower.next()), (Some(l), None) if l == letter) && c != 'İ'
}

/// Where the alternative's greedy runs end when they match at `at`.
fn runs_end(runs: &[Run], text: &[char], at: usize) -> Option<usize> {
    let mut position = at;
    for run in runs {
        let mut count = 0;
        while position < text.len()
            && run.max.is_none_or(|max| count < max)
            && folds_to(text[position], run.letter)
        {
            position += 1;
            count += 1;
        }
        if count < run.min {
            return None;
        }
    }
    Some(position)
}

fn is_space(c: char) -> bool {
    unicode::is_white_space(c)
}

/// `(?i)(?:^|(?<=[\s,]))(?:fillers)(?=(?:[\s,.!?]|$)),?\s*` replaced by " ".
pub(super) fn strip_fillers(text: &str, fillers: &str) -> String {
    let alternatives = alternatives(fillers);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut position = 0;
    while position < chars.len() {
        let starts = position == 0 || is_space(chars[position - 1]) || chars[position - 1] == ',';
        let found = starts
            .then(|| {
                alternatives.iter().find_map(|runs| {
                    let end = runs_end(runs, &chars, position)?;
                    let ok =
                        end == chars.len() || is_space(chars[end]) || ",.!?".contains(chars[end]);
                    ok.then_some(end)
                })
            })
            .flatten();
        match found {
            Some(mut end) => {
                if chars.get(end) == Some(&',') {
                    end += 1;
                }
                while end < chars.len() && is_space(chars[end]) {
                    end += 1;
                }
                out.push(' ');
                position = end;
            }
            None => {
                out.push(chars[position]);
                position += 1;
            }
        }
    }
    out
}

/// `^(?:fillers)$`, case-insensitive: the whole word is a hesitation sound.
pub(super) fn is_filler_word(word: &str, fillers: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    alternatives(fillers)
        .iter()
        .any(|runs| runs_end(runs, &chars, 0) == Some(chars.len()))
}

/// `(?<![\p{L}\p{N}@.'’])from(?=(?:['’](?:m|ll|ve|d))?(?![\p{L}\p{N}@'’]|\.\p{L}))` replaced by
/// `to`: the English pronoun, alone or in "I'm", "I'll", "I've", "I'd".
pub(super) fn recase_pronoun(text: &str, from: char, to: char) -> String {
    let chars: Vec<char> = text.chars().collect();
    let word = |c: char| {
        unicode::icu_letter(c) || unicode::icu_number(c) || c == '@' || c == '\'' || c == '’'
    };
    let free = |at: usize| match chars.get(at) {
        None => true,
        Some(&c) => {
            !word(c) && !(c == '.' && chars.get(at + 1).is_some_and(|&n| unicode::icu_letter(n)))
        }
    };
    let mut out = String::with_capacity(text.len());
    for (index, &c) in chars.iter().enumerate() {
        let matches = c == from
            && (index == 0 || {
                let before = chars[index - 1];
                !word(before) && before != '.'
            })
            && {
                let after = index + 1;
                let contraction = chars
                    .get(after)
                    .is_some_and(|&a| a == '\'' || a == '’')
                    .then(|| {
                        ["m", "ll", "ve", "d"].iter().find_map(|ending| {
                            let end = after + 1 + ending.len();
                            let same = end <= chars.len()
                                && chars[after + 1..end].iter().copied().eq(ending.chars());
                            same.then_some(end)
                        })
                    });
                matches!(contraction, Some(Some(end)) if free(end)) || free(after)
            };
        out.push(if matches { to } else { c });
    }
    out
}

/// `\s+([,.;:!?])(?!-?[()])` replaced by the mark, then `^[,;:]+\s*` removed.
pub(super) fn tighten_marks(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut position = 0;
    while position < chars.len() {
        if !is_space(chars[position]) {
            out.push(chars[position]);
            position += 1;
            continue;
        }
        let mut end = position;
        while end < chars.len() && is_space(chars[end]) {
            end += 1;
        }
        let mark = chars.get(end).copied().filter(|&m| ",.;:!?".contains(m));
        let emoticon = |at: usize| match chars.get(at) {
            Some('-') => chars.get(at + 1).is_some_and(|&c| c == '(' || c == ')'),
            Some(&c) => c == '(' || c == ')',
            None => false,
        };
        match mark {
            Some(m) if !emoticon(end + 1) => {
                out.push(m);
                position = end + 1;
            }
            _ => {
                out.extend(&chars[position..end]);
                position = end;
            }
        }
    }
    let leading = out.chars().take_while(|&c| ",;:".contains(c)).count();
    if leading == 0 {
        return out;
    }
    let rest: String = out
        .chars()
        .skip(leading)
        .skip_while(|&c| is_space(c))
        .collect();
    rest
}

/// ICU's `\b` (without `UREGEX_UWORD`): a change between its word set and the rest, looking
/// through Grapheme_Extend and Cf, never before one of those.
fn boundary(chars: &[char], at: usize) -> bool {
    let current = match chars.get(at) {
        Some(&c) if unicode::icu_extend(c) => return false,
        Some(&c) => unicode::icu_word(c),
        None => false,
    };
    let previous = chars[..at.min(chars.len())]
        .iter()
        .rev()
        .find(|&&c| !unicode::icu_extend(c))
        .is_some_and(|&c| unicode::icu_word(c));
    current != previous
}

/// Whether `phrase` (lowercase ASCII) is at `at`, case-insensitively.
fn phrase_at(chars: &[char], at: usize, phrase: &str) -> Option<usize> {
    let mut position = at;
    for p in phrase.chars() {
        let c = *chars.get(position)?;
        if c.to_ascii_lowercase() != p || !c.is_ascii() {
            return None;
        }
        position += 1;
    }
    Some(position)
}

fn skip_optional(chars: &[char], at: usize, set: &str) -> usize {
    let mut end = at;
    if chars.get(end).is_some_and(|&c| set.contains(c)) {
        end += 1;
    }
    while end < chars.len() && is_space(chars[end]) {
        end += 1;
    }
    end
}

/// `(?i)\bphrase\b[,.]?\s*` replaced by `with`.
pub(super) fn replace_spoken_break(text: &str, phrase: &str, with: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut position = 0;
    while position < chars.len() {
        let end = boundary(&chars, position)
            .then(|| phrase_at(&chars, position, phrase))
            .flatten()
            .filter(|&end| boundary(&chars, end));
        match end {
            Some(end) => {
                out.push_str(with);
                position = skip_optional(&chars, end, ",.");
            }
            None => {
                out.push(chars[position]);
                position += 1;
            }
        }
    }
    out
}

/// `(?i)(?:^|\s+)(?:bullet point|next bullet|bullet)\b[:,-]?\s*` replaced by "\n- ".
pub(super) fn replace_spoken_bullets(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let bullet_at = |at: usize| {
        ["bullet point", "next bullet", "bullet"]
            .iter()
            .find_map(|phrase| phrase_at(&chars, at, phrase).filter(|&end| boundary(&chars, end)))
    };
    let mut out = String::with_capacity(text.len());
    let mut position = 0;
    while position < chars.len() {
        let mut found = if position == 0 { bullet_at(0) } else { None };
        if found.is_none() && is_space(chars[position]) {
            let mut start = position;
            while start < chars.len() && is_space(chars[start]) {
                start += 1;
            }
            found = bullet_at(start);
        }
        match found {
            Some(end) => {
                out.push_str("\n- ");
                position = skip_optional(&chars, end, ":,-");
            }
            None => {
                out.push(chars[position]);
                position += 1;
            }
        }
    }
    out
}
