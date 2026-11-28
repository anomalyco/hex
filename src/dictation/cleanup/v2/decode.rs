//! The model's four answers for each word, decoded into what happens to
//! it: kept or deleted (and why), the punctuation after it, its case, and
//! the span it belongs to.
//!
//! Deletion is gated the way the model was calibrated (GECToR's confidence
//! bias): KEEP's probability gets `keep_bias` added before the argmax, and a
//! deletion must also be at least `min_edit_prob` sure. Then two guards only
//! ever keep more:
//! - a correction must carry both what was taken back and a cue ("Tuesday,
//!   no wait,"), never a cue alone or a number's first word
//!   ([`guard_corrections`], the reference's `_guard_correction_runs`);
//! - outside a correction, a run is kept whole when Hex's [`CleanupGuard`]
//!   protects any word of it: a hyphenated compound, a word with a digit, a
//!   vocabulary term, a name, or a negation ([`protect`]).
//!
//! A port of the tagger's Python reference (`decode.py`, `itn.py`), checked
//! against the real model's recorded logits by `parity_tests`.

use super::super::guard::{CleanupGuard, Protection};
use super::super::words::Words;

/// What happens to a word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Keep,
    DeleteFiller,
    DeleteOpener,
    DeleteRestart,
    DeleteCorrection,
}

impl Action {
    const ALL: [Self; 5] = [
        Self::Keep,
        Self::DeleteFiller,
        Self::DeleteOpener,
        Self::DeleteRestart,
        Self::DeleteCorrection,
    ];

    pub(crate) fn is_delete(self) -> bool {
        self != Self::Keep
    }
}

/// The punctuation the model would write after a word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Punct {
    None,
    Comma,
    Period,
    Question,
    Exclamation,
    Colon,
    Newline,
    Bullet,
}

impl Punct {
    const ALL: [Self; 8] = [
        Self::None,
        Self::Comma,
        Self::Period,
        Self::Question,
        Self::Exclamation,
        Self::Colon,
        Self::Newline,
        Self::Bullet,
    ];

    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Comma => ",",
            Self::Period => ".",
            Self::Question => "?",
            Self::Exclamation => "!",
            Self::Colon => ":",
            Self::Newline => "\n",
            Self::Bullet => "\n- ",
        }
    }
}

/// The case the model would give a word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Case {
    Keep,
    Lower,
    Title,
    Upper,
}

impl Case {
    const ALL: [Self; 4] = [Self::Keep, Self::Lower, Self::Title, Self::Upper];
}

/// The span a word belongs to, in BIO form: `Begin` starts one, `Inside`
/// continues it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Span {
    Outside,
    Begin(super::itn::Kind),
    Inside(super::itn::Kind),
}

impl Span {
    const ALL: [Self; 11] = {
        use super::itn::Kind::{Code, Email, Number, Unit, Url};
        [
            Self::Outside,
            Self::Begin(Number),
            Self::Inside(Number),
            Self::Begin(Email),
            Self::Inside(Email),
            Self::Begin(Url),
            Self::Inside(Url),
            Self::Begin(Code),
            Self::Inside(Code),
            Self::Begin(Unit),
            Self::Inside(Unit),
        ]
    };

    pub(crate) fn kind(self) -> Option<super::itn::Kind> {
        match self {
            Self::Outside => None,
            Self::Begin(kind) | Self::Inside(kind) => Some(kind),
        }
    }
}

/// How many classes each head has, in the order the model writes them.
pub(crate) const ACTIONS: usize = Action::ALL.len();
pub(crate) const PUNCTS: usize = Punct::ALL.len();
pub(crate) const CASES: usize = Case::ALL.len();
pub(crate) const SPANS: usize = Span::ALL.len();

/// The schema's label names, head by head, as `runtime.json` lists them.
pub(crate) const SCHEMA: [(&str, &[&str]); 4] = [
    (
        "action",
        &[
            "KEEP",
            "DELETE_FILLER",
            "DELETE_OPENER",
            "DELETE_RESTART",
            "DELETE_CORRECTION",
        ],
    ),
    (
        "punct_after",
        &[
            "NONE",
            "COMMA",
            "PERIOD",
            "QUESTION",
            "EXCLAMATION",
            "COLON",
            "NEWLINE",
            "BULLET",
        ],
    ),
    ("case", &["KEEP", "LOWER", "TITLE", "UPPER"]),
    (
        "span",
        &[
            "O", "B_NUMBER", "I_NUMBER", "B_EMAIL", "I_EMAIL", "B_URL", "I_URL", "B_CODE",
            "I_CODE", "B_UNIT", "I_UNIT",
        ],
    ),
];

/// Everything decided about one word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Tag {
    pub(crate) action: Action,
    pub(crate) punct: Punct,
    pub(crate) case: Case,
    pub(crate) span: Span,
}

impl Tag {
    pub(crate) const KEEP: Self = Self {
        action: Action::Keep,
        punct: Punct::None,
        case: Case::Keep,
        span: Span::Outside,
    };

    /// A tag from the schema's label names, as the reference's fixtures
    /// write them.
    #[cfg(test)]
    pub(crate) fn from_labels(action: &str, punct: &str, case: &str, span: &str) -> Option<Self> {
        let at = |head: usize, label: &str| SCHEMA[head].1.iter().position(|name| *name == label);
        Some(Self {
            action: Action::ALL[at(0, action)?],
            punct: Punct::ALL[at(1, punct)?],
            case: Case::ALL[at(2, case)?],
            span: Span::ALL[at(3, span)?],
        })
    }

    /// The schema's names for its action and span.
    #[cfg(test)]
    pub(crate) fn labels(self) -> (&'static str, &'static str) {
        let action = Action::ALL
            .iter()
            .position(|&it| it == self.action)
            .unwrap_or(0);
        let span = Span::ALL
            .iter()
            .position(|&it| it == self.span)
            .unwrap_or(0);
        (SCHEMA[0].1[action], SCHEMA[3].1[span])
    }
}

/// The model's logits for one word, read at its first sub-token.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WordLogits {
    pub(crate) action: [f32; ACTIONS],
    pub(crate) punct: [f32; PUNCTS],
    pub(crate) case: [f32; CASES],
    pub(crate) span: [f32; SPANS],
}

/// How the answer is read, from the model folder's `runtime.json`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Decode {
    pub(crate) keep_bias: f64,
    pub(crate) min_edit_prob: f64,
    /// Whether "Okay, so" openers go; kept by default, as the model was
    /// measured.
    pub(crate) delete_openers: bool,
    /// Whether tagged spans are written as digits and symbols.
    pub(crate) use_itn: bool,
    /// Whether the punctuation and case heads rewrite each kept word;
    /// off, the recogniser's own punctuation and capitals stay.
    pub(crate) apply_case_punct: bool,
}

impl Default for Decode {
    /// The calibration the model shipped with: it removed the most while
    /// over-deleting at most 0.5% of clean sentences and keeping every
    /// hedge.
    fn default() -> Self {
        Self {
            keep_bias: 0.20,
            min_edit_prob: 0.50,
            delete_openers: false,
            use_itn: true,
            apply_case_punct: false,
        }
    }
}

fn softmax(logits: &[f32]) -> Vec<f64> {
    let peak = logits
        .iter()
        .copied()
        .map(f64::from)
        .fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = logits
        .iter()
        .map(|&x| (f64::from(x) - peak).exp())
        .collect();
    let total: f64 = exps.iter().sum();
    exps.into_iter().map(|e| e / total).collect()
}

/// The first of the largest, as NumPy's `argmax` picks it.
fn argmax(logits: &[f32]) -> usize {
    let mut best = 0;
    for (index, &value) in logits.iter().enumerate() {
        if value > logits[best] {
            best = index;
        }
    }
    best
}

/// One word's action under the KEEP bias and the minimum edit probability.
fn action(logits: &[f32; ACTIONS], decode: &Decode) -> Action {
    let probs = softmax(logits);
    let mut best = 0;
    let mut best_score = probs[0] + decode.keep_bias;
    for (class, &p) in probs.iter().enumerate().skip(1) {
        if p > best_score {
            best = class;
            best_score = p;
        }
    }
    if best != 0 && probs[best] < decode.min_edit_prob {
        return Action::Keep;
    }
    Action::ALL[best]
}

/// Each word's tag, straight from the model's answer.
pub(crate) fn tags(logits: &[WordLogits], decode: &Decode) -> Vec<Tag> {
    logits
        .iter()
        .map(|word| Tag {
            action: action(&word.action, decode),
            punct: Punct::ALL[argmax(&word.punct)],
            case: Case::ALL[argmax(&word.case)],
            span: Span::ALL[argmax(&word.span)],
        })
        .collect()
}

const CUE_WORDS: &[&str] = &[
    "sorry",
    "actually",
    "wait",
    "rather",
    "correction",
    "oops",
    "nope",
    "no",
];
const CUE_PHRASES: &[&[&str]] = &[
    &["i", "mean"],
    &["i", "meant"],
    &["no", "wait"],
    &["scratch", "that"],
    &["strike", "that"],
    &["delete", "that"],
    &["never", "mind"],
    &["my", "bad"],
    &["hold", "on"],
    &["i'm", "sorry"],
    &["make", "that"],
    &["make", "it"],
    &["change", "that", "to"],
    &["let", "me", "correct", "that"],
    &["let", "me", "rephrase", "that"],
];
/// Hesitations a correction may carry besides its cue.
const HESITATIONS: &[&str] = &["um", "uh", "er", "erm", "ah", "hmm", "like"];
/// Discourse markers, never what a correction takes back on their own:
/// "Okay, sorry," takes nothing back.
const DISCOURSE: &[&str] = &["oh", "okay", "ok", "alright", "well", "so", "right", "yeah"];
/// How many kept words may separate a correction from a cue-less piece of
/// the same reparandum.
const SPLIT_WINDOW: usize = 3;

/// Where the last cue in `norms` ends, if there is one.
fn last_cue_end(norms: &[String]) -> Option<usize> {
    let single = norms
        .iter()
        .rposition(|word| CUE_WORDS.contains(&word.as_str()));
    let phrase = CUE_PHRASES
        .iter()
        .filter_map(|phrase| {
            norms
                .windows(phrase.len())
                .rposition(|window| window.iter().map(String::as_str).eq(phrase.iter().copied()))
                .map(|at| at + phrase.len() - 1)
        })
        .max();
    single.max(phrase)
}

fn is_cue_word(word: &str) -> bool {
    CUE_WORDS.contains(&word)
        || HESITATIONS.contains(&word)
        || CUE_PHRASES.iter().any(|phrase| phrase.contains(&word))
}

/// Every run of correction deletions, with the fillers inside it, must
/// take something back, say so, and leave its repair and the sentence
/// around it whole. The guard keeps:
/// - every correction word of a run without a cue ("came to four hundred
///   twelve" keeps "four") or with nothing taken back but cues,
///   hesitations and discourse markers ("um sorry could you", "Okay,
///   sorry,");
/// - the repair: correction words after the run's last cue, other than
///   hesitations ("Tuesday. Oh no wait, Wednesday." keeps "Wednesday.");
/// - the whole run when it ends a sentence the kept words before it left
///   open ("today is Tuesday. Scratch that." would leave "today is");
/// - the whole run when a cue-less correction word it kept sits just
///   before it in the same sentence: the model split one reparandum, and
///   cutting part of it leaves a fragment ("fifty megabytes of cache, I
///   mean sixty megabytes" would leave "fifty megabytes of").
pub(crate) fn guard_corrections(words: &[&str], actions: &mut [Action]) {
    let count = actions.len();
    let mut cueless = vec![false; count];
    let mut start = 0;
    while start < count {
        if actions[start] != Action::DeleteCorrection {
            start += 1;
            continue;
        }
        let mut end = start;
        let mut corrections = Vec::new();
        while end < count
            && matches!(
                actions[end],
                Action::DeleteCorrection | Action::DeleteFiller
            )
        {
            if actions[end] == Action::DeleteCorrection {
                corrections.push(end);
            }
            end += 1;
        }
        let norms: Vec<String> = corrections
            .iter()
            .map(|&index| super::itn::norm(words[index]))
            .collect();
        let Some(cue_end) = last_cue_end(&norms) else {
            for &index in &corrections {
                actions[index] = Action::Keep;
                cueless[index] = true;
            }
            start = end;
            continue;
        };
        for (position, &index) in corrections.iter().enumerate().skip(cue_end + 1) {
            if !HESITATIONS.contains(&norms[position].as_str()) {
                actions[index] = Action::Keep;
            }
        }
        let taken_back = norms[..=cue_end].iter().any(|word| {
            !word.is_empty() && !is_cue_word(word) && !DISCOURSE.contains(&word.as_str())
        });
        let mut last_deleted = start;
        while last_deleted + 1 < end && actions[last_deleted + 1].is_delete() {
            last_deleted += 1;
        }
        let before = (0..start).rev().find(|&index| !actions[index].is_delete());
        let leaves_open = before.is_some_and(|before| {
            super::render::ends_sentence(words[last_deleted])
                && !super::render::ends_sentence(words[before])
        });
        let mut split = false;
        for back in (start.saturating_sub(SPLIT_WINDOW)..start).rev() {
            if cueless[back] {
                split = true;
                break;
            }
            if super::render::ends_sentence(words[back]) {
                break;
            }
        }
        if !taken_back || leaves_open || split {
            for index in corrections {
                actions[index] = Action::Keep;
            }
        }
        start = end;
    }
}

/// Keeps every run of deletions outside a correction, whole, when the
/// guard protects any of its words. A content adverb is left to the model,
/// which was measured to keep every hedge: "actually" is also a cue.
pub(crate) fn protect(words: &Words, tags: &mut [Tag], guard: &CleanupGuard<'_>) -> usize {
    let protections = guard.protections(words);
    let protected = |index: usize| {
        protections
            .get(index)
            .copied()
            .flatten()
            .is_some_and(|protection| protection != Protection::Adverb)
    };
    let mut kept = 0;
    let mut start = 0;
    while start < tags.len() {
        let action = tags[start].action;
        if !action.is_delete() || action == Action::DeleteCorrection {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < tags.len() && tags[end].action == action {
            end += 1;
        }
        if (start..end).any(protected) {
            for tag in &mut tags[start..end] {
                tag.action = Action::Keep;
            }
            kept += end - start;
        }
        start = end;
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::super::super::guard::{Policy, ProtectedTerms};
    use super::*;

    fn logits(action: [f32; ACTIONS]) -> WordLogits {
        WordLogits {
            action,
            punct: [0.0; PUNCTS],
            case: [0.0; CASES],
            span: [0.0; SPANS],
        }
    }

    #[test]
    fn a_deletion_must_beat_keep_by_the_bias_and_clear_the_floor() {
        let decode = Decode::default();
        // p(filler) = 0.6, p(keep) = 0.4: 0.4 + 0.2 < 0.6, so it goes.
        let sure = [0.4_f32.ln(), 0.6_f32.ln(), f32::MIN, f32::MIN, f32::MIN];
        assert_eq!(action(&sure, &decode), Action::DeleteFiller);
        // p(filler) = 0.55, p(keep) = 0.45: 0.65 beats 0.55, so it stays.
        let close = [0.45_f32.ln(), 0.55_f32.ln(), f32::MIN, f32::MIN, f32::MIN];
        assert_eq!(action(&close, &decode), Action::Keep);
        // Without the bias a deletion wins, but no deletion is half sure.
        let split = [0.0_f32, 0.4_f32.ln(), 0.4_f32.ln(), 0.0_f32, 0.0_f32];
        let unbiased = Decode {
            keep_bias: -1.0,
            ..decode
        };
        assert_eq!(action(&split, &unbiased), Action::Keep);
        let tags = tags(&[logits(sure)], &decode);
        assert_eq!(tags[0].span, Span::Outside);
        assert_eq!(tags[0].case, Case::Keep);
    }

    #[test]
    fn a_correction_needs_a_cue_and_something_taken_back() {
        use Action::{DeleteCorrection as C, DeleteFiller as F, Keep as K};
        let cases: [(&[&str], &[Action], &[Action]); 8] = [
            (
                &["um", "sorry", "could", "you"],
                &[F, C, K, K],
                &[F, K, K, K],
            ),
            (
                &["honestly", "I", "mean", "it"],
                &[K, C, C, K],
                &[K, K, K, K],
            ),
            (
                &["came", "to", "four", "hundred", "twelve"],
                &[K, K, C, K, K],
                &[K, K, K, K, K],
            ),
            (
                &["Meet", "Tuesday,", "um,", "actually", "Wednesday."],
                &[K, C, F, C, K],
                &[K, C, F, C, K],
            ),
            // The repair after the last cue stays, and the discourse
            // marker the model swept in with it.
            (
                &[
                    "is",
                    "Tuesday.",
                    "Oh",
                    "no",
                    "wait,",
                    "Wednesday.",
                    "Alright,",
                ],
                &[K, C, C, C, C, C, C],
                &[K, C, C, C, C, K, K],
            ),
            // A cue with only discourse markers takes nothing back.
            (
                &["Okay,", "sorry,", "let's", "start."],
                &[C, C, K, K],
                &[K, K, K, K],
            ),
            // Ending a sentence the kept words left open.
            (
                &["pizza", "with", "mushrooms.", "Scratch", "that.", "Order"],
                &[K, K, C, C, C, K],
                &[K, K, K, K, K, K],
            ),
            // One reparandum the model split around "of".
            (
                &["fifty", "megabytes", "of", "cache,", "I", "mean", "sixty"],
                &[C, C, K, C, C, C, K],
                &[K, K, K, K, K, K, K],
            ),
        ];
        for (words, actions, expected) in cases {
            let mut actions = actions.to_vec();
            guard_corrections(words, &mut actions);
            assert_eq!(actions, expected, "{words:?}");
        }
    }

    #[test]
    fn a_protected_word_keeps_its_whole_run_outside_a_correction() {
        let said =
            Words::new("Tell Priya, tell Sam we did not, we shipped it, actually, with kubectl");
        let terms = ProtectedTerms::new(["kubectl"]);
        let guard = CleanupGuard {
            policy: Policy::DESKTOP,
            protected: &terms,
        };
        let restart = |range: std::ops::Range<usize>, tags: &mut [Tag]| {
            for tag in &mut tags[range] {
                tag.action = Action::DeleteRestart;
            }
        };
        let mut tags = vec![Tag::KEEP; said.len()];
        // "Tell Priya," holds a name, "we did not," a negation.
        restart(0..2, &mut tags);
        restart(4..7, &mut tags);
        tags[10].action = Action::DeleteFiller; // "actually,": an adverb only
        tags[12].action = Action::DeleteFiller; // "kubectl": vocabulary
        assert_eq!(protect(&said, &mut tags, &guard), 6);
        let gone: Vec<usize> = (0..tags.len())
            .filter(|&index| tags[index].action.is_delete())
            .collect();
        assert_eq!(gone, [10]);

        // A correction is the guard's own to judge.
        let mut tags = vec![Tag::KEEP; said.len()];
        tags[1].action = Action::DeleteCorrection;
        assert_eq!(protect(&said, &mut tags, &guard), 0);
        assert_eq!(tags[1].action, Action::DeleteCorrection);
    }
}
