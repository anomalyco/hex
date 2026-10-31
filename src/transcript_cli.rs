//! Read-only, release-available transcript commands. No app services or migrations.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;

use clap::{Args, Subcommand};
use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::history::{HistoryEntry, HistoryKind, HistoryRetention};
use crate::meeting::query::{
    ChapterView, Library, ListQuery, Meeting, OffsetRange, SearchQuery, TextHit, TimeRange,
    TranscriptSource, search_texts,
};
use crate::vocabulary::VocabularyEntry;

#[derive(Args, Debug)]
pub struct HistoryArgs {
    /// Lower time bound: 3d, today, YYYY-MM-DD, or RFC 3339.
    #[arg(long)]
    since: Option<String>,
    /// Exclusive upper bound; a date includes that entire local day.
    #[arg(long)]
    until: Option<String>,
    /// Exact application name, ignoring case.
    #[arg(long)]
    app: Option<String>,
    /// Case-insensitive substring in final or raw text.
    #[arg(long)]
    grep: Option<String>,
    /// Rank by relevance with the meeting search's stemmed, fuzzy BM25.
    #[arg(long, conflicts_with = "oldest_first")]
    search: Option<String>,
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=10_000))]
    limit: u32,
    /// Include the stored pre-processing transcript.
    #[arg(long)]
    raw: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    oldest_first: bool,
    #[command(subcommand)]
    pub action: Option<HistoryAction>,
}

/// History changes, carried out by the running Hex (`crate::control`).
#[derive(Subcommand, Debug)]
pub enum HistoryAction {
    /// Delete every retained dictation.
    Clear {
        /// Confirm deleting the history.
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        output: crate::control::cli::Output,
    },
}

#[derive(Args, Debug)]
pub struct MeetingListArgs {
    /// Include meetings overlapping this time or later.
    #[arg(long)]
    since: Option<String>,
    #[arg(long)]
    until: Option<String>,
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=200))]
    limit: u32,
    #[arg(long)]
    json: bool,
}

impl Default for MeetingListArgs {
    fn default() -> Self {
        Self {
            since: None,
            until: None,
            limit: 20,
            json: false,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum MeetingsCommand {
    /// List recorded meetings, newest first, including recordings in progress.
    List(MeetingListArgs),
    /// Read a full transcript and stored notes, by id, unique prefix, or latest.
    Show {
        id: String,
        /// Markdown output (the default).
        #[arg(long, conflicts_with = "json")]
        md: bool,
        #[arg(long)]
        json: bool,
        /// Only entries overlapping this offset or later: mm:ss, h:mm:ss, or seconds.
        #[arg(long)]
        from: Option<String>,
        /// Only entries starting before this offset: mm:ss, h:mm:ss, or seconds.
        #[arg(long)]
        to: Option<String>,
        /// Only chapter N (counting from 1), as `hex meetings chapters` lists them.
        #[arg(long, conflicts_with_all = ["from", "to"])]
        chapter: Option<usize>,
    },
    /// List a meeting's chapters (Labs), by id, unique
    /// prefix, or latest.
    Chapters {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Search transcripts and notes with the same stemmed, fuzzy BM25 query as MCP.
    Search {
        query: String,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        until: Option<String>,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..=50))]
        limit: u32,
    },
    /// Start recording a meeting in the running Hex, visibly, as the menu
    /// bar's Record Meeting does (needs meetings.enabled).
    Record {
        /// The meeting's title (default: the calendar event's, or a
        /// provisional one).
        #[arg(long)]
        title: Option<String>,
        #[command(flatten)]
        output: crate::control::cli::Output,
    },
    /// Stop the recording meeting and transcribe it (Stop & Transcribe).
    Stop {
        #[command(flatten)]
        output: crate::control::cli::Output,
    },
    /// Copy the last meeting's transcript to the clipboard.
    CopyLast {
        #[command(flatten)]
        output: crate::control::cli::Output,
    },
}

#[derive(Serialize)]
struct HistoryRow<'a> {
    id: u64,
    timestamp_ms: u64,
    application: Option<&'a str>,
    mode: Option<&'a str>,
    kind: HistoryKind,
    final_text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snippet: Option<String>,
}

#[derive(Serialize)]
struct HistoryOutput<'a> {
    version: u32,
    retention: HistoryRetention,
    entries: Vec<HistoryRow<'a>>,
    truncated: bool,
}

fn history_output<'a>(
    entries: &'a [HistoryEntry],
    args: &HistoryArgs,
    retention: HistoryRetention,
    now: u64,
    vocabulary: &[VocabularyEntry],
) -> Result<HistoryOutput<'a>> {
    let range = TimeRange::parse(args.since.as_deref(), args.until.as_deref(), now)?;
    let cutoff = retention.max_age_ms().map(|age| now.saturating_sub(age));
    let app = args.app.as_ref().map(|s| s.to_lowercase());
    let grep = args.grep.as_ref().map(|s| s.to_lowercase());
    let mut matching: Vec<_> = entries
        .iter()
        .filter(|entry| cutoff.is_none_or(|cutoff| entry.timestamp_ms >= cutoff))
        .filter(|entry| range.contains(entry.timestamp_ms))
        .filter(|entry| {
            app.as_ref().is_none_or(|app| {
                entry
                    .application
                    .as_ref()
                    .is_some_and(|name| name.to_lowercase() == *app)
            })
        })
        .filter(|entry| {
            grep.as_ref().is_none_or(|grep| {
                entry.final_text.to_lowercase().contains(grep)
                    || entry.raw_text.to_lowercase().contains(grep)
            })
        })
        .collect();
    matching.sort_by_key(|entry| (entry.timestamp_ms, entry.id));
    if !args.oldest_first {
        matching.reverse();
    }
    let ranked: Vec<(&HistoryEntry, Option<TextHit>)> = match &args.search {
        Some(query) => rank_history(&matching, query, vocabulary)?
            .into_iter()
            .map(|(entry, hit)| (entry, Some(hit)))
            .collect(),
        None => matching.into_iter().map(|entry| (entry, None)).collect(),
    };
    let truncated = ranked.len() > args.limit as usize;
    let entries = ranked
        .into_iter()
        .take(args.limit as usize)
        .map(|(entry, hit)| HistoryRow {
            id: entry.id,
            timestamp_ms: entry.timestamp_ms,
            application: entry.application.as_deref(),
            mode: entry.processing.as_ref().map(|p| p.profile.as_str()),
            kind: entry.kind,
            final_text: &entry.final_text,
            raw_text: args.raw.then_some(entry.raw_text.as_str()),
            score: hit.as_ref().map(|hit| (hit.score * 100.0).round() / 100.0),
            snippet: hit.map(|hit| hit.snippet),
        })
        .collect();
    Ok(HistoryOutput {
        version: 1,
        retention,
        entries,
        truncated,
    })
}

/// Rank newest-first entries by their final text and, when it differs, their
/// raw transcript, as one document per entry with the better text's match.
/// Ties stay newest first.
fn rank_history<'a>(
    entries: &[&'a HistoryEntry],
    query: &str,
    vocabulary: &[VocabularyEntry],
) -> Result<Vec<(&'a HistoryEntry, TextHit)>> {
    let documents = entries
        .iter()
        .map(|entry| {
            let mut texts = vec![entry.final_text.as_str()];
            if !entry.raw_text.is_empty() && entry.raw_text != entry.final_text {
                texts.push(entry.raw_text.as_str());
            }
            texts
        })
        .collect::<Vec<_>>();
    Ok(search_texts(&documents, query, vocabulary)?
        .into_iter()
        .map(|hit| (entries[hit.index], hit))
        .collect())
}

/// Read only this setting, without loading AppSettings (which migrates and writes).
fn retention(root: &Path) -> Result<HistoryRetention> {
    #[derive(Default, Deserialize)]
    struct Settings {
        #[serde(default)]
        history_retention: HistoryRetention,
    }
    let file = match std::fs::File::open(root.join("settings.json")) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(HistoryRetention::default());
        }
        Err(error) => return Err(error).wrap_err("could not read history retention"),
    };
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(eyre!("settings file exceeds the read limit"));
    }
    let settings: Settings = serde_json::from_slice(&bytes)
        .wrap_err("could not read history retention; settings.json is invalid")?;
    Ok(settings.history_retention)
}

/// Labs' History word count: the words of the listed entries' final text,
/// or nothing when none are listed.
fn word_total_line(rows: &[HistoryRow<'_>]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let words: usize = rows
        .iter()
        .map(|row| row.final_text.split_whitespace().count())
        .sum();
    let word_label = if words == 1 { "word" } else { "words" };
    let dictation_label = if rows.len() == 1 {
        "dictation"
    } else {
        "dictations"
    };
    Some(format!(
        "Total: {words} {word_label} across {} {dictation_label}",
        rows.len()
    ))
}

pub fn history(args: &HistoryArgs) -> Result<()> {
    let root = crate::app_paths::support_dir()?;
    let entries = crate::history::read_snapshot(&root.join("history.json"))
        .wrap_err("could not read history.json")?;
    let output = history_output(
        &entries,
        args,
        retention(&root)?,
        crate::events::now_ms(),
        &crate::vocabulary::read_saved(&root),
    )?;
    let mut out = io::stdout().lock();
    if args.json {
        return write_json(&mut out, &output);
    }
    for row in &output.entries {
        write!(
            out,
            "{} · {}",
            crate::meeting::query::iso_local(row.timestamp_ms),
            row.application.unwrap_or("Unknown app")
        )?;
        if let Some(mode) = row.mode {
            write!(out, " · {mode}")?;
        }
        if let Some(score) = row.score {
            write!(out, " · score {score:.1}")?;
        }
        writeln!(out, "\n{}", row.final_text)?;
        if let Some(raw) = row.raw_text {
            writeln!(out, "Raw: {raw}")?;
        }
    }
    if output.entries.is_empty() {
        writeln!(out, "No history matches.")?;
    }
    if crate::labs::history_word_count()
        && let Some(line) = word_total_line(&output.entries)
    {
        writeln!(out, "{line}")?;
    }
    if output.truncated {
        writeln!(out, "More matches; raise --limit or narrow the time range.")?;
    }
    Ok(())
}

#[derive(Serialize)]
struct MeetingRow<'a> {
    id: &'a str,
    title: String,
    timestamp_ms: u64,
    duration_ms: u64,
    application: Option<&'a str>,
    status: crate::meeting::MeetingStatus,
    in_progress: bool,
    transcript_source: &'static str,
    warnings: &'a [String],
}

fn meeting_row(meeting: &Meeting, now: u64) -> MeetingRow<'_> {
    MeetingRow {
        id: &meeting.id,
        title: meeting.title(),
        timestamp_ms: meeting.start_ms(),
        duration_ms: meeting.duration_ms(now),
        application: meeting.manifest.app.as_deref(),
        status: meeting.status,
        in_progress: meeting.in_progress,
        transcript_source: match meeting.source {
            TranscriptSource::Final => "final",
            TranscriptSource::Rolling => "rolling",
            TranscriptSource::Missing => "missing",
        },
        warnings: &meeting.warnings,
    }
}

fn meeting_json(meeting: &Meeting, range: OffsetRange, now: u64) -> Result<serde_json::Value> {
    let notes = meeting.manifest.notes.as_ref().map(|notes| serde_json::json!({
        "title": notes.title, "summary": notes.summary, "generated_at_ms": notes.generated_at_ms,
    }));
    let mut json = serde_json::json!({
        "version": 1, "meeting": meeting_row(meeting, now), "notes": notes,
        "entries": meeting.entries_in(range, now)?,
    });
    if range.is_bounded() {
        json["range"] = serde_json::json!({
            "from_ms": range.from_ms.unwrap_or_default(), "to_ms": range.to_ms,
        });
    }
    // Only meetings with chapters carry the field.
    if !meeting.manifest.chapters.is_empty() {
        json["chapters"] = chapters_json(meeting, now);
    }
    Ok(json)
}

fn chapter_json(view: &ChapterView) -> serde_json::Value {
    serde_json::json!({
        "number": view.number, "title": view.chapter.title, "summary": view.chapter.summary,
        "start_ms": view.chapter.start_ms, "end_ms": view.end_ms,
    })
}

fn chapters_json(meeting: &Meeting, now: u64) -> serde_json::Value {
    meeting
        .chapter_views(now)
        .iter()
        .map(chapter_json)
        .collect()
}

/// `show --chapter N --json`: the meeting, its chapter, and only that
/// chapter's entries.
fn meeting_chapter_json(meeting: &Meeting, number: usize, now: u64) -> Result<serde_json::Value> {
    let view = meeting.chapter(number, now)?;
    let mut json = meeting_json(meeting, OffsetRange::default(), now)?;
    json["entries"] = serde_json::to_value(&view.entries)?;
    json["chapter"] = chapter_json(&view);
    Ok(json)
}

fn meeting_chapters_json(meeting: &Meeting, now: u64) -> serde_json::Value {
    serde_json::json!({
        "version": 1, "meeting": meeting_row(meeting, now),
        "chapters": chapters_json(meeting, now),
        "refined": meeting.manifest.chapters_refined,
    })
}

fn resolve(library: &Library, id: &str) -> Result<Arc<Meeting>> {
    if id == "latest" {
        library
            .scan()?
            .meetings
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("No meetings found."))
    } else {
        library.meeting(id)
    }
}

fn meeting_list_json(
    scan: &crate::meeting::query::Scan,
    limit: usize,
    now: u64,
) -> serde_json::Value {
    let rows: Vec<_> = scan
        .meetings
        .iter()
        .take(limit)
        .map(|meeting| meeting_row(meeting, now))
        .collect();
    let skipped: Vec<_> = scan
        .skipped
        .iter()
        .map(|item| serde_json::json!({"id": item.id, "reason": item.reason}))
        .collect();
    serde_json::json!({"version": 1, "meetings": rows, "skipped": skipped, "truncated": scan.meetings.len() > limit})
}

pub fn meetings(command: &MeetingsCommand) -> Result<()> {
    let root = crate::app_paths::support_dir()?;
    let library = Library::new(root.join("meetings"));
    let now = crate::events::now_ms();
    let mut out = io::stdout().lock();
    match command {
        MeetingsCommand::List(args) => {
            let query = ListQuery {
                since: args.since.clone(),
                until: args.until.clone(),
                query: None,
                limit: args.limit as usize,
                include_in_progress: true,
            };
            if args.json {
                let scan = library.list_matches(&query, now)?;
                write_json(
                    &mut out,
                    &meeting_list_json(&scan, args.limit as usize, now),
                )
            } else {
                write!(out, "{}", library.list(&query, now)?)?;
                Ok(())
            }
        }
        MeetingsCommand::Show {
            id,
            json,
            from,
            to,
            chapter,
            ..
        } => {
            let range = OffsetRange::parse(from.as_deref(), to.as_deref())?;
            let meeting = resolve(&library, id)?;
            match (chapter, *json) {
                (Some(number), true) => {
                    write_json(&mut out, &meeting_chapter_json(&meeting, *number, now)?)
                }
                (Some(number), false) => {
                    write!(
                        out,
                        "{}",
                        library.chapter_markdown(&meeting.id, *number, now)?
                    )?;
                    Ok(())
                }
                (None, true) => write_json(&mut out, &meeting_json(&meeting, range, now)?),
                (None, false) => {
                    write!(out, "{}", library.markdown(&meeting.id, range, now)?)?;
                    Ok(())
                }
            }
        }
        MeetingsCommand::Chapters { id, json } => {
            let meeting = resolve(&library, id)?;
            if *json {
                write_json(&mut out, &meeting_chapters_json(&meeting, now))
            } else {
                write!(out, "{}", library.chapters(&meeting.id, now)?)?;
                Ok(())
            }
        }
        MeetingsCommand::Search {
            query,
            since,
            until,
            limit,
        } => {
            write!(
                out,
                "{}",
                library.search(
                    &SearchQuery {
                        query: query.clone(),
                        since: since.clone(),
                        until: until.clone(),
                        meeting_id: None,
                        limit: *limit as usize,
                        fuzzy: true,
                        include_in_progress: true,
                        vocabulary: crate::vocabulary::read_saved(&root),
                    },
                    now
                )?
            )?;
            Ok(())
        }
        // `main` sends these to the running Hex.
        MeetingsCommand::Record { .. }
        | MeetingsCommand::Stop { .. }
        | MeetingsCommand::CopyLast { .. } => Err(eyre!("this command needs the running Hex")),
    }
}

fn write_json(out: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer_pretty(&mut *out, value)?;
    writeln!(out)?;
    Ok(())
}

#[cfg(test)]
#[path = "transcript_cli_tests.rs"]
mod tests;
