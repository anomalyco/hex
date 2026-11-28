//! Labs › Clean up what I say: the cleanup tagger as one step of a macOS
//! dictation, after speech preparation and the mode's corrections and before
//! the Apple Intelligence pass or the OpenCode rewrite.
//!
//! The words as heard go to the tagger in chunks, its answer becomes
//! proposals, the guard admits the ones that are disfluencies it knows, and
//! the admitted plan is rendered. The step can only take words out. It is
//! skipped for a language other than English (the tagger only knows English:
//! on Spanish it dropped negations), for the Verbatim style, for a
//! transcript of numbers alone, and for typed text.
//!
//! Whatever happens, the dictation pastes: a model that is still
//! downloading, an answer later than [`TIME_BUDGET`], or a failed run keeps
//! the words as heard and records why. The log gets counts and
//! milliseconds, never a word of the dictation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::chunk::Chunk;
use super::guard::{CleanupGuard, Policy, ProtectedTerms};
use super::plan::{Disfluency, EditOp, Proposal};
use super::render::render;
use super::tokenizer::Tokenizer;
use super::words::Words;
use crate::app_settings::DictationStyle;
use crate::dictation::processor::ProcessingObservation;
use crate::dictation::vocabulary::VocabularyEntry;

/// How long a dictation waits for its cleanup before it pastes as heard
/// (the iPhone app's budget).
pub(crate) const TIME_BUDGET: Duration = Duration::from_millis(150);

/// The step's name in History and the live log.
const PROFILE: &str = "Clean up what I say";

/// Why a dictation that asked for a cleanup pasted as heard.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Skip {
    /// The model's answer was not there by the deadline.
    Late,
    /// The model is not installed yet; it downloads in the background.
    ModelMissing,
    /// Labs › Clean up what I say v2's model folder is not on this Mac.
    NotInstalled,
    /// The model could not load or run.
    Failed(String),
}

impl Skip {
    /// The reason History shows after "fell back:".
    pub(super) fn reason(&self) -> String {
        match self {
            Self::Late => format!(
                "the cleanup took longer than {} ms",
                TIME_BUDGET.as_millis()
            ),
            Self::ModelMissing => "the cleanup model is still downloading".into(),
            Self::NotInstalled => "the cleanup v2 model is not installed".into(),
            Self::Failed(why) => why.clone(),
        }
    }
}

/// What the step reads about one dictation, besides its words.
#[derive(Clone, Copy)]
pub(crate) struct Dictation<'a> {
    /// The language the speech model was asked for: a BCP 47 tag, or "auto".
    pub(crate) language: &'a str,
    /// Labs › Dictation style's choice, while that lab is on.
    pub(crate) style: Option<DictationStyle>,
    /// Whether the words are numbers alone.
    pub(crate) numbers_only: bool,
    pub(crate) cancelled: &'a AtomicBool,
    /// The mode's own vocabulary; the shared list joins it.
    pub(crate) vocabulary: &'a [VocabularyEntry],
}

/// Whether the tagger reads a dictation in `language`. Only English: "auto"
/// says nothing about the language, so it stays as heard, like every other
/// language.
pub(crate) fn applies_to(language: &str) -> bool {
    let primary = language.trim().split(['-', '_']).next().unwrap_or_default();
    primary.eq_ignore_ascii_case("en")
}

/// Whether one dictation is cleaned, over what its caller read: the lab's
/// switch, and the dictation itself. Pure, so every OS tests it.
pub(crate) fn wanted(lab: bool, text: &str, dictation: &Dictation<'_>) -> bool {
    lab && applies_to(dictation.language)
        && dictation.style != Some(DictationStyle::Verbatim)
        && !dictation.numbers_only
        && !dictation.cancelled.load(Ordering::Acquire)
        && !text.trim().is_empty()
}

/// Which tagger cleans a dictation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Model {
    /// Labs › Clean up what I say.
    V1,
    /// Labs › Clean up what I say v2, which wins while both are on.
    V2,
}

impl Model {
    /// The tagger the two labs' switches choose, if either is on.
    pub(crate) fn chosen(v1: bool, v2: bool) -> Option<Self> {
        if v2 {
            Some(Self::V2)
        } else if v1 {
            Some(Self::V1)
        } else {
            None
        }
    }
}

/// The step for one dictation: the words to paste and, when a tagger was
/// asked, the observation History and the live log record. `model` is the
/// tagger the labs chose as the dictation's processing read them.
pub(crate) fn stage(
    model: Option<Model>,
    text: &str,
    dictation: &Dictation<'_>,
) -> (String, Option<ProcessingObservation>) {
    if !wanted(model.is_some(), text, dictation) {
        return (text.to_owned(), None);
    }
    match model {
        Some(Model::V2) => super::v2::clean(text, dictation.vocabulary),
        _ => clean(text, dictation.vocabulary),
    }
}

/// As a capture starts: while a lab is on, its model installs if it is
/// missing and loads, behind the person talking. With both labs off nothing
/// is fetched or loaded. No test fetches or loads the real model.
#[cfg(target_os = "macos")]
pub(crate) fn prepare() {
    #[cfg(not(test))]
    match Model::chosen(
        crate::labs::cleanup_tagger(),
        crate::labs::cleanup_tagger_v2(),
    ) {
        Some(Model::V2) => super::v2::warm(),
        Some(Model::V1) => TAGGER.warm(),
        None => {}
    }
}

/// As the Labs switches change: a lab turned off frees its loaded model.
pub(crate) fn reconcile(v1_on: bool, v2_on: bool) {
    #[cfg(all(target_os = "macos", not(test)))]
    if !v1_on {
        TAGGER.release();
    }
    #[cfg(not(all(target_os = "macos", not(test))))]
    let _ = v1_on;
    if !v2_on {
        super::v2::release();
    }
}

#[cfg(all(target_os = "macos", not(test)))]
static TAGGER: super::tagger::Tagger<super::tagger::CoreMl> =
    super::tagger::Tagger::new(super::tagger::CoreMl);

#[cfg(all(target_os = "macos", not(test)))]
fn clean(text: &str, vocabulary: &[VocabularyEntry]) -> (String, Option<ProcessingObservation>) {
    let vocabulary =
        crate::dictation::vocabulary::merged(vocabulary, &crate::dictation::vocabulary::entries());
    let protected = ProtectedTerms::new(vocabulary.iter().map(|entry| entry.term.as_str()));
    clean_with(
        text,
        &protected,
        tokenizer(),
        TIME_BUDGET,
        |chunks, deadline| TAGGER.score(chunks, deadline),
    )
}

/// No other OS runs the tagger yet.
#[cfg(not(any(target_os = "macos", test)))]
fn clean(text: &str, _vocabulary: &[VocabularyEntry]) -> (String, Option<ProcessingObservation>) {
    (text.to_owned(), None)
}

/// A test's dictation is tagged by the stand-in its thread names, and stays
/// as heard without one: no test loads the real model.
#[cfg(test)]
fn clean(text: &str, vocabulary: &[VocabularyEntry]) -> (String, Option<ProcessingObservation>) {
    let Some(tagger) = tests::STAND_IN.with(|tagger| tagger.borrow().clone()) else {
        return (text.to_owned(), None);
    };
    let protected = ProtectedTerms::new(vocabulary.iter().map(|entry| entry.term.as_str()));
    clean_with(
        text,
        &protected,
        tokenizer(),
        TIME_BUDGET,
        |chunks, deadline| tagger.and_then(|sure| tests::deleting(sure)(chunks, deadline)),
    )
}

/// Stands a tagger in for the real one on this test's thread: one sure of
/// exactly these words, or one that cannot answer. None takes it away.
#[cfg(test)]
pub(crate) fn stand_in(tagger: Option<Result<&'static [&'static str], Skip>>) {
    tests::STAND_IN.with(|current| *current.borrow_mut() = tagger);
}

/// The tagger's vocabulary: `bert-base-uncased`'s, the file the iPhone app
/// ships beside the model.
#[cfg(any(target_os = "macos", test))]
const VOCABULARY: &str =
    include_str!("../../../ios/HEXKeyboard/CleanupTagger/HexCleanupTaggerVocab.txt");

/// The tokenizer, built the first time it is asked for.
#[cfg(any(target_os = "macos", test))]
pub(super) fn tokenizer() -> Option<&'static Tokenizer> {
    static TOKENIZER: std::sync::LazyLock<Option<Tokenizer>> = std::sync::LazyLock::new(|| {
        Tokenizer::from_vocabulary(VOCABULARY)
            .inspect_err(|error| tracing::error!(%error, "the cleanup vocabulary is unusable"))
            .ok()
    });
    TOKENIZER.as_ref()
}

/// How many words one cleanup removed, by what they were.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Removed {
    fillers: usize,
    repeats: usize,
    false_starts: usize,
    corrections: usize,
}

/// One cleanup with its model, vocabulary, and time budget handed in.
/// `score` answers the model's logits for each chunk by the deadline it is
/// given.
pub(super) fn clean_with(
    text: &str,
    protected: &ProtectedTerms,
    tokenizer: Option<&Tokenizer>,
    budget: Duration,
    score: impl FnOnce(Vec<Chunk>, Instant) -> Result<Vec<Vec<f32>>, Skip>,
) -> (String, Option<ProcessingObservation>) {
    let started = Instant::now();
    let outcome = tokenizer
        .ok_or_else(|| Skip::Failed("the cleanup vocabulary is unusable".into()))
        .and_then(|tokenizer| tagged(text, protected, tokenizer, started + budget, score));
    let latency_ms = started.elapsed().as_millis() as u64;
    let observation = |fallback| ProcessingObservation {
        profile: PROFILE.into(),
        latency_ms,
        // The tagger runs on this Mac; no OpenCode model is asked.
        model: None,
        fallback,
    };
    match outcome {
        Ok((cleaned, words, removed)) => {
            tracing::info!(
                latency_ms,
                words,
                fillers = removed.fillers,
                repeats = removed.repeats,
                false_starts = removed.false_starts,
                corrections = removed.corrections,
                "dictation tagger cleanup completed"
            );
            (cleaned, Some(observation(None)))
        }
        Err(skip) => {
            let reason = skip.reason();
            tracing::warn!(
                latency_ms,
                %reason,
                "dictation tagger cleanup kept the words as heard"
            );
            (text.to_owned(), Some(observation(Some(reason))))
        }
    }
}

/// The cleaned text, how many words were heard, and how many went.
///
/// Each line is cleaned on its own and keeps the space around it, so the
/// line breaks spoken formatting wrote stay where they are: the renderer
/// joins the words it keeps with single spaces.
fn tagged(
    text: &str,
    protected: &ProtectedTerms,
    tokenizer: &Tokenizer,
    deadline: Instant,
    score: impl FnOnce(Vec<Chunk>, Instant) -> Result<Vec<Vec<f32>>, Skip>,
) -> Result<(String, usize, Removed), Skip> {
    let lines: Vec<(&str, Words, Vec<Chunk>)> = text
        .split('\n')
        .map(|line| {
            let words = Words::new(line);
            let chunks = tokenizer.chunks(&words);
            (line, words, chunks)
        })
        .collect();
    let chunks: Vec<Chunk> = lines
        .iter()
        .flat_map(|(_, _, chunks)| chunks.iter().cloned())
        .collect();
    let heard = lines.iter().map(|(_, words, _)| words.len()).sum();
    if chunks.is_empty() {
        return Ok((text.to_owned(), heard, Removed::default()));
    }
    let asked = chunks.len();
    let scores = score(chunks, deadline)?;
    if scores.len() != asked {
        return Err(Skip::Failed(
            "the cleanup model answered fewer words than it was given".into(),
        ));
    }
    let guard = CleanupGuard {
        policy: Policy::DESKTOP,
        protected,
    };
    let mut scores = scores.iter();
    let mut removed = Removed::default();
    let mut cleaned = Vec::with_capacity(lines.len());
    for (line, words, chunks) in &lines {
        let mut proposals: Vec<Proposal> = Vec::new();
        for (chunk, logits) in chunks.iter().zip(scores.by_ref()) {
            proposals.extend(chunk.proposals(logits).ok_or_else(|| {
                Skip::Failed("the cleanup model answered fewer words than it was given".into())
            })?);
        }
        let plan = guard.admit(words, &proposals).plan;
        if plan.is_empty() {
            cleaned.push((*line).to_owned());
            continue;
        }
        for edit in plan.edits() {
            let EditOp::Delete(kind) = edit.op;
            let count = edit.at.range().len();
            match kind {
                Disfluency::Filler => removed.fillers += count,
                Disfluency::Repeat => removed.repeats += count,
                Disfluency::FalseStart => removed.false_starts += count,
                Disfluency::Correction => removed.corrections += count,
            }
        }
        let rendered = render(words, &plan);
        if rendered.is_empty() {
            // The guard leaves a word on every line; never paste less.
            cleaned.push((*line).to_owned());
            continue;
        }
        let leading = line.len() - line.trim_start().len();
        let trailing = line.trim_end().len();
        cleaned.push(format!(
            "{}{rendered}{}",
            &line[..leading],
            &line[trailing..]
        ));
    }
    Ok((cleaned.join("\n"), heard, removed))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        /// The tagger [`stand_in`] named for this thread.
        pub(super) static STAND_IN: RefCell<Option<Result<&'static [&'static str], Skip>>> =
            const { RefCell::new(None) };
    }

    fn dictation<'a>(language: &'a str, cancelled: &'a AtomicBool) -> Dictation<'a> {
        Dictation {
            language,
            style: None,
            numbers_only: false,
            cancelled,
            vocabulary: &[],
        }
    }

    /// A tagger sure of every word in `delete`, read without its
    /// punctuation, and of nothing else. It sees only token ids, so it finds
    /// each word by encoding it the way the chunks did.
    pub(super) fn deleting(
        delete: &'static [&'static str],
    ) -> impl FnOnce(Vec<Chunk>, Instant) -> Result<Vec<Vec<f32>>, Skip> {
        move |chunks, _| {
            let tokenizer = tokenizer().unwrap();
            let marked: Vec<i32> = delete
                .iter()
                .flat_map(|word| tokenizer.chunks(&Words::new(*word))[0].input_ids()[1..2].to_vec())
                .collect();
            Ok(chunks
                .iter()
                .map(|chunk| {
                    chunk.input_ids()[..chunk.length()]
                        .iter()
                        .map(|id| if marked.contains(id) { 12.0 } else { -12.0 })
                        .collect()
                })
                .collect())
        }
    }

    fn cleaned(
        text: &str,
        terms: &[&str],
        score: impl FnOnce(Vec<Chunk>, Instant) -> Result<Vec<Vec<f32>>, Skip>,
    ) -> (String, Option<ProcessingObservation>) {
        let protected = ProtectedTerms::new(terms.iter().copied());
        clean_with(text, &protected, tokenizer(), TIME_BUDGET, score)
    }

    #[test]
    fn only_an_english_dictation_with_words_to_clean_is_cleaned() {
        let live = AtomicBool::new(false);
        let english = dictation("en", &live);
        assert!(wanted(true, "um hello", &english));
        assert!(!wanted(false, "um hello", &english), "the lab is off");
        assert!(!wanted(true, " \n ", &english), "nothing was said");
        assert!(!wanted(
            true,
            "um hello",
            &Dictation {
                numbers_only: true,
                ..english
            }
        ));
        assert!(!wanted(
            true,
            "um hello",
            &Dictation {
                style: Some(DictationStyle::Verbatim),
                ..english
            }
        ));
        for style in [
            DictationStyle::Polished,
            DictationStyle::Casual,
            DictationStyle::Notes,
        ] {
            assert!(wanted(
                true,
                "um hello",
                &Dictation {
                    style: Some(style),
                    ..english
                }
            ));
        }
        let cancelled = AtomicBool::new(true);
        assert!(!wanted(true, "um hello", &dictation("en", &cancelled)));
    }

    /// Labs › Clean up what I say v2 cleans in place of v1 while it is on,
    /// under the same rules for what is cleaned.
    #[test]
    fn v2_cleans_in_place_of_v1_while_its_lab_is_on() {
        assert_eq!(Model::chosen(false, false), None);
        assert_eq!(Model::chosen(true, false), Some(Model::V1));
        assert_eq!(Model::chosen(true, true), Some(Model::V2));
        assert_eq!(Model::chosen(false, true), Some(Model::V2));
        let live = AtomicBool::new(false);
        stand_in(Some(Ok(&["um"])));
        super::super::v2::stand_in(Some(Ok(&["um", "so"])));
        let said = "Um, so we go";
        let (text, observation) = stage(Some(Model::V2), said, &dictation("en", &live));
        assert_eq!(text, "We go");
        assert_eq!(observation.unwrap().profile, "Clean up what I say v2");
        let (text, observation) = stage(Some(Model::V1), said, &dictation("en", &live));
        assert_eq!(text, "So we go");
        assert_eq!(observation.unwrap().profile, "Clean up what I say");
        assert_eq!(
            stage(Some(Model::V2), said, &dictation("es", &live)),
            (said.to_owned(), None)
        );
        super::super::v2::stand_in(None);
        stand_in(None);
    }

    /// The tagger only knows English: on Spanish it took the "no" out of
    /// "Te digo que no puedo ir".
    #[test]
    fn every_language_but_english_stays_as_heard() {
        for language in ["en", "EN", "en-US", "en_GB", " en "] {
            assert!(applies_to(language), "{language}");
        }
        for language in ["es", "de", "nb", "auto", "", "eng", "es-en", "zh-Hant"] {
            assert!(!applies_to(language), "{language}");
        }
        let live = AtomicBool::new(false);
        let text = "Te digo que no puedo ir";
        assert!(!wanted(true, text, &dictation("es", &live)));
        assert!(!wanted(true, text, &dictation("auto", &live)));
        // The whole step, with a tagger that would take the "no" out.
        stand_in(Some(Ok(&["no"])));
        for language in ["es", "auto"] {
            assert_eq!(
                stage(Some(Model::V1), text, &dictation(language, &live)),
                (text.to_owned(), None)
            );
        }
        stand_in(None);
    }

    #[test]
    fn the_lab_off_asks_no_tagger_and_changes_nothing() {
        let live = AtomicBool::new(false);
        stand_in(Some(Ok(&["um"])));
        let (text, observation) = stage(None, "Um, so we go", &dictation("en", &live));
        assert_eq!(text, "Um, so we go");
        assert_eq!(observation, None);
        let (text, observation) = stage(Some(Model::V1), "Um, so we go", &dictation("en", &live));
        assert_eq!(text, "So we go");
        assert_eq!(observation.unwrap().fallback, None);
        stand_in(None);
    }

    #[test]
    fn the_taggers_deletions_pass_the_guard_and_are_rendered() {
        let (text, observation) = cleaned(
            "Hey Tim, um, are you free on Tuesday, no wait, Thursday?",
            &[],
            deleting(&["um", "Tuesday", "no", "wait"]),
        );
        assert_eq!(text, "Hey Tim, are you free on Thursday?");
        let observation = observation.unwrap();
        assert_eq!(observation.profile, "Clean up what I say");
        assert_eq!(observation.model, None);
        assert_eq!(observation.fallback, None);
    }

    /// The guard, not the tagger, has the last word: a name, a number, a
    /// vocabulary term, and a negation stay however sure the tagger is.
    #[test]
    fn the_guard_keeps_what_the_tagger_may_not_remove() {
        let said = "Tell Priya the kubectl rollout did not finish by 5pm.";
        let (text, observation) = cleaned(
            said,
            &["kubectl"],
            deleting(&["Priya", "kubectl", "not", "5pm"]),
        );
        assert_eq!(text, said);
        assert_eq!(observation.unwrap().fallback, None);

        // Sure of every word: something is always left to paste.
        let (text, _) = cleaned("um uh", &[], deleting(&["um", "uh"]));
        assert_eq!(text, "um uh");
    }

    #[test]
    fn a_clean_dictation_comes_back_exactly_as_it_was() {
        let said = "  Ship it on  Thursday.\n\nThanks!  ";
        let (text, observation) = cleaned(said, &[], deleting(&[]));
        assert_eq!(text, said);
        assert_eq!(observation.unwrap().fallback, None);
    }

    /// Spoken formatting's line breaks are part of what was said.
    #[test]
    fn each_line_is_cleaned_and_the_line_breaks_stay() {
        let (text, _) = cleaned(
            "Hi Sam,\n\nUm, the build is green.\n  We can, uh, ship today.  \nThanks",
            &[],
            deleting(&["um", "uh"]),
        );
        assert_eq!(
            text,
            "Hi Sam,\n\nThe build is green.\n  We can ship today.  \nThanks"
        );
    }

    #[test]
    fn a_long_dictation_is_scored_in_chunks_of_whole_words() {
        let said = vec!["we ship um today"; 100].join(" ");
        let asked = std::cell::Cell::new(0);
        let (text, observation) = cleaned(&said, &[], |chunks, deadline| {
            asked.set(chunks.len());
            deleting(&["um"])(chunks, deadline)
        });
        assert_eq!(asked.get(), 4);
        assert_eq!(text, vec!["we ship today"; 100].join(" "));
        assert_eq!(observation.unwrap().fallback, None);
    }

    /// Every way the tagger can fail to answer pastes the words as heard and
    /// says why.
    #[test]
    fn a_missing_late_or_failing_model_keeps_the_words_as_heard() {
        let said = "Um, so we go";
        for (skip, why) in [
            (Skip::ModelMissing, "the cleanup model is still downloading"),
            (Skip::Late, "the cleanup took longer than 150 ms"),
            (
                Skip::Failed("Core ML could not run the model".into()),
                "Core ML could not run the model",
            ),
        ] {
            let (text, observation) = cleaned(said, &[], |_, _| Err(skip));
            assert_eq!(text, said);
            let observation = observation.unwrap();
            assert_eq!(observation.profile, "Clean up what I say");
            assert_eq!(observation.fallback.as_deref(), Some(why));
        }

        // An answer of the wrong shape is no answer.
        for scores in [vec![], vec![vec![12.0; 3]], vec![vec![12.0; 16]; 2]] {
            let (text, observation) = cleaned(said, &[], |_, _| Ok(scores));
            assert_eq!(text, said);
            assert_eq!(
                observation.unwrap().fallback.as_deref(),
                Some("the cleanup model answered fewer words than it was given")
            );
        }

        let protected = ProtectedTerms::default();
        let (text, observation) = clean_with(said, &protected, None, TIME_BUDGET, |_, _| {
            panic!("scored without a vocabulary")
        });
        assert_eq!(text, said);
        assert_eq!(
            observation.unwrap().fallback.as_deref(),
            Some("the cleanup vocabulary is unusable")
        );
    }

    #[test]
    fn the_tagger_is_given_the_time_budget_as_its_deadline() {
        let before = Instant::now();
        let (_, observation) = cleaned("um hello", &[], |chunks, deadline| {
            assert!(deadline >= before + TIME_BUDGET);
            assert!(deadline <= Instant::now() + TIME_BUDGET);
            deleting(&["um"])(chunks, deadline)
        });
        assert!(observation.unwrap().latency_ms < 5_000);
    }
}
