//! Live meeting notes. While a meeting records, Apple's on-device model reads
//! only the transcript added since its last pass and keeps the current topic
//! and a short, append-mostly outline in `live-notes.json`. Each note keeps
//! the transcript lines it came from, so it expands without another read.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};

use super::{
    TranscriptEntry, TranscriptReader, coalesce_transcript, format_duration, root, set_owner_only,
    sync_directory,
};
use crate::apple_intelligence::{self, GeneratedLiveNote, GeneratedLiveNotes, GenerationError};
use crate::events::now_ms;

pub(super) const FILE: &str = "live-notes.json";

/// How often the worker looks at the transcript; passes are far rarer.
const CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// The first pass waits for the opening; later passes need this much new
/// meeting time and enough new words.
const FIRST_PASS_MS: u64 = 45_000;
const PASS_INTERVAL_MS: u64 = 75_000;
const MIN_NEW_WORDS: usize = 40;
/// Failures back off by a minute of meeting time per consecutive failure.
const RETRY_MS: u64 = 60_000;
const MAX_RETRY_MS: u64 = 5 * 60_000;
/// About 1,200 tokens of new transcript per pass; older backlog is skipped.
const PROMPT_CHARS: usize = 3_600;
const PASS_TIMEOUT: Duration = Duration::from_secs(20);
/// A topic stays at least this long, and a new one must be heard on two
/// passes in a row.
const MIN_TOPIC_DWELL_MS: u64 = 90_000;
/// A topic sharing this share of its words with what was heard in the
/// current section is the same topic.
const SAME_TOPIC_OVERLAP: f32 = 0.5;
/// The topic names a section remembers for that comparison.
const HEARD_TOPICS: usize = 3;
const DUPLICATE_OVERLAP: f32 = 0.6;
const MAX_SECTIONS: usize = 40;
/// Decisions, actions, and questions replace plain points in a full section.
const MAX_BULLETS: usize = 3;
const MAX_TOPIC_WORDS: usize = 6;
const MAX_NOTE_CHARS: usize = 160;
const MAX_OWNER_WORDS: usize = 3;
const EXCERPT_ENTRIES: usize = 4;
const EXCERPT_CHARS: usize = 280;
const EXCERPT_SPAN_MS: u64 = 20_000;
const EXCERPT_LEAD_MS: u64 = 10_000;

const INSTRUCTIONS: &str = "You keep short live notes for a meeting in progress. Each \
transcript line starts with [mm:ss] and a speaker: You is the person who recorded the meeting, \
and Them is one or more other participants. Name what this part of the conversation is about. \
Keep at most three notes worth reading later: decisions, action items with who will do them, \
open questions, or key points. Copy the [mm:ss] of the line each note comes from. Skip small \
talk and return no notes when nothing is worth keeping. Use only what the transcript says; \
never invent names, dates, or numbers.";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveNotes {
    #[serde(default)]
    pub sections: Vec<NoteSection>,
    /// The meeting timeline through which the transcript has been read.
    #[serde(default)]
    pub covered_ms: u64,
    #[serde(default)]
    pub passes: u32,
    #[serde(default)]
    pub updated_at_ms: u64,
    /// A different topic heard on the last pass, opened as a section when
    /// the next pass also departs from the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingTopic>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingTopic {
    pub title: String,
    pub start_ms: u64,
}

impl LiveNotes {
    /// The topic being discussed now: the newest section.
    pub fn topic(&self) -> Option<&str> {
        self.sections.last().map(|section| section.title.as_str())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteSection {
    pub title: String,
    pub start_ms: u64,
    #[serde(default)]
    pub bullets: Vec<NoteBullet>,
    /// The latest topic names reported while this section was current.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heard: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteBullet {
    pub kind: NoteKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub at_ms: u64,
    /// The transcript lines the note came from.
    #[serde(default)]
    pub excerpt: Vec<TranscriptEntry>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    #[default]
    Point,
    Decision,
    Action,
    Question,
}

impl NoteKind {
    fn parse(kind: &str) -> Self {
        match kind.trim().to_ascii_lowercase().as_str() {
            "decision" => Self::Decision,
            "action" | "action item" | "todo" => Self::Action,
            "question" | "open question" => Self::Question,
            _ => Self::Point,
        }
    }

    /// The English label shown before a note; points have none.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Point => None,
            Self::Decision => Some("Decision"),
            Self::Action => Some("To do"),
            Self::Question => Some("Question"),
        }
    }
}

/// When to run a pass: after the opening, then once enough new meeting time
/// and words exist. Failures back off without giving up.
#[derive(Debug, Default)]
pub(super) struct PassPlan {
    retry_at_ms: u64,
    failures: u32,
}

impl PassPlan {
    pub fn due(&self, notes: &LiveNotes, timeline_ms: u64, fresh_words: usize) -> bool {
        if timeline_ms < self.retry_at_ms || fresh_words < MIN_NEW_WORDS {
            return false;
        }
        if notes.passes == 0 {
            timeline_ms >= FIRST_PASS_MS
        } else {
            timeline_ms >= notes.covered_ms + PASS_INTERVAL_MS
        }
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
    /// The uncoalesced entries in the prompt, for timestamps and excerpts.
    entries: Vec<TranscriptEntry>,
    text: String,
    /// Backlog older than the prompt budget was skipped.
    skipped: bool,
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
    let text = coalesce_transcript(kept.iter().cloned())
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
        entries: kept,
        text,
        skipped,
        end_ms,
    })
}

/// Only the new transcript: given the current topic, the model keeps
/// repeating it, so topic changes are decided in [`merge`].
pub(super) fn prompt(window: &PassWindow) -> String {
    let mut prompt = String::from("Transcript:\n");
    if window.skipped {
        prompt.push_str("[…]\n");
    }
    prompt.push_str(&window.text);
    prompt
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PassOutcome {
    pub topic_changed: bool,
    pub added: usize,
}

/// Folds one pass into the outline. The first topic opens the outline. A
/// later topic is the same one when most of its words were already heard in
/// the current section; a different topic opens a section, named and dated
/// by the first pass away, only when the next pass departs too and the
/// current topic has had its dwell. It takes the notes made since. Notes join the current section unless
/// they repeat one; a full section trades a plain point for anything else.
pub(super) fn merge(
    notes: &mut LiveNotes,
    generated: GeneratedLiveNotes,
    window: &PassWindow,
) -> PassOutcome {
    let mut outcome = PassOutcome::default();
    let topic = clean_topic(&generated.topic);
    let window_start = window
        .entries
        .first()
        .map_or(window.end_ms, |entry| entry.start_ms);
    let room = notes.sections.len() < MAX_SECTIONS;
    if !topic.is_empty() {
        match notes.sections.last_mut() {
            None => {
                open_section(notes, topic, window_start, Vec::new());
                outcome.topic_changed = true;
            }
            Some(section) if same_topic(section, &topic) => {
                remember(section, topic);
                notes.pending = None;
            }
            Some(section) => match notes.pending.take() {
                Some(pending)
                    if room && pending.start_ms >= section.start_ms + MIN_TOPIC_DWELL_MS =>
                {
                    let split = section
                        .bullets
                        .iter()
                        .position(|bullet| bullet.at_ms >= pending.start_ms)
                        .unwrap_or(section.bullets.len());
                    let moved = section.bullets.split_off(split);
                    open_section(notes, pending.title, pending.start_ms, moved);
                    if let Some(section) = notes.sections.last_mut() {
                        remember(section, topic);
                    }
                    outcome.topic_changed = true;
                }
                _ => {
                    notes.pending = Some(PendingTopic {
                        title: topic,
                        start_ms: window_start,
                    });
                }
            },
        }
    }
    if let Some(section_index) = notes.sections.len().checked_sub(1) {
        for note in generated.notes {
            let Some(bullet) = bullet(note, window) else {
                continue;
            };
            let repeated = notes.sections.iter().rev().take(2).any(|section| {
                section
                    .bullets
                    .iter()
                    .any(|known| overlap(&bullet.text, &known.text) >= DUPLICATE_OVERLAP)
            });
            if !repeated && add_bullet(&mut notes.sections[section_index], bullet) {
                outcome.added += 1;
            }
        }
    }
    notes.covered_ms = notes.covered_ms.max(window.end_ms);
    notes.passes += 1;
    outcome
}

fn open_section(notes: &mut LiveNotes, title: String, start_ms: u64, bullets: Vec<NoteBullet>) {
    notes.sections.push(NoteSection {
        heard: vec![title.clone()],
        title,
        start_ms,
        bullets,
    });
    notes.pending = None;
}

fn remember(section: &mut NoteSection, topic: String) {
    section.heard.push(topic);
    let excess = section.heard.len().saturating_sub(HEARD_TOPICS);
    section.heard.drain(..excess);
}

/// Most of the topic's words were in the section's title or a topic heard
/// while it was current.
fn same_topic(section: &NoteSection, topic: &str) -> bool {
    let words = content_words(topic);
    if words.is_empty() {
        return true;
    }
    let mut known = content_words(&section.title);
    for heard in &section.heard {
        known.extend(content_words(heard));
    }
    words.intersection(&known).count() as f32 / words.len() as f32 >= SAME_TOPIC_OVERLAP
}

/// Adds a note to a section with room, or in place of its oldest plain
/// point when the note is a decision, action, or question.
fn add_bullet(section: &mut NoteSection, bullet: NoteBullet) -> bool {
    if section.bullets.len() >= MAX_BULLETS {
        if bullet.kind == NoteKind::Point {
            return false;
        }
        let Some(point) = section
            .bullets
            .iter()
            .position(|known| known.kind == NoteKind::Point)
        else {
            return false;
        };
        section.bullets.remove(point);
    }
    let at = section
        .bullets
        .partition_point(|known| known.at_ms <= bullet.at_ms);
    section.bullets.insert(at, bullet);
    true
}

fn bullet(note: GeneratedLiveNote, window: &PassWindow) -> Option<NoteBullet> {
    let text = truncate_words(
        note.text
            .trim()
            .trim_start_matches(['-', '*', '•'])
            .trim()
            .trim_end_matches('.'),
        MAX_NOTE_CHARS,
    );
    if text.split_whitespace().count() < 2 {
        return None;
    }
    let kind = NoteKind::parse(&note.kind);
    let owner = (kind == NoteKind::Action)
        .then(|| {
            note.owner
                .split_whitespace()
                .take(MAX_OWNER_WORDS)
                .collect::<Vec<_>>()
                .join(" ")
                .trim_matches(['.', ','])
                .to_string()
        })
        .filter(|owner| {
            !owner.is_empty()
                && !matches!(
                    owner.to_ascii_lowercase().as_str(),
                    "none" | "unknown" | "n/a" | "nobody"
                )
        });
    let anchor = anchor(&note.time, &text, &window.entries)?;
    let at_ms = window.entries[anchor].start_ms;
    // The line just before gives the note its context when it is close.
    let first = anchor
        .checked_sub(1)
        .filter(|previous| window.entries[*previous].start_ms + EXCERPT_LEAD_MS >= at_ms)
        .unwrap_or(anchor);
    let excerpt = coalesce_transcript(
        window.entries[first..]
            .iter()
            .take(EXCERPT_ENTRIES)
            .filter(|entry| entry.start_ms < at_ms + EXCERPT_SPAN_MS)
            .cloned(),
    )
    .into_iter()
    .map(|mut turn| {
        turn.text = truncate_words(turn.text.trim(), EXCERPT_CHARS);
        turn
    })
    .collect();
    Some(NoteBullet {
        kind,
        text,
        owner,
        at_ms,
        excerpt,
    })
}

/// The entry a note came from: the latest line starting at its timestamp,
/// else the line sharing the most words with it.
fn anchor(time: &str, text: &str, entries: &[TranscriptEntry]) -> Option<usize> {
    if entries.is_empty() {
        return None;
    }
    let first = entries[0].start_ms;
    let last = entries[entries.len() - 1].end_ms;
    if let Some(at_ms) = parse_clock(time)
        && at_ms + 1_000 >= first
        && at_ms <= last + 1_000
    {
        return Some(
            entries
                .iter()
                .rposition(|entry| entry.start_ms < at_ms + 1_000)
                .unwrap_or(0),
        );
    }
    let words = content_words(text);
    let best = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            (
                content_words(&entry.text).intersection(&words).count(),
                index,
            )
        })
        .max_by_key(|(shared, index)| (*shared, std::cmp::Reverse(*index)))
        .filter(|(shared, _)| *shared > 0)
        .map_or(0, |(_, index)| index);
    Some(best)
}

/// `mm:ss`, `h:mm:ss`, or either in brackets, as milliseconds.
fn parse_clock(time: &str) -> Option<u64> {
    let parts = time
        .trim()
        .trim_matches(['[', ']'])
        .split(':')
        .map(|part| part.trim().parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let seconds = match parts.as_slice() {
        [minutes, seconds] if *seconds < 60 => minutes * 60 + seconds,
        [hours, minutes, seconds] if *minutes < 60 && *seconds < 60 => {
            hours * 3_600 + minutes * 60 + seconds
        }
        _ => return None,
    };
    Some(seconds * 1_000)
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

/// The share of the shorter phrase's content words found in the other.
fn overlap(left: &str, right: &str) -> f32 {
    let left = content_words(left);
    let right = content_words(right);
    let smaller = left.len().min(right.len());
    if smaller == 0 {
        return 0.0;
    }
    left.intersection(&right).count() as f32 / smaller as f32
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

/// Writes the outline atomically, readable only by the owner.
pub(super) fn write(directory: &Path, notes: &LiveNotes) -> Result<()> {
    let temporary = directory.join(format!("{FILE}.tmp"));
    let mut file = File::create(&temporary)?;
    set_owner_only(&temporary, false)?;
    serde_json::to_writer_pretty(&mut file, notes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temporary, directory.join(FILE))?;
    sync_directory(directory)
}

fn read_path(path: &Path) -> Result<Option<LiveNotes>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(serde_json::from_slice(&data)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// The stored outline of a meeting, if it has one.
pub fn read(id: &str) -> Result<Option<LiveNotes>> {
    read_path(&root()?.join(id).join(FILE))
}

/// Re-reads a meeting's outline only when the file changes, keeping the last
/// good copy through a failed read.
#[derive(Default)]
pub struct LiveNotesReader {
    id: Option<String>,
    modified: Option<SystemTime>,
    notes: Option<LiveNotes>,
}

impl LiveNotesReader {
    /// Returns whether the outline changed since the last call.
    pub fn refresh(&mut self, id: &str) -> bool {
        if self.id.as_deref() != Some(id) {
            *self = Self {
                id: Some(id.into()),
                ..Self::default()
            };
        }
        let Ok(path) = root().map(|root| root.join(id).join(FILE)) else {
            return false;
        };
        let modified = fs::metadata(&path)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        if modified.is_none() || modified == self.modified {
            return false;
        }
        match read_path(&path) {
            Ok(notes) => {
                self.modified = modified;
                let changed = self.notes != notes;
                self.notes = notes;
                changed
            }
            Err(error) => {
                tracing::debug!(%error, meeting = id, "could not read live meeting notes");
                false
            }
        }
    }

    pub fn notes(&self) -> Option<&LiveNotes> {
        self.notes.as_ref()
    }
}

type Generate =
    Box<dyn Fn(&str, &str, Duration) -> Result<GeneratedLiveNotes, GenerationError> + Send>;

/// Keeps a recording meeting's outline in the background. Dropping it stops
/// the worker and discards a pass still running.
pub(super) struct LiveNotesWorker {
    stop: Arc<AtomicBool>,
}

impl LiveNotesWorker {
    pub fn start(id: String) -> Result<Self> {
        let directory = root()?.join(&id);
        Ok(Self::start_with(
            id,
            directory,
            CHECK_INTERVAL,
            super::notes::notes_wanted,
            Box::new(apple_intelligence::generate_live_notes),
        ))
    }

    fn start_with(
        id: String,
        directory: PathBuf,
        check_interval: Duration,
        wanted: fn() -> bool,
        generate: Generate,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        if let Err(error) = thread::Builder::new()
            .name("meeting-live-notes".into())
            .spawn(move || {
                crate::parakeet::deprioritize_background_thread();
                keep_notes(
                    &id,
                    &directory,
                    check_interval,
                    wanted,
                    &generate,
                    &worker_stop,
                );
            })
        {
            tracing::warn!(%error, "could not start live meeting notes");
        }
        Self { stop }
    }
}

impl Drop for LiveNotesWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn keep_notes(
    id: &str,
    directory: &Path,
    check_interval: Duration,
    wanted: fn() -> bool,
    generate: &Generate,
    stop: &AtomicBool,
) {
    let mut reader = TranscriptReader::default();
    let mut notes = read_path(&directory.join(FILE))
        .ok()
        .flatten()
        .unwrap_or_default();
    let mut plan = PassPlan::default();
    let mut next_check = Instant::now() + check_interval;
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() < next_check {
            thread::sleep(check_interval.min(Duration::from_millis(250)));
            continue;
        }
        next_check = Instant::now() + check_interval;
        // Unavailable or turned off: nothing new is shown and nothing is logged.
        if !wanted() {
            continue;
        }
        let entries = match reader.read(id) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::debug!(%error, meeting = id, "could not read the transcript for live notes");
                continue;
            }
        };
        let timeline_ms = entries.iter().map(|entry| entry.end_ms).max().unwrap_or(0);
        if !plan.due(&notes, timeline_ms, fresh_words(&entries, notes.covered_ms)) {
            continue;
        }
        let Some(window) = pass_window(&entries, notes.covered_ms, PROMPT_CHARS) else {
            continue;
        };
        if !super::notes::wait_for_dictation(stop) {
            return;
        }
        let prompt = prompt(&window);
        let started = Instant::now();
        let result = skip_refused(generate(INSTRUCTIONS, &prompt, PASS_TIMEOUT));
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(generated) => {
                let mut next = notes.clone();
                let outcome = merge(&mut next, generated, &window);
                next.updated_at_ms = now_ms();
                match write(directory, &next) {
                    Ok(()) => {
                        tracing::info!(
                            meeting = id,
                            elapsed_ms,
                            prompt_chars = prompt.len(),
                            skipped = window.skipped,
                            added = outcome.added,
                            topic_changed = outcome.topic_changed,
                            "updated live meeting notes"
                        );
                        notes = next;
                        plan.record(true, timeline_ms);
                    }
                    Err(error) => {
                        tracing::warn!(%error, meeting = id, "could not save live meeting notes");
                        plan.record(false, timeline_ms);
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, meeting = id, elapsed_ms, "could not update live meeting notes");
                plan.record(false, timeline_ms);
            }
        }
    }
}

/// Retrying a refused transcript gets the same refusal, so its window counts
/// as read with nothing to note.
fn skip_refused(
    result: Result<GeneratedLiveNotes, GenerationError>,
) -> Result<GeneratedLiveNotes, GenerationError> {
    match result {
        Err(GenerationError::Refused) => {
            tracing::info!("live meeting notes skipped a passage the model refused");
            Ok(GeneratedLiveNotes::default())
        }
        result => result,
    }
}

/// `[mm:ss] Topic` lines of the outline, for the final summary prompt.
pub(super) fn topic_lines(notes: &LiveNotes, max_chars: usize) -> String {
    let mut lines = String::new();
    for section in &notes.sections {
        let line = format!(
            "[{}] {}\n",
            format_duration(section.start_ms),
            section.title
        );
        if lines.len() + line.len() > max_chars {
            break;
        }
        lines.push_str(&line);
    }
    lines
}

/// The outline as Markdown, for the exported notes block.
pub(super) fn outline_markdown(notes: &LiveNotes) -> String {
    let mut markdown = String::new();
    for section in &notes.sections {
        markdown.push_str(&format!(
            "- **{} {}**\n",
            format_duration(section.start_ms),
            section.title
        ));
        for bullet in &section.bullets {
            let label = match (bullet.kind.label(), &bullet.owner) {
                (Some(label), Some(owner)) => format!("{label} ({owner}): "),
                (Some(label), None) => format!("{label}: "),
                (None, _) => String::new(),
            };
            markdown.push_str(&format!(
                "  - {label}{} · {}\n",
                bullet.text,
                format_duration(bullet.at_ms)
            ));
        }
    }
    markdown
}

/// A deterministic stand-in for the model: the window's most frequent words
/// name the topic, and questions, plans, and agreements become notes. Used by
/// the replay preview where Apple Intelligence is unavailable.
#[cfg(any(debug_assertions, test))]
pub fn canned_generate(
    _instructions: &str,
    prompt: &str,
    _timeout: Duration,
) -> Result<GeneratedLiveNotes, GenerationError> {
    let lines = prompt
        .lines()
        .filter_map(|line| {
            let (clock, rest) = line.strip_prefix('[')?.split_once("] ")?;
            let (speaker, text) = rest.split_once(": ")?;
            Some((clock, speaker, text))
        })
        .collect::<Vec<_>>();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for (_, _, text) in &lines {
        for word in content_words(text) {
            if word.len() >= 5 {
                *counts.entry(word).or_default() += 1;
            }
        }
    }
    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let topic = ranked
        .iter()
        .take(3)
        .map(|(word, _)| word.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut topic = topic;
    if let Some(first) = topic.get(..1) {
        topic = first.to_uppercase() + &topic[1..];
    }
    let mut notes = Vec::new();
    for (clock, speaker, text) in &lines {
        let lower = text.to_lowercase();
        let kind = if text.ends_with('?') && text.split_whitespace().count() >= 5 {
            "question"
        } else if lower.contains("let's") || lower.contains("i'll") || lower.contains("we'll") {
            "action"
        } else if lower.contains("agreed") || lower.contains("decided") || lower.contains("go with")
        {
            "decision"
        } else {
            continue;
        };
        notes.push(GeneratedLiveNote {
            kind: kind.into(),
            text: truncate_words(text, 80),
            owner: if kind == "action" {
                (*speaker).into()
            } else {
                String::new()
            },
            time: (*clock).into(),
        });
        if notes.len() == 3 {
            break;
        }
    }
    Ok(GeneratedLiveNotes { topic, notes })
}

/// Replays a transcript into a recording meeting folder at `speed`, with the
/// live notes worker reading it as it would a real call. Refuses to run
/// outside an isolated `HEX_APPLICATION_SUPPORT_DIR`. Returns the meeting id
/// and title; the replay holds the meeting open until the process exits.
#[cfg(debug_assertions)]
pub fn replay(
    entries: Vec<TranscriptEntry>,
    speed: f32,
    title: &str,
    canned: bool,
) -> Result<(String, String)> {
    if std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_none() {
        return Err(color_eyre::eyre::eyre!(
            "the meeting replay writes a meeting folder; set HEX_APPLICATION_SUPPORT_DIR to an empty folder"
        ));
    }
    let created_at_ms = now_ms();
    let id = format!("{created_at_ms}-replay");
    let directory = root()?.join(&id);
    fs::create_dir_all(&directory)?;
    set_owner_only(&directory, true)?;
    let active = super::ActiveMeeting::acquire(&directory)?;
    let manifest = super::MeetingManifest {
        schema_version: 1,
        id: id.clone(),
        title: title.into(),
        status: super::MeetingStatus::Recording,
        created_at_ms,
        ended_at_ms: None,
        duration_ms: None,
        system: Default::default(),
        microphone: Default::default(),
        live_transcription_error: None,
        error: None,
        app: None,
        export_path: None,
        export_error: None,
        echo_suppressed_utterances: 0,
        provisional_title: None,
        notes: None,
        notes_error: None,
    };
    super::write_manifest(&directory, &manifest)?;
    let generate: Generate = if canned || !apple_intelligence::availability().is_available() {
        tracing::info!("meeting replay uses canned live notes");
        Box::new(canned_generate)
    } else {
        Box::new(apple_intelligence::generate_live_notes)
    };
    let worker = LiveNotesWorker::start_with(
        id.clone(),
        directory.clone(),
        Duration::from_millis(250),
        || true,
        generate,
    );
    let speed = speed.max(1.0);
    thread::Builder::new()
        .name("meeting-replay".into())
        .spawn(move || {
            let _active = active;
            let _worker = worker;
            let started = Instant::now();
            let mut written = usize::MAX;
            loop {
                let timeline_ms =
                    (started.elapsed().as_secs_f64() * f64::from(speed) * 1_000.0) as u64;
                let visible = entries.partition_point(|entry| entry.end_ms <= timeline_ms);
                if visible != written {
                    if let Err(error) =
                        write_replay_snapshot(&directory, &entries[..visible], timeline_ms)
                    {
                        tracing::warn!(%error, "could not write the replay transcript");
                    }
                    written = visible;
                }
                thread::sleep(Duration::from_millis(200));
            }
        })?;
    Ok((id, title.into()))
}

#[cfg(debug_assertions)]
fn write_replay_snapshot(
    directory: &Path,
    entries: &[TranscriptEntry],
    timeline_ms: u64,
) -> Result<()> {
    use super::transcript::{ROLLING_FILE, RollingCoverage, RollingLine};
    let temporary = directory.join(format!("{ROLLING_FILE}.tmp"));
    let mut file = std::io::BufWriter::new(File::create(&temporary)?);
    set_owner_only(&temporary, false)?;
    serde_json::to_writer(
        &mut file,
        &RollingLine::Coverage {
            covered_ms: RollingCoverage {
                system: Some(timeline_ms),
                microphone: Some(timeline_ms),
            },
        },
    )?;
    file.write_all(b"\n")?;
    for entry in entries {
        serde_json::to_writer(&mut file, entry)?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    fs::rename(&temporary, directory.join(ROLLING_FILE))?;
    Ok(())
}

/// Reads replay entries from a `transcript.ndjson` or an exported Markdown
/// transcript, keeping the first `limit_ms` of the meeting.
#[cfg(debug_assertions)]
pub fn replay_entries(path: &Path, limit_ms: Option<u64>) -> Result<Vec<TranscriptEntry>> {
    let mut entries = if path
        .extension()
        .is_some_and(|extension| extension == "ndjson")
    {
        super::read_ndjson::<TranscriptEntry>(path)?
    } else {
        super::notes::entries_from_markdown(&fs::read_to_string(path)?)
    };
    entries.retain(|entry| limit_ms.is_none_or(|limit| entry.end_ms <= limit));
    entries.sort_by_key(|entry| entry.end_ms);
    if entries.is_empty() {
        return Err(color_eyre::eyre::eyre!(
            "{} has no transcript turns",
            path.display()
        ));
    }
    Ok(entries)
}

/// A debug export of notes or transcript lines, readable only by the owner.
#[cfg(debug_assertions)]
fn create_private(path: &Path) -> Result<File> {
    let file = File::create(path)?;
    set_owner_only(path, false)?;
    Ok(file)
}

/// The model behind [`simulate`].
#[cfg(debug_assertions)]
pub enum SimulatedModel<'a> {
    /// Apple Intelligence, optionally saving each pass's raw output.
    AppleIntelligence {
        record: Option<&'a Path>,
    },
    Canned,
    /// Raw outputs saved by an earlier run, in pass order.
    Recorded(&'a Path),
}

/// Runs live notes over a transcript as a recording would, checking every
/// five seconds of meeting time, and reports each pass's latency and size.
/// Prompts don't depend on earlier passes, so a recorded run replays the
/// same passes. `snapshots` saves the outline after each pass, as
/// `{"at_ms", "notes"}` lines for the capsule lab. Debug CLI:
/// `hex meeting live-notes <path>`.
#[cfg(debug_assertions)]
pub fn simulate(
    entries: &[TranscriptEntry],
    model: SimulatedModel<'_>,
    snapshots: Option<&Path>,
) -> Result<String> {
    use std::fmt::Write as _;
    let mut snapshots = snapshots.map(create_private).transpose()?;
    let generate: Generate = match model {
        SimulatedModel::Canned => Box::new(canned_generate),
        SimulatedModel::AppleIntelligence { record } => {
            let availability = apple_intelligence::availability();
            if !availability.is_available() {
                return Err(color_eyre::eyre::eyre!("{}", availability.description()));
            }
            let record = record
                .map(create_private)
                .transpose()?
                .map(std::sync::Mutex::new);
            Box::new(move |instructions, prompt, timeout| {
                let result = apple_intelligence::generate_live_notes(instructions, prompt, timeout);
                if let Some(file) = &record {
                    let line = match &result {
                        Ok(generated) => serde_json::to_string(generated).unwrap_or_default(),
                        Err(GenerationError::Refused) => "\"refused\"".into(),
                        Err(error) => serde_json::to_string(&error.to_string()).unwrap_or_default(),
                    };
                    let _ = writeln!(file.lock().unwrap(), "{line}");
                }
                result
            })
        }
        SimulatedModel::Recorded(path) => {
            let lines = fs::read_to_string(path)?
                .lines()
                .map(str::to_owned)
                .collect::<std::collections::VecDeque<_>>();
            let lines = std::sync::Mutex::new(lines);
            Box::new(move |_, _, _| {
                let line = lines.lock().unwrap().pop_front().unwrap_or_default();
                match serde_json::from_str::<GeneratedLiveNotes>(&line) {
                    Ok(generated) => Ok(generated),
                    Err(_) if line == "\"refused\"" => Err(GenerationError::Refused),
                    Err(_) => Err(apple_intelligence::classify_error(
                        serde_json::from_str::<String>(&line).unwrap_or(line),
                    )),
                }
            })
        }
    };
    let end_ms = entries.iter().map(|entry| entry.end_ms).max().unwrap_or(0);
    let mut notes = LiveNotes::default();
    let mut plan = PassPlan::default();
    let mut report = String::new();
    let mut timings = Vec::new();
    let mut prompt_sizes = Vec::new();
    let mut check_ms = 0;
    while check_ms <= end_ms + CHECK_INTERVAL.as_millis() as u64 {
        check_ms += CHECK_INTERVAL.as_millis() as u64;
        let visible = entries
            .iter()
            .filter(|entry| entry.end_ms <= check_ms)
            .cloned()
            .collect::<Vec<_>>();
        let timeline_ms = visible.iter().map(|entry| entry.end_ms).max().unwrap_or(0);
        if !plan.due(&notes, timeline_ms, fresh_words(&visible, notes.covered_ms)) {
            continue;
        }
        let Some(window) = pass_window(&visible, notes.covered_ms, PROMPT_CHARS) else {
            continue;
        };
        let prompt = prompt(&window);
        let started = Instant::now();
        let result = generate(INSTRUCTIONS, &prompt, PASS_TIMEOUT);
        let elapsed = started.elapsed();
        let refused = result == Err(GenerationError::Refused);
        let result = skip_refused(result);
        match result {
            Ok(generated) => {
                let raw_topic = generated.topic.clone();
                let outcome = merge(&mut notes, generated, &window);
                if let Some(file) = &mut snapshots {
                    let line = serde_json::json!({ "at_ms": timeline_ms, "notes": notes });
                    writeln!(file, "{line}")?;
                }
                timings.push(elapsed.as_millis() as u64);
                prompt_sizes.push(prompt.len());
                let _ = writeln!(
                    report,
                    "pass {:>2} at {} · {:.2}s · {} prompt chars{} · model topic \"{raw_topic}\"{} -> {}{} · +{} notes",
                    notes.passes,
                    format_duration(timeline_ms),
                    elapsed.as_secs_f32(),
                    prompt.len(),
                    if window.skipped {
                        " (backlog skipped)"
                    } else {
                        ""
                    },
                    if refused { " (refused)" } else { "" },
                    notes.topic().unwrap_or("(no topic)"),
                    if outcome.topic_changed { " (new)" } else { "" },
                    outcome.added
                );
                plan.record(true, timeline_ms);
            }
            Err(error) => {
                let _ = writeln!(
                    report,
                    "pass at {} failed after {:.2}s: {error}",
                    format_duration(timeline_ms),
                    elapsed.as_secs_f32()
                );
                plan.record(false, timeline_ms);
            }
        }
    }
    timings.sort_unstable();
    if let (Some(fastest), Some(slowest)) = (timings.first(), timings.last()) {
        let mean_prompt = prompt_sizes.iter().sum::<usize>() / prompt_sizes.len().max(1);
        let _ = writeln!(
            report,
            "\n{} passes over {} · latency p50 {:.2}s, min {:.2}s, max {:.2}s · prompt mean {mean_prompt} chars (~{} tokens) · stored {} bytes",
            timings.len(),
            format_duration(end_ms),
            timings[timings.len() / 2] as f32 / 1_000.0,
            *fastest as f32 / 1_000.0,
            *slowest as f32 / 1_000.0,
            mean_prompt / 4,
            serde_json::to_vec_pretty(&notes)?.len()
        );
    }
    let _ = write!(report, "\n{}", outline_markdown(&notes));
    Ok(report)
}

/// A synthetic outline for tests.
#[cfg(test)]
fn preview_notes() -> LiveNotes {
    let line = |source, seconds: u64, text: &str| TranscriptEntry {
        source,
        start_ms: seconds * 1_000,
        end_ms: seconds * 1_000 + 4_000,
        text: text.into(),
    };
    let note =
        |kind, text: &str, owner: Option<&str>, seconds: u64, excerpt: Vec<TranscriptEntry>| {
            NoteBullet {
                kind,
                text: text.into(),
                owner: owner.map(Into::into),
                at_ms: seconds * 1_000,
                excerpt,
            }
        };
    use super::MeetingSource::{Microphone as You, System as Them};
    LiveNotes {
        sections: vec![
            NoteSection {
                title: "Arcade login and accounts".into(),
                start_ms: 12_000,
                heard: Vec::new(),
                bullets: vec![
                    note(
                        NoteKind::Decision,
                        "One login across every Arcade game",
                        None,
                        41,
                        vec![
                            line(Them, 41, "So every game shares the same account, right?"),
                            line(You, 45, "Yes, one login. We agreed on that last week."),
                        ],
                    ),
                    note(
                        NoteKind::Point,
                        "Twenty to thirty games over the next two months",
                        None,
                        96,
                        vec![line(
                            Them,
                            96,
                            "We're aiming for twenty to thirty games over the next couple of months.",
                        )],
                    ),
                ],
            },
            NoteSection {
                title: "Build screen polish".into(),
                start_ms: 214_000,
                heard: Vec::new(),
                bullets: vec![
                    note(
                        NoteKind::Action,
                        "Write a proposal for the build screen",
                        Some("You"),
                        236,
                        vec![
                            line(Them, 231, "The build screen still feels unfinished."),
                            line(You, 236, "I'll write a proposal before we polish it more."),
                        ],
                    ),
                    note(
                        NoteKind::Question,
                        "Should builds stream logs or show steps?",
                        None,
                        262,
                        vec![line(
                            Them,
                            262,
                            "Do we stream the raw logs, or just show the steps?",
                        )],
                    ),
                ],
            },
            NoteSection {
                title: "Launch checklist and owners".into(),
                start_ms: 371_000,
                heard: Vec::new(),
                bullets: vec![note(
                    NoteKind::Action,
                    "Draft the launch post and changelog",
                    Some("Them"),
                    402,
                    vec![line(
                        Them,
                        402,
                        "I can draft the launch post and the changelog this week.",
                    )],
                )],
            },
        ],
        covered_ms: 420_000,
        passes: 6,
        updated_at_ms: now_ms(),
        pending: None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::MeetingSource;
    use super::*;

    fn entry(source: MeetingSource, seconds: u64, text: &str) -> TranscriptEntry {
        TranscriptEntry {
            source,
            start_ms: seconds * 1_000,
            end_ms: seconds * 1_000 + 3_000,
            text: text.into(),
        }
    }

    fn generated(topic: &str, notes: &[(&str, &str, &str, &str)]) -> GeneratedLiveNotes {
        GeneratedLiveNotes {
            topic: topic.into(),
            notes: notes
                .iter()
                .map(|(kind, text, owner, time)| GeneratedLiveNote {
                    kind: (*kind).into(),
                    text: (*text).into(),
                    owner: (*owner).into(),
                    time: (*time).into(),
                })
                .collect(),
        }
    }

    fn window(entries: &[TranscriptEntry], covered_ms: u64) -> PassWindow {
        pass_window(entries, covered_ms, PROMPT_CHARS).unwrap()
    }

    #[test]
    fn passes_wait_for_the_opening_then_new_time_and_words_and_back_off() {
        let mut notes = LiveNotes::default();
        let mut plan = PassPlan::default();
        assert!(!plan.due(&notes, 40_000, 200), "opening too short");
        assert!(!plan.due(&notes, 60_000, 20), "too few words");
        assert!(plan.due(&notes, 45_000, 40));
        notes.passes = 1;
        notes.covered_ms = 50_000;
        assert!(!plan.due(&notes, 120_000, 200));
        assert!(plan.due(&notes, 125_000, 200));
        plan.record(false, 125_000);
        assert!(!plan.due(&notes, 180_000, 200), "a minute of back-off");
        assert!(plan.due(&notes, 185_000, 200));
        plan.record(false, 185_000);
        assert!(
            !plan.due(&notes, 300_000, 200),
            "two minutes after a second failure"
        );
        assert!(plan.due(&notes, 305_000, 200));
        plan.record(true, 305_000);
        assert!(plan.due(&notes, 305_000, 200));
        for _ in 0..20 {
            plan.record(false, 0);
        }
        assert!(plan.due(&notes, MAX_RETRY_MS, 200), "back-off is capped");
    }

    #[test]
    fn a_pass_reads_only_new_transcript_and_keeps_the_newest_within_budget() {
        let entries = (0..40)
            .map(|index| {
                entry(
                    if index % 2 == 0 {
                        MeetingSource::Microphone
                    } else {
                        MeetingSource::System
                    },
                    index * 10,
                    &format!("Line {index} is about the arcade launch and its login."),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(fresh_words(&entries, 200_000), 20 * 10);
        let whole = window(&entries, 200_000);
        assert!(!whole.skipped);
        assert!(
            whole.text.starts_with("[03:20] You: Line 20"),
            "{}",
            whole.text
        );
        assert_eq!(whole.end_ms, 393_000);

        let tight = pass_window(&entries, 0, 400).unwrap();
        assert!(tight.skipped);
        assert!(tight.text.len() <= 400, "{}", tight.text.len());
        assert!(
            tight
                .text
                .ends_with("Line 39 is about the arcade launch and its login.")
        );
        assert_eq!(tight.end_ms, 393_000);
        assert!(prompt(&tight).starts_with("Transcript:\n[…]\n["));
        assert!(pass_window(&entries, 400_000, PROMPT_CHARS).is_none());
    }

    #[test]
    fn topics_change_after_two_passes_away_and_their_dwell() {
        let at = |seconds, text: &str| [entry(MeetingSource::System, seconds, text)];
        let mut notes = LiveNotes::default();
        let outcome = merge(
            &mut notes,
            generated("“Arcade login plan.”", &[]),
            &window(&at(10, "Let's start with the arcade login."), 0),
        );
        assert!(outcome.topic_changed);
        assert_eq!(notes.topic(), Some("Arcade login plan"));
        assert_eq!(notes.sections[0].start_ms, 10_000);
        assert_eq!(notes.covered_ms, 13_000);

        for seconds in [60, 70] {
            merge(
                &mut notes,
                generated("Build screen", &[]),
                &window(&at(seconds, "Now the build screen."), 13_000),
            );
        }
        assert_eq!(notes.sections.len(), 1, "the opening topic keeps its dwell");

        merge(
            &mut notes,
            generated("Login accounts", &[]),
            &window(&at(130, "Accounts again."), 73_000),
        );
        assert_eq!(notes.pending, None, "a familiar topic cancels a departure");
        merge(
            &mut notes,
            generated("Arcade accounts", &[]),
            &window(&at(150, "Sign-in for accounts."), 133_000),
        );
        assert_eq!(
            notes.sections.len(),
            1,
            "words heard in the section stay in it"
        );

        merge(
            &mut notes,
            generated(
                "Build screen polish",
                &[(
                    "action",
                    "Write a proposal for the build screen",
                    "You",
                    "03:20",
                )],
            ),
            &window(&at(200, "The build screen needs a proposal."), 153_000),
        );
        assert_eq!(notes.sections.len(), 1, "one pass away is not enough");
        assert_eq!(notes.sections[0].bullets.len(), 1);
        merge(
            &mut notes,
            generated("Build logs and steps", &[]),
            &window(&at(280, "Do builds stream logs?"), 203_000),
        );
        assert_eq!(notes.topic(), Some("Build screen polish"));
        let section = &notes.sections[1];
        assert_eq!(
            section.start_ms, 200_000,
            "the section starts at the departure"
        );
        assert_eq!(
            section.heard,
            ["Build screen polish", "Build logs and steps"]
        );
        assert_eq!(section.bullets.len(), 1, "notes since the departure move");
        assert!(notes.sections[0].bullets.is_empty());
        assert_eq!(notes.pending, None);
        assert_eq!(notes.passes, 7);
    }

    #[test]
    fn full_sections_keep_decisions_actions_and_questions_over_points() {
        let lines = (0..6)
            .map(|index| entry(MeetingSource::System, index * 10, "Words about the arcade."))
            .collect::<Vec<_>>();
        let mut notes = LiveNotes::default();
        merge(
            &mut notes,
            generated(
                "Arcade",
                &[
                    ("point", "Forty games planned this year", "", "00:00"),
                    ("question", "Who owns the arcade login", "", "00:10"),
                    ("point", "Lighting matters most for mood", "", "00:20"),
                ],
            ),
            &window(&lines, 0),
        );
        let outcome = merge(
            &mut notes,
            generated(
                "Arcade",
                &[
                    ("point", "Sound is broken in the demo", "", "00:30"),
                    ("decision", "Ship one login across games", "", "00:40"),
                ],
            ),
            &window(&lines, 0),
        );
        assert_eq!(outcome.added, 1);
        let kept = notes.sections[0]
            .bullets
            .iter()
            .map(|bullet| bullet.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kept,
            [NoteKind::Question, NoteKind::Point, NoteKind::Decision],
            "the oldest point made room, in meeting order"
        );
    }

    #[test]
    fn notes_anchor_to_their_line_carry_an_excerpt_and_skip_repeats() {
        let lines = [
            entry(
                MeetingSource::System,
                300,
                "The build screen still feels unfinished.",
            ),
            entry(
                MeetingSource::Microphone,
                305,
                "I'll write a proposal before we polish it.",
            ),
            entry(MeetingSource::Microphone, 309, "Probably by Friday."),
            entry(
                MeetingSource::System,
                340,
                "Do we stream logs or show steps?",
            ),
        ];
        let mut notes = LiveNotes::default();
        let outcome = merge(
            &mut notes,
            generated(
                "Build screen",
                &[
                    (
                        "action",
                        "Write a proposal for the build screen.",
                        "You",
                        "05:05",
                    ),
                    ("question", "Stream logs or show steps", "none", "[5:40]"),
                    ("point", "ok", "", "05:00"),
                    ("decision", "Ship the arcade login first", "Them", "99:99"),
                ],
            ),
            &window(&lines, 0),
        );
        assert_eq!(outcome.added, 3);
        let bullets = &notes.sections[0].bullets;
        assert_eq!(bullets[0].kind, NoteKind::Decision);
        assert_eq!(bullets[0].owner, None);
        assert_eq!(
            bullets[0].at_ms, 300_000,
            "an invalid time falls back to the first line"
        );
        assert_eq!(bullets[1].kind, NoteKind::Action);
        assert_eq!(bullets[1].text, "Write a proposal for the build screen");
        assert_eq!(bullets[1].owner.as_deref(), Some("You"));
        assert_eq!(bullets[1].at_ms, 305_000);
        assert_eq!(bullets[1].excerpt.len(), 2, "{:?}", bullets[1].excerpt);
        assert_eq!(bullets[1].excerpt[0].source, MeetingSource::System);
        assert_eq!(
            bullets[1].excerpt[1].text,
            "I'll write a proposal before we polish it. Probably by Friday."
        );
        assert_eq!(bullets[2].kind, NoteKind::Question);
        assert_eq!(bullets[2].owner, None, "only actions have owners");
        assert_eq!(bullets[2].at_ms, 340_000);

        let again = merge(
            &mut notes,
            generated(
                "Build screen",
                &[("action", "Write the build screen proposal", "You", "05:05")],
            ),
            &window(&lines, 0),
        );
        assert_eq!(again.added, 0);
    }

    #[test]
    fn outlines_are_bounded() {
        let mut notes = LiveNotes::default();
        for index in 0..100u64 {
            let covered_ms = notes.covered_ms;
            let lines = [entry(
                MeetingSource::System,
                index * 200,
                &format!("Topic {index} needs words."),
            )];
            let topics = [
                "Arcade login",
                "Build screen",
                "Pricing tiers",
                "Launch checklist",
            ];
            merge(
                &mut notes,
                generated(
                    &format!("{} {index}", topics[index as usize / 2 % 4]),
                    &[
                        ("point", &format!("First fact number {index} alpha"), "", ""),
                        ("point", &format!("Second fact number {index} beta"), "", ""),
                        ("point", &format!("Third fact number {index} gamma"), "", ""),
                    ],
                ),
                &window(&lines, covered_ms),
            );
        }
        assert_eq!(notes.sections.len(), MAX_SECTIONS);
        assert!(
            notes
                .sections
                .iter()
                .all(|section| section.bullets.len() <= MAX_BULLETS)
        );
        assert!(serde_json::to_vec(&notes).unwrap().len() < 64 * 1024);
    }

    #[test]
    fn notes_files_are_owner_only_and_survive_a_bad_write() {
        use std::os::unix::fs::PermissionsExt;
        let directory = std::env::temp_dir().join(format!(
            "hex-live-notes-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&directory).unwrap();
        let notes = preview_notes();
        write(&directory, &notes).unwrap();
        let path = directory.join(FILE);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(read_path(&path).unwrap(), Some(notes));
        fs::write(&path, b"{not json").unwrap();
        assert!(read_path(&path).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_outline_feeds_the_final_prompt_and_markdown() {
        let notes = preview_notes();
        assert_eq!(
            topic_lines(&notes, 1_000),
            "[00:12] Arcade login and accounts\n[03:34] Build screen polish\n[06:11] Launch checklist and owners\n"
        );
        assert_eq!(
            topic_lines(&notes, 40),
            "[00:12] Arcade login and accounts\n"
        );
        let markdown = outline_markdown(&notes);
        assert!(
            markdown.starts_with("- **00:12 Arcade login and accounts**\n  - Decision: One login")
        );
        assert!(
            markdown.contains("  - To do (You): Write a proposal for the build screen · 03:56\n")
        );
    }

    #[test]
    fn canned_notes_name_a_topic_and_pick_questions_and_plans() {
        let lines = [
            entry(
                MeetingSource::System,
                0,
                "The multiplayer map needs hunters and monsters.",
            ),
            entry(
                MeetingSource::Microphone,
                5,
                "Multiplayer hunters could share the monsters map.",
            ),
            entry(
                MeetingSource::System,
                9,
                "Should the monsters respawn after each round?",
            ),
            entry(
                MeetingSource::Microphone,
                14,
                "I'll prototype the multiplayer lobby tonight.",
            ),
        ];
        let window = window(&lines, 0);
        let generated = canned_generate(INSTRUCTIONS, &prompt(&window), PASS_TIMEOUT).unwrap();
        assert!(
            generated.topic.starts_with("Monster"),
            "{}",
            generated.topic
        );
        assert_eq!(generated.notes.len(), 2);
        assert_eq!(generated.notes[0].kind, "question");
        assert_eq!(generated.notes[1].owner, "You");
    }
}
