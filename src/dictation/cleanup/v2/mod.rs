//! Labs › Clean up what I say v2 (a Dev lab, macOS): the second cleanup
//! tagger, in place of v1 while its lab is on.
//!
//! Where v1 scores one thing, how sure it is a word should go, v2 answers
//! four for every word: what happens to it (kept, or deleted as a filler,
//! an opener, a restart, or a correction), the punctuation after it, its
//! case, and which span it belongs to (a number, unit, email, link, or
//! code). [`decode`] reads those, [`itn`] writes each tagged span the way it
//! is read ("two hundred fifty milliseconds" is 250 milliseconds), and
//! [`render`] writes the line. Like v1 it never writes a word that was not
//! heard; its spans only turn spoken words into digits and symbols.
//!
//! The model is a folder on this Mac, not a download (`runtime`): a
//! missing folder, a late answer, or a failed run pastes the words as heard
//! and History says why.

mod bpe;
mod clock;
mod decode;
mod itn;
mod render;
mod runtime;

use std::time::{Duration, Instant};

use super::guard::{CleanupGuard, Policy, ProtectedTerms};
use super::stage::Skip;
use super::words::Words;
use crate::dictation::processor::ProcessingObservation;
use crate::dictation::vocabulary::VocabularyEntry;
use decode::Action;
use runtime::Scored;

/// The step's name in History and the live log.
const PROFILE: &str = "Clean up what I say v2";

#[cfg(all(target_os = "macos", not(test)))]
static TAGGER: super::tagger::Tagger<runtime::CoreMlV2> =
    super::tagger::Tagger::new(runtime::CoreMlV2);

/// Loads the model in the background (compiling it the first time), so the
/// next dictation finds it warm.
#[cfg(all(target_os = "macos", not(test)))]
pub(super) fn warm() {
    TAGGER.warm();
}

/// Frees the loaded model.
pub(super) fn release() {
    #[cfg(all(target_os = "macos", not(test)))]
    TAGGER.release();
}

#[cfg(all(target_os = "macos", not(test)))]
pub(super) fn clean(
    text: &str,
    vocabulary: &[VocabularyEntry],
) -> (String, Option<ProcessingObservation>) {
    let vocabulary =
        crate::dictation::vocabulary::merged(vocabulary, &crate::dictation::vocabulary::entries());
    let protected = ProtectedTerms::new(vocabulary.iter().map(|entry| entry.term.as_str()));
    // A model still loading (the first dictation after launch) is given
    // the time a load and its warm-up run take; a warm one, the budget.
    let budget = if TAGGER.loaded() {
        super::stage::TIME_BUDGET
    } else {
        COLD_BUDGET
    };
    clean_with(text, &protected, budget, |lines, deadline| {
        TAGGER.score(lines, deadline)
    })
}

/// How long a dictation waits for a model that is not loaded yet: its load
/// (about 30 ms) and warm-up run (about 120 ms on a cold Core ML), with room.
#[cfg(any(all(target_os = "macos", not(test)), test))]
const COLD_BUDGET: Duration = Duration::from_millis(1_000);

/// No other OS runs the tagger yet.
#[cfg(not(any(target_os = "macos", test)))]
pub(super) fn clean(
    text: &str,
    _vocabulary: &[VocabularyEntry],
) -> (String, Option<ProcessingObservation>) {
    (text.to_owned(), None)
}

/// A test's dictation is tagged by the stand-in its thread names, and stays
/// as heard without one: no test loads the real model.
#[cfg(test)]
pub(super) fn clean(
    text: &str,
    vocabulary: &[VocabularyEntry],
) -> (String, Option<ProcessingObservation>) {
    let Some(tagger) = tests::STAND_IN.with(|tagger| tagger.borrow().clone()) else {
        return (text.to_owned(), None);
    };
    let protected = ProtectedTerms::new(vocabulary.iter().map(|entry| entry.term.as_str()));
    clean_with(text, &protected, super::stage::TIME_BUDGET, |lines, _| {
        tagger.map(|delete| tests::deleting(delete, &lines))
    })
}

/// Stands a v2 tagger in for the real one on this test's thread: one that
/// deletes exactly these words as fillers, or one that cannot answer.
#[cfg(test)]
pub(crate) fn stand_in(tagger: Option<Result<&'static [&'static str], Skip>>) {
    tests::STAND_IN.with(|current| *current.borrow_mut() = tagger);
}

/// How many words one cleanup removed, by what they were, and how many the
/// guards kept.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Removed {
    fillers: usize,
    openers: usize,
    restarts: usize,
    corrections: usize,
    protected: usize,
}

/// One cleanup with its model and time budget handed in. `score` answers
/// each line's words by the deadline it is given.
fn clean_with(
    text: &str,
    protected: &ProtectedTerms,
    budget: Duration,
    score: impl FnOnce(Vec<Vec<String>>, Instant) -> Result<Vec<Scored>, Skip>,
) -> (String, Option<ProcessingObservation>) {
    let started = Instant::now();
    let outcome = tagged(text, protected, started + budget, score);
    let latency_ms = started.elapsed().as_millis() as u64;
    let observation = |fallback| ProcessingObservation {
        profile: PROFILE.into(),
        latency_ms,
        // The tagger runs on this Mac; no OpenCode model is asked.
        model: None,
        fallback,
    };
    match outcome {
        Ok((cleaned, words, changed, removed)) => {
            tracing::info!(
                latency_ms,
                words,
                changed_lines = changed,
                fillers = removed.fillers,
                openers = removed.openers,
                restarts = removed.restarts,
                corrections = removed.corrections,
                protected = removed.protected,
                "dictation tagger v2 cleanup completed"
            );
            (cleaned, Some(observation(None)))
        }
        Err(skip) => {
            let reason = match skip {
                Skip::Late if budget > super::stage::TIME_BUDGET => format!(
                    "the cleanup model was still loading after {} ms",
                    budget.as_millis()
                ),
                _ => skip.reason(),
            };
            tracing::warn!(
                latency_ms,
                %reason,
                "dictation tagger v2 cleanup kept the words as heard"
            );
            (text.to_owned(), Some(observation(Some(reason))))
        }
    }
}

/// The cleaned text, how many words were heard, how many lines changed,
/// and what went. Each line is cleaned on its own and keeps the space
/// around it, so spoken formatting's line breaks stay where they are.
fn tagged(
    text: &str,
    protected: &ProtectedTerms,
    deadline: Instant,
    score: impl FnOnce(Vec<Vec<String>>, Instant) -> Result<Vec<Scored>, Skip>,
) -> Result<(String, usize, usize, Removed), Skip> {
    let lines: Vec<(&str, Words)> = text
        .split('\n')
        .map(|line| (line, Words::new(line)))
        .collect();
    let asked: Vec<Vec<String>> = lines
        .iter()
        .filter(|(_, words)| !words.is_empty())
        .map(|(_, words)| words.iter().map(str::to_owned).collect())
        .collect();
    let heard = asked.iter().map(Vec::len).sum();
    if asked.is_empty() {
        return Ok((text.to_owned(), 0, 0, Removed::default()));
    }
    let count = asked.len();
    let scores = score(asked, deadline)?;
    if scores.len() != count {
        return Err(Skip::Failed(
            "the cleanup model answered fewer lines than it was given".into(),
        ));
    }
    let guard = CleanupGuard {
        policy: Policy::DESKTOP,
        protected,
    };
    let mut scores = scores.into_iter();
    let mut removed = Removed::default();
    let mut changed = 0;
    let mut cleaned = Vec::with_capacity(lines.len());
    for (line, words) in &lines {
        if words.is_empty() {
            cleaned.push((*line).to_owned());
            continue;
        }
        let Some(scored) = scores
            .next()
            .filter(|scored| scored.words.len() == words.len())
        else {
            return Err(Skip::Failed(
                "the cleanup model answered fewer words than it was given".into(),
            ));
        };
        let mut tags = decode::tags(&scored.words, &scored.decode);
        removed.protected += decode::protect(words, &mut tags, &guard);
        let heard: Vec<&str> = words.iter().collect();
        for action in render::actions(&heard, &tags, &scored.decode) {
            match action {
                Action::Keep => {}
                Action::DeleteFiller => removed.fillers += 1,
                Action::DeleteOpener => removed.openers += 1,
                Action::DeleteRestart => removed.restarts += 1,
                Action::DeleteCorrection => removed.corrections += 1,
            }
        }
        let rendered = render::render(&heard, &tags, &scored.decode);
        if rendered.is_empty() || rendered == heard.join(" ") {
            cleaned.push((*line).to_owned());
            continue;
        }
        changed += 1;
        let leading = line.len() - line.trim_start().len();
        let trailing = line.trim_end().len();
        cleaned.push(format!(
            "{}{rendered}{}",
            &line[..leading],
            &line[trailing..]
        ));
    }
    Ok((cleaned.join("\n"), heard, changed, removed))
}

#[cfg(test)]
mod parity_tests;
#[cfg(all(test, target_os = "macos"))]
mod real_model_tests;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::decode::{ACTIONS, CASES, Decode, PUNCTS, SPANS, WordLogits};
    use super::*;

    thread_local! {
        /// The tagger [`stand_in`] named for this thread.
        pub(super) static STAND_IN: RefCell<Option<Result<&'static [&'static str], Skip>>> =
            const { RefCell::new(None) };
    }

    /// A tagger sure that each word in `delete`, read without its
    /// punctuation, is a filler, and sure of nothing else.
    pub(super) fn deleting(delete: &[&str], lines: &[Vec<String>]) -> Vec<Scored> {
        lines
            .iter()
            .map(|words| Scored {
                words: words
                    .iter()
                    .map(|word| {
                        let goes = delete.contains(&itn::norm(word).as_str());
                        let mut action = [-8.0; ACTIONS];
                        action[usize::from(goes)] = 8.0;
                        WordLogits {
                            action,
                            punct: [0.0; PUNCTS],
                            case: [0.0; CASES],
                            span: [0.0; SPANS],
                        }
                    })
                    .collect(),
                decode: Decode::default(),
            })
            .collect()
    }

    fn cleaned(
        text: &str,
        terms: &[&str],
        delete: &'static [&'static str],
    ) -> (String, Option<ProcessingObservation>) {
        let protected = ProtectedTerms::new(terms.iter().copied());
        clean_with(text, &protected, Duration::from_secs(5), |lines, _| {
            Ok(deleting(delete, &lines))
        })
    }

    #[test]
    fn each_line_is_cleaned_and_its_spacing_and_breaks_stay() {
        let (text, observation) = cleaned(
            "Hi Sam,\n\nUm, the build is green.\n  We can, uh, ship today.  \nThanks",
            &[],
            &["um", "uh"],
        );
        assert_eq!(
            text,
            "Hi Sam,\n\nThe build is green.\n  We can, ship today.  \nThanks"
        );
        let observation = observation.unwrap();
        assert_eq!(observation.profile, "Clean up what I say v2");
        assert_eq!((observation.model, observation.fallback), (None, None));
        // A clean dictation comes back exactly, spacing included.
        let said = "  Ship it on  Thursday.\n\nThanks!  ";
        assert_eq!(cleaned(said, &[], &[]).0, said);
    }

    /// The guard keeps a vocabulary term, a name, a number, and a negation,
    /// however sure the model is.
    #[test]
    fn the_guard_keeps_what_the_model_may_not_remove() {
        let said = "Tell Priya the kubectl rollout did not finish by 5pm.";
        let (text, _) = cleaned(said, &["kubectl"], &["priya", "kubectl", "not", "5pm"]);
        assert_eq!(text, said);
    }

    #[test]
    fn a_missing_late_or_failing_model_keeps_the_words_as_heard() {
        let said = "Um, so we go";
        let protected = ProtectedTerms::default();
        for (skip, why) in [
            (Skip::NotInstalled, "the cleanup v2 model is not installed"),
            (Skip::Late, "the cleanup took longer than 150 ms"),
            (Skip::Failed("no model".into()), "no model"),
        ] {
            let (text, observation) = clean_with(
                said,
                &protected,
                super::super::stage::TIME_BUDGET,
                |_, _| Err(skip),
            );
            assert_eq!(text, said);
            assert_eq!(observation.unwrap().fallback.as_deref(), Some(why));
        }
        // A model still loading past the cold budget says so.
        let (_, observation) = clean_with(said, &protected, COLD_BUDGET, |_, _| Err(Skip::Late));
        assert_eq!(
            observation.unwrap().fallback.as_deref(),
            Some("the cleanup model was still loading after 1000 ms")
        );
        // An answer of the wrong shape is no answer.
        let (text, observation) =
            clean_with(said, &protected, Duration::from_secs(5), |lines, _| {
                let mut scored = deleting(&["um"], &lines);
                scored[0].words.pop();
                Ok(scored)
            });
        assert_eq!(text, said);
        assert!(observation.unwrap().fallback.is_some());
        let (_, observation) = clean_with(said, &protected, Duration::from_secs(5), |_, _| {
            Ok(Vec::new())
        });
        assert!(observation.unwrap().fallback.is_some());
        // Nothing said asks nothing.
        let (text, _) = clean_with(" \n ", &protected, Duration::from_secs(5), |_, _| {
            panic!("asked about no words")
        });
        assert_eq!(text, " \n ");
    }
}
