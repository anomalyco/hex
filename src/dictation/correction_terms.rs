//! What to learn from a correction: after Hex pastes a dictation, the field
//! can be read once more when the watch ends and compared with the text it
//! held right after the paste. A replacement inside the pasted text that
//! turns into a name-like word is the person telling us how Hex should have
//! spelled it, so it becomes a vocabulary term. Everything here is pure: no
//! settings store, no Accessibility, no clock.

use std::ops::Range;

use crate::dictation::vocabulary::{MAX_TERM_CHARS, VocabularyEntry, same};

/// Most terms one corrected paste teaches.
const MAX_LEARNED: usize = 5;
/// Longest field, in words, an edit is read from. Longer fields are not
/// diffed at all.
const MAX_WORDS: usize = 400;
/// Most consecutive name-like words one phrase term keeps.
const MAX_PHRASE_WORDS: usize = 3;
/// The share of the inserted words an edit may replace or delete before it
/// reads as a rewrite rather than a correction, once more than
/// [`MIN_CHANGED`] of them changed.
const MAX_CHANGED_PERCENT: usize = 60;
/// Most inserted words an edit may replace or delete however short the
/// paste: fixing a two- or three-word dictation usually rewrites all of it.
const MIN_CHANGED: usize = 3;

/// The vocabulary terms an edit of just-pasted text teaches.
///
/// `baseline` is the whole field right after the paste, `inserted` the text
/// Hex actually inserted into it (found at its last whole-word occurrence),
/// `edited` the field when the watch ended, and `known` the current
/// vocabulary. Empty when the field did not change, `inserted` is blank or
/// missing from `baseline`, either field has more than [`MAX_WORDS`] words,
/// or the edit changed more than [`MAX_CHANGED_PERCENT`] percent of the
/// inserted words and more than [`MIN_CHANGED`] of them.
///
/// Both fields are split on whitespace and compared by each token without its
/// leading and trailing punctuation, so "Raunak," compares as Raunak while
/// Node.js, GPT-4, and O'Brien keep their insides. The words are diffed with a
/// longest-common-subsequence walk, and only replacement hunks are read: a
/// hunk must replace words (never a pure insertion or deletion), and all of
/// its replaced words must begin inside the inserted text's span in
/// `baseline`. Edits elsewhere in the field teach nothing. The inserted text
/// must sit on word boundaries, so a later "book" never stands in for a
/// pasted "ok". The rewrite rule counts the inserted words a hunk inside the
/// span replaced or deleted; words the edit only inserted around them do not
/// count.
///
/// A hunk is read when its added words include a name-like word, in the
/// `edited` field's context:
///
/// - all alphabetics uppercase and at least two letters, like HEX;
/// - uppercase after the first letter, like iPhone or OpenCode;
/// - letters mixed with digits, like GPT4 or B2B;
/// - capitalized and not at the start of a sentence, like Raunak.
///
/// A word needs at least two letters, so I and A are never learned, and a
/// word whose part before an apostrophe is shorter than two letters (I'm,
/// I'll, I've, I'd) never is either. A sentence starts at the field's first
/// word, after a line break, and after a word ending in a colon, or in a
/// period, exclamation mark, or question mark once closing quotes and
/// brackets are ignored.
///
/// The exact capitalization rule is: learning is a casing decision, not a
/// dictionary. A replacement that only capitalizes the first word of a
/// sentence teaches nothing, because a capitalized word at a sentence start is
/// not name-like; capitalizing that word mid-sentence (kit to Kit) does,
/// because the new casing is name-like in context. Every term therefore comes
/// from added words and differs case-sensitively from the words it replaced.
///
/// Consecutive name-like added words in one hunk become one phrase of at most
/// [`MAX_PHRASE_WORDS`] words (Raunak Chowdhuri); a longer run keeps its first
/// three. Terms already in `known` as a term or a "sounds like" spelling (in
/// any case), duplicates, and terms longer than [`MAX_TERM_CHARS`] are
/// dropped. At most [`MAX_LEARNED`] terms are returned, in the edited field's
/// order.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn learned_terms(
    baseline: &str,
    inserted: &str,
    edited: &str,
    known: &[VocabularyEntry],
) -> Vec<String> {
    if edited == baseline {
        return Vec::new();
    }
    let inserted = inserted.trim();
    if inserted.is_empty() {
        return Vec::new();
    }
    let Some(start) = last_whole_word(baseline, inserted) else {
        return Vec::new();
    };
    let baseline_tokens = tokenize(baseline);
    let edited_tokens = tokenize(edited);
    if baseline_tokens.len() > MAX_WORDS || edited_tokens.len() > MAX_WORDS {
        return Vec::new();
    }
    let Some(inserted_span) = span(&baseline_tokens, start, start + inserted.len()) else {
        return Vec::new();
    };
    let hunks = hunks(&baseline_tokens, &edited_tokens);
    let changed: usize = hunks
        .iter()
        .filter(|hunk| contained(&hunk.removed, &inserted_span))
        .map(|hunk| hunk.removed.len())
        .sum();
    let allowed = (inserted_span.len() * MAX_CHANGED_PERCENT / 100).max(MIN_CHANGED);
    if changed > allowed {
        return Vec::new();
    }
    let mut terms: Vec<String> = Vec::new();
    for hunk in &hunks {
        if hunk.removed.is_empty()
            || hunk.added.is_empty()
            || !contained(&hunk.removed, &inserted_span)
        {
            continue;
        }
        for phrase in phrases(&edited_tokens, &hunk.added) {
            if phrase.chars().count() > MAX_TERM_CHARS
                || known.iter().any(|entry| names(entry, &phrase))
                || terms.iter().any(|kept| same(kept, &phrase))
            {
                continue;
            }
            terms.push(phrase);
            if terms.len() == MAX_LEARNED {
                return terms;
            }
        }
    }
    terms
}

/// One whitespace-delimited token of a field.
struct Token<'a> {
    /// The token without its leading and trailing punctuation, compared and
    /// learned as is. Empty for a punctuation-only token.
    word: &'a str,
    /// Byte offsets of the whole token in its field.
    start: usize,
    end: usize,
    /// Whether the token opens a sentence: the field's first token, one
    /// after a line break, or one after a token ending in a colon, period,
    /// exclamation mark, or question mark. A capitalized word there is
    /// ordinary, so it is not name-like.
    sentence_start: bool,
}

/// One gap between two matched words of the word diff.
struct Hunk {
    removed: Range<usize>,
    added: Range<usize>,
}

fn tokenize(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if character.is_whitespace() {
            if let Some(start) = start.take() {
                push_token(&mut tokens, text, start, index);
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(start) = start {
        push_token(&mut tokens, text, start, text.len());
    }
    tokens
}

fn push_token<'a>(tokens: &mut Vec<Token<'a>>, text: &'a str, start: usize, end: usize) {
    let sentence_start = tokens.last().is_none_or(|previous| {
        text[previous.end..start].contains('\n')
            || ends_sentence(&text[previous.start..previous.end])
    });
    tokens.push(Token {
        word: strip_punctuation(&text[start..end]),
        start,
        end,
        sentence_start,
    });
}

/// Whether a token's end closes its sentence: a colon always opens a new
/// one, and a period, exclamation mark, or question mark does once trailing
/// closing quotes and brackets are ignored, so "go." ends a sentence.
fn ends_sentence(token: &str) -> bool {
    token
        .trim_end_matches(['"', '\'', '”', '’', ')', ']'])
        .ends_with([':', '.', '!', '?'])
}

/// The last byte offset of `needle` in `haystack` that stands on word
/// boundaries: the characters around it are not alphanumeric, or it touches
/// the field's edge. A later occurrence inside a longer word ("ok" in "book")
/// is skipped for the pasted one before it.
fn last_whole_word(haystack: &str, needle: &str) -> Option<usize> {
    let mut end = haystack.len();
    loop {
        let index = haystack[..end].rfind(needle)?;
        let before = haystack[..index].chars().next_back();
        let after = haystack[index + needle.len()..].chars().next();
        if !before.is_some_and(|character| character.is_alphanumeric())
            && !after.is_some_and(|character| character.is_alphanumeric())
        {
            return Some(index);
        }
        // Search strictly before this occurrence. `index` is a character
        // boundary because it begins a match.
        if index == 0 {
            return None;
        }
        end = index;
    }
}

/// A token between its first and last alphanumeric character, so internal
/// apostrophes, hyphens, and dots stay.
fn strip_punctuation(token: &str) -> &str {
    let Some(start) = token
        .char_indices()
        .find(|(_, character)| character.is_alphanumeric())
        .map(|(index, _)| index)
    else {
        return "";
    };
    let end = token
        .char_indices()
        .rev()
        .find(|(_, character)| character.is_alphanumeric())
        .map_or(token.len(), |(index, character)| {
            index + character.len_utf8()
        });
    &token[start..end]
}

/// The baseline tokens that begin inside `start..end`, the inserted text's
/// last byte span. A token that carries trailing punctuation past the span
/// still belongs to the insertion.
fn span(tokens: &[Token<'_>], start: usize, end: usize) -> Option<Range<usize>> {
    let inside = |token: &Token<'_>| token.start >= start && token.start < end;
    let first = tokens.iter().position(inside)?;
    let last = tokens.iter().rposition(inside)?;
    Some(first..last + 1)
}

fn contained(range: &Range<usize>, span: &Range<usize>) -> bool {
    range.start >= span.start && range.end <= span.end
}

/// The gaps between the longest common subsequence of the two token lists.
/// Pure insertions and deletions are hunks too; the caller decides which to
/// read.
fn hunks(baseline: &[Token<'_>], edited: &[Token<'_>]) -> Vec<Hunk> {
    let rows = baseline.len();
    let columns = edited.len();
    let mut lengths = vec![vec![0u16; columns + 1]; rows + 1];
    for row in (0..rows).rev() {
        for column in (0..columns).rev() {
            lengths[row][column] = if baseline[row].word == edited[column].word {
                lengths[row + 1][column + 1] + 1
            } else {
                lengths[row + 1][column].max(lengths[row][column + 1])
            };
        }
    }
    let mut matches = Vec::new();
    let (mut row, mut column) = (0, 0);
    while row < rows && column < columns {
        if baseline[row].word == edited[column].word {
            matches.push((row, column));
            row += 1;
            column += 1;
        } else if lengths[row + 1][column] >= lengths[row][column + 1] {
            row += 1;
        } else {
            column += 1;
        }
    }
    let mut hunks = Vec::new();
    let (mut previous_row, mut previous_column): (Option<usize>, Option<usize>) = (None, None);
    for (row, column) in matches.into_iter().chain([(rows, columns)]) {
        let removed = previous_row.map_or(0, |previous| previous + 1);
        let added = previous_column.map_or(0, |previous| previous + 1);
        if removed < row || added < column {
            hunks.push(Hunk {
                removed: removed..row,
                added: added..column,
            });
        }
        previous_row = Some(row);
        previous_column = Some(column);
    }
    hunks
}

/// The phrase terms in one hunk: each run of consecutive name-like added
/// words, at most [`MAX_PHRASE_WORDS`] long.
fn phrases<'a>(tokens: &[Token<'a>], added: &Range<usize>) -> Vec<String> {
    let mut phrases = Vec::new();
    let mut run: Vec<&'a str> = Vec::new();
    for token in &tokens[added.clone()] {
        if !name_like(token) {
            if !run.is_empty() {
                phrases.push(run.join(" "));
                run.clear();
            }
            continue;
        }
        if run.len() < MAX_PHRASE_WORDS {
            run.push(token.word);
        }
    }
    if !run.is_empty() {
        phrases.push(run.join(" "));
    }
    phrases
}

/// Whether an added word can be a vocabulary term: all capitals, mixed case
/// after the first letter, letters with digits, or capitalized away from a
/// sentence start. Plain lowercase and letterless words never are.
fn name_like(token: &Token<'_>) -> bool {
    let word = token.word;
    let letters: Vec<char> = word
        .chars()
        .filter(|character| character.is_alphabetic())
        .collect();
    if letters.len() < 2 {
        return false;
    }
    // I'm, I'll, I've, and I'd are never names, however they are capitalized.
    if word.split(['\'', '’']).next().is_some_and(|before| {
        before
            .chars()
            .filter(|character| character.is_alphabetic())
            .count()
            < 2
    }) {
        return false;
    }
    let lowercase = letters.iter().any(|character| character.is_lowercase());
    let uppercase = letters.iter().any(|character| character.is_uppercase());
    if uppercase && !lowercase {
        return true;
    }
    if uppercase
        && lowercase
        && word
            .char_indices()
            .any(|(index, character)| index > 0 && character.is_uppercase())
    {
        return true;
    }
    if word.chars().any(|character| character.is_ascii_digit()) {
        return true;
    }
    !token.sentence_start
        && word
            .chars()
            .next()
            .is_some_and(|first| first.is_uppercase())
}

/// Whether `entry` already holds `term` as its term or a "sounds like"
/// spelling, in any case.
fn names(entry: &VocabularyEntry, term: &str) -> bool {
    same(&entry.term, term)
        || entry
            .sounds_like
            .iter()
            .any(|spelling| same(spelling, term))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: &str, sounds_like: &[&str]) -> VocabularyEntry {
        VocabularyEntry {
            term: term.into(),
            sounds_like: sounds_like
                .iter()
                .map(|spelling| (*spelling).into())
                .collect(),
        }
    }

    fn learned(baseline: &str, inserted: &str, edited: &str) -> Vec<String> {
        learned_terms(baseline, inserted, edited, &[])
    }

    #[test]
    fn learns_a_name_fix_mid_sentence() {
        assert_eq!(
            learned(
                "please talk to rowan ak tomorrow",
                "talk to rowan ak",
                "please talk to Raunak tomorrow",
            ),
            ["Raunak"]
        );
    }

    #[test]
    fn reads_only_the_inserted_span_in_a_surrounding_field() {
        assert_eq!(
            learned(
                "Subject: plan. talk to rowan ak before friday.",
                "talk to rowan ak",
                "Subject: plan. talk to Raunak before friday.",
            ),
            ["Raunak"]
        );
    }

    #[test]
    fn ignores_edits_outside_the_inserted_span() {
        assert_eq!(
            learned(
                "hey, my name is rowan",
                "hey, my name",
                "Hello, my name is Rowan",
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn ignores_a_plain_lowercase_fix() {
        assert_eq!(
            learned(
                "send it to rowan ak now",
                "send it to rowan ak now",
                "send it to rowan ack now",
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn ignores_sentence_start_capitalization() {
        assert_eq!(
            learned("rowan said hello", "rowan said hello", "Rowan said hello",),
            Vec::<String>::new()
        );
    }

    #[test]
    fn learns_all_caps_mixed_case_and_digit_terms() {
        assert_eq!(
            learned(
                "we use opencode daily, gpt4 now, iphone here, and nasa too",
                "we use opencode daily, gpt4 now, iphone here, and nasa too",
                "we use OpenCode daily, GPT4 now, iPhone here, and NASA too",
            ),
            ["OpenCode", "GPT4", "iPhone", "NASA"]
        );
    }

    #[test]
    fn joins_consecutive_names_into_one_phrase() {
        assert_eq!(
            learned(
                "ask rowan ak about the api",
                "ask rowan ak about the api",
                "ask Raunak Chowdhuri about the api",
            ),
            ["Raunak Chowdhuri"]
        );
    }

    #[test]
    fn ignores_a_rewrite_of_the_inserted_text() {
        assert_eq!(
            learned(
                "one two three four five six seven eight nine ten",
                "one two three four five six seven eight nine ten",
                "one Kappa beta gamma delta epsilon zeta eta theta iota",
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn ignores_fields_over_four_hundred_words() {
        let baseline = format!("{} rowan ak {}", "filler ".repeat(200), "more ".repeat(200));
        assert_eq!(
            learned(
                &baseline,
                "rowan ak",
                &baseline.replace("rowan ak", "Raunak")
            ),
            Vec::<String>::new()
        );
        let baseline = format!("{} rowan ak", "filler ".repeat(398));
        let edited = format!("{} Raunak Chowdhuri Delta", "filler ".repeat(398));
        assert_eq!(
            learned(&baseline, "rowan ak", &edited),
            Vec::<String>::new()
        );
    }

    #[test]
    fn skips_terms_already_in_the_vocabulary() {
        let known = [entry("raunak", &[]), entry("Someone", &["chowdhuri"])];
        assert_eq!(
            learned_terms(
                "call rowan ak, then rowan ch, then rowan q for me today",
                "call rowan ak, then rowan ch, then rowan q for me today",
                "call Raunak, then Chowdhuri, then Raunak Quinn for me today",
                &known,
            ),
            ["Raunak Quinn"]
        );
    }

    #[test]
    fn returns_at_most_five_terms_in_order() {
        assert_eq!(
            learned(
                "one two three four five six seven eight nine ten eleven twelve",
                "one two three four five six seven eight nine ten eleven twelve",
                "one Alpha three Bravo five Charlie seven Delta nine Echo eleven Foxtrot",
            ),
            ["Alpha", "Bravo", "Charlie", "Delta", "Echo"]
        );
    }

    #[test]
    fn reads_the_last_occurrence_of_the_inserted_text() {
        assert_eq!(
            learned(
                "rowan ak said hi and rowan ak said hi",
                "rowan ak said hi",
                "rowan ak said hi and Raunak said hi",
            ),
            ["Raunak"]
        );
    }

    #[test]
    fn learns_a_short_correction_past_its_sixty_percent() {
        // Two of two inserted words corrected, mid-sentence: the rewrite
        // rule alone would refuse the fix this lab exists for.
        assert_eq!(
            learned("got rowan ak", "rowan ak", "got Raunak"),
            ["Raunak"]
        );
        // Three inserted words with two corrected still learns.
        assert_eq!(
            learned("hi rowan ak", "hi rowan ak", "hi Raunak"),
            ["Raunak"]
        );
    }

    #[test]
    fn ignores_a_long_rewrite() {
        // Seven of ten inserted words replaced is a rewrite, not a fix.
        assert_eq!(
            learned(
                "one two three four five six seven eight nine ten",
                "one two three four five six seven eight nine ten",
                "Alpha Bravo Charlie Delta Echo Foxtrot Golf eight nine ten",
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_apostrophe_word_is_never_a_name() {
        assert_eq!(
            learned("so im going", "im", "so I'm going"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_line_break_starts_a_sentence() {
        assert_eq!(
            learned(
                "Hi Bob,\nthanks for the notes",
                "thanks",
                "Hi Bob,\nThanks for the notes",
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_colon_starts_a_sentence() {
        assert_eq!(
            learned("note: the plan", "the", "note: The plan"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_closing_quote_after_a_period_still_starts_a_sentence() {
        assert_eq!(
            learned("\"go.\" then", "then", "\"go.\" Then"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_pasted_text_matches_only_on_word_boundaries() {
        // The later "ok" begins "okay", which was in the field before the
        // paste: a fix there is not a fix of the pasted text.
        assert_eq!(learned("ok okay", "ok", "ok Okay"), Vec::<String>::new());
        // Of two whole-word occurrences, the last is still the pasted one.
        assert_eq!(learned("ok ok", "ok", "ok OK"), ["OK"]);
    }

    #[test]
    fn unchanged_or_missing_text_learns_nothing() {
        assert_eq!(
            learned("talk to rowan ak", "rowan ak", "talk to rowan ak"),
            Vec::<String>::new()
        );
        assert_eq!(
            learned("talk to rowan ak", "   ", "talk to Raunak"),
            Vec::<String>::new()
        );
        assert_eq!(
            learned("talk to rowan ak", "not there", "talk to Raunak"),
            Vec::<String>::new()
        );
    }
}
