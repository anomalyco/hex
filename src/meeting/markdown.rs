use super::transcript::LocalTime;
use super::{
    MeetingManifest, MeetingSource, TranscriptEntry, coalesce_transcript, display_title,
    format_duration,
};

/// What a transcript line's speaker is called in user-facing output: an
/// imported audio file has one track (`Speaker`), while a recorded call
/// separates `You` and `Them`.
pub fn speaker_label(source: MeetingSource, imported: bool) -> &'static str {
    if imported { "Speaker" } else { source.label() }
}

/// Renders already-coalesced `(source, start_ms, text)` turns as Markdown
/// blocks separated by blank lines.
pub fn render_turns_markdown<'a>(
    turns: impl IntoIterator<Item = (MeetingSource, u64, &'a str)>,
    imported: bool,
) -> String {
    turns
        .into_iter()
        .map(|(source, start_ms, text)| {
            format!(
                "**{}** · {}\n{}",
                speaker_label(source, imported),
                format_duration(start_ms),
                text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Coalesces raw transcript entries and renders them as speaker-turn Markdown,
/// labelling imported meetings as `Speaker` and recorded calls as `You`/`Them`.
pub fn transcript_markdown_with(entries: &[TranscriptEntry], imported: bool) -> String {
    let turns = coalesce_transcript(entries.iter().cloned());
    render_turns_markdown(
        turns
            .iter()
            .map(|turn| (turn.source, turn.start_ms, turn.text.as_str())),
        imported,
    )
}

/// Coalesces raw transcript entries for `manifest` and renders them as
/// speaker-turn Markdown, respecting `manifest.imported`.
pub fn transcript_markdown(manifest: &MeetingManifest, entries: &[TranscriptEntry]) -> String {
    transcript_markdown_with(entries, manifest.imported)
}

/// Speaker-turn Markdown for a live, in-progress recording (always two-track
/// `You` / `Them`, never imported).
pub fn live_transcript_markdown(entries: &[TranscriptEntry]) -> String {
    transcript_markdown_with(entries, false)
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
