//! The cleaned text: the words as heard with a plan's deletions taken out.
//!
//! Joining the kept words leaves punctuation and capitals stranded beside
//! each gap, and this repairs them. It only removes characters (commas,
//! hesitation ellipses), moves a deleted word's sentence mark onto the word
//! before it, or changes the case of a first letter. It never adds,
//! reorders or changes a word, a name or a number.
//!
//! It is a line-for-line port of the iOS `CleanupReconstruction`, itself a
//! port of the tagger's `js/reconstruct.js`, and keeps JavaScript's string
//! rules: a word's first letter is its first UTF-16 unit, so an emoji or
//! any other character outside the basic plane is never cased, while the
//! rest of a word is read by code point.

use super::plan::EditPlan;
use super::words::Words;

/// Words that never keep a trailing comma when the disfluency after them is
/// removed: "Can you, um, send" becomes "Can you send".
const NO_COMMA: &[&str] = &[
    "a", "an", "the", "to", "of", "for", "with", "at", "in", "on", "from", "by", "about", "into",
    "onto", "my", "your", "his", "her", "our", "their", "its", "i", "we", "you", "he", "she",
    "they", "can", "could", "would", "will", "should", "shall", "may", "might", "must", "do",
    "does", "did", "is", "are", "was", "were", "be", "been", "am", "have", "has", "had", "and",
    "or", "but", "if", "that", "because", "than", "just", "gonna", "wanna", "want", "need",
    "think", "thinking", "guess", "get", "got", "let's", "i'm", "we're", "you're", "they're",
    "it's", "there's", "i'll", "we'll", "you'll", "i'd", "we'd", "i've", "we've", "really", "very",
    "so", "like", "isn't", "don't", "can't", "won't", "didn't", "this", "these", "those", "some",
    "any", "every", "what", "where", "when", "how", "which", "who", "also", "go", "going", "s",
];
/// Function words a correction's repair may start with. They are lowercased
/// when a deleted sentence end had capitalized them: "at 5. Wait, no. At 6."
/// becomes "at 6."
const LOWERABLE: &[&str] = &[
    "at", "on", "in", "the", "a", "an", "to", "for", "with", "from", "by", "and", "or", "but",
    "about", "around", "before", "after", "until", "into", "my", "your", "our", "their", "his",
    "her", "this", "that", "these", "those",
];
const FILLER_WORDS: &[&str] = &[
    "um", "uh", "er", "ah", "hmm", "umm", "uhh", "erm", "uhm", "mm", "like", "you", "know", "i",
    "mean",
];
const TRAILING: &[char] = &[',', '.', '?', '!', ';', ':', '\u{2026}'];
const PAUSES: &[char] = &[',', ';', ':'];
const SENTENCE_ENDS: &[char] = &['.', '?', '!'];

/// The text a plan makes of the words as heard. A plan that changes nothing
/// gives back the transcript exactly, spacing included. Otherwise the kept
/// words are joined by single spaces.
pub(crate) fn render(words: &Words, plan: &EditPlan) -> String {
    let deletes = plan.deletes(words.len());
    if !deletes.contains(&true) {
        return words.text().to_owned();
    }
    let mut out: Vec<Word> = Vec::new();
    let mut pending: Vec<Word> = Vec::new();
    for (word, delete) in words.iter().zip(deletes) {
        let mut word: Word = word.chars().collect();
        if delete {
            pending.push(word);
            continue;
        }
        if !pending.is_empty() {
            word = after_gap(&mut out, &pending, word);
            pending.clear();
        }
        out.push(word);
    }
    if !pending.is_empty() {
        at_end(&mut out, &pending);
    }
    let out: Vec<String> = out.into_iter().map(String::from_iter).collect();
    out.join(" ")
}

type Word = Vec<char>;

/// Repairs the kept words on both sides of a gap, and gives back the word
/// after it.
fn after_gap(out: &mut [Word], deleted: &[Word], next: Word) -> Word {
    let mut b = next;
    let Some(last) = out.last_mut() else {
        return capitalized(b); // The deletion was at the very start.
    };
    let mut a = strip_ellipsis(last); // A hesitation "..." before the gap.
    let term = deleted.last().map_or(&[][..], |word| terminal(word));
    if a.last() == Some(&',') && NO_COMMA.contains(&bare(&a).as_str()) {
        a.pop();
    }
    if !term.is_empty() && !ends_sentence(&a) && is_upper(first_unit(&b)) {
        if LOWERABLE.contains(&bare(&b).as_str()) {
            b = lowercased_first(b);
        } else if deleted
            .iter()
            .all(|word| FILLER_WORDS.contains(&bare(word).as_str()))
        {
            // "Thanks, um. See you" becomes "Thanks. See you".
            a = trimming(a, PAUSES);
            a.extend_from_slice(term);
        }
    }
    if a.is_empty() || ends_sentence(&a) {
        b = capitalized(b);
    }
    *last = a;
    b
}

/// Repairs the last kept word when the transcript ended in a deletion.
fn at_end(out: &mut [Word], deleted: &[Word]) {
    let Some(last) = out.last_mut() else { return };
    let mut a = strip_ellipsis(last);
    if !ends_sentence(&a) {
        a = trimming(a, PAUSES);
        a.extend_from_slice(deleted.last().map_or(&[][..], |word| terminal(word)));
    }
    *last = a;
}

/// JavaScript's `t[0]`: the first UTF-16 unit. A first character outside the
/// basic plane is half a surrogate pair there, which has no case.
fn first_unit(word: &[char]) -> Option<char> {
    word.first()
        .copied()
        .filter(|&first| first as u32 <= 0xFFFF)
}

/// `ch === ch.toUpperCase() && ch !== ch.toLowerCase()`
fn is_upper(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| ch.to_uppercase().eq([ch]) && !ch.to_lowercase().eq([ch]))
}

/// `ch === ch.toLowerCase() && ch !== ch.toUpperCase()`
fn is_lower(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| ch.to_lowercase().eq([ch]) && !ch.to_uppercase().eq([ch]))
}

fn ends_sentence(word: &[char]) -> bool {
    word.last().is_some_and(|last| SENTENCE_ENDS.contains(last)) && !has_ellipsis(word)
}

fn has_ellipsis(word: &[char]) -> bool {
    word.ends_with(&['.', '.', '.'])
}

/// The sentence-final mark (. ? !) a deleted word carries, ignoring ellipses.
fn terminal(word: &[char]) -> &[char] {
    if ends_sentence(word) {
        &word[word.len() - 1..]
    } else {
        &[]
    }
}

/// `t.replace(/[,.?!;:…]+$/, "").toLowerCase()`
fn bare(word: &[char]) -> String {
    let end = word.len()
        - word
            .iter()
            .rev()
            .take_while(|ch| TRAILING.contains(ch))
            .count();
    word[..end]
        .iter()
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn capitalized(word: Word) -> Word {
    match first_unit(&word) {
        Some(first) if is_lower(Some(first)) && !word[1..].iter().any(|&ch| is_upper(Some(ch))) => {
            first
                .to_uppercase()
                .chain(word[1..].iter().copied())
                .collect()
        }
        _ => word,
    }
}

fn lowercased_first(word: Word) -> Word {
    match first_unit(&word) {
        Some(first) => first
            .to_lowercase()
            .chain(word[1..].iter().copied())
            .collect(),
        None => word,
    }
}

fn strip_ellipsis(word: &[char]) -> Word {
    if has_ellipsis(word) {
        word[..word.len() - 3].to_vec()
    } else if word.last() == Some(&'\u{2026}') {
        word[..word.len() - 1].to_vec()
    } else {
        word.to_vec()
    }
}

fn trimming(mut word: Word, marks: &[char]) -> Word {
    while word.last().is_some_and(|last| marks.contains(last)) {
        word.pop();
    }
    word
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebuild(text: &str, labels: &[u8]) -> String {
        let words = Words::new(text);
        let deletes: Vec<bool> = labels.iter().map(|&label| label == 1).collect();
        render(&words, &EditPlan::deleting(&words, &deletes))
    }

    /// The iOS suite's own cases (`testReconstructionRepairsPunctuationAndCapitals`).
    #[test]
    fn deletions_take_their_commas_and_leave_capitals_right() {
        assert_eq!(
            rebuild(
                "Hey Tim, um, are you available on Friday at 3pm, actually 4pm?",
                &[0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0]
            ),
            "Hey Tim, are you available on Friday at 4pm?"
        );
        assert_eq!(
            rebuild("Can you, um, send it", &[0, 0, 1, 0, 0]),
            "Can you send it"
        );
        assert_eq!(
            rebuild("Thanks, um. See you", &[0, 1, 0, 0]),
            "Thanks. See you"
        );
        assert_eq!(rebuild("Um, so we go", &[1, 0, 0, 0]), "So we go");
        assert_eq!(
            rebuild("We left... um, early", &[0, 0, 1, 0]),
            "We left early"
        );
        assert_eq!(
            rebuild("Call me at 5, um,", &[0, 0, 0, 0, 1]),
            "Call me at 5"
        );
        assert_eq!(rebuild("See you, uh.", &[0, 0, 1]), "See you.");
        assert_eq!(
            rebuild("Meet at 5. Wait, no. At 6.", &[0, 1, 1, 1, 1, 0, 0]),
            "Meet at 6."
        );
        assert_eq!(rebuild("um uh", &[1, 1]), "");
    }

    /// JavaScript cases the first UTF-16 unit: an emoji is never
    /// capitalized, and ß becomes SS.
    #[test]
    fn a_first_letter_is_cased_the_way_javascript_cases_it() {
        assert_eq!(rebuild("Um, 😀ok", &[1, 0]), "😀ok");
        assert_eq!(rebuild("Um, ßtraße", &[1, 0]), "SStraße");
        assert_eq!(rebuild("Um, iPhone", &[1, 0]), "iPhone");
    }

    #[test]
    fn a_plan_that_changes_nothing_gives_the_transcript_back_exactly() {
        let words = Words::new("  Hey   Tim,\n are you free?  ");
        assert_eq!(render(&words, &EditPlan::default()), words.text());
        assert_eq!(render(&Words::new(""), &EditPlan::default()), "");
    }
}
