//! Structural reasons to keep a proposed deletion. These rules only refuse:
//! the guard never widens a deletion to words the proposer did not mark.

use super::{CUES, FUNCTION_WORDS, Heard, PURE_FILLERS, cue_mask, ends_with_any};
use std::ops::Range;

/// A comma-less "like" introducing a clause is a conjunction. We do not
/// need to recognize every English verb to protect a pronoun-led clause;
/// for noun subjects, require a recognizable finite verb.
pub(super) fn comparative_clause(heard: &Heard<'_>, index: usize) -> bool {
    let after = &heard.bares[index + 1..];
    let Some(subject) = after.first() else {
        return false;
    };
    if ["i", "you", "he", "she", "we", "they", "it"].contains(&subject.as_str()) {
        return after.len() >= 2;
    }
    let verb = if ["a", "an", "the", "my", "your", "our", "their"].contains(&subject.as_str()) {
        after.get(2)
    } else {
        after.get(1)
    };
    verb.is_some_and(|verb| {
        [
            "am", "is", "are", "was", "were", "do", "does", "did", "have", "has", "had", "can",
            "could", "will", "would", "should", "said", "says", "didn't", "doesn't", "don't",
            "works", "worked",
        ]
        .contains(&verb.as_str())
    })
}

/// An adjacent comma-separated item with the same frame but different
/// content is not a restart, even when the first word repeats.
pub(super) fn parallel_items(heard: &Heard<'_>, run: &Range<usize>) -> bool {
    if !heard.words[run.end - 1].ends_with(',') || run.end == heard.words.len() {
        return false;
    }
    let left = &heard.bares[run.clone()];
    let right_end = (run.end..heard.words.len())
        .find(|&index| ends_with_any(heard.words[index], &[',', ';', '.', '?', '!']))
        .map_or(heard.words.len(), |index| index + 1);
    let right = &heard.bares[run.end..right_end];
    if cue_mask(left).contains(&true)
        || cue_mask(right).contains(&true)
        || left
            .iter()
            .chain(right)
            .any(|word| PURE_FILLERS.contains(&word.as_str()))
    {
        return false;
    }
    // The run may include an introductory verb ("add a bag of ...").
    (0..left.len()).any(|start| {
        let left = &left[start..];
        let prefix = left.iter().zip(right).take_while(|(a, b)| a == b).count();
        let suffix = left
            .iter()
            .rev()
            .zip(right.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        prefix > 0
            && prefix + suffix >= 2
            && left
                .iter()
                .any(|word| content(word) && !right.contains(word))
            && right
                .iter()
                .any(|word| content(word) && !left.contains(word))
    })
}

/// "The striped mug, sorry, the plain one" still needs the head noun.
/// Without a parser, keep the whole repair whenever a content word would
/// disappear before an anaphoric one/ones. An explicit noun in the repair
/// is allowed ("the striped mug, sorry, the plain mug").
pub(super) fn anaphoric_repair(heard: &Heard<'_>, run: &Range<usize>) -> bool {
    let end = (run.end..heard.words.len())
        .find(|&index| ends_with_any(heard.words[index], &[',', ';', '.', '?', '!']))
        .map_or(heard.words.len(), |index| index + 1);
    let repair = &heard.bares[run.end..end];
    repair.iter().enumerate().any(|(index, word)| {
        ["one", "ones"].contains(&word.as_str())
            && (ends_with_any(heard.words[run.end + index], &[',', ';', '.', '?', '!'])
                || repair.get(index + 1).is_none_or(|next| {
                    [
                        "by", "on", "in", "at", "over", "under", "from", "near", "with", "without",
                        "beside", "behind", "that", "which", "and", "or", "is", "are", "was",
                        "were",
                    ]
                    .contains(&next.as_str())
                }))
    }) && heard.bares[run.clone()]
        .iter()
        .any(|word| content(word) && !repair.contains(word))
}

/// Keep an unresolved correction rather than strand "Yes," before its
/// retraction. If that particle is already in the proposed run, the whole
/// correction can go. No unproposed word is ever silently attached.
pub(super) fn stranded_polarity(heard: &Heard<'_>, run: &Range<usize>) -> bool {
    let Some(previous) = run.start.checked_sub(1) else {
        return false;
    };
    let clause_initial =
        previous == 0 || ends_with_any(heard.words[previous - 1], &['.', '?', '!', ';', ':']);
    if !clause_initial || !heard.words[previous].ends_with(',') {
        return false;
    }
    let affirmative = |word: &str| ["yes", "yeah", "sure", "yep"].contains(&word);
    let negative = |word: &str| ["no", "nope"].contains(&word);
    let particle = &heard.bares[previous];
    let repair = &heard.bares[run.clone()];
    (affirmative(particle) && repair.iter().any(|word| negative(word)))
        || (negative(particle) && repair.iter().any(|word| affirmative(word)))
}

fn content(word: &str) -> bool {
    !word.is_empty()
        && !FUNCTION_WORDS.contains(&word)
        && !PURE_FILLERS.contains(&word)
        && !CUES.iter().any(|cue| cue.contains(&word))
}
