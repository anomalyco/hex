//! The guard: which proposed deletions are safe to make.
//!
//! A proposer, such as the tagger, only says which words it would remove
//! and how sure it is. The tagger alone deleted every hyphenated name
//! ("Tell Hye-jin I said thanks." became "Tell thanks."), "maybe" in
//! "later, maybe after dinner", and words of clean sentences it had seen
//! removed in training. So a word goes only when its proposer is sure and
//! its removal is a disfluency the guard knows.
//!
//! Neighbouring words proposed above the delete threshold make a run. A run
//! goes when it is:
//! - a correction: something said, then a cue ("Tuesday, no wait,", "John,
//!   I mean"), or a hesitation before the same thing said again differently
//!   ("15 euros, er," before "50 euros", "3, uh," before "4"), with every
//!   word above the repair floor;
//! - a repeat: the run is said again right after it ("I I", "the, the
//!   office", "w- we");
//! - a false start: a fragment between a clause start and a pause ("Did
//!   you, have you"), made only of function words or with every word above
//!   the sure floor;
//! - otherwise only its fillers go: um and uh, and "like", "you know", "I
//!   mean" set off by commas, or without commas above the sure floor where
//!   "like" can't be the verb.
//!
//! Outside a correction it never removes a protected word: a hyphenated
//! compound (Jean-Luc, follow-up), a word with a digit, a capitalized word
//! mid-sentence (likely a name), a custom vocabulary term, or a content
//! adverb (maybe, actually, really), which also stays when repeated for
//! emphasis ("really, really").
//!
//! [`Policy::IOS`] is the iOS `CleanupGuard` ported rule for rule, and the
//! two are held together by `fixtures/guard-reference.json`, which the Swift
//! guard writes. [`Policy::DESKTOP`] adds four rules that only ever keep
//! more, each named at its switch in [`Policy`].
//!
//! Words are compared by Unicode scalar after composing them (NFC), where
//! Swift compares whole characters. One known difference: Swift counts a
//! Han numeral such as 三 as a digit, and this does not.

use std::collections::HashSet;
use std::ops::Range;

use unicode_normalization::UnicodeNormalization;

use super::plan::{Disfluency, Edit, EditOp, EditPlan, Proposal};
use super::words::Words;

const PURE_FILLERS: &[&str] = &[
    "um", "uh", "er", "erm", "ah", "hmm", "umm", "uhh", "uhm", "mm", "hm", "eh", "mhm", "uh-huh",
];
const CUES: &[&[&str]] = &[
    &["actually"],
    &["no"],
    &["wait"],
    &["sorry"],
    &["i", "mean"],
    &["i", "meant"],
    &["rather"],
    &["make", "that"],
    &["make", "it"],
    &["scratch", "that"],
    &["correction"],
    &["oops"],
    &["my", "bad"],
    &["nope"],
    &["hold", "on"],
    &["change", "that", "to"],
    &["let", "me", "correct", "that"],
    &["let", "me", "rephrase", "that"],
    &["strike", "that"],
    &["delete", "that"],
    &["never", "mind"],
    &["i'm", "sorry"],
];
const ADVERBS: &[&str] = &[
    "maybe",
    "actually",
    "really",
    "probably",
    "definitely",
    "perhaps",
];
/// "It was like super expensive": "like" after a form of be is a filler...
const BE: &[&str] = &[
    "was", "is", "are", "were", "it's", "that's", "be", "been", "being", "i'm", "you're", "he's",
    "she's", "we're", "they're", "am", "so",
];
/// ...unless what follows starts a noun phrase: "it was like a dream".
const NOUN_STARTS: &[&str] = &[
    "a", "an", "the", "this", "that", "my", "your", "his", "her", "our", "their", "its", "i",
    "you", "he", "she", "we", "they", "it", "me", "him", "them", "us",
];
/// Words a false start may be made of at the delete threshold.
const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "the", "to", "of", "for", "with", "at", "in", "on", "from", "by", "about", "my",
    "your", "his", "her", "our", "their", "i", "we", "you", "he", "she", "they", "it", "can",
    "could", "would", "will", "should", "do", "does", "did", "is", "are", "was", "were", "be",
    "have", "has", "had", "and", "or", "but", "if", "that", "because", "so", "just", "want",
    "need", "think", "get", "got", "let's", "i'm", "we're", "you're", "it's", "there's", "i'll",
    "we'll", "i'd", "this", "me", "him", "them", "us", "some", "gonna", "wanna", "go", "going",
    "what", "when", "where", "how", "why", "which", "who",
];
/// After these "like" is the verb ("I like it", "would like to"), never a
/// filler.
const LIKE_VERB_BEFORE: &[&str] = &[
    "i", "you", "we", "they", "he", "she", "would", "i'd", "you'd", "we'd", "they'd", "to", "do",
    "don't", "did", "didn't", "does", "doesn't", "really", "also", "just", "will", "i'll", "we'll",
    "might", "may", "people", "kids", "who", "not", "you'll", "they'll", "can", "could", "should",
];
/// Words that turn a sentence around. A contraction ending in "n't" is one
/// too.
const NEGATIONS: &[&str] = &[
    "no", "not", "never", "none", "nothing", "nobody", "nowhere", "neither", "nor", "cannot",
];
/// A filler right before one of these is the sentence's subject: "Um is a
/// filler word".
const SAID_OF_A_WORD: &[&str] = &["is", "isn't", "was", "wasn't", "means", "sounds", "counts"];
/// A filler right after one of these, ending its sentence, is what the
/// sentence names: "and so is uh.", "the word um."
const NAMES_A_WORD: &[&str] = &[
    "is", "isn't", "was", "wasn't", "say", "says", "said", "saying", "word", "words",
];
const QUOTES: &[char] = &['"', '\'', '\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}'];

/// How sure a proposer must be, and which words stay whatever it says.
/// Both policies are fixed here, and each was checked against the
/// hand-written sentences: a new number comes with a new measurement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Policy {
    /// A word is marked above this (the threshold the tagger shipped with).
    delete_threshold: f64,
    /// Every word of a correction must be above this.
    repair_floor: f64,
    /// A false start with content words, or a filler without commas, must
    /// be above this.
    sure_floor: f64,
    /// A word said three or more times in a row is emphasis, not a
    /// stutter, unless it is a function word: "No, no, no" and "stop, stop,
    /// stop the deploy" stay, "I I I think" does not.
    emphatic_repeats: bool,
    /// A filler that is talked about stays: in quotes, as the subject of a
    /// verb, or as what a sentence names ("Um is a filler word, and so is
    /// uh.").
    mentioned_fillers: bool,
    /// A negation goes only in a correction or a repeat, never in a false
    /// start, because losing one turns the sentence around.
    negations: bool,
    /// Edits that would leave no word are all refused. iOS makes this check
    /// after rendering and inserts the transcript as heard.
    something_left: bool,
}

impl Policy {
    /// The iOS guard exactly, for proving the two apps agree.
    pub(crate) const IOS: Self = Self {
        delete_threshold: 0.98,
        repair_floor: 0.995,
        sure_floor: 0.999,
        emphatic_repeats: false,
        mentioned_fillers: false,
        negations: false,
        something_left: false,
    };

    /// What desktop dictation uses: the iOS guard, and four more reasons to
    /// keep a word.
    pub(crate) const DESKTOP: Self = Self {
        emphatic_repeats: true,
        mentioned_fillers: true,
        negations: true,
        something_left: true,
        ..Self::IOS
    };
}

/// Custom vocabulary and learned terms, held word by word the way the guard
/// compares words. Only a correction or a repeat removes one.
#[derive(Clone, Debug, Default)]
pub(crate) struct ProtectedTerms(HashSet<String>);

impl ProtectedTerms {
    /// Terms as the person wrote them. A term of several words protects
    /// each of them, and a single letter protects nothing.
    pub(crate) fn new<'a>(terms: impl IntoIterator<Item = &'a str>) -> Self {
        Self(
            terms
                .into_iter()
                .flat_map(str::split_whitespace)
                .map(bare)
                .filter(|word| word.chars().count() >= 2)
                .collect(),
        )
    }
}

/// Why a protected word stays.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Protection {
    /// A compound with a hyphen between letters: Jean-Luc, follow-up.
    Hyphenated,
    /// A word with a digit in it.
    Digit,
    /// One of the [`ProtectedTerms`].
    Vocabulary,
    /// A content adverb: maybe, actually, really.
    Adverb,
    /// Capitalized mid-sentence, so likely a name.
    Name,
    /// A word that turns the sentence around: no, not, never, don't.
    Negation,
}

/// Why the guard refused to remove a word.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Reason {
    /// The proposer was not sure enough to mark the word.
    Unsure,
    /// Marked, but not part of any disfluency the guard knows.
    NotADisfluency,
    /// A word only a correction or a repeat may remove.
    Protected(Protection),
    /// Said again on purpose: "really, really", "No, no, no".
    Emphasis,
    /// A filler that is talked about, not hesitated.
    Mentioned,
    /// Removing everything admitted would leave no word.
    LeavesNothing,
}

/// Neighbouring words the guard kept for one reason.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Refused {
    pub(crate) at: super::words::Span,
    pub(crate) reason: Reason,
}

/// What became of every proposal: the edits to make, and the words that
/// stay with the reason each one stays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Admitted {
    pub(crate) plan: EditPlan,
    pub(crate) refused: Vec<Refused>,
}

/// Decides which proposed edits are made. It is pure: the same words and
/// proposals always give the same plan, with no model, clock, setting or
/// file involved.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CleanupGuard<'a> {
    pub(crate) policy: Policy,
    pub(crate) protected: &'a ProtectedTerms,
}

impl CleanupGuard<'_> {
    /// Admits or refuses each proposed word. A word several proposals cover
    /// is as sure as the surest of them. No word is removed that was not
    /// proposed.
    pub(crate) fn admit(&self, words: &Words, proposals: &[Proposal]) -> Admitted {
        let heard = Heard::new(words, proposals);
        let count = heard.words.len();
        let marked = |index: usize| heard.sure[index] > self.policy.delete_threshold;
        let mut verdicts: Vec<Option<Verdict>> = heard
            .proposed
            .iter()
            .map(|proposed| proposed.then_some(Verdict::Refused(Reason::Unsure)))
            .collect();
        let mut start = 0;
        while start < count {
            if !marked(start) {
                start += 1;
                continue;
            }
            let mut end = start;
            while end < count && marked(end) {
                end += 1;
            }
            self.judge(&heard, start..end, &mut verdicts);
            start = end;
        }
        if self.policy.something_left && leaves_nothing(&heard, &verdicts) {
            for verdict in verdicts.iter_mut().flatten() {
                if matches!(verdict, Verdict::Admitted(_)) {
                    *verdict = Verdict::Refused(Reason::LeavesNothing);
                }
            }
        }
        settle(words, &verdicts)
    }

    /// Judges one run of marked words.
    fn judge(&self, heard: &Heard<'_>, run: Range<usize>, verdicts: &mut [Option<Verdict>]) {
        let Heard {
            words, bares, sure, ..
        } = heard;
        let count = words.len();
        let run_bares = &bares[run.clone()];
        let surest = sure[run.clone()]
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        let protection: Vec<Option<Protection>> = run
            .clone()
            .map(|index| self.protection(heard, index))
            .collect();
        let cues = cue_mask(run_bares);
        let fillers: Vec<bool> = run_bares
            .iter()
            .map(|bare| PURE_FILLERS.contains(&bare.as_str()) || bare.is_empty())
            .collect();
        let mut admit_all = |kind: Disfluency| {
            for index in run.clone() {
                verdicts[index] = Some(Verdict::Admitted(kind));
            }
        };

        let mut repair = surest > self.policy.repair_floor
            && cues.contains(&true)
            && cues
                .iter()
                .zip(&fillers)
                .any(|(cue, filler)| !cue && !filler);
        if !repair && fillers.contains(&true) && run.end < count {
            let content: Vec<usize> = run
                .clone()
                .filter(|index| !fillers[index - run.start])
                .collect();
            let next = &bares[run.end..count.min(run.end + 4)];
            if let Some(&last) = content.last()
                && ((content.iter().all(|&index| has_digit(words[index]))
                    && has_digit(words[run.end]))
                    || next.contains(&bares[last]))
            {
                repair = surest > self.policy.repair_floor;
            }
        }
        if repair {
            admit_all(Disfluency::Correction);
            return;
        }

        let after = &bares[run.end..];
        let first = run_bares[0].as_str();
        let is_repeat = after.get(..run.len()) == Some(run_bares)
            || (!first.is_empty() && after.first().is_some_and(|next| next == first))
            || (run.len() == 1
                && words[run.start].ends_with('-')
                && !first.is_empty()
                && after.first().is_some_and(|next| next.starts_with(first)));
        if is_repeat {
            for index in run {
                let emphasis = ADVERBS.contains(&bares[index].as_str())
                    || (self.policy.emphatic_repeats && is_emphatic(bares, index));
                verdicts[index] = Some(if emphasis {
                    Verdict::Refused(Reason::Emphasis)
                } else {
                    Verdict::Admitted(Disfluency::Repeat)
                });
            }
            return;
        }

        if protection.iter().all(Option::is_none) {
            let starts_clause =
                run.start == 0 || ends_with_any(words[run.start - 1], &['.', '?', '!', ',']);
            let false_start = starts_clause
                && run.end < count
                && ends_with_pause(words[run.end - 1])
                && (surest > self.policy.sure_floor
                    || run_bares
                        .iter()
                        .all(|bare| FUNCTION_WORDS.contains(&bare.as_str())));
            if false_start {
                admit_all(Disfluency::FalseStart);
                return;
            }
        }
        for index in run.clone() {
            verdicts[index] = Some(match self.filler(heard, &run, index) {
                Some(Disfluency::Filler)
                    if self.policy.mentioned_fillers && is_mentioned(heard, index) =>
                {
                    Verdict::Refused(Reason::Mentioned)
                }
                Some(kind) => Verdict::Admitted(kind),
                None => Verdict::Refused(
                    protection[index - run.start].map_or(Reason::NotADisfluency, Reason::Protected),
                ),
            });
        }
    }

    /// Whether the word at `index` of a run that is no correction, repeat or
    /// false start goes on its own, and as what.
    fn filler(&self, heard: &Heard<'_>, run: &Range<usize>, index: usize) -> Option<Disfluency> {
        let Heard {
            words, bares, sure, ..
        } = heard;
        let word = bares[index].as_str();
        let previous = if index > 0 { words[index - 1] } else { "" };
        let previous_bare = if index > 0 {
            bares[index - 1].as_str()
        } else {
            ""
        };
        let next_in_run = (index + 1 < run.end).then(|| bares[index + 1].as_str());
        let previous_in_run = (index > run.start).then(|| bares[index - 1].as_str());
        if PURE_FILLERS.contains(&word) {
            return Some(Disfluency::Filler);
        }
        if words[index].ends_with('-')
            && !word.is_empty()
            && bares
                .get(index + 1)
                .is_some_and(|next| next.starts_with(word))
        {
            return Some(Disfluency::Repeat); // "w- we"
        }
        let is_sure = |index: usize| sure[index] > self.policy.sure_floor;
        let goes = match word {
            "like" => {
                let next = bares.get(index + 1).map_or("", String::as_str);
                let commas = words[index].ends_with(',') || previous.ends_with(',');
                commas
                    || (BE.contains(&previous_bare)
                        && !NOUN_STARTS.contains(&next)
                        && !next.is_empty())
                    || (is_sure(index) && !LIKE_VERB_BEFORE.contains(&previous_bare))
            }
            "know" if previous_in_run == Some("you") => {
                let you = index - 1;
                words[index].ends_with(',')
                    || (you > 0 && words[you - 1].ends_with(','))
                    || (is_sure(index) && is_sure(you))
            }
            "you" if next_in_run == Some("know") => {
                let know = index + 1;
                words[know].ends_with(',')
                    || previous.ends_with(',')
                    || (is_sure(index) && is_sure(know))
            }
            "i" if next_in_run == Some("mean") => {
                let mean = index + 1;
                words[mean].ends_with(',') || (is_sure(index) && is_sure(mean))
            }
            "mean" if previous_in_run == Some("i") => {
                words[index].ends_with(',') || (is_sure(index) && is_sure(index - 1))
            }
            _ => false,
        };
        goes.then_some(Disfluency::Filler)
    }

    fn protection(&self, heard: &Heard<'_>, index: usize) -> Option<Protection> {
        let word = heard.words[index];
        let bare = heard.bares[index].as_str();
        if has_inner_hyphen(word) {
            Some(Protection::Hyphenated)
        } else if has_digit(word) {
            Some(Protection::Digit)
        } else if self.protected.0.contains(bare) {
            Some(Protection::Vocabulary)
        } else if ADVERBS.contains(&bare) {
            Some(Protection::Adverb)
        } else if is_capitalized_mid_sentence(heard, index) {
            Some(Protection::Name)
        } else if self.policy.negations && is_negation(bare) {
            Some(Protection::Negation)
        } else {
            None
        }
    }
}

/// The transcript as the guard reads it.
struct Heard<'a> {
    words: Vec<&'a str>,
    /// Each word as it is compared ([`bare`]).
    bares: Vec<String>,
    /// How sure the surest proposal covering each word is: 0 for a word
    /// nothing proposed.
    sure: Vec<f64>,
    proposed: Vec<bool>,
}

impl<'a> Heard<'a> {
    fn new(words: &'a Words, proposals: &[Proposal]) -> Self {
        let words: Vec<&str> = words.iter().collect();
        let mut sure = vec![0.0_f64; words.len()];
        let mut proposed = vec![false; words.len()];
        for proposal in proposals {
            for index in proposal.at().range().filter(|&index| index < words.len()) {
                sure[index] = sure[index].max(proposal.confidence());
                proposed[index] = true;
            }
        }
        Self {
            bares: words.iter().copied().map(bare).collect(),
            words,
            sure,
            proposed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Verdict {
    Admitted(Disfluency),
    Refused(Reason),
}

/// Whether what the admitted edits leave has no word in it.
fn leaves_nothing(heard: &Heard<'_>, verdicts: &[Option<Verdict>]) -> bool {
    let goes = |index: usize| matches!(verdicts[index], Some(Verdict::Admitted(_)));
    (0..verdicts.len()).any(goes)
        && (0..verdicts.len()).all(|index| goes(index) || heard.bares[index].is_empty())
}

/// Neighbouring words with the same verdict become one edit or one refusal.
fn settle(words: &Words, verdicts: &[Option<Verdict>]) -> Admitted {
    let mut edits = Vec::new();
    let mut refused = Vec::new();
    let mut start = 0;
    while start < verdicts.len() {
        let mut end = start + 1;
        while end < verdicts.len() && verdicts[end] == verdicts[start] {
            end += 1;
        }
        if let (Some(verdict), Some(at)) = (verdicts[start], words.span(start..end)) {
            match verdict {
                Verdict::Admitted(kind) => edits.push(Edit {
                    at,
                    op: EditOp::Delete(kind),
                }),
                Verdict::Refused(reason) => refused.push(Refused { at, reason }),
            }
        }
        start = end;
    }
    Admitted {
        plan: EditPlan::admitted(edits),
        refused,
    }
}

/// Whether the word at `index` is one of three or more of the same content
/// word in a row.
fn is_emphatic(bares: &[String], index: usize) -> bool {
    let word = &bares[index];
    if word.is_empty() || FUNCTION_WORDS.contains(&word.as_str()) {
        return false;
    }
    let before = bares[..index]
        .iter()
        .rev()
        .take_while(|bare| *bare == word)
        .count();
    let after = bares[index + 1..]
        .iter()
        .take_while(|bare| *bare == word)
        .count();
    before + 1 + after >= 3
}

/// Whether the filler at `index` is talked about: quoted, the subject of a
/// verb with no pause between them, or what its sentence names.
fn is_mentioned(heard: &Heard<'_>, index: usize) -> bool {
    let Heard { words, bares, .. } = heard;
    let word = words[index];
    let unpaused = |word: &str| word.chars().next_back().is_some_and(is_word_character);
    if word.contains(QUOTES) {
        return true;
    }
    if unpaused(word)
        && bares
            .get(index + 1)
            .is_some_and(|next| SAID_OF_A_WORD.contains(&next.as_str()))
    {
        return true;
    }
    index > 0
        && unpaused(words[index - 1])
        && NAMES_A_WORD.contains(&bares[index - 1].as_str())
        && (index + 1 == words.len() || ends_with_any(word, &['.', '?', '!']))
}

fn is_negation(bare: &str) -> bool {
    NEGATIONS.contains(&bare) || bare.ends_with("n't") || bare.ends_with("n\u{2019}t")
}

fn is_capitalized_mid_sentence(heard: &Heard<'_>, index: usize) -> bool {
    let Heard { words, bares, .. } = heard;
    let capitalized = words[index]
        .chars()
        .find(|&ch| is_word_character(ch))
        .is_some_and(char::is_uppercase);
    if index == 0 || !capitalized {
        return false;
    }
    if ends_with_any(words[index - 1], &['.', '?', '!', '\u{2026}', ':', '"']) {
        return false;
    }
    let word = bares[index].as_str();
    word != "i" && !word.starts_with("i'")
}

/// Which words of a run belong to a correction cue.
fn cue_mask(bares: &[String]) -> Vec<bool> {
    let mut mask = vec![false; bares.len()];
    for start in 0..bares.len() {
        for cue in CUES {
            let end = start + cue.len();
            if bares
                .get(start..end)
                .is_some_and(|said| said.iter().eq(cue.iter()))
            {
                mask[start..end].fill(true);
            }
        }
    }
    mask
}

/// A word as it is compared: lowercased and composed, without leading
/// punctuation (but "$", "#") or trailing punctuation (but "%").
fn bare(word: &str) -> String {
    let lowered: String = word.to_lowercase().nfc().collect();
    lowered
        .trim_start_matches(|ch| !(is_word_character(ch) || ch == '$' || ch == '#'))
        .trim_end_matches(|ch| !(is_word_character(ch) || ch == '%'))
        .to_owned()
}

fn is_word_character(ch: char) -> bool {
    ch.is_alphabetic() || ch.is_numeric() || ch == '_'
}

fn has_digit(word: &str) -> bool {
    word.chars().any(char::is_numeric)
}

fn has_inner_hyphen(word: &str) -> bool {
    let characters: Vec<char> = word.chars().collect();
    characters
        .windows(3)
        .any(|window| window[1] == '-' && window[0].is_alphabetic() && window[2].is_alphabetic())
}

fn ends_with_any(word: &str, marks: &[char]) -> bool {
    word.chars()
        .next_back()
        .is_some_and(|last| marks.contains(&last))
}

fn ends_with_pause(word: &str) -> bool {
    word.ends_with("...") || ends_with_any(word, &[',', ';', '\u{2026}', '\u{2014}', '\u{2013}'])
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;
