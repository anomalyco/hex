use super::*;

/// Returns the Markdown transcript for `manifest`, preferring its exported
/// Markdown when present and falling back to internal or live speaker turns.
pub fn transcript_markdown_for_manifest(manifest: &MeetingManifest) -> Result<String> {
    if let Some(path) = manifest.export_path.as_ref().filter(|path| path.exists()) {
        return fs::read_to_string(path)
            .wrap_err_with(|| format!("could not read {}", path.display()));
    }
    let internal_md = root()?.join(&manifest.id).join("transcript.md");
    if internal_md.exists() {
        return fs::read_to_string(&internal_md)
            .wrap_err_with(|| format!("could not read {}", internal_md.display()));
    }
    let entries = TranscriptReader::default().read(&manifest.id)?;
    if !entries.is_empty() {
        return Ok(transcript_markdown(&entries));
    }
    Err(eyre!("no meeting transcript is available"))
}

/// Returns the speaker-turn Markdown for `manifest` (without file headers),
/// falling back to exported Markdown if raw entries are unavailable.
pub fn speaker_turns_markdown_for_manifest(manifest: &MeetingManifest) -> Result<String> {
    if let Ok(entries) = TranscriptReader::default().read(&manifest.id)
        && !entries.is_empty()
    {
        return Ok(transcript_markdown(&entries));
    }
    transcript_markdown_for_manifest(manifest)
}

/// Returns the Markdown transcript for the active or most recent meeting.
pub fn latest_transcript_markdown() -> Result<String> {
    let meetings = list()?;
    for meeting in meetings
        .iter()
        .filter(|m| m.status.is_active())
        .chain(meetings.iter().filter(|m| !m.status.is_active()))
    {
        if let Ok(markdown) = speaker_turns_markdown_for_manifest(meeting)
            && !markdown.trim().is_empty()
        {
            return Ok(markdown);
        }
    }
    Err(eyre!("no meeting transcript is available"))
}
