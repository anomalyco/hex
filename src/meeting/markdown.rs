use super::transcript::LocalTime;
use super::{
    Chapter, Label, Labels, MeetingManifest, TranscriptEntry, coalesce_transcript, display_title,
    format_duration,
};

/// Renders already-coalesced `(label, start_ms, text)` turns as Markdown
/// blocks separated by blank lines.
pub fn render_turns_markdown<'a>(
    turns: impl IntoIterator<Item = (Label<'a>, u64, &'a str)>,
) -> String {
    turns
        .into_iter()
        .map(|(label, start_ms, text)| {
            format!(
                "**{label}** · {}\n{}",
                format_duration(start_ms),
                text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Coalesces raw transcript entries and renders them as speaker-turn
/// Markdown, each turn under what `labels` calls it.
pub fn transcript_markdown_with(entries: &[TranscriptEntry], labels: Labels<'_>) -> String {
    let turns = coalesce_transcript(entries.iter().cloned());
    render_turns_markdown(
        turns
            .iter()
            .map(|turn| (labels.of(turn), turn.start_ms, turn.text.as_str())),
    )
}

/// Coalesces raw transcript entries for `manifest` and renders them as
/// speaker-turn Markdown under the meeting's own labels.
pub fn transcript_markdown(manifest: &MeetingManifest, entries: &[TranscriptEntry]) -> String {
    transcript_markdown_with(entries, manifest.labels())
}

/// Speaker-turn Markdown for a live, in-progress recording (always two-track
/// `You` / `Them`, never imported, and nobody named yet).
pub fn live_transcript_markdown(entries: &[TranscriptEntry]) -> String {
    transcript_markdown_with(entries, Labels::default())
}

/// One chapter as Markdown for copying: its name, its line, and its turns.
pub fn section_markdown(
    chapter: &Chapter,
    entries: &[TranscriptEntry],
    labels: Labels<'_>,
) -> String {
    let mut markdown = format!("## {}\n\n", chapter.title.trim());
    if !chapter.summary.trim().is_empty() {
        markdown.push_str(chapter.summary.trim());
        markdown.push_str("\n\n");
    }
    markdown.push_str(&transcript_markdown_with(entries, labels));
    markdown.trim_end().to_string()
}

/// One line of the meeting folder's own `transcript.md`:
/// `**mm:ss You**  text`.
pub(super) fn folder_line(labels: Labels<'_>, entry: &TranscriptEntry) -> String {
    format!(
        "**{} {}**  {}",
        format_duration(entry.start_ms),
        labels.of(entry),
        entry.text
    )
}

/// The full exported Markdown document for a meeting: title, metadata, and
/// `## Transcript` with coalesced speaker turns.
pub(super) fn render_document(manifest: &MeetingManifest, entries: &[TranscriptEntry]) -> String {
    let started = LocalTime::from_ms(manifest.created_at_ms);
    let mut markdown = format!("# {}\n\n", display_title(manifest).trim());
    markdown.push_str(&format!(
        "- **Date:** {}, {}\n",
        started.date(),
        started.clock()
    ));
    if let Some(duration) = manifest.duration_ms {
        markdown.push_str(&format!("- **Duration:** {}\n", format_duration(duration)));
    }
    if let Some(app) = &manifest.app {
        markdown.push_str(&format!("- **App:** {app}\n"));
    }
    if let Some(calendar) = &manifest.calendar {
        if calendar.title.trim() != manifest.title.trim() && !calendar.title.trim().is_empty() {
            markdown.push_str(&format!("- **Calendar:** {}\n", calendar.title.trim()));
        }
        let attendees = calendar
            .attendees
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if !attendees.is_empty() {
            markdown.push_str(&format!("- **Attendees:** {}\n", attendees.join(", ")));
        }
    }
    markdown.push_str("\n## Transcript\n\n");
    let body = transcript_markdown(manifest, entries);
    if body.is_empty() {
        markdown.push_str("_No speech was transcribed._\n");
    } else {
        markdown.push_str(&body);
        markdown.push_str("\n\n");
    }
    markdown
}

/// Parses the turns of an exported meeting Markdown file (`**You** · 01:02`
/// followed by the turn's text). `You` is the microphone and every other
/// heading, a name included, is remote speech. A name is not kept: the file
/// has no speaker table, so its lines come back unnamed.
#[cfg(any(debug_assertions, test))]
pub(super) fn entries_from_markdown(markdown: &str) -> Vec<TranscriptEntry> {
    let mut entries: Vec<TranscriptEntry> = Vec::new();
    let mut in_turn = false;
    for line in markdown.lines() {
        let header = line
            .strip_prefix("**")
            .and_then(|rest| rest.split_once("** · "))
            .and_then(|(label, clock)| {
                let (minutes, seconds) = clock.trim().split_once(':')?;
                let start_ms =
                    (minutes.parse::<u64>().ok()? * 60 + seconds.parse::<u64>().ok()?) * 1_000;
                Some((Label::heard_on(label), start_ms))
            });
        if let Some((source, start_ms)) = header {
            entries.push(TranscriptEntry::new(source, start_ms, start_ms, ""));
            in_turn = true;
        } else if line.starts_with('#') || line.starts_with(super::notes::NOTES_START) {
            in_turn = false;
        } else if in_turn
            && !line.trim().is_empty()
            && let Some(entry) = entries.last_mut()
        {
            if !entry.text.is_empty() {
                entry.text.push(' ');
            }
            entry.text.push_str(line.trim());
        }
    }
    entries.retain(|entry| !entry.text.is_empty());
    // Markdown has start times only: a turn lasts until the next one, or
    // about 150 words a minute for the last.
    let starts = entries
        .iter()
        .skip(1)
        .map(|entry| entry.start_ms)
        .collect::<Vec<_>>();
    for (index, entry) in entries.iter_mut().enumerate() {
        let spoken = entry.start_ms + entry.text.split_whitespace().count() as u64 * 400;
        entry.end_ms = starts
            .get(index)
            .map_or(spoken, |next| (*next).max(entry.start_ms));
    }
    entries
}
