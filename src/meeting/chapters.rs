//! Live chapters (Labs). While a meeting records,
//! Apple's on-device model reads each ~3 minutes of settled transcript and
//! says whether it is still the current chapter's topic or a new one, with a
//! short name and one line. After Stop, one call per change labels the lines
//! around it, and each boundary moves to the line where the topic begins.
//! Chapters live in the meeting manifest and extend the shown title unless
//! the user typed it.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

use super::generator::{self, GeneratedChapter, GenerationError};
use super::{
    MeetingManifest, TranscriptEntry, TranscriptReader, coalesce_transcript, display_title,
    format_duration, read_manifest, write_manifest,
};

/// How often the worker looks at the transcript; passes are far rarer.
const CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// One pass per this much new meeting time, given enough new words.
pub(super) const PASS_INTERVAL_MS: u64 = 180_000;
const MIN_NEW_WORDS: usize = 40;
/// Failures back off by a minute of meeting time per consecutive failure.
const RETRY_MS: u64 = 60_000;
const MAX_RETRY_MS: u64 = 5 * 60_000;
/// About 1,200 tokens of new transcript per pass; older backlog is skipped.
const PROMPT_CHARS: usize = 3_600;
const PASS_TIMEOUT: Duration = Duration::from_secs(20);
/// A chapter lasts at least this long, and a new one must be heard on two
/// passes in a row.
const MIN_CHAPTER_MS: u64 = 90_000;
/// A name sharing more than this share of its words with the names heard
/// in the current chapter is the same topic, whatever the model said.
const SAME_TOPIC_OVERLAP: f32 = 0.5;
/// The names a chapter remembers for that comparison.
const HEARD_TOPICS: usize = 3;
const MAX_CHAPTERS: usize = 24;
const MAX_TOPIC_WORDS: usize = 6;
const MAX_SUMMARY_CHARS: usize = 160;
/// The vocabulary line lists at most this many terms.
const MAX_TERMS: usize = 30;

/// The boundary pass sees lines this far either side of each live boundary.
const REFINE_SPAN_MS: u64 = PASS_INTERVAL_MS;
const REFINE_LINE_CHARS: usize = 240;
const REFINE_EXCERPT_CHARS: usize = 6_000;
const REFINE_TIMEOUT: Duration = Duration::from_secs(30);

/// A title with chapters names at most this many.
const TITLE_CHAPTERS: usize = 3;
const MAX_TITLE_CHARS: usize = 100;

const INSTRUCTIONS: &str = "You split a meeting in progress into chapters by topic. Each \
transcript line starts with [mm:ss] and a speaker: You is the person who recorded the meeting, \
and Them is one or more other participants. Name what most of the new lines are about in two to \
five words, judging them on their own, and say what they cover in one short sentence. Then say \
whether they are still about the current chapter named after them. Small talk, tangents, and follow-up questions on the same subject stay in the \
current chapter. Use only what the transcript says; never invent names, dates, or numbers.";

const REFINE_INSTRUCTIONS: &str = "You find where a finished meeting changed topic. Each \
line starts with an L number, [mm:ss], and a speaker: You is the person who recorded the \
meeting, and Them is one or more other participants. The lines run from the end of the chapter \
before into the chapter after. List the L number of every line about the chapter after, \
including the line that introduces it. Use only L numbers shown.";

/// One topic of a meeting, from `start_ms` to the next chapter's start.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Chapter {
    pub title: String,
    /// One line on what the chapter covers.
    #[serde(default)]
    pub summary: String,
    pub start_ms: u64,
}

/// The entries of chapter `index`: those starting at or after its start and
/// before the next chapter's.
pub fn chapter_entries(
    entries: &[TranscriptEntry],
    chapters: &[Chapter],
    index: usize,
) -> Vec<TranscriptEntry> {
    let Some(chapter) = chapters.get(index) else {
        return Vec::new();
    };
    let end = chapters.get(index + 1).map(|next| next.start_ms);
    entries
        .iter()
        .filter(|entry| {
            entry.start_ms >= chapter.start_ms && end.is_none_or(|end| entry.start_ms < end)
        })
        .cloned()
        .collect()
}

/// One chapter as Markdown for copying: its name, its line, and its turns.
pub fn section_markdown(chapter: &Chapter, entries: &[TranscriptEntry]) -> String {
    let mut markdown = format!("## {}\n\n", chapter.title.trim());
    if !chapter.summary.trim().is_empty() {
        markdown.push_str(chapter.summary.trim());
        markdown.push_str("\n\n");
    }
    markdown.push_str(&super::transcript_markdown(entries));
    markdown.trim_end().to_string()
}

/// Chapter `index` of meeting `id` as Markdown, from its current transcript
/// (rolling while it records). For the menu bar's Copy Section.
pub fn section_for_id(id: &str, index: usize) -> Result<String> {
    let manifest = read_manifest(&super::root()?.join(id))?;
    let chapter = manifest
        .chapters
        .get(index)
        .ok_or_else(|| eyre!("the meeting has no chapter {}", index + 1))?;
    let entries = TranscriptReader::default().read(id)?;
    Ok(section_markdown(
        chapter,
        &chapter_entries(&entries, &manifest.chapters, index),
    ))
}

/// The vocabulary line ahead of a prompt: names and terms the transcript
/// may contain, so chapter names spell them the same way.
pub(super) fn vocabulary_line(terms: &[String]) -> String {
    let terms = terms
        .iter()
        .map(|term| term.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|term| !term.is_empty())
        .take(MAX_TERMS)
        .map(|term| term.chars().take(40).collect::<String>())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        String::new()
    } else {
        format!("Names and terms that may appear: {}\n\n", terms.join(", "))
    }
}

/// `base` followed by the chapters that cover the most time, in meeting
/// order: `Team sync: image storage, motion lint, chat app name`. Chapters
/// that repeat the base, or would take the title past `MAX_TITLE_CHARS`,
/// are left out; with fewer than two chapters, or nothing left to add, the
/// base stands alone.
pub(super) fn titled(base: &str, chapters: &[Chapter], end_ms: Option<u64>) -> String {
    let base = base.trim();
    if chapters.len() < 2 {
        return base.into();
    }
    let bases = [base.to_string()];
    let last_start = chapters.last().map_or(0, |chapter| chapter.start_ms);
    let end_ms = end_ms.unwrap_or(last_start).max(last_start);
    let mut ranked = chapters
        .iter()
        .enumerate()
        .filter(|(_, chapter)| overlap(&chapter.title, &bases) <= SAME_TOPIC_OVERLAP)
        .map(|(index, chapter)| {
            let end = chapters.get(index + 1).map_or(end_ms, |next| next.start_ms);
            (end.saturating_sub(chapter.start_ms), index)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(length, index)| (std::cmp::Reverse(*length), *index));
    let mut picked = ranked
        .into_iter()
        .take(TITLE_CHAPTERS)
        .map(|(_, index)| index)
        .collect::<Vec<_>>();
    picked.sort_unstable();
    let mut names = Vec::<&str>::new();
    for index in picked {
        let title = chapters[index].title.trim();
        if names.iter().any(|name| name.eq_ignore_ascii_case(title)) {
            continue;
        }
        let length = base.len() + 2 + names.iter().map(|name| name.len() + 2).sum::<usize>();
        // A shorter chapter later on may still fit.
        if length + title.len() <= MAX_TITLE_CHARS {
            names.push(title);
        }
    }
    if names.is_empty() {
        base.into()
    } else {
        format!("{base}: {}", names.join(", "))
    }
}

/// Chapters a recording has so far, and what the next pass compares with.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Chaptering {
    pub chapters: Vec<Chapter>,
    /// The latest names reported while the current chapter was current.
    heard: Vec<String>,
    /// A different topic heard on the last pass, opened as a chapter when
    /// the next pass also departs from the current one.
    pending: Option<Chapter>,
    /// The meeting timeline through which the transcript has been read.
    pub covered_ms: u64,
    pub passes: u32,
}

impl Chaptering {
    fn current(&self) -> Option<&str> {
        self.chapters.last().map(|chapter| chapter.title.as_str())
    }

    /// Folds one pass in and returns whether the chapters changed. The
    /// first name opens the first chapter at the meeting's start. Later, a
    /// pass departs unless its name `stays` in the current chapter; a
    /// departure opens a chapter, named and dated by the first pass away,
    /// only when the next pass departs too and the current chapter has had
    /// its minimum length.
    pub fn merge(&mut self, generated: GeneratedChapter, window: &PassWindow) -> bool {
        let title = clean_topic(&generated.title);
        let summary = clean_summary(&generated.summary);
        let mut changed = false;
        if !title.is_empty() {
            if self.chapters.is_empty() {
                self.open(Chapter {
                    title,
                    summary,
                    start_ms: 0,
                });
                changed = true;
            } else if self.stays(&title, generated.same_topic) {
                self.remember(title);
                self.pending = None;
            } else {
                let current_start = self.chapters.last().map_or(0, |chapter| chapter.start_ms);
                match self.pending.take() {
                    Some(pending)
                        if self.chapters.len() < MAX_CHAPTERS
                            && pending.start_ms >= current_start + MIN_CHAPTER_MS =>
                    {
                        // A pass naming yet another topic still confirms
                        // that the current chapter ended, and waits for its
                        // own confirmation.
                        let continues = overlap(&title, std::slice::from_ref(&pending.title))
                            > SAME_TOPIC_OVERLAP;
                        self.open(pending);
                        if continues {
                            self.remember(title);
                        } else {
                            self.pending = Some(Chapter {
                                title,
                                summary,
                                start_ms: window.start_ms,
                            });
                        }
                        changed = true;
                    }
                    _ => {
                        self.pending = Some(Chapter {
                            title,
                            summary,
                            start_ms: window.start_ms,
                        });
                    }
                }
            }
        }
        self.read(window);
        changed
    }

    /// Counts a window as read without a decision, as for a refused passage.
    pub fn read(&mut self, window: &PassWindow) {
        self.covered_ms = self.covered_ms.max(window.end_ms);
        self.passes += 1;
    }

    fn open(&mut self, chapter: Chapter) {
        self.heard = vec![chapter.title.clone()];
        self.chapters.push(chapter);
        self.pending = None;
    }

    fn remember(&mut self, title: String) {
        self.heard.push(title);
        let excess = self.heard.len().saturating_sub(HEARD_TOPICS);
        self.heard.drain(..excess);
    }

    /// Whether a name stays in the current chapter: most of its words were
    /// in a name heard in this chapter, or the model said so and the name
    /// shares at least one of those words. The model's flag alone is not
    /// enough; it calls an unrelated name the same topic too often.
    fn stays(&self, title: &str, same_topic: bool) -> bool {
        let known = overlap(title, &self.heard);
        known > SAME_TOPIC_OVERLAP || (same_topic && known > 0.0)
    }

    /// At Stop, opens a departure still waiting for its second pass: the
    /// meeting ended before one could confirm it. Returns whether it did.
    pub fn close(&mut self) -> bool {
        let current_start = self.chapters.last().map_or(0, |chapter| chapter.start_ms);
        match self.pending.take() {
            Some(pending)
                if !self.chapters.is_empty()
                    && self.chapters.len() < MAX_CHAPTERS
                    && pending.start_ms >= current_start + MIN_CHAPTER_MS =>
            {
                self.open(pending);
                true
            }
            _ => false,
        }
    }
}

/// The share of `title`'s words found in `names`; a name without content
/// words counts as entirely known.
fn overlap(title: &str, names: &[String]) -> f32 {
    let words = content_words(title);
    if words.is_empty() {
        return 1.0;
    }
    let known = names
        .iter()
        .flat_map(|name| content_words(name))
        .collect::<BTreeSet<_>>();
    words.intersection(&known).count() as f32 / words.len() as f32
}

/// When to run a pass: once enough new meeting time and words exist.
/// Failures back off without giving up.
#[derive(Debug, Default)]
pub(super) struct PassPlan {
    retry_at_ms: u64,
    failures: u32,
}

impl PassPlan {
    pub fn due(&self, state: &Chaptering, timeline_ms: u64, fresh_words: usize) -> bool {
        timeline_ms >= self.retry_at_ms
            && fresh_words >= MIN_NEW_WORDS
            && timeline_ms >= state.covered_ms + PASS_INTERVAL_MS
    }

    pub fn record(&mut self, succeeded: bool, timeline_ms: u64) {
        if succeeded {
            *self = Self::default();
        } else {
            self.failures += 1;
            self.retry_at_ms =
                timeline_ms + (RETRY_MS * u64::from(self.failures)).min(MAX_RETRY_MS);
        }
    }
}

/// Words transcribed after `covered_ms`.
pub(super) fn fresh_words(entries: &[TranscriptEntry], covered_ms: u64) -> usize {
    entries
        .iter()
        .filter(|entry| entry.end_ms > covered_ms)
        .map(|entry| entry.text.split_whitespace().count())
        .sum()
}

/// The new transcript one pass reads.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct PassWindow {
    text: String,
    /// Backlog older than the prompt budget was skipped.
    skipped: bool,
    /// The first new line's start: where a chapter opened here begins.
    start_ms: u64,
    end_ms: u64,
}

/// The newest transcript after `covered_ms` that fits in `max_chars`, as
/// `[mm:ss] You: text` turns.
pub(super) fn pass_window(
    entries: &[TranscriptEntry],
    covered_ms: u64,
    max_chars: usize,
) -> Option<PassWindow> {
    let mut fresh = entries
        .iter()
        .filter(|entry| entry.end_ms > covered_ms && !entry.text.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    fresh.sort_by_key(|entry| entry.start_ms);
    let start_ms = fresh.first()?.start_ms;
    let end_ms = fresh.iter().map(|entry| entry.end_ms).max()?;
    let mut kept = Vec::new();
    let mut used = 0;
    for entry in fresh.iter().rev() {
        let cost = entry.text.len() + 16;
        if used + cost > max_chars && !kept.is_empty() {
            break;
        }
        used += cost;
        kept.push(entry.clone());
    }
    kept.reverse();
    let skipped = kept.len() < fresh.len();
    let text = coalesce_transcript(kept)
        .iter()
        .map(|turn| {
            format!(
                "[{}] {}: {}",
                format_duration(turn.start_ms),
                turn.source.label(),
                truncate_words(turn.text.trim(), max_chars)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(PassWindow {
        text,
        skipped,
        start_ms,
        end_ms,
    })
}

/// One pass's prompt: the vocabulary, only the new transcript, then the
/// current chapter's name, so the model names the new lines before it
/// compares them.
pub(super) fn prompt(window: &PassWindow, current: Option<&str>, terms: &[String]) -> String {
    let mut prompt = vocabulary_line(terms);
    prompt.push_str("New transcript:\n");
    if window.skipped {
        prompt.push_str("[…]\n");
    }
    prompt.push_str(&window.text);
    prompt.push_str(&format!(
        "\n\nCurrent chapter: {}",
        current.unwrap_or("none yet")
    ));
    prompt
}

/// What one pass did, for its log line.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum PassOutcome {
    Changed,
    Unchanged,
    /// The model refused the passage; retrying gets the same refusal, so it
    /// counts as read.
    Refused,
}

/// Runs one pass over `window` and folds it into `state`.
pub(super) fn run_pass(
    state: &mut Chaptering,
    window: &PassWindow,
    terms: &[String],
    generate: impl FnOnce(&str, &str, Duration) -> Result<GeneratedChapter, GenerationError>,
) -> Result<PassOutcome, GenerationError> {
    let prompt = prompt(window, state.current(), terms);
    match generate(INSTRUCTIONS, &prompt, PASS_TIMEOUT) {
        Ok(generated) => Ok(if state.merge(generated, window) {
            PassOutcome::Changed
        } else {
            PassOutcome::Unchanged
        }),
        Err(GenerationError::Refused) => {
            state.read(window);
            Ok(PassOutcome::Refused)
        }
        Err(error) => Err(error),
    }
}

type Generate =
    Box<dyn Fn(&str, &str, Duration) -> Result<GeneratedChapter, GenerationError> + Send>;
type ReadTranscript = Box<dyn FnMut() -> Result<Vec<TranscriptEntry>> + Send>;

/// Chapters a recording meeting in the background. The recorder takes each
/// new set and records it in its manifest; dropping this stops the worker
/// and discards a pass still running.
pub(super) struct LiveChapters {
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
}

/// The worker's chapters, and whether the recorder has yet to take them.
#[derive(Default)]
struct Shared {
    state: Chaptering,
    fresh: bool,
}

impl LiveChapters {
    pub fn start(id: String) -> Self {
        let mut reader = TranscriptReader::default();
        let worker_id = id.clone();
        Self::start_with(
            id,
            Box::new(move || reader.read(&worker_id)),
            CHECK_INTERVAL,
            super::notes_wanted,
            Box::new(generator::generate_chapter),
        )
    }

    fn start_with(
        id: String,
        read: ReadTranscript,
        check_interval: Duration,
        wanted: fn() -> bool,
        generate: Generate,
    ) -> Self {
        let chapters = Self {
            stop: Arc::new(AtomicBool::new(false)),
            shared: Arc::default(),
        };
        let stop = chapters.stop.clone();
        let shared = chapters.shared.clone();
        if let Err(error) = thread::Builder::new()
            .name("meeting-chapters".into())
            .spawn(move || {
                crate::transcription::background_priority::deprioritize_background_thread();
                chapter_while_recording(
                    &id,
                    read,
                    check_interval,
                    wanted,
                    &generate,
                    &stop,
                    &shared,
                );
            })
        {
            tracing::warn!(%error, "could not start live meeting chapters");
        }
        chapters
    }

    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// The chapters, when they changed since the last take.
    pub fn take(&self) -> Option<Vec<Chapter>> {
        let mut shared = self.shared();
        std::mem::take(&mut shared.fresh).then(|| shared.state.chapters.clone())
    }

    /// Stops the worker and opens a departure the meeting ended before a
    /// second pass could confirm. Returns the chapters when they changed
    /// since the last take.
    pub fn finish(&self) -> Option<Vec<Chapter>> {
        self.stop.store(true, Ordering::Relaxed);
        let mut shared = self.shared();
        let closed = shared.state.close();
        (std::mem::take(&mut shared.fresh) || closed).then(|| shared.state.chapters.clone())
    }
}

impl Drop for LiveChapters {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn chapter_while_recording(
    id: &str,
    mut read: ReadTranscript,
    check_interval: Duration,
    wanted: fn() -> bool,
    generate: &Generate,
    stop: &AtomicBool,
    shared: &Mutex<Shared>,
) {
    let mut plan = PassPlan::default();
    let mut next_check = Instant::now() + check_interval;
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() < next_check {
            thread::sleep(check_interval.min(Duration::from_millis(250)));
            continue;
        }
        next_check = Instant::now() + check_interval;
        // Unavailable or turned off: no chapters and nothing logged.
        if !wanted() {
            continue;
        }
        let entries = match read() {
            Ok(entries) => entries,
            Err(error) => {
                tracing::debug!(%error, meeting = id, "could not read the transcript for chapters");
                continue;
            }
        };
        let mut next = shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .state
            .clone();
        let timeline_ms = entries.iter().map(|entry| entry.end_ms).max().unwrap_or(0);
        if !plan.due(&next, timeline_ms, fresh_words(&entries, next.covered_ms)) {
            continue;
        }
        let Some(window) = pass_window(&entries, next.covered_ms, PROMPT_CHARS) else {
            continue;
        };
        if !super::notes::wait_for_dictation(stop) {
            return;
        }
        let started = Instant::now();
        // Read per pass, so vocabulary edits apply mid-meeting.
        let result = run_pass(
            &mut next,
            &window,
            &crate::dictation::vocabulary::terms(),
            generate,
        );
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(outcome) => {
                tracing::info!(
                    meeting = id,
                    elapsed_ms,
                    skipped = window.skipped,
                    ?outcome,
                    chapters = next.chapters.len(),
                    "live meeting chapter pass"
                );
                let mut shared = shared.lock().unwrap_or_else(|error| error.into_inner());
                // Stop already took the chapters; a late pass is discarded.
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                shared.fresh |= outcome == PassOutcome::Changed;
                shared.state = next;
                plan.record(true, timeline_ms);
            }
            Err(error) => {
                tracing::warn!(%error, meeting = id, elapsed_ms, "live meeting chapter pass failed");
                plan.record(false, timeline_ms);
            }
        }
    }
}

/// One numbered transcript line of the boundary pass.
#[derive(Debug)]
struct Line {
    number: usize,
    start_ms: u64,
    text: String,
}

/// The lines around one live boundary that the boundary pass may choose.
#[derive(Debug)]
struct Section {
    chapter: usize,
    lines: Vec<Line>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Refinement {
    pub chapters: Vec<Chapter>,
    /// Boundaries the pass moved.
    pub moved: usize,
    /// Calls made, one per boundary until the deadline.
    pub calls: usize,
    pub prompt_chars: usize,
}

/// The boundary pass's sections: for each chapter after the first, the
/// transcribed lines within `REFINE_SPAN_MS` of its live start, after the
/// previous chapter's start and before the next one's, nearest first within
/// `max_chars`. Lines are not joined into turns, since a topic often
/// changes between two lines of one speaker.
fn sections(entries: &[TranscriptEntry], chapters: &[Chapter], max_chars: usize) -> Vec<Section> {
    let mut lines = entries
        .iter()
        .filter(|entry| !entry.text.trim().is_empty())
        .collect::<Vec<_>>();
    lines.sort_by_key(|entry| entry.start_ms);
    (1..chapters.len())
        .filter_map(|chapter| {
            let live = chapters[chapter].start_ms;
            let after = chapters[chapter - 1].start_ms;
            let before = chapters
                .get(chapter + 1)
                .map_or(u64::MAX, |next| next.start_ms);
            let mut candidates = lines
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.start_ms > after
                        && entry.start_ms < before
                        && entry.start_ms + REFINE_SPAN_MS >= live
                        && entry.start_ms <= live + REFINE_SPAN_MS
                })
                .map(|(index, entry)| Line {
                    number: index + 1,
                    start_ms: entry.start_ms,
                    text: format!(
                        "L{} [{}] {}: {}",
                        index + 1,
                        format_duration(entry.start_ms),
                        entry.source.label(),
                        truncate_words(entry.text.trim(), REFINE_LINE_CHARS)
                    ),
                })
                .collect::<Vec<_>>();
            candidates.sort_by_key(|line| line.start_ms.abs_diff(live));
            let mut used = 0;
            let mut kept = Vec::new();
            for line in candidates {
                if used + line.text.len() + 1 > max_chars && !kept.is_empty() {
                    break;
                }
                used += line.text.len() + 1;
                kept.push(line);
            }
            kept.sort_by_key(|line| line.number);
            (!kept.is_empty()).then_some(Section {
                chapter,
                lines: kept,
            })
        })
        .collect()
}

fn refine_prompt(section: &Section, chapters: &[Chapter], terms: &[String]) -> String {
    let describe = |chapter: &Chapter| {
        if chapter.summary.is_empty() {
            chapter.title.clone()
        } else {
            format!("{} ({})", chapter.title, chapter.summary)
        }
    };
    let mut prompt = vocabulary_line(terms);
    prompt.push_str(&format!(
        "Chapter before: {}\nChapter after: {}\n\nLines:\n",
        describe(&chapters[section.chapter - 1]),
        describe(&chapters[section.chapter])
    ));
    for line in &section.lines {
        prompt.push_str(&line.text);
        prompt.push('\n');
    }
    prompt
}

/// Where a section's chapter starts, given the L numbers the model said
/// are about it: the split that disagrees with the fewest of those labels,
/// nearest the live boundary on a tie, and only after `after_ms`. The
/// labels must cover at least half the lines from the live boundary on,
/// which the live passes already judged to be the chapter after; otherwise
/// the answer is ignored. `None` keeps the live boundary.
fn split(section: &Section, about_next: &[i64], live_ms: u64, after_ms: u64) -> Option<u64> {
    let labels = section
        .lines
        .iter()
        .map(|line| about_next.contains(&(line.number as i64)))
        .collect::<Vec<_>>();
    let live = section
        .lines
        .iter()
        .position(|line| line.start_ms >= live_ms)
        .unwrap_or(section.lines.len());
    let confirmed = labels[live..].iter().filter(|about| **about).count();
    if confirmed == 0 || confirmed * 2 < labels.len() - live {
        return None;
    }
    let disagreements = |split: usize| {
        labels[..split].iter().filter(|about| **about).count()
            + labels[split..].iter().filter(|about| !**about).count()
    };
    let best = (0..section.lines.len())
        .filter(|&index| section.lines[index].start_ms > after_ms)
        .min_by_key(|&index| (disagreements(index), index.abs_diff(live)))?;
    (disagreements(best) < disagreements(live)).then(|| section.lines[best].start_ms)
}

/// One call per live boundary, within one deadline: the model lists the
/// lines about the chapter after, and Rust moves the boundary to the split
/// that agrees with it best. A failed or refused call keeps its boundary;
/// the pass fails only when every call does. `proceed` runs before each
/// call; when it returns false the whole pass fails, so nothing is kept.
pub(super) fn refine(
    entries: &[TranscriptEntry],
    chapters: &[Chapter],
    max_chars: usize,
    timeout: Duration,
    terms: &[String],
    mut proceed: impl FnMut() -> bool,
    mut generate: impl FnMut(&str, &str, Duration) -> Result<Vec<i64>, GenerationError>,
) -> Result<Refinement> {
    let deadline = Instant::now() + timeout;
    let mut refinement = Refinement {
        chapters: chapters.to_vec(),
        moved: 0,
        calls: 0,
        prompt_chars: 0,
    };
    let mut first_error = None;
    let mut answered = false;
    for section in sections(entries, chapters, max_chars) {
        if !proceed() {
            return Err(eyre!("the boundary pass stopped"));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let prompt = refine_prompt(&section, chapters, terms);
        refinement.calls += 1;
        refinement.prompt_chars += prompt.len();
        let about_next = match generate(REFINE_INSTRUCTIONS, &prompt, remaining) {
            Ok(lines) => lines,
            Err(error) => {
                first_error.get_or_insert(error);
                continue;
            }
        };
        answered = true;
        let after_ms = refinement.chapters[section.chapter - 1].start_ms;
        let live_ms = chapters[section.chapter].start_ms;
        if let Some(start_ms) = split(&section, &about_next, live_ms, after_ms) {
            refinement.chapters[section.chapter].start_ms = start_ms;
            refinement.moved += 1;
        }
    }
    match first_error {
        Some(error) if !answered => Err(eyre!("{error}")),
        _ => Ok(refinement),
    }
}

/// Corrects a finished meeting's live boundaries once and records them.
/// The Markdown heading follows a title the new chapters change. Failures
/// leave the live chapters in place.
/// Each call first yields to dictation; a stop or quit ends the pass
/// without recording anything, so the next notes run redoes it.
pub(super) fn refine_meeting(directory: &Path, entries: &[TranscriptEntry], stop: &AtomicBool) {
    refine_meeting_with(
        directory,
        entries,
        super::notes::excerpt_budget(generator::context_tokens(), REFINE_EXCERPT_CHARS),
        || super::notes::wait_for_dictation(stop) && !crate::desktop::process_exit::quitting(),
        generator::generate_chapter_lines,
    );
}

fn refine_meeting_with(
    directory: &Path,
    entries: &[TranscriptEntry],
    max_chars: usize,
    proceed: impl FnMut() -> bool,
    generate: impl FnMut(&str, &str, Duration) -> Result<Vec<i64>, GenerationError>,
) {
    let Ok(manifest) = read_manifest(directory) else {
        return;
    };
    if manifest.chapters.len() < 2 || manifest.chapters_refined {
        return;
    }
    let started = Instant::now();
    let result = refine(
        entries,
        &manifest.chapters,
        max_chars,
        REFINE_TIMEOUT,
        &crate::dictation::vocabulary::terms(),
        proceed,
        generate,
    );
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let refinement = match result {
        Ok(refinement) => refinement,
        Err(error) => {
            tracing::warn!(%error, meeting = manifest.id, elapsed_ms, "could not refine meeting chapters");
            return;
        }
    };
    tracing::info!(
        meeting = manifest.id,
        elapsed_ms,
        moved = refinement.moved,
        calls = refinement.calls,
        prompt_chars = refinement.prompt_chars,
        "refined meeting chapters"
    );
    // Re-read so concurrent manifest updates are not lost.
    let Ok(mut manifest) = read_manifest(directory) else {
        return;
    };
    let previous = display_title(&manifest);
    manifest.chapters = refinement.chapters;
    manifest.chapters_refined = true;
    retitle_export(&mut manifest, &previous);
    if let Err(error) = write_manifest(directory, &manifest) {
        tracing::warn!(%error, meeting = manifest.id, "could not record refined chapters");
    }
}

fn retitle_export(manifest: &mut MeetingManifest, previous: &str) {
    let title = display_title(manifest);
    if title != previous
        && let Some(path) = manifest.export_path.clone().filter(|path| path.exists())
        && let Err(error) = super::notes::retitle_markdown_file(
            &path,
            &mut manifest.export_sha256,
            previous,
            &title,
        )
    {
        tracing::warn!(%error, meeting = manifest.id, "could not retitle meeting Markdown");
    }
}

fn clean_topic(topic: &str) -> String {
    let title = super::notes::clean_title(topic);
    let words = title.split_whitespace().collect::<Vec<_>>();
    if words.len() > MAX_TOPIC_WORDS {
        words[..MAX_TOPIC_WORDS].join(" ")
    } else {
        title
    }
}

fn clean_summary(summary: &str) -> String {
    let text = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_words(text.trim_matches(['"', '“', '”']), MAX_SUMMARY_CHARS)
}

const STOP_WORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "with",
    "about",
    "from",
    "that",
    "this",
    "into",
    "their",
    "our",
    "your",
    "are",
    "was",
    "will",
    "its",
    "discussion",
    "talk",
    "meeting",
    "review",
    "update",
];

fn content_words(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| word.chars().count() >= 3)
        .map(|word| {
            let word = word.to_lowercase();
            word.strip_suffix('s')
                .filter(|stem| stem.len() >= 3)
                .map_or(word.clone(), str::to_owned)
        })
        .filter(|word| !STOP_WORDS.contains(&word.as_str()))
        .collect()
}

fn truncate_words(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.into();
    }
    let cut = text[..text.floor_char_boundary(max_chars)]
        .rfind(' ')
        .unwrap_or_else(|| text.floor_char_boundary(max_chars));
    format!("{}…", text[..cut].trim_end_matches([',', ';', ':', '.']))
}

/// Runs the real model over a transcript Markdown file as a recording
/// would: one pass per `PASS_INTERVAL_MS` of meeting time, then the
/// boundary pass. Saves nothing. Debug CLI: `hex meeting chapters <path>`.
#[cfg(debug_assertions)]
pub fn chapters_file(path: &Path) -> Result<String> {
    use color_eyre::eyre::WrapErr;
    generator::available().map_err(|reason| eyre!("{reason}"))?;
    let text = std::fs::read_to_string(path)
        .wrap_err_with(|| format!("could not read {}", path.display()))?;
    let entries = super::notes::entries_from_markdown(&text);
    let end_ms = entries.iter().map(|entry| entry.end_ms).max().unwrap_or(0);
    // This process never loads settings, so read the saved list directly.
    let terms = crate::app_paths::support_dir()
        .map(|root| crate::dictation::vocabulary::read_saved(&root))
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.term)
        .collect::<Vec<_>>();
    let mut report = String::new();
    let mut state = Chaptering::default();
    let mut plan = PassPlan::default();
    let mut at_ms = 0;
    while at_ms < end_ms + PASS_INTERVAL_MS {
        at_ms += 5_000;
        let heard = entries
            .iter()
            .filter(|entry| entry.end_ms <= at_ms)
            .cloned()
            .collect::<Vec<_>>();
        let timeline_ms = heard.iter().map(|entry| entry.end_ms).max().unwrap_or(0);
        if !plan.due(&state, timeline_ms, fresh_words(&heard, state.covered_ms)) {
            continue;
        }
        let Some(window) = pass_window(&heard, state.covered_ms, PROMPT_CHARS) else {
            continue;
        };
        let started = Instant::now();
        let mut said = String::new();
        let result = run_pass(
            &mut state,
            &window,
            &terms,
            |instructions, prompt, timeout| {
                let generated = generator::generate_chapter(instructions, prompt, timeout)?;
                said = format!(
                    "{} ({})",
                    generated.title,
                    if generated.same_topic { "same" } else { "new" }
                );
                Ok(generated)
            },
        );
        report.push_str(&format!(
            "pass at {} · {:.1}s · {}\n",
            format_duration(timeline_ms),
            started.elapsed().as_secs_f32(),
            match &result {
                Ok(outcome) => format!(
                    "{outcome:?} · said: {said} · current: {} · pending: {}",
                    state.current().unwrap_or("-"),
                    state
                        .pending
                        .as_ref()
                        .map_or("-", |pending| pending.title.as_str())
                ),
                Err(error) => format!("failed: {error}"),
            }
        ));
        plan.record(result.is_ok(), timeline_ms);
    }
    if state.close() {
        report.push_str("stop · opened the waiting chapter\n");
    }
    report.push_str("\nlive chapters:\n");
    for chapter in &state.chapters {
        report.push_str(&format!(
            "  [{}] {} — {}\n",
            format_duration(chapter.start_ms),
            chapter.title,
            chapter.summary
        ));
    }
    let started = Instant::now();
    let refinement = refine(
        &entries,
        &state.chapters,
        super::notes::excerpt_budget(generator::context_tokens(), REFINE_EXCERPT_CHARS),
        REFINE_TIMEOUT,
        &terms,
        || true,
        generator::generate_chapter_lines,
    )?;
    report.push_str(&format!(
        "\nboundary pass · {:.1}s · {} calls · {} prompt chars · {} moved\n",
        started.elapsed().as_secs_f32(),
        refinement.calls,
        refinement.prompt_chars,
        refinement.moved
    ));
    for chapter in &refinement.chapters {
        report.push_str(&format!(
            "  [{}] {}\n",
            format_duration(chapter.start_ms),
            chapter.title
        ));
    }
    report.push_str(&format!(
        "\ntitle: {}\n",
        titled("Meeting", &refinement.chapters, Some(end_ms))
    ));
    Ok(report)
}

#[cfg(test)]
#[path = "chapters_tests.rs"]
mod tests;
