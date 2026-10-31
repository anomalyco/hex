//! Read-only queries over recorded meetings, for agents (`hex mcp`).
//!
//! Nothing here writes to the meetings folder. A meeting whose recorder holds
//! `active.lock` is in progress: its rolling transcript is read and
//! labeled. Otherwise only the published `transcript.ndjson` is
//! trusted, and an unfinished draft is shown as interrupted, as the app does.
//! Unreadable folders and lines are skipped and reported, never repaired.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use color_eyre::eyre::{Result, eyre};
use serde::de::DeserializeOwned;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use super::transcript::{RollingLine, RollingView};
use super::{
    Chapter, MeetingManifest, MeetingSource, MeetingStatus, TranscriptEntry, chapter_entries,
    coalesce_transcript, display_title, format_duration, notes_markdown, recovered_status,
    section_markdown, transcript_markdown,
};
use crate::vocabulary::VocabularyEntry;

const TURN_GAP_MS: u64 = 3_000;
const PASSAGE_WORDS: usize = 60;
const SNIPPET_TOKENS: usize = 36;
const CONTEXT_CHARS: usize = 120;
const BM25_K1: f32 = 1.2;
const BM25_B: f32 = 0.75;

/// Where a meeting's transcript came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptSource {
    /// The published final transcript.
    Final,
    /// The rolling in-call transcript (in progress or interrupted).
    Rolling,
    Missing,
}

impl TranscriptSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Final => "final transcript",
            Self::Rolling => "rolling transcript",
            Self::Missing => "no transcript",
        }
    }
}

/// One meeting folder as read at the moment of the query.
pub struct Meeting {
    pub id: String,
    pub manifest: MeetingManifest,
    /// The status after applying the app's recovery rules, without writing.
    pub status: MeetingStatus,
    /// Whether a recorder process holds the meeting's lock right now.
    pub in_progress: bool,
    pub source: TranscriptSource,
    pub entries: Vec<TranscriptEntry>,
    pub warnings: Vec<String>,
    passages: Vec<Passage>,
}

impl Meeting {
    pub fn title(&self) -> String {
        display_title(&self.manifest)
    }

    pub fn start_ms(&self) -> u64 {
        self.manifest.created_at_ms
    }

    pub fn duration_ms(&self, now_ms: u64) -> u64 {
        if self.in_progress {
            return now_ms.saturating_sub(self.manifest.created_at_ms);
        }
        self.manifest.duration_ms.unwrap_or_else(|| {
            self.entries
                .iter()
                .map(|entry| entry.end_ms)
                .max()
                .unwrap_or_default()
        })
    }

    /// Entries overlapping `range`, or an error when it starts after the meeting.
    pub fn entries_in(&self, range: OffsetRange, now_ms: u64) -> Result<Vec<&TranscriptEntry>> {
        range.check(self, now_ms)?;
        Ok(self
            .entries
            .iter()
            .filter(|entry| range.overlaps(entry))
            .collect())
    }

    /// Chapter `number` (counting from 1), where it ends, and its entries.
    pub fn chapter(&self, number: usize, now_ms: u64) -> Result<ChapterView<'_>> {
        let chapters = &self.manifest.chapters;
        if chapters.is_empty() {
            return Err(eyre!("Meeting {} has no chapters.", self.id));
        }
        let index = number
            .checked_sub(1)
            .filter(|index| *index < chapters.len())
            .ok_or_else(|| {
                eyre!(
                    "Meeting {} has chapters 1 to {}; there is no chapter {number}.",
                    self.id,
                    chapters.len()
                )
            })?;
        Ok(ChapterView {
            number,
            chapter: &chapters[index],
            end_ms: chapters
                .get(index + 1)
                .map_or_else(|| self.duration_ms(now_ms), |next| next.start_ms)
                .max(chapters[index].start_ms),
            entries: chapter_entries(&self.entries, chapters, index),
        })
    }

    /// Every chapter, in order.
    pub fn chapter_views(&self, now_ms: u64) -> Vec<ChapterView<'_>> {
        (1..=self.manifest.chapters.len())
            .filter_map(|number| self.chapter(number, now_ms).ok())
            .collect()
    }

    fn status_label(&self) -> String {
        let status = status_name(self.status);
        if self.in_progress {
            format!("{status} (in progress)")
        } else {
            status.into()
        }
    }

    fn participants(&self) -> String {
        match self.manifest.app.as_deref() {
            Some(app) => format!("You, Them ({app})"),
            None => "You, Them".into(),
        }
    }

    fn heading(&self) -> String {
        format!(
            "{} \"{}\" · {}",
            self.id,
            self.title(),
            iso_local(self.start_ms())
        )
    }
}

/// One chapter of a meeting with the entries that start inside it.
pub struct ChapterView<'a> {
    /// Counting from 1.
    pub number: usize,
    pub chapter: &'a Chapter,
    pub end_ms: u64,
    pub entries: Vec<TranscriptEntry>,
}

impl ChapterView<'_> {
    fn span(&self) -> String {
        format!(
            "{}–{}",
            format_duration(self.chapter.start_ms),
            format_duration(self.end_ms)
        )
    }

    /// `## title`, its one line, and its turns, for copying one section.
    pub fn markdown(&self) -> String {
        section_markdown(self.chapter, &self.entries)
    }
}

fn status_name(status: MeetingStatus) -> &'static str {
    match status {
        MeetingStatus::Starting => "starting",
        MeetingStatus::Recording => "recording",
        MeetingStatus::Transcribing => "transcribing",
        MeetingStatus::Complete => "complete",
        MeetingStatus::Interrupted => "interrupted",
        MeetingStatus::Failed => "failed",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PassageKind {
    Speech(MeetingSource),
    Notes,
}

struct Passage {
    kind: PassageKind,
    start_ms: u64,
    text: String,
    tokens: Vec<Token>,
    frequencies: HashMap<String, u32>,
}

struct Token {
    stem: String,
    start: usize,
    end: usize,
}

impl Passage {
    fn new(kind: PassageKind, start_ms: u64, text: String) -> Self {
        let tokens = tokenize(&text);
        let mut frequencies = HashMap::new();
        for token in &tokens {
            *frequencies.entry(token.stem.clone()).or_insert(0) += 1;
        }
        Self {
            kind,
            start_ms,
            text,
            tokens,
            frequencies,
        }
    }

    fn speaker(&self) -> &'static str {
        match self.kind {
            PassageKind::Speech(source) => source.label(),
            PassageKind::Notes => "Notes",
        }
    }
}

#[derive(Clone, PartialEq)]
struct Fingerprint {
    files: Vec<Option<(u64, Option<SystemTime>)>>,
    in_progress: bool,
}

const FINGERPRINT_FILES: [&str; 5] = [
    "manifest.json",
    super::transcript::ERROR_FILE,
    "transcript.ndjson",
    "transcript.md",
    super::transcript::ROLLING_FILE,
];

/// The meetings folder plus an in-memory cache keyed by file sizes and times.
pub struct Library {
    root: PathBuf,
    cache: Mutex<HashMap<String, (Fingerprint, Arc<Meeting>)>>,
}

/// A folder that could not be read as a meeting.
pub struct Skipped {
    pub id: String,
    pub reason: String,
}

pub struct Scan {
    pub meetings: Vec<Arc<Meeting>>,
    pub skipped: Vec<Skipped>,
}

impl Library {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cache: Mutex::default(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every readable meeting, newest first.
    pub fn scan(&self) -> Result<Scan> {
        let mut meetings = Vec::new();
        let mut skipped = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Scan { meetings, skipped });
            }
            Err(error) => {
                return Err(eyre!(
                    "could not read the meetings folder {}: {error}",
                    self.root.display()
                ));
            }
        };
        let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        let mut seen = HashSet::new();
        for entry in entries.flatten() {
            let directory = entry.path();
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if id.starts_with('.') || !directory.is_dir() {
                continue;
            }
            seen.insert(id.clone());
            let fingerprint = fingerprint(&directory);
            if let Some((cached, meeting)) = cache.get(&id)
                && *cached == fingerprint
            {
                meetings.push(meeting.clone());
                continue;
            }
            match load(&directory, &id, fingerprint.in_progress) {
                Ok(meeting) => {
                    let meeting = Arc::new(meeting);
                    cache.insert(id, (fingerprint, meeting.clone()));
                    meetings.push(meeting);
                }
                Err(error) => {
                    cache.remove(&id);
                    skipped.push(Skipped {
                        id,
                        reason: error.to_string(),
                    });
                }
            }
        }
        cache.retain(|id, _| seen.contains(id));
        meetings.sort_by_key(|meeting| std::cmp::Reverse(meeting.start_ms()));
        skipped.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(Scan { meetings, skipped })
    }

    /// A meeting by exact id, or by a unique prefix of at least six characters.
    pub fn meeting(&self, id: &str) -> Result<Arc<Meeting>> {
        let id = id.trim();
        let meetings = self.scan()?.meetings;
        if let Some(meeting) = meetings.iter().find(|meeting| meeting.id == id) {
            return Ok(meeting.clone());
        }
        let matches = meetings
            .iter()
            .filter(|meeting| id.len() >= 6 && meeting.id.starts_with(id))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [meeting] => Ok((*meeting).clone()),
            [] => Err(eyre!(
                "No meeting with id {id:?}. Use list_meetings to find meeting ids."
            )),
            _ => Err(eyre!(
                "The id prefix {id:?} matches {} meetings; use the full id.",
                matches.len()
            )),
        }
    }

    pub fn list(&self, query: &ListQuery, now_ms: u64) -> Result<String> {
        let scan = self.list_matches(query, now_ms)?;
        let matching = &scan.meetings;
        let limit = query.limit.clamp(1, 200);
        let mut output = String::new();
        if matching.is_empty() {
            let _ = writeln!(output, "No meetings match.");
        } else {
            let _ = writeln!(
                output,
                "{} of {} meetings, newest first:",
                matching.len().min(limit),
                matching.len()
            );
        }
        for meeting in matching.iter().take(limit) {
            let turns = coalesce_transcript(meeting.entries.iter().cloned()).len();
            let _ = writeln!(
                output,
                "- {} · \"{}\" · {} · {} · {} · {} ({turns} turns) · participants: {} · notes: {}",
                meeting.id,
                meeting.title(),
                iso_local(meeting.start_ms()),
                format_duration(meeting.duration_ms(now_ms)),
                meeting.status_label(),
                meeting.source.label(),
                meeting.participants(),
                if meeting.manifest.notes.is_some() {
                    "yes"
                } else {
                    "no"
                }
            );
            for warning in &meeting.warnings {
                let _ = writeln!(output, "  warning: {warning}");
            }
        }
        write_skipped(&mut output, &scan.skipped);
        Ok(output)
    }

    /// Shared selection for CLI JSON and MCP text; limit is applied by the renderer.
    pub fn list_matches(&self, query: &ListQuery, now_ms: u64) -> Result<Scan> {
        let mut scan = self.scan()?;
        let range = TimeRange::parse(query.since.as_deref(), query.until.as_deref(), now_ms)?;
        let words = query
            .query
            .as_deref()
            .map(|query| query.split_whitespace().map(fold).collect::<Vec<_>>())
            .unwrap_or_default();
        scan.meetings = scan
            .meetings
            .into_iter()
            .filter(|meeting| query.include_in_progress || !meeting.in_progress)
            .filter(|meeting| range.overlaps(meeting, now_ms))
            .filter(|meeting| {
                if words.is_empty() {
                    return true;
                }
                let haystack = fold(&format!(
                    "{} {} {} {}",
                    meeting.title(),
                    meeting.manifest.title,
                    meeting.manifest.app.as_deref().unwrap_or_default(),
                    meeting.id
                ));
                words.iter().all(|word| haystack.contains(word.as_str()))
            })
            .collect::<Vec<_>>();
        Ok(scan)
    }

    pub fn search(&self, query: &SearchQuery, now_ms: u64) -> Result<String> {
        let parsed = ParsedQuery::parse(&query.query);
        if parsed.is_empty() {
            return Err(eyre!("The search query has no words."));
        }
        let scan = self.scan()?;
        let range = TimeRange::parse(query.since.as_deref(), query.until.as_deref(), now_ms)?;
        let scope = match &query.meeting_id {
            Some(id) => vec![self.meeting(id)?],
            None => scan
                .meetings
                .iter()
                .filter(|meeting| query.include_in_progress || !meeting.in_progress)
                .filter(|meeting| range.overlaps(meeting, now_ms))
                .cloned()
                .collect(),
        };
        let hits = rank(&scope, &parsed, query.fuzzy, &query.vocabulary);
        Ok(render_search(query, &parsed, &scope, &hits, &scan.skipped))
    }

    pub fn transcript(&self, query: &TranscriptQuery, now_ms: u64) -> Result<String> {
        let meeting = self.meeting(&query.meeting_id)?;
        let range = OffsetRange::parse(query.start.as_deref(), query.end.as_deref())?;
        range.check(&meeting, now_ms)?;
        let speaker = query.speaker.as_deref().map(parse_speaker).transpose()?;
        let cursor = query.cursor.as_deref().map(Cursor::parse).transpose()?;
        Ok(render_transcript(
            &meeting,
            TranscriptWindow {
                range,
                speaker: speaker.flatten(),
                cursor,
                max_chars: query.max_chars.clamp(500, 200_000),
            },
            now_ms,
        ))
    }

    pub fn notes(&self, id: &str) -> Result<String> {
        let meeting = self.meeting(id)?;
        let mut output = String::new();
        let _ = writeln!(output, "{}", meeting.heading());
        let Some(notes) = &meeting.manifest.notes else {
            if meeting.in_progress {
                let _ = writeln!(
                    output,
                    "No notes yet: the meeting is still {}. Hex writes a title and summary after it is transcribed.",
                    status_name(meeting.status)
                );
            } else {
                let _ = writeln!(
                    output,
                    "No Apple Intelligence notes are stored for this meeting."
                );
                if let Some(error) = &meeting.manifest.notes_error {
                    let _ = writeln!(output, "Last notes attempt failed: {error}");
                }
                let _ = writeln!(
                    output,
                    "This read-only server never generates notes. Use get_transcript to read the meeting."
                );
            }
            return Ok(output);
        };
        let _ = writeln!(
            output,
            "Apple Intelligence title and summary, generated {}{}",
            iso_local(notes.generated_at_ms),
            if notes.excerpted() {
                " from an excerpt of the transcript"
            } else {
                ""
            }
        );
        if !notes.title.trim().is_empty() {
            let _ = writeln!(output, "Title: {}", notes.title.trim());
        }
        let summary = notes.summary.trim();
        let _ = writeln!(
            output,
            "Summary: {}",
            if summary.is_empty() {
                "(none)"
            } else {
                summary
            }
        );
        let _ = writeln!(
            output,
            "\nThis is only a short model-written overview and may be wrong. Read the transcript \
with get_transcript or search_transcripts for details, decisions, and action items."
        );
        Ok(output)
    }

    /// The meeting as Markdown: metadata, stored notes, and labeled turns,
    /// limited to the entries overlapping `range`.
    pub fn markdown(&self, id: &str, range: OffsetRange, now_ms: u64) -> Result<String> {
        let meeting = self.meeting(id)?;
        let entries = meeting
            .entries_in(range, now_ms)?
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let mut output = markdown_header(&meeting, now_ms);
        if range.is_bounded() {
            let _ = writeln!(output, "- Range: {}", range.describe());
        }
        if let Some(notes) = &meeting.manifest.notes {
            let _ = write!(output, "\n{}\n", notes_markdown(notes));
        }
        let chapters = meeting.chapter_views(now_ms);
        if !chapters.is_empty() {
            output.push_str("\n## Chapters\n\n");
            for view in &chapters {
                let _ = write!(
                    output,
                    "{}. {} · {}",
                    view.number,
                    format_duration(view.chapter.start_ms),
                    view.chapter.title
                );
                if !view.chapter.summary.is_empty() {
                    let _ = write!(output, " — {}", view.chapter.summary);
                }
                output.push('\n');
            }
        }
        let transcript = if entries.is_empty() && range.is_bounded() {
            "No transcript entries in this range.".into()
        } else {
            transcript_markdown(&entries)
        };
        let _ = write!(output, "\n## Transcript\n\n{transcript}\n");
        Ok(output)
    }

    /// One chapter as Markdown: the meeting's metadata, then the chapter's
    /// name, line, and turns.
    pub fn chapter_markdown(&self, id: &str, number: usize, now_ms: u64) -> Result<String> {
        let meeting = self.meeting(id)?;
        let view = meeting.chapter(number, now_ms)?;
        let mut output = markdown_header(&meeting, now_ms);
        let _ = writeln!(
            output,
            "- Chapter: {} of {} · {}",
            view.number,
            meeting.manifest.chapters.len(),
            view.span()
        );
        let _ = write!(output, "\n{}\n", view.markdown());
        Ok(output)
    }

    /// A meeting's chapters with their spans and lines.
    pub fn chapters(&self, id: &str, now_ms: u64) -> Result<String> {
        let meeting = self.meeting(id)?;
        let mut output = String::new();
        let _ = writeln!(output, "{}", meeting.heading());
        let chapters = meeting.chapter_views(now_ms);
        if chapters.is_empty() {
            let _ = writeln!(
                output,
                "{}",
                if meeting.in_progress {
                    "No chapters yet. Chapters appear every few minutes while a meeting records \
with live chapters on."
                } else {
                    "No chapters are stored for this meeting. This read-only reader never \
generates them; read the meeting with get_transcript or `hex meetings show`."
                }
            );
            return Ok(output);
        }
        let _ = writeln!(
            output,
            "{} chapters, {}:",
            chapters.len(),
            if meeting.manifest.chapters_refined {
                "boundaries corrected over the whole transcript"
            } else if meeting.in_progress {
                "named live while recording"
            } else {
                "named live while recording; boundaries not corrected"
            }
        );
        for view in &chapters {
            let _ = writeln!(
                output,
                "{}. {} · {}",
                view.number,
                view.span(),
                view.chapter.title
            );
            if !view.chapter.summary.is_empty() {
                let _ = writeln!(output, "   {}", view.chapter.summary);
            }
        }
        let _ = writeln!(
            output,
            "\nModel-written names that may be wrong. Read a chapter with get_transcript (start \
and end as shown) or `hex meetings show {} --chapter N`.",
            meeting.id
        );
        Ok(output)
    }
}

fn markdown_header(meeting: &Meeting, now_ms: u64) -> String {
    let mut output = format!("# {}\n\n", meeting.title());
    let _ = writeln!(output, "- Meeting: {}", meeting.id);
    let _ = writeln!(output, "- Started: {}", iso_local(meeting.start_ms()));
    let _ = writeln!(
        output,
        "- Duration: {}",
        format_duration(meeting.duration_ms(now_ms))
    );
    if let Some(app) = &meeting.manifest.app {
        let _ = writeln!(output, "- App: {app}");
    }
    let _ = writeln!(
        output,
        "- Status: {} · {}",
        meeting.status_label(),
        meeting.source.label()
    );
    output
}

fn write_skipped(output: &mut String, skipped: &[Skipped]) {
    if skipped.is_empty() {
        return;
    }
    let _ = writeln!(output, "Skipped {} unreadable folders:", skipped.len());
    for folder in skipped {
        let _ = writeln!(output, "- {}: {}", folder.id, folder.reason);
    }
}

fn fingerprint(directory: &Path) -> Fingerprint {
    Fingerprint {
        files: FINGERPRINT_FILES
            .iter()
            .map(|name| {
                fs::metadata(directory.join(name))
                    .ok()
                    .map(|metadata| (metadata.len(), metadata.modified().ok()))
            })
            .collect(),
        in_progress: recorder_holds_lock(directory),
    }
}

/// Whether a recorder holds `active.lock`. Opens it read-only and tries a
/// shared lock, so a stale lock file left by a finished recording reads as
/// not in progress and nothing is created or written.
fn recorder_holds_lock(directory: &Path) -> bool {
    let Ok(file) = File::open(directory.join("active.lock")) else {
        return false;
    };
    match fs2::FileExt::try_lock_shared(&file) {
        Ok(()) => {
            let _ = fs2::FileExt::unlock(&file);
            false
        }
        Err(error) => error.kind() == std::io::ErrorKind::WouldBlock,
    }
}

fn load(directory: &Path, id: &str, in_progress: bool) -> Result<Meeting> {
    let manifest_path = directory.join("manifest.json");
    let data = fs::read(&manifest_path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => eyre!("no manifest.json"),
        _ => eyre!("could not read manifest.json: {error}"),
    })?;
    let mut manifest: MeetingManifest = serde_json::from_slice(&data)
        .map_err(|error| eyre!("could not decode manifest.json: {error}"))?;
    manifest.live_transcription_error =
        fs::read_to_string(directory.join(super::transcript::ERROR_FILE))
            .ok()
            .map(|error| error.trim().to_string());
    let final_path = directory.join("transcript.ndjson");
    // A recorder publishes the final NDJSON before the Markdown; without one,
    // the NDJSON alone is the finished publication.
    let final_ready = if in_progress {
        final_path.exists() && directory.join("transcript.md").exists()
    } else {
        final_path.exists()
    };
    let status = if in_progress {
        manifest.status
    } else {
        recovered_status(manifest.status, false, final_ready)
    };
    let mut warnings = Vec::new();
    let (source, mut entries) = if final_ready {
        (
            TranscriptSource::Final,
            read_lines::<TranscriptEntry>(&final_path, &mut warnings),
        )
    } else {
        let rolling: RollingView = read_lines::<RollingLine>(
            &directory.join(super::transcript::ROLLING_FILE),
            &mut warnings,
        )
        .into_iter()
        .collect();
        let source = if rolling.entries.is_empty() {
            TranscriptSource::Missing
        } else {
            TranscriptSource::Rolling
        };
        (source, rolling.entries)
    };
    entries.retain(|entry| !entry.text.trim().is_empty());
    entries.sort_by_key(|entry| entry.start_ms);
    let passages = passages(&manifest, &entries);
    Ok(Meeting {
        id: id.into(),
        manifest,
        status,
        in_progress,
        source,
        entries,
        warnings,
        passages,
    })
}

/// Complete NDJSON lines; an unterminated last line is a write in progress
/// and is ignored, while other unreadable lines are skipped and counted.
fn read_lines<T: DeserializeOwned>(path: &Path, warnings: &mut Vec<String>) -> Vec<T> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            warnings.push(format!("could not read {}: {error}", file_name(path)));
            return Vec::new();
        }
    };
    let complete = contents
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(&[][..], |newline| &contents[..newline]);
    let mut skipped = 0;
    let values = complete
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
        .filter_map(|line| match serde_json::from_slice(line) {
            Ok(value) => Some(value),
            Err(_) => {
                skipped += 1;
                None
            }
        })
        .collect();
    if skipped > 0 {
        warnings.push(format!(
            "skipped {skipped} unreadable lines in {}",
            file_name(path)
        ));
    }
    values
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn passages(manifest: &MeetingManifest, entries: &[TranscriptEntry]) -> Vec<Passage> {
    let mut groups: Vec<(MeetingSource, u64, u64, usize, String)> = Vec::new();
    for entry in entries {
        let text = entry.text.trim();
        let words = text.split_whitespace().count();
        if let Some((source, _, end, count, joined)) = groups.last_mut()
            && *source == entry.source
            && entry.start_ms.saturating_sub(*end) <= TURN_GAP_MS
            && *count + words <= PASSAGE_WORDS
        {
            joined.push(' ');
            joined.push_str(text);
            *end = (*end).max(entry.end_ms);
            *count += words;
        } else {
            groups.push((
                entry.source,
                entry.start_ms,
                entry.end_ms,
                words,
                text.into(),
            ));
        }
    }
    let mut passages = groups
        .into_iter()
        .map(|(source, start, _, _, text)| Passage::new(PassageKind::Speech(source), start, text))
        .collect::<Vec<_>>();
    if let Some(notes) = &manifest.notes {
        let text = format!("{}. {}", notes.title.trim(), notes.summary.trim());
        passages.push(Passage::new(PassageKind::Notes, 0, text));
    }
    passages
}

// MARK: Text

/// Lowercase without diacritics: `Café` -> `cafe`.
pub fn fold(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for character in text.chars() {
        for lower in character.to_lowercase() {
            folded.extend(
                lower
                    .nfkd()
                    .filter(|character| !is_combining_mark(*character)),
            );
        }
    }
    folded
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut word = String::new();
    let mut finish = |start: &mut Option<usize>, word: &mut String, end: usize| {
        if let Some(begin) = start.take() {
            let folded = fold(word);
            if !folded.is_empty() {
                tokens.push(Token {
                    stem: stem(&folded),
                    start: begin,
                    end,
                });
            }
            word.clear();
        }
    };
    let mut last_end = 0;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
            word.push(character);
            last_end = index + character.len_utf8();
        } else if matches!(character, '\'' | '’') && start.is_some() {
            // `don't` and `Dax's` stay one word.
        } else {
            finish(&mut start, &mut word, last_end);
        }
    }
    finish(&mut start, &mut word, last_end);
    tokens
}

/// A light English stemmer: `scheduled`, `schedules`, and `scheduling` all
/// become `schedul`; `recurring` and `recur` become `recur`.
fn stem(word: &str) -> String {
    let mut stem = word.to_string();
    if stem.chars().count() <= 3 || stem.bytes().any(|byte| byte.is_ascii_digit()) {
        return stem;
    }
    for (suffix, minimum, replacement) in [
        ("ingly", 3, ""),
        ("edly", 3, ""),
        ("ings", 3, ""),
        ("ing", 3, ""),
        ("ied", 2, "y"),
        ("ies", 2, "y"),
        ("ed", 3, ""),
        ("es", 3, ""),
        ("ly", 3, ""),
        ("s", 3, ""),
    ] {
        if stem.ends_with(suffix)
            && stem.len() - suffix.len() >= minimum
            && !(suffix == "s" && (stem.ends_with("ss") || stem.ends_with("us")))
        {
            stem.truncate(stem.len() - suffix.len());
            stem.push_str(replacement);
            break;
        }
    }
    let bytes = stem.as_bytes();
    if bytes.len() >= 4 {
        let (last, previous) = (bytes[bytes.len() - 1], bytes[bytes.len() - 2]);
        if last == previous && last.is_ascii_lowercase() && !b"aeioulsz".contains(&last) {
            stem.pop();
        }
    }
    if stem.len() >= 4 && stem.ends_with('e') {
        stem.pop();
    }
    stem
}

const STOPWORDS: &[&str] = &[
    "a", "about", "an", "and", "are", "as", "at", "be", "but", "by", "did", "do", "for", "from",
    "had", "has", "have", "he", "i", "in", "is", "it", "its", "of", "on", "or", "our", "she", "so",
    "that", "the", "their", "them", "there", "they", "this", "to", "was", "we", "were", "what",
    "when", "where", "which", "who", "with", "you",
];

fn soundex(word: &str) -> Option<String> {
    let mut key = sound_key(word)?;
    key.truncate(4);
    while key.len() < 4 {
        key.push('0');
    }
    Some(key)
}

/// Soundex without its four-character cap or padding: the first letter, then
/// every consonant sound (`abattoir` and `avatar` are both `a136`).
fn sound_key(word: &str) -> Option<String> {
    let mut characters = word.chars().filter(char::is_ascii_alphabetic);
    let first = characters.next()?;
    let code = |character: char| match character {
        'b' | 'f' | 'p' | 'v' => Some('1'),
        'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => Some('2'),
        'd' | 't' => Some('3'),
        'l' => Some('4'),
        'm' | 'n' => Some('5'),
        'r' => Some('6'),
        _ => None,
    };
    let mut key = String::from(first);
    let mut previous = code(first);
    for character in characters {
        let current = code(character);
        if let Some(digit) = current
            && current != previous
        {
            key.push(digit);
        }
        if !matches!(character, 'h' | 'w') {
            previous = current;
        }
    }
    Some(key)
}

/// Optimal string alignment distance, or `None` when above `limit`.
fn edit_distance(left: &str, right: &str, limit: usize) -> Option<usize> {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    if left.len().abs_diff(right.len()) > limit {
        return None;
    }
    let width = right.len() + 1;
    let mut rows = vec![vec![0; width]; left.len() + 1];
    for (index, row) in rows.iter_mut().enumerate() {
        row[0] = index;
    }
    for (index, cell) in rows[0].iter_mut().enumerate() {
        *cell = index;
    }
    for i in 1..=left.len() {
        for j in 1..=right.len() {
            let cost = usize::from(left[i - 1] != right[j - 1]);
            let mut value = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && left[i - 1] == right[j - 2] && left[i - 2] == right[j - 1] {
                value = value.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = value;
        }
    }
    let distance = rows[left.len()][right.len()];
    (distance <= limit).then_some(distance)
}

// MARK: Search

#[derive(Clone, Debug)]
pub struct ListQuery {
    pub since: Option<String>,
    pub until: Option<String>,
    pub query: Option<String>,
    pub limit: usize,
    pub include_in_progress: bool,
}

#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub query: String,
    pub since: Option<String>,
    pub until: Option<String>,
    pub meeting_id: Option<String>,
    pub limit: usize,
    pub fuzzy: bool,
    pub include_in_progress: bool,
    /// Fuzzy search also finds these terms' likely mis-transcriptions.
    pub vocabulary: Vec<VocabularyEntry>,
}

#[derive(Debug, Default)]
struct ParsedQuery {
    terms: Vec<String>,
    phrases: Vec<Vec<String>>,
}

impl ParsedQuery {
    /// Unquoted words are ranked terms; `"quoted words"` are required exact
    /// phrases (stem-insensitive, never fuzzy).
    fn parse(query: &str) -> Self {
        let mut parsed = Self::default();
        let mut words = Vec::new();
        for (index, part) in query.split('"').enumerate() {
            let stems = tokenize(part)
                .into_iter()
                .map(|token| token.stem)
                .collect::<Vec<_>>();
            if index % 2 == 1 {
                if !stems.is_empty() {
                    parsed.phrases.push(stems);
                }
            } else {
                words.extend(stems);
            }
        }
        let content = words
            .iter()
            .filter(|word| !STOPWORDS.contains(&word.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        parsed.terms = if content.is_empty() && parsed.phrases.is_empty() {
            words
        } else {
            content
        };
        let mut seen = HashSet::new();
        parsed.terms.retain(|term| seen.insert(term.clone()));
        parsed
    }

    fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.phrases.is_empty()
    }

    fn describe(&self) -> String {
        let mut parts = self.terms.clone();
        parts.extend(
            self.phrases
                .iter()
                .map(|phrase| format!("\"{}\"", phrase.join(" "))),
        );
        parts.join(" ")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MatchKind {
    Exact,
    Prefix,
    /// A spelling variant this many edits away.
    Edit(usize),
    Sound,
    /// A vocabulary term's "sounds like" spelling, or the term as two words.
    Alias,
    /// A word that sounds like a vocabulary term ([`sounds_alike`]).
    Phonetic,
}

impl MatchKind {
    fn weight(self) -> f32 {
        match self {
            Self::Exact => 1.0,
            Self::Prefix | Self::Alias => 0.5,
            Self::Edit(_) => 0.35,
            Self::Phonetic => 0.3,
            Self::Sound => 0.2,
        }
    }
}

/// Spelling fallbacks (edit distance and Soundex) apply only to a term found
/// exactly in fewer passages than this: with more, the transcripts already
/// spell the word that way and its near neighbours are other words.
const RARE_EXACT_PASSAGES: u32 = 3;
/// A spelling fallback is skipped when it is a common word: in more than this
/// share of the passages searched, and at least `COMMON_MIN_PASSAGES`.
const COMMON_SHARE: f32 = 0.005;
const COMMON_MIN_PASSAGES: f32 = 5.0;

/// Whether an expansion found by [`expand`] is used. Prefixes are kept like
/// stems. Spelling fallbacks need a term whose exact hits are rare and a
/// candidate that is not a common word, except that a term with no exact hit
/// at all still reaches a common word one edit away (a plain typo). A
/// vocabulary alias the person wrote is always used; a phonetic one only when
/// it is not a common word, however often the term itself appears.
fn admits(kind: MatchKind, exact_passages: u32, passages: u32, common: f32) -> bool {
    match kind {
        MatchKind::Exact | MatchKind::Prefix | MatchKind::Alias => true,
        MatchKind::Phonetic => passages as f32 <= common,
        MatchKind::Edit(_) | MatchKind::Sound => {
            exact_passages < RARE_EXACT_PASSAGES
                && (passages as f32 <= common
                    || (kind == MatchKind::Edit(1) && exact_passages == 0))
        }
    }
}

fn expand<'a>(
    term: &str,
    vocabulary: impl Iterator<Item = &'a String>,
    fuzzy: bool,
) -> Vec<(String, MatchKind)> {
    let term_sound = soundex(term);
    let length = term.chars().count();
    let mut expansions = Vec::new();
    for candidate in vocabulary {
        let kind = if candidate == term {
            Some(MatchKind::Exact)
        } else if !fuzzy {
            None
        } else {
            let candidate_length = candidate.chars().count();
            if (length >= 4 && candidate.starts_with(term) && candidate_length <= length + 5)
                || (candidate_length >= 5
                    && term.starts_with(candidate.as_str())
                    && length - candidate_length <= 3)
            {
                Some(MatchKind::Prefix)
            } else if let Some(distance) = (length >= 4)
                .then(|| edit_distance(term, candidate, if length >= 8 { 2 } else { 1 }))
                .flatten()
            {
                Some(MatchKind::Edit(distance))
            } else if (3..=5).contains(&length)
                && (3..=6).contains(&candidate_length)
                && length.abs_diff(candidate_length) <= 2
                && term_sound.is_some()
                && soundex(candidate) == term_sound
            {
                Some(MatchKind::Sound)
            } else {
                None
            }
        };
        if let Some(kind) = kind {
            expansions.push((candidate.clone(), kind));
        }
    }
    expansions.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    expansions
}

/// Transcript stems, one word or an adjacent phrase, that stand for a
/// vocabulary term.
struct Alias {
    stems: Vec<String>,
    kind: MatchKind,
}

/// The aliases a query `term` gets when it names a one-word vocabulary term:
/// the term's "sounds like" spellings, the term split into two adjacent words
/// found in `corpus` (`open code` for OpenCode), and words in `corpus` that
/// sound like it ([`sounds_alike`]) and pass [`admits`]. Multi-word terms get
/// none; their words are searched like any other.
fn vocabulary_aliases(
    term: &str,
    vocabulary: &[VocabularyEntry],
    corpus: &HashMap<&String, u32>,
    common: f32,
) -> Vec<Alias> {
    let mut aliases: Vec<Alias> = Vec::new();
    let mut add = |stems: Vec<String>, kind: MatchKind| {
        if stems.is_empty() || stems == [term] {
            return;
        }
        match aliases.iter_mut().find(|alias| alias.stems == stems) {
            Some(alias) if alias.kind.weight() < kind.weight() => alias.kind = kind,
            Some(_) => {}
            None => aliases.push(Alias { stems, kind }),
        }
    };
    for entry in vocabulary {
        let tokens = tokenize(&entry.term);
        let [token] = tokens.as_slice() else {
            continue;
        };
        if token.stem != term {
            continue;
        }
        for spelling in &entry.sounds_like {
            add(
                tokenize(spelling)
                    .into_iter()
                    .map(|token| token.stem)
                    .collect(),
                MatchKind::Alias,
            );
        }
        let word = fold(&entry.term[token.start..token.end])
            .chars()
            .filter(|character| character.is_alphanumeric())
            .collect::<String>();
        let boundaries = word
            .char_indices()
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for &split in boundaries.iter().skip(3) {
            if word[split..].chars().count() < 3 {
                break;
            }
            let (left, right) = (stem(&word[..split]), stem(&word[split..]));
            if corpus.contains_key(&left) && corpus.contains_key(&right) {
                add(vec![left, right], MatchKind::Alias);
            }
        }
        for (candidate, passages) in corpus {
            if sounds_alike(term, candidate) && admits(MatchKind::Phonetic, 0, *passages, common) {
                add(vec![(*candidate).clone()], MatchKind::Phonetic);
            }
        }
    }
    aliases.sort_unstable_by(|left, right| left.stems.cmp(&right.stems));
    aliases
}

/// Consonant sounds a vocabulary term needs before words that merely sound
/// like it count as hits. Names with fewer (Rune, Dax) match too many words
/// this way and rely on "sounds like" spellings.
const SOUND_ALIKE_MIN_CONSONANTS: usize = 2;

/// Whether the corpus stem `candidate` is a likely mis-transcription of the
/// vocabulary stem `term` (`avatar` for `abattoir`): the same uncapped
/// Soundex key with at least [`SOUND_ALIKE_MIN_CONSONANTS`] consonant sounds,
/// at most half the letters changed, and not a stopword.
fn sounds_alike(term: &str, candidate: &str) -> bool {
    let length = term.chars().count().max(candidate.chars().count());
    candidate != term
        && candidate.chars().count() >= 4
        && !STOPWORDS.contains(&candidate)
        // The key is the first letter, then one digit per consonant sound.
        && sound_key(term).is_some_and(|key| {
            key.len() > SOUND_ALIKE_MIN_CONSONANTS && sound_key(candidate) == Some(key)
        })
        && edit_distance(term, candidate, length / 2).is_some()
}

struct Hit {
    meeting: Arc<Meeting>,
    passage: usize,
    score: f32,
    matched: Vec<usize>,
    labels: Vec<String>,
}

fn rank(
    meetings: &[Arc<Meeting>],
    query: &ParsedQuery,
    fuzzy: bool,
    vocabulary: &[VocabularyEntry],
) -> Vec<Hit> {
    let owners = meetings
        .iter()
        .flat_map(|meeting| (0..meeting.passages.len()).map(move |index| (meeting, index)))
        .collect::<Vec<_>>();
    let passages = owners
        .iter()
        .map(|(meeting, index)| &meeting.passages[*index])
        .collect::<Vec<_>>();
    let documents = (0..passages.len()).collect::<Vec<_>>();
    let mut scored = score(&passages, &documents, query, fuzzy, vocabulary);
    scored.sort_by(|left, right| {
        let (left_meeting, left_passage) = owners[left.passage];
        let (right_meeting, right_passage) = owners[right.passage];
        relevance(left, right)
            .then_with(|| right_meeting.start_ms().cmp(&left_meeting.start_ms()))
            .then_with(|| left_passage.cmp(&right_passage))
    });
    scored
        .into_iter()
        .map(|scored| {
            let (meeting, passage) = owners[scored.passage];
            Hit {
                meeting: meeting.clone(),
                passage,
                score: scored.score,
                matched: scored.matched,
                labels: scored.labels,
            }
        })
        .collect()
}

struct Scored {
    passage: usize,
    score: f32,
    /// Every query term matched an exact stem.
    exact: bool,
    matched: Vec<usize>,
    labels: Vec<String>,
}

/// Best first: a passage matching every term exactly outranks one that needs
/// a fuzzy match, whatever their scores; then by score.
fn relevance(left: &Scored, right: &Scored) -> std::cmp::Ordering {
    right
        .exact
        .cmp(&left.exact)
        .then_with(|| right.score.total_cmp(&left.score))
}

/// Which of two matches in a passage stands for a query term: the exact stem
/// whenever present, then the higher score, the more trusted kind, and the
/// alphabetically first stem, so equal matches resolve the same way every run.
fn stands_for_term(
    (score, stem, kind): (f32, &str, MatchKind),
    (other_score, other_stem, other_kind): (f32, &str, MatchKind),
) -> std::cmp::Ordering {
    (kind == MatchKind::Exact)
        .cmp(&(other_kind == MatchKind::Exact))
        .then(score.total_cmp(&other_score))
        .then(kind.weight().total_cmp(&other_kind.weight()))
        .then(other_stem.cmp(stem))
}

/// BM25 over `passages` as one corpus, with stemmed, optionally fuzzy terms.
/// `owners[i]` is passage `i`'s document: alternative texts of one document
/// (contiguous, such as a raw and a corrected transcript) count once in
/// document frequencies. Fuzzy search adds `vocabulary` aliases, which never
/// count as exact. Unsorted; passages that match nothing are omitted.
fn score(
    passages: &[&Passage],
    owners: &[usize],
    query: &ParsedQuery,
    fuzzy: bool,
    vocabulary: &[VocabularyEntry],
) -> Vec<Scored> {
    debug_assert_eq!(passages.len(), owners.len());
    // Per stem: documents containing it, and the last document counted.
    let mut counted: HashMap<&String, (u32, usize)> = HashMap::new();
    let mut total_length = 0_usize;
    let mut documents = 0_u32;
    for (index, (passage, owner)) in passages.iter().zip(owners).enumerate() {
        total_length += passage.tokens.len();
        if index == 0 || owners[index - 1] != *owner {
            documents += 1;
        }
        for stem in passage.frequencies.keys() {
            let (count, last) = counted.entry(stem).or_insert((0, usize::MAX));
            if *last != *owner {
                *count += 1;
                *last = *owner;
            }
        }
    }
    if passages.is_empty() {
        return Vec::new();
    }
    let document_frequency = counted
        .into_iter()
        .map(|(stem, (count, _))| (stem, count))
        .collect::<HashMap<_, _>>();
    let average_length = total_length as f32 / passages.len() as f32;
    let idf = |stem: &str| {
        let frequency = document_frequency
            .get(&stem.to_string())
            .copied()
            .unwrap_or(0) as f32;
        (1.0 + (documents as f32 - frequency + 0.5) / (frequency + 0.5)).ln()
    };
    let passages_with = |stem: &String| document_frequency.get(stem).copied().unwrap_or(0);
    let common = (documents as f32 * COMMON_SHARE).max(COMMON_MIN_PASSAGES);
    let expansions = query
        .terms
        .iter()
        .map(|term| {
            let exact_passages = passages_with(term);
            let mut stems = expand(term, document_frequency.keys().copied(), fuzzy)
                .into_iter()
                .filter(|(stem, kind)| admits(*kind, exact_passages, passages_with(stem), common))
                .collect::<Vec<_>>();
            let mut phrases = Vec::new();
            if fuzzy {
                for alias in vocabulary_aliases(term, vocabulary, &document_frequency, common) {
                    let [stem] = alias.stems.as_slice() else {
                        phrases.push(alias);
                        continue;
                    };
                    stems.retain(|(other, kind)| {
                        other != stem || kind.weight() >= alias.kind.weight()
                    });
                    if !stems.iter().any(|(other, _)| other == stem) {
                        stems.push((stem.clone(), alias.kind));
                    }
                }
            }
            let stems = stems
                .into_iter()
                .map(|(stem, kind)| {
                    let weight = kind.weight() * idf(&stem);
                    (stem, kind, weight)
                })
                .collect::<Vec<_>>();
            let phrases = phrases
                .into_iter()
                .map(|alias| {
                    let weight =
                        alias.kind.weight() * alias.stems.iter().map(|stem| idf(stem)).sum::<f32>();
                    let label = alias.stems.join(" ");
                    (alias, label, weight)
                })
                .collect::<Vec<_>>();
            (stems, phrases)
        })
        .collect::<Vec<_>>();
    let mut hits = Vec::new();
    for (index, passage) in passages.iter().enumerate() {
        let length_norm =
            1.0 - BM25_B + BM25_B * passage.tokens.len() as f32 / average_length.max(1.0);
        let saturate = |frequency: u32| {
            let frequency = frequency as f32;
            frequency * (BM25_K1 + 1.0) / (frequency + BM25_K1 * length_norm)
        };
        let mut matched_stems: HashSet<&str> = HashSet::new();
        let mut matched_positions = Vec::new();
        let mut labels = Vec::new();
        let mut phrase_score = 0.0;
        let mut missing_phrase = false;
        for phrase in &query.phrases {
            let starts = phrase_starts(passage, phrase);
            if starts.is_empty() {
                missing_phrase = true;
                break;
            }
            let phrase_idf = phrase.iter().map(|stem| idf(stem)).sum::<f32>();
            phrase_score += 1.5 * phrase_idf * saturate(starts.len() as u32);
            for start in starts {
                matched_positions.extend(start..start + phrase.len());
            }
            labels.push(format!("\"{}\"", phrase.join(" ")));
        }
        if missing_phrase {
            continue;
        }
        let mut term_score = 0.0;
        let mut matched_weight = 0.0;
        let mut exact_terms = Vec::with_capacity(query.terms.len());
        for (term, (candidates, phrases)) in query.terms.iter().zip(&expansions) {
            let mut best: Option<(f32, &str, MatchKind)> = None;
            for (stem, kind, weight) in candidates {
                if let Some(frequency) = passage.frequencies.get(stem) {
                    matched_stems.insert(stem);
                    let candidate = (weight * saturate(*frequency), stem.as_str(), *kind);
                    if best.is_none_or(|best| stands_for_term(candidate, best).is_gt()) {
                        best = Some(candidate);
                    }
                }
            }
            for (alias, label, weight) in phrases {
                let starts = phrase_starts(passage, &alias.stems);
                if starts.is_empty() {
                    continue;
                }
                for start in &starts {
                    matched_positions.extend(*start..*start + alias.stems.len());
                }
                let candidate = (
                    weight * saturate(starts.len() as u32),
                    label.as_str(),
                    alias.kind,
                );
                if best.is_none_or(|best| stands_for_term(candidate, best).is_gt()) {
                    best = Some(candidate);
                }
            }
            exact_terms.push(matches!(best, Some((_, _, MatchKind::Exact))));
            if let Some((score, stem, kind)) = best {
                term_score += score;
                matched_weight += kind.weight();
                labels.push(if kind == MatchKind::Exact {
                    term.clone()
                } else {
                    format!("{stem}~{term}")
                });
            }
        }
        if term_score == 0.0 && phrase_score == 0.0 {
            continue;
        }
        // Fuzzy matches cover a term only as far as they are trusted.
        let coverage = if query.terms.is_empty() {
            1.0
        } else {
            0.5 + 0.5 * matched_weight / query.terms.len() as f32
        };
        let mut adjacency = 0.0;
        for pair in 0..query.terms.len().saturating_sub(1) {
            if !(exact_terms[pair] && exact_terms[pair + 1]) {
                continue;
            }
            let (first, second) = (&query.terms[pair], &query.terms[pair + 1]);
            if passage
                .tokens
                .windows(2)
                .any(|window| window[0].stem == *first && window[1].stem == *second)
            {
                adjacency += 0.5 * (idf(first) + idf(second));
            }
        }
        for (position, token) in passage.tokens.iter().enumerate() {
            if matched_stems.contains(token.stem.as_str()) {
                matched_positions.push(position);
            }
        }
        matched_positions.sort_unstable();
        matched_positions.dedup();
        hits.push(Scored {
            passage: index,
            score: term_score * coverage + phrase_score + adjacency,
            exact: exact_terms.iter().all(|exact| *exact),
            matched: matched_positions,
            labels,
        });
    }
    hits
}

/// One ranked document from [`search_texts`], through its best text.
#[derive(Debug)]
pub struct TextHit {
    /// The document's index.
    pub index: usize,
    pub score: f32,
    /// A window of the best text around the matches, which are wrapped in `**`.
    pub snippet: String,
}

/// Rank documents of plain text with the meeting search's stemmed, fuzzy BM25
/// and quoted-phrase rules. A document's texts are alternatives, such as a raw
/// and a corrected transcript: they count as one document for term statistics,
/// and the best of them is its hit. Best first; ties keep input order.
pub fn search_texts(
    documents: &[Vec<&str>],
    query: &str,
    vocabulary: &[VocabularyEntry],
) -> Result<Vec<TextHit>> {
    let parsed = ParsedQuery::parse(query);
    if parsed.is_empty() {
        return Err(eyre!("The search query has no words."));
    }
    let mut owners = Vec::new();
    let mut passages = Vec::new();
    for (document, texts) in documents.iter().enumerate() {
        for text in texts {
            owners.push(document);
            passages.push(Passage::new(PassageKind::Notes, 0, (*text).to_owned()));
        }
    }
    let texts = passages.iter().collect::<Vec<_>>();
    let mut scored = score(&texts, &owners, &parsed, true, vocabulary);
    scored.sort_by(|left, right| relevance(left, right).then(left.passage.cmp(&right.passage)));
    let mut seen = HashSet::new();
    Ok(scored
        .into_iter()
        .filter(|scored| seen.insert(owners[scored.passage]))
        .map(|scored| TextHit {
            index: owners[scored.passage],
            score: scored.score,
            snippet: snippet(&passages[scored.passage], &scored.matched),
        })
        .collect())
}

fn phrase_starts(passage: &Passage, phrase: &[String]) -> Vec<usize> {
    if phrase.is_empty() || passage.tokens.len() < phrase.len() {
        return Vec::new();
    }
    (0..=passage.tokens.len() - phrase.len())
        .filter(|start| {
            phrase
                .iter()
                .enumerate()
                .all(|(offset, stem)| passage.tokens[start + offset].stem == *stem)
        })
        .collect()
}

fn snippet(passage: &Passage, matched: &[usize]) -> String {
    let tokens = &passage.tokens;
    if tokens.is_empty() {
        return passage.text.clone();
    }
    let window = SNIPPET_TOKENS.min(tokens.len());
    let best_start = (0..=tokens.len() - window)
        .max_by_key(|start| {
            let count = matched
                .iter()
                .filter(|&&position| (*start..*start + window).contains(&position))
                .count();
            // Prefer windows that lead into the first match a little.
            (
                count,
                std::cmp::Reverse(
                    matched
                        .first()
                        .map_or(0, |first| first.saturating_sub(4).abs_diff(*start)),
                ),
            )
        })
        .unwrap_or(0);
    let end = best_start + window;
    let text_start = if best_start == 0 {
        0
    } else {
        tokens[best_start].start
    };
    let text_end = if end == tokens.len() {
        passage.text.len()
    } else {
        tokens[end - 1].end
    };
    let mut output = String::new();
    if best_start > 0 {
        output.push('…');
    }
    let mut cursor = text_start;
    for (position, token) in tokens.iter().enumerate().take(end).skip(best_start) {
        if !matched.contains(&position) {
            continue;
        }
        output.push_str(&passage.text[cursor..token.start]);
        output.push_str("**");
        output.push_str(&passage.text[token.start..token.end]);
        output.push_str("**");
        cursor = token.end;
    }
    output.push_str(&passage.text[cursor..text_end]);
    if end < tokens.len() {
        output.push('…');
    }
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_search(
    query: &SearchQuery,
    parsed: &ParsedQuery,
    scope: &[Arc<Meeting>],
    hits: &[Hit],
    skipped: &[Skipped],
) -> String {
    let mut output = String::new();
    let limit = query.limit.clamp(1, 50);
    let _ = writeln!(
        output,
        "{} matching passages for {} ({}) in {} meetings{}.",
        hits.len(),
        parsed.describe(),
        if query.fuzzy {
            "stemmed, fuzzy; quote words for exact phrases"
        } else {
            "stemmed, exact"
        },
        scope.len(),
        if hits.len() > limit {
            format!("; showing the top {limit}")
        } else {
            String::new()
        }
    );
    if hits.is_empty() {
        let _ = writeln!(
            output,
            "Try fewer or different words, fuzzy=true, or a wider since/until."
        );
        write_skipped(&mut output, skipped);
        return output;
    }
    let mut by_meeting: Vec<(&Arc<Meeting>, usize, f32)> = Vec::new();
    for hit in hits {
        match by_meeting
            .iter_mut()
            .find(|(meeting, _, _)| meeting.id == hit.meeting.id)
        {
            Some((_, count, _)) => *count += 1,
            None => by_meeting.push((&hit.meeting, 1, hit.score)),
        }
    }
    let _ = writeln!(output, "By meeting:");
    for (meeting, count, best) in &by_meeting {
        let _ = writeln!(
            output,
            "- {} · {count} passages · best {best:.1}{}",
            meeting.heading(),
            if meeting.source == TranscriptSource::Final {
                String::new()
            } else {
                format!(" · {} ({})", meeting.status_label(), meeting.source.label())
            }
        );
    }
    let _ = writeln!(output);
    for (rank, hit) in hits.iter().take(limit).enumerate() {
        let meeting = &hit.meeting;
        let passage = &meeting.passages[hit.passage];
        let when = match passage.kind {
            PassageKind::Speech(_) => format!(
                "{} @ {}",
                format_duration(passage.start_ms),
                iso_local(meeting.start_ms() + passage.start_ms)
            ),
            PassageKind::Notes => "stored notes".into(),
        };
        let _ = writeln!(
            output,
            "[{}] {} \"{}\" · {when} · {} · score {:.1} · matched {}",
            rank + 1,
            meeting.id,
            meeting.title(),
            passage.speaker(),
            hit.score,
            hit.labels.join(", ")
        );
        if let PassageKind::Speech(_) = passage.kind
            && let Some(previous) = neighbor(meeting, hit.passage, false)
        {
            let _ = writeln!(
                output,
                "    before ({} {}): {}",
                previous.speaker(),
                format_duration(previous.start_ms),
                tail(&previous.text, CONTEXT_CHARS)
            );
        }
        let _ = writeln!(output, "    hit: {}", snippet(passage, &hit.matched));
        if let PassageKind::Speech(_) = passage.kind
            && let Some(next) = neighbor(meeting, hit.passage, true)
        {
            let _ = writeln!(
                output,
                "    after ({} {}): {}",
                next.speaker(),
                format_duration(next.start_ms),
                head(&next.text, CONTEXT_CHARS)
            );
        }
    }
    let _ = writeln!(
        output,
        "\nRead around a hit with get_transcript(meeting_id, start=\"mm:ss\" a minute before)."
    );
    write_skipped(&mut output, skipped);
    output
}

fn neighbor(meeting: &Meeting, index: usize, forward: bool) -> Option<&Passage> {
    let candidate = if forward {
        meeting.passages.get(index + 1)
    } else {
        index
            .checked_sub(1)
            .and_then(|index| meeting.passages.get(index))
    };
    candidate.filter(|passage| matches!(passage.kind, PassageKind::Speech(_)))
}

fn head(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.into();
    }
    let cut = text
        .char_indices()
        .nth(limit)
        .map_or(text.len(), |(index, _)| index);
    let cut = text[..cut]
        .rfind(' ')
        .filter(|space| *space > 0)
        .unwrap_or(cut);
    format!("{}…", &text[..cut])
}

fn tail(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.into();
    }
    let cut = text
        .char_indices()
        .nth(count - limit)
        .map_or(0, |(index, _)| index);
    let cut = text[cut..].find(' ').map_or(cut, |space| cut + space + 1);
    format!("…{}", &text[cut..])
}

// MARK: Transcript

#[derive(Clone, Debug)]
pub struct TranscriptQuery {
    pub meeting_id: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub speaker: Option<String>,
    pub max_chars: usize,
    pub cursor: Option<String>,
}

struct TranscriptWindow {
    range: OffsetRange,
    speaker: Option<MeetingSource>,
    cursor: Option<Cursor>,
    max_chars: usize,
}

/// The next entry to show: its start and how many entries starting at the
/// same millisecond were already shown.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Cursor {
    start_ms: u64,
    ordinal: usize,
}

impl Cursor {
    fn parse(cursor: &str) -> Result<Self> {
        let invalid =
            || eyre!("Invalid cursor {cursor:?}; pass next_cursor from the previous call.");
        let rest = cursor.trim().strip_prefix('t').ok_or_else(invalid)?;
        let (start, ordinal) = rest.split_once('.').ok_or_else(invalid)?;
        Ok(Self {
            start_ms: start.parse().map_err(|_| invalid())?,
            ordinal: ordinal.parse().map_err(|_| invalid())?,
        })
    }

    fn render(self) -> String {
        format!("t{}.{}", self.start_ms, self.ordinal)
    }
}

fn parse_speaker(speaker: &str) -> Result<Option<MeetingSource>> {
    match fold(speaker.trim()).as_str() {
        "" | "all" | "both" | "any" => Ok(None),
        "you" | "me" | "kit" | "microphone" | "mic" | "local" => {
            Ok(Some(MeetingSource::Microphone))
        }
        "them" | "others" | "other" | "remote" | "system" | "computer" => {
            Ok(Some(MeetingSource::System))
        }
        other => Err(eyre!(
            "Unknown speaker {other:?}; use \"you\", \"them\", or \"all\"."
        )),
    }
}

fn render_transcript(meeting: &Meeting, window: TranscriptWindow, now_ms: u64) -> String {
    let mut ordinals: HashMap<u64, usize> = HashMap::new();
    let selected = meeting
        .entries
        .iter()
        .filter(|entry| window.speaker.is_none_or(|speaker| entry.source == speaker))
        .filter(|entry| window.range.overlaps(entry))
        .map(|entry| {
            let ordinal = ordinals.entry(entry.start_ms).or_insert(0);
            let cursor = Cursor {
                start_ms: entry.start_ms,
                ordinal: *ordinal,
            };
            *ordinal += 1;
            (cursor, entry)
        })
        .filter(|(cursor, _)| window.cursor.is_none_or(|resume| *cursor >= resume))
        .collect::<Vec<_>>();

    let mut output = String::new();
    let _ = writeln!(
        output,
        "{} · {} · {} · {}",
        meeting.heading(),
        format_duration(meeting.duration_ms(now_ms)),
        meeting.status_label(),
        meeting.source.label()
    );
    if meeting.in_progress {
        let _ = writeln!(
            output,
            "Still being recorded: this is a provisional transcript and will change."
        );
    } else if meeting.source != TranscriptSource::Final
        && meeting.source != TranscriptSource::Missing
    {
        let _ = writeln!(
            output,
            "No final transcript was published; this is the unfinished draft."
        );
    }
    for warning in &meeting.warnings {
        let _ = writeln!(output, "warning: {warning}");
    }
    if selected.is_empty() {
        let _ = writeln!(
            output,
            "No transcript entries in this range{}.",
            if meeting.entries.is_empty() {
                " (the meeting has none yet)"
            } else {
                ""
            }
        );
        return output;
    }

    let mut turns: Vec<TranscriptEntry> = Vec::new();
    let mut used = 0;
    let mut next = None;
    for (cursor, entry) in &selected {
        let cost = entry.text.len() + 1;
        if !turns.is_empty() && used + cost > window.max_chars {
            next = Some(*cursor);
            break;
        }
        match turns.last_mut() {
            Some(turn)
                if turn.source == entry.source
                    && entry.start_ms.saturating_sub(turn.end_ms) <= TURN_GAP_MS =>
            {
                turn.text.push(' ');
                turn.text.push_str(entry.text.trim());
                turn.end_ms = turn.end_ms.max(entry.end_ms);
                used += cost;
            }
            _ => {
                let mut turn = (*entry).clone();
                turn.text = turn.text.trim().into();
                used += cost + 26;
                turns.push(turn);
            }
        }
    }
    let shown_end = turns.last().map_or(0, |turn| turn.end_ms);
    let _ = writeln!(
        output,
        "Showing {}–{}{}:",
        format_duration(turns.first().map_or(0, |turn| turn.start_ms)),
        format_duration(shown_end),
        match window.speaker {
            Some(source) => format!(" ({} only)", source.label()),
            None => String::new(),
        }
    );
    for turn in &turns {
        let _ = writeln!(
            output,
            "[{} · {}] {}: {}",
            format_duration(turn.start_ms),
            clock_seconds(meeting.start_ms() + turn.start_ms),
            turn.source.label(),
            turn.text
        );
    }
    match next {
        Some(cursor) => {
            let _ = writeln!(
                output,
                "next_cursor: {} (more follows; call get_transcript again with this cursor and the same filters)",
                cursor.render()
            );
        }
        None => {
            let _ = writeln!(output, "(end of range)");
        }
    }
    output
}

// MARK: Time

#[cfg(unix)]
use libc::mktime;

// The Universal CRT exports `mktime` only as its 64-bit form.
#[cfg(windows)]
unsafe extern "C" {
    #[link_name = "_mktime64"]
    fn mktime(tm: *mut libc::tm) -> libc::time_t;
}

fn local_tm(milliseconds: u64) -> libc::tm {
    let seconds = (milliseconds / 1_000) as libc::time_t;
    // SAFETY: `localtime_r` and `localtime_s` write only into the provided
    // zeroed `tm`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    #[cfg(unix)]
    unsafe {
        libc::localtime_r(&seconds, &mut tm)
    };
    #[cfg(windows)]
    unsafe {
        libc::localtime_s(&mut tm, &seconds)
    };
    tm
}

/// Local time with its UTC offset, such as `2026-10-15T14:58:31-07:00`.
pub fn iso_local(milliseconds: u64) -> String {
    let tm = local_tm(milliseconds);
    #[cfg(unix)]
    let offset = tm.tm_gmtoff;
    // The local wall clock read as UTC, less the instant itself.
    #[cfg(windows)]
    let offset = {
        let wall = days_from_civil(
            i64::from(tm.tm_year) + 1_900,
            i64::from(tm.tm_mon) + 1,
            i64::from(tm.tm_mday),
        ) * 86_400
            + i64::from(tm.tm_hour) * 3_600
            + i64::from(tm.tm_min) * 60
            + i64::from(tm.tm_sec);
        wall - (milliseconds / 1_000) as i64
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
        tm.tm_year + 1_900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        if offset < 0 { '-' } else { '+' },
        offset.abs() / 3_600,
        offset.abs() % 3_600 / 60
    )
}

fn clock_seconds(milliseconds: u64) -> String {
    let tm = local_tm(milliseconds);
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

fn local_to_ms(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<u64> {
    // SAFETY: `mktime` reads and normalizes only the provided `tm`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year - 1_900;
    tm.tm_mon = month as i32 - 1;
    tm.tm_mday = day as i32;
    tm.tm_hour = hour as i32;
    tm.tm_min = minute as i32;
    tm.tm_sec = second as i32;
    tm.tm_isdst = -1;
    let seconds = unsafe { mktime(&mut tm) };
    u64::try_from(seconds).ok().map(|seconds| seconds * 1_000)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `2026-10-15`, `2026-10-15T14:00`, RFC 3339 with `Z` or an offset,
/// `today`, `yesterday`, `now`, relative `90m`/`36h`/`7d`/`2w`, or epoch ms.
/// A date alone means local midnight; as an upper bound it covers the day.
pub fn parse_time(input: &str, now_ms: u64, end_of_day: bool) -> Result<u64> {
    let text = input.trim().to_ascii_lowercase();
    let invalid = || {
        eyre!(
            "Could not read the time {input:?}; use 2026-10-15, 2026-10-15T14:00, today, yesterday, or 7d."
        )
    };
    let today = local_tm(now_ms);
    let midnight = |days_ago: i32, end: bool| {
        // SAFETY: `mktime` reads and normalizes only the provided `tm`,
        // including out-of-range days across months.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        tm.tm_year = today.tm_year;
        tm.tm_mon = today.tm_mon;
        tm.tm_mday = today.tm_mday - days_ago + i32::from(end);
        tm.tm_isdst = -1;
        u64::try_from(unsafe { mktime(&mut tm) })
            .ok()
            .map(|seconds| seconds * 1_000)
    };
    match text.as_str() {
        "now" => return Ok(now_ms),
        "today" => return midnight(0, end_of_day).ok_or_else(invalid),
        "yesterday" => return midnight(1, end_of_day).ok_or_else(invalid),
        _ => {}
    }
    if text.bytes().all(|byte| byte.is_ascii_digit()) && text.len() >= 10 {
        let value: u64 = text.parse().map_err(|_| invalid())?;
        return Ok(if text.len() <= 10 {
            value * 1_000
        } else {
            value
        });
    }
    let relative = text.strip_suffix(" ago").unwrap_or(&text).trim();
    if let Some(split) = relative.find(|character: char| !character.is_ascii_digit())
        && split > 0
    {
        let (amount, unit) = relative.split_at(split);
        let unit_ms = match unit.trim() {
            "m" | "min" | "mins" | "minutes" => Some(60_000),
            "h" | "hr" | "hrs" | "hours" => Some(3_600_000),
            "d" | "day" | "days" => Some(86_400_000),
            "w" | "week" | "weeks" => Some(7 * 86_400_000),
            _ => None,
        };
        if let Some(unit_ms) = unit_ms {
            let amount: u64 = amount.parse().map_err(|_| invalid())?;
            return Ok(now_ms.saturating_sub(amount.checked_mul(unit_ms).ok_or_else(invalid)?));
        }
    }
    let pattern = regex::Regex::new(
        r"^(\d{4})-(\d{2})-(\d{2})(?:[t ](\d{2}):(\d{2})(?::(\d{2})(?:\.(?P<fraction>\d+))?)?)?\s*(?P<zone>z|[+-]\d{2}:?\d{2})?$",
    )
    .expect("valid time pattern");
    let captures = pattern.captures(&text).ok_or_else(invalid)?;
    let number = |index: usize| {
        captures
            .get(index)
            .map_or(Ok(0), |value| value.as_str().parse::<u32>())
            .map_err(|_| invalid())
    };
    let (year, month, day) = (number(1)?, number(2)?, number(3)?);
    let has_time = captures.get(4).is_some();
    let (hour, minute, second) = (number(4)?, number(5)?, number(6)?);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    if day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return Err(invalid());
    }
    let day = if !has_time && end_of_day {
        day + 1
    } else {
        day
    };
    let milliseconds = match captures.name("zone").map(|zone| zone.as_str()) {
        Some(zone) => {
            let offset_seconds = if zone == "z" {
                0
            } else {
                let sign = if zone.starts_with('-') { -1 } else { 1 };
                let digits = zone[1..].replace(':', "");
                let hours: i64 = digits[..2].parse().map_err(|_| invalid())?;
                let minutes: i64 = digits[2..].parse().map_err(|_| invalid())?;
                if hours > 23 || minutes > 59 {
                    return Err(invalid());
                }
                sign * (hours * 3_600 + minutes * 60)
            };
            let seconds = days_from_civil(year.into(), month.into(), day.into()) * 86_400
                + i64::from(hour) * 3_600
                + i64::from(minute) * 60
                + i64::from(second)
                - offset_seconds;
            u64::try_from(seconds)
                .map(|seconds| seconds * 1_000)
                .map_err(|_| invalid())
        }
        None => local_to_ms(year as i32, month, day, hour, minute, second).ok_or_else(invalid),
    }?;
    let fraction = captures.name("fraction").map_or(0, |digits| {
        digits
            .as_str()
            .bytes()
            .take(3)
            .zip([100, 10, 1])
            .map(|(digit, scale)| u64::from(digit - b'0') * scale)
            .sum::<u64>()
    });
    Ok(milliseconds + fraction)
}

pub struct TimeRange {
    since: Option<u64>,
    until: Option<u64>,
}

impl TimeRange {
    pub fn parse(since: Option<&str>, until: Option<&str>, now_ms: u64) -> Result<Self> {
        let since = since
            .filter(|value| !value.trim().is_empty())
            .map(|value| parse_time(value, now_ms, false))
            .transpose()?;
        let until = until
            .filter(|value| !value.trim().is_empty())
            .map(|value| parse_time(value, now_ms, true))
            .transpose()?;
        if since
            .zip(until)
            .is_some_and(|(since, until)| since >= until)
        {
            return Err(eyre!("since must be earlier than until"));
        }
        Ok(Self { since, until })
    }

    pub fn contains(&self, timestamp_ms: u64) -> bool {
        self.since.is_none_or(|since| timestamp_ms >= since)
            && self.until.is_none_or(|until| timestamp_ms < until)
    }

    /// Whether the meeting overlaps `[since, until)`.
    fn overlaps(&self, meeting: &Meeting, now_ms: u64) -> bool {
        let start = meeting.start_ms();
        let end = start + meeting.duration_ms(now_ms);
        self.since.is_none_or(|since| end >= since) && self.until.is_none_or(|until| start < until)
    }
}

/// A window of offsets into a meeting. Entries overlapping it are kept whole.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OffsetRange {
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
}

impl OffsetRange {
    /// Offsets as [`parse_offset`] reads them; a blank bound is open.
    pub fn parse(from: Option<&str>, to: Option<&str>) -> Result<Self> {
        let offset = |value: Option<&str>| {
            value
                .filter(|value| !value.trim().is_empty())
                .map(parse_offset)
                .transpose()
        };
        let range = Self {
            from_ms: offset(from)?,
            to_ms: offset(to)?,
        };
        let from = range.from_ms.unwrap_or_default();
        if let Some(to) = range.to_ms
            && from >= to
        {
            return Err(eyre!(
                "The range start {} must be earlier than its end {}.",
                format_duration(from),
                format_duration(to)
            ));
        }
        Ok(range)
    }

    pub fn is_bounded(self) -> bool {
        self.from_ms.is_some() || self.to_ms.is_some()
    }

    fn overlaps(self, entry: &TranscriptEntry) -> bool {
        self.from_ms.is_none_or(|from| entry.end_ms > from)
            && self.to_ms.is_none_or(|to| entry.start_ms < to)
    }

    /// A range that starts at or after the meeting's end selects nothing and
    /// is an error rather than an empty answer.
    fn check(self, meeting: &Meeting, now_ms: u64) -> Result<()> {
        let Some(from) = self.from_ms.filter(|from| *from > 0) else {
            return Ok(());
        };
        let end = meeting.duration_ms(now_ms).max(
            meeting
                .entries
                .iter()
                .map(|entry| entry.end_ms)
                .max()
                .unwrap_or_default(),
        );
        if from >= end {
            return Err(eyre!(
                "The range starts at {}, after the meeting's end at {}{}.",
                format_duration(from),
                format_duration(end),
                if meeting.in_progress { " so far" } else { "" }
            ));
        }
        Ok(())
    }

    fn describe(self) -> String {
        format!(
            "{}–{}",
            format_duration(self.from_ms.unwrap_or_default()),
            self.to_ms.map_or_else(|| "end".into(), format_duration)
        )
    }
}

/// `mm:ss`, `h:mm:ss`, plain seconds (`90`, `90.5`), or `90s`/`12m`/`1h`.
/// Fields before the seconds are whole numbers, and seconds stay below 60, as
/// do `h:mm:ss` minutes; `mm:ss` minutes may pass 59 (`113:03`).
pub fn parse_offset(input: &str) -> Result<u64> {
    let text = input.trim().to_ascii_lowercase();
    let invalid = || eyre!("Could not read the offset {input:?}; use mm:ss, h:mm:ss, or seconds.");
    let number = |part: &str| {
        let part = part.trim();
        let value = part.parse::<f64>().map_err(|_| invalid())?;
        if part.starts_with(['-', '+']) || !value.is_finite() || value < 0.0 {
            return Err(invalid());
        }
        Ok(value)
    };
    let whole = |value: f64, limit: Option<f64>| {
        if value.fract() == 0.0 && limit.is_none_or(|limit| value < limit) {
            Ok(value)
        } else {
            Err(invalid())
        }
    };
    let seconds = if text.contains(':') {
        let parts = text.split(':').map(number).collect::<Result<Vec<_>>>()?;
        let seconds = |value: f64| {
            if value < 60.0 {
                Ok(value)
            } else {
                Err(invalid())
            }
        };
        match parts.as_slice() {
            [minutes, rest] => whole(*minutes, None)? * 60.0 + seconds(*rest)?,
            [hours, minutes, rest] => {
                whole(*hours, None)? * 3_600.0
                    + whole(*minutes, Some(60.0))? * 60.0
                    + seconds(*rest)?
            }
            _ => return Err(invalid()),
        }
    } else {
        let (value, scale) = if let Some(value) = text.strip_suffix('h') {
            (value, 3_600.0)
        } else if let Some(value) = text.strip_suffix('m') {
            (value, 60.0)
        } else if let Some(value) = text.strip_suffix('s') {
            (value, 1.0)
        } else {
            (text.as_str(), 1.0)
        };
        number(value)? * scale
    };
    let milliseconds = (seconds * 1_000.0).round();
    // `u64::MAX as f64` rounds up to 2^64, the first value that does not fit.
    if !milliseconds.is_finite() || milliseconds >= u64::MAX as f64 {
        return Err(invalid());
    }
    Ok(milliseconds as u64)
}

#[cfg(test)]
#[path = "meeting_query_tests.rs"]
mod tests;
