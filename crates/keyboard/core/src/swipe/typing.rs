//! What a swipe types: the space before the word, the word cased as the sentence and your
//! words need, the strip's other readings, the rare names a reading ranks lower, and whether a
//! swipe was a tap that slid. Ported from iOS's `FullKeyboardEngine+SwipeTyping.swift` and
//! `SwipeTyping.swift`, so both platforms type a swipe alike.
//!
//! The host keeps what only it can know and hands it in: whether the dictionary knows a word
//! (`is_word_valid`, iOS's `UITextChecker`), your text replacements and words
//! ([`SwipeWords`]), and the field, shift and auto-capitalisation state with the text before
//! the cursor ([`SwipeContext`]). Which readings may be offered at all (offensive words) is the
//! host's filter between [`SwipeDecoder::ranked`] and [`typed`].

use std::collections::{HashMap, HashSet};

use super::characters::{
    characters, is_letter, is_lowercase, is_newline, is_number, is_uppercase, is_whitespace, key,
    lowercased, same, uppercased,
};
use super::{Candidate, SwipeDecoder};

/// Your text replacements and words, as autocorrect has them: made once, and again whenever
/// one of them changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SwipeWords {
    /// German keeps a capital a word has (`withoutStrayCapital`).
    german: bool,
    /// English's "I" and its contractions keep their capital (`usesEnglishFixes`).
    english: bool,
    has_dictionary: bool,
    /// Shortcut (lowercased, as a Swift key) to replacement.
    replacements: HashMap<String, String>,
    /// The replacements, as Swift keys: your own spellings too.
    outputs: HashSet<String>,
    own: HashSet<String>,
    vocabulary: HashSet<String>,
}

impl SwipeWords {
    /// `language` is the layout's ("en", "de"…): German keeps a capital a word has, and in
    /// English "I" and its contractions keep theirs. `has_dictionary` is whether the host has
    /// one for it (else `is_word_valid` is never asked). `replacements` are (shortcut,
    /// replacement) with the shortcut lowercased; `own` your other own spellings (lexicon
    /// terms, words you kept, fixes Hex learned); `vocabulary` your vocabulary's words as
    /// you added them.
    pub fn new(
        language: &str,
        has_dictionary: bool,
        replacements: Vec<(String, String)>,
        own: Vec<String>,
        vocabulary: Vec<String>,
    ) -> Self {
        Self {
            german: language == "de",
            english: language == "en",
            has_dictionary,
            outputs: replacements.iter().map(|(_, r)| key(r)).collect(),
            replacements: replacements
                .into_iter()
                .map(|(shortcut, replacement)| (key(&shortcut), replacement))
                .collect(),
            own: own.iter().map(|w| key(w)).collect(),
            vocabulary: vocabulary.iter().map(|w| key(w)).collect(),
        }
    }

    /// `AutocorrectEngine.withoutStrayCapital(word, typed: "")`: `word` without a capital first
    /// letter it only has from where it was learned (a Contacts name that is also a common
    /// word, "Bell"), when the dictionary knows it in lowercase.
    fn without_stray_capital(
        &self,
        word: &str,
        is_word_valid: &mut dyn FnMut(&str) -> bool,
    ) -> String {
        let chars = characters(word);
        let keeps = self.german
            || !self.has_dictionary
            || chars.len() < 2
            || !is_uppercase(chars[0])
            || chars[1..].iter().any(|c| is_uppercase(c))
            || chars.iter().any(|c| same(c, " "))
            || self.vocabulary.contains(&key(word));
        if keeps {
            return word.to_owned();
        }
        let lower = lowercased(chars[0]) + &chars[1..].concat();
        let lower_chars = characters(&lower);
        let starts = |second: &str| {
            lower_chars.len() >= 2 && same(lower_chars[0], "i") && same(lower_chars[1], second)
        };
        if self.english && (starts("'") || starts("\u{2019}")) {
            return word.to_owned();
        }
        if is_word_valid(&lower) {
            lower
        } else {
            word.to_owned()
        }
    }

    /// `AutocorrectEngine.isOwnSpelling`: a text replacement's output, or one of your other
    /// words spelled so (unless that spelling's capital is a stray one).
    fn is_own_spelling(&self, word: &str, is_word_valid: &mut dyn FnMut(&str) -> bool) -> bool {
        let word_key = key(word);
        self.outputs.contains(&word_key)
            || (self.own.contains(&word_key)
                && same(&self.without_stray_capital(word, is_word_valid), word))
    }

    /// `FullKeyboardEngine.swipeReadsAsName`: a reading, as the model spells it (lowercase),
    /// that the dictionary knows only with a capital (a name, day or place, "monday").
    pub fn reads_as_name(&self, word: &str, is_word_valid: &mut dyn FnMut(&str) -> bool) -> bool {
        let chars = characters(word);
        if !self.has_dictionary || !same(word, &lowercased(word)) || chars.len() < 2 {
            return false;
        }
        is_word_valid(&(uppercased(chars[0]) + &chars[1..].concat())) && !is_word_valid(word)
    }

    /// `AutocorrectEngine.shortcut(for:)`: the replacement for `token`, given a capital first
    /// letter when `token` starts with one (and isn't all capitals).
    fn shortcut(&self, token: &str) -> Option<String> {
        let stored = self.replacements.get(&key(&lowercased(token)))?;
        let chars = characters(token);
        let letters: String = chars.iter().copied().filter(|c| is_letter(c)).collect();
        let leading_capital = chars.first().is_some_and(|c| is_uppercase(c))
            && (characters(&letters).len() < 2 || !same(&letters, &uppercased(&letters)));
        let stored_chars = characters(stored);
        match stored_chars.first() {
            Some(first) if leading_capital && is_lowercase(first) => {
                Some(uppercased(first) + &stored_chars[1..].concat())
            }
            _ => Some(stored.clone()),
        }
    }

    /// `AutocorrectEngine.textReplacement(before:)`: your text replacement whose shortcut ends
    /// `before`, with the shortcut as typed.
    fn text_replacement(&self, before: &str) -> Option<(String, String)> {
        let chars = characters(before);
        if self.replacements.is_empty() || chars.last().is_none_or(|c| is_whitespace(c)) {
            return None;
        }
        let tail = &chars[chars.len().saturating_sub(64)..];
        let run = tail.iter().rev().take_while(|c| !is_whitespace(c)).count();
        let chunk = &tail[tail.len() - run..];
        let joins = |c: &str| is_letter(c) || is_number(c) || same(c, "'") || same(c, "\u{2019}");
        for start in 0..chunk.len() {
            let at_boundary = start == 0 || !joins(chunk[start - 1]) || !joins(chunk[start]);
            let typed = chunk[start..].concat();
            if at_boundary && let Some(replacement) = self.shortcut(&typed) {
                return Some((typed, replacement));
            }
        }
        None
    }
}

/// Shift as the keys show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Off,
    /// One-shot: the next letter is a capital.
    On,
    /// Caps lock.
    Locked,
}

/// The field's auto-capitalisation (`UITextAutocapitalizationType`), `None` also when
/// Settings › Auto-capitalization is off or the language never capitalises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCapitalization {
    None,
    Words,
    Sentences,
    AllCharacters,
}

/// The state a swipe is typed in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwipeContext<'a> {
    /// The text before the cursor, as Hex expects the host to report it; `None` when the host
    /// reports none.
    pub before: Option<&'a str>,
    pub shift: Shift,
    /// Whether shift is held down.
    pub shift_held: bool,
    pub auto_capitalization: AutoCapitalization,
    /// Settings › Auto-capitalization.
    pub auto_capitalizes: bool,
    /// Whether the field allows autocorrection: text replacements show only then.
    pub autocorrection: bool,
}

/// What a finished swipe types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typed {
    /// Whether a space goes in before the word.
    pub space: bool,
    pub word: String,
    /// The strip's other readings, cased as the word is, best first (at most three).
    pub alternatives: Vec<String>,
}

/// How much more (nats) a reading costs that the dictionary knows only capitalised and the
/// model counts as rare (`SwipeTyping.nameCost`).
pub const NAME_COST: f64 = 4.0;
/// The model's cost past which such a reading is rare, about its 2,500th word
/// (`SwipeTyping.commonNameCost`).
pub const COMMON_NAME_COST: f64 = 10.0;
/// How far (key pitches) a tap that slid may go from where it went down
/// (`SwipeTyping.slidTapReach`).
pub const SLID_TAP_REACH: f64 = 1.6;

impl SwipeDecoder {
    /// The order of `candidates` (a reading's, best first) once a rare name or abbreviation
    /// costs [`NAME_COST`] more: indices into `candidates`.
    pub fn ranked(
        &self,
        candidates: &[Candidate],
        words: &SwipeWords,
        is_word_valid: &mut dyn FnMut(&str) -> bool,
    ) -> Vec<usize> {
        let model = self.model();
        let mut rare = |text: &str| {
            words.reads_as_name(text, is_word_valid)
                && model
                    .index(text)
                    .is_none_or(|i| model.unigram_cost(i) > COMMON_NAME_COST)
        };
        let costs: Vec<f64> = candidates
            .iter()
            .map(|c| c.cost + if rare(&c.text) { NAME_COST } else { 0.0 })
            .collect();
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        // Swift's `sorted` by cost, then by place: a total order, so any sort gives it.
        order.sort_by(|&a, &b| {
            if costs[a] != costs[b] {
                if costs[a] < costs[b] {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            } else {
                a.cmp(&b)
            }
        });
        order
    }
}

/// What a swipe whose readings are `readings` (best first, those that may be offered) types in
/// `context`; `None` when there are none.
pub fn typed(
    readings: &[&str],
    context: &SwipeContext,
    words: &SwipeWords,
    is_word_valid: &mut dyn FnMut(&str) -> bool,
) -> Option<Typed> {
    let first = readings.first()?;
    let capital = capital(context);
    let word = cased(first, capital, context, words, is_word_valid);
    let alternatives = alternatives(readings, capital, context, words, is_word_valid);
    Some(Typed {
        space: needs_space(context.before),
        word,
        alternatives,
    })
}

/// `FullKeyboardEngine.swipeCapital`: shift (one-shot, held, caps lock, or automatic), or the
/// automatic capital the space the swipe adds brings ("Hi." then a swipe types " There").
fn capital(context: &SwipeContext) -> bool {
    if context.shift != Shift::Off || context.shift_held {
        return true;
    }
    if !needs_space(context.before) {
        return false;
    }
    should_capitalize(
        context.auto_capitalization,
        Some(&(context.before.unwrap_or("").to_owned() + " ")),
        true,
    )
}

/// `AutoCapitalization.shouldCapitalize` (English: `;` ends no sentence): whether the next
/// letter is a capital after `before`.
fn should_capitalize(mode: AutoCapitalization, before: Option<&str>, has_text: bool) -> bool {
    match mode {
        AutoCapitalization::None => return false,
        AutoCapitalization::AllCharacters => return true,
        AutoCapitalization::Words | AutoCapitalization::Sentences => {}
    }
    let Some(before) = before else {
        return !has_text;
    };
    let chars = characters(before);
    let Some(last) = chars.last() else {
        return true;
    };
    if is_newline(last) {
        return true;
    }
    if !is_whitespace(last) {
        return false;
    }
    if mode == AutoCapitalization::Words {
        return true;
    }
    let mut end = chars.len();
    while end > 0 && is_whitespace(chars[end - 1]) && !is_newline(chars[end - 1]) {
        end -= 1;
    }
    let Some(last_written) = end.checked_sub(1).map(|i| chars[i]) else {
        return true;
    };
    if is_newline(last_written) {
        return true;
    }
    const CLOSERS: [&str; 6] = ["\"", "'", ")", "]", "\u{201D}", "\u{2019}"];
    while end > 0 && CLOSERS.iter().any(|c| same(chars[end - 1], c)) {
        end -= 1;
    }
    let Some(ender) = end.checked_sub(1).map(|i| chars[i]) else {
        return false;
    };
    [".", "!", "?"].iter().any(|e| same(ender, e))
}

/// `FullKeyboardEngine.swipeCased`: a reading as it's typed. All capitals with caps lock, a
/// capital first letter with `capital`, else a name the dictionary only knows capitalised
/// ("monday" -> "Monday"); one of your own words with a capital that is also a common word is
/// written as the sentence needs it. With Auto-capitalization off, no capital you didn't ask
/// for, but your own spellings keep theirs.
fn cased(
    word: &str,
    capital: bool,
    context: &SwipeContext,
    words: &SwipeWords,
    is_word_valid: &mut dyn FnMut(&str) -> bool,
) -> String {
    if context.shift == Shift::Locked {
        return uppercased(word);
    }
    let chars = characters(word);
    let Some(first) = chars.first() else {
        return word.to_owned();
    };
    if capital {
        return uppercased(first) + &chars[1..].concat();
    }
    if !context.auto_capitalizes {
        return if words.is_own_spelling(word, is_word_valid) {
            word.to_owned()
        } else {
            lowercased(word)
        };
    }
    if words.reads_as_name(word, is_word_valid) {
        return uppercased(first) + &chars[1..].concat();
    }
    words.without_stray_capital(word, is_word_valid)
}

/// `FullKeyboardEngine.swipeAlternatives`: the strip's other readings, cased as the word is. A
/// reading that is a text replacement's shortcut shows as its replacement, the swiped word's
/// first.
fn alternatives(
    readings: &[&str],
    capital: bool,
    context: &SwipeContext,
    words: &SwipeWords,
    is_word_valid: &mut dyn FnMut(&str) -> bool,
) -> Vec<String> {
    let replacement = |reading: &str| -> Option<String> {
        if !context.autocorrection {
            return None;
        }
        let chars = characters(reading);
        let typed = if capital {
            chars.first().map_or(String::new(), |c| uppercased(c))
                + &chars.get(1..).unwrap_or(&[]).concat()
        } else {
            reading.to_owned()
        };
        let (shortcut, replacement) = words.text_replacement(&typed)?;
        (characters(&shortcut).len() == characters(&typed).len()).then_some(replacement)
    };
    let mut out: Vec<String> = Vec::new();
    if let Some(phrase) = readings.first().and_then(|first| replacement(first)) {
        out.push(phrase);
    }
    for reading in readings.iter().skip(1) {
        let item = replacement(reading)
            .unwrap_or_else(|| cased(reading, capital, context, words, is_word_valid));
        if !out.iter().any(|o| same(o, &item)) {
            out.push(item);
        }
    }
    out.truncate(3);
    out
}

/// Whether a swiped word needs a space before it after `before`, as Apple's keyboard adds one:
/// not after nothing, a space, a line break, an attachment (U+FFFC: a link's card or a picture
/// in the text) or opening punctuation.
pub fn needs_space(before: Option<&str>) -> bool {
    const OPENING: [&str; 14] = [
        "(", "[", "{", "<", "\u{AB}", "\u{2039}", "\u{201C}", "\u{2018}", "\u{201E}", "\u{BF}",
        "\u{A1}", "@", "#", "/",
    ];
    let opening = |c: &str| OPENING.iter().any(|o| same(c, o));
    let Some(before) = before else {
        return false;
    };
    let chars = characters(before);
    let Some(&last) = chars.last() else {
        return false;
    };
    if is_whitespace(last) || is_newline(last) || same(last, "\u{FFFC}") || opening(last) {
        return false;
    }
    if same(last, "\"") || same(last, "'") {
        let previous = chars.len().checked_sub(2).map(|i| chars[i]);
        return !previous.is_none_or(|p| is_whitespace(p) || is_newline(p) || opening(p));
    }
    true
}

/// Whether a swipe through `points` (the touch's, in points) was a tap that slid: it never went
/// more than [`SLID_TAP_REACH`] × `pitch` from where it went down, it lifted on the key it
/// went down on (`key`, that key's character; `None` when it lifted elsewhere), and its best
/// reading (`first_reading`) doesn't start and end on that key ("lol").
pub fn is_slid_tap(
    points: &[(f64, f64)],
    pitch: f64,
    key: Option<&str>,
    first_reading: Option<&str>,
) -> bool {
    let (Some(&first), Some(_)) = (points.first(), points.last()) else {
        return false;
    };
    let reach = pitch * SLID_TAP_REACH;
    if !points
        .iter()
        .all(|p| (p.0 - first.0).hypot(p.1 - first.1) <= reach)
    {
        return false;
    }
    let Some(key) = key.filter(|k| characters(k).len() == 1) else {
        return false;
    };
    let letter = lowercased(key);
    first_reading.is_none_or(|word| !starts_and_ends_with(word, &letter))
}

/// `SwipeTyping.word(_:startsAndEndsWith:)`: whether `word` begins and ends on `letter`'s key
/// ("lol" on l): its first and last letters, lowercased, without their accents.
pub fn starts_and_ends_with(word: &str, letter: &str) -> bool {
    let chars = characters(word);
    let base = |c: &str| -> String {
        use unicode_normalization::UnicodeNormalization;
        lowercased(c)
            .nfd()
            .next()
            .map(String::from)
            .unwrap_or_default()
    };
    chars.len() > 1 && same(&base(chars[0]), letter) && same(&base(chars[chars.len() - 1]), letter)
}
