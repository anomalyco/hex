use std::sync::OnceLock;

use super::*;

static MEETING_PASTE_CURSOR: OnceLock<Mutex<MeetingPasteCursor>> = OnceLock::new();

pub fn shared_paste_cursor() -> &'static Mutex<MeetingPasteCursor> {
    MEETING_PASTE_CURSOR.get_or_init(Default::default)
}

/// What Option-Control-V already handed out: for each source, the capture
/// time spans of the entries pasted. The in-call transcript can still change
/// near its live edge, where echo is judged again as audio arrives, and the
/// final transcript replaces it after Stop, so the cursor follows time rather
/// than identical entries. An entry is new when its midpoint lies outside
/// every pasted span, so text the final pass recovers for audio the in-call
/// transcript skipped is still pasted later.
#[derive(Clone, Debug, Default)]
pub struct MeetingPasteCursor {
    meeting_id: Option<String>,
    copied: [Vec<(u64, u64)>; 2],
}

/// Pasted spans closer than this merge, so a sentence split differently in
/// the final transcript is not pasted again through the pause between two
/// pasted ones.
const PASTE_SPAN_JOIN_MS: u64 = 1_500;

impl MeetingPasteCursor {
    fn unseen(&self, id: &str, entries: &[TranscriptEntry]) -> Vec<TranscriptEntry> {
        let same_meeting = self.meeting_id.as_deref() == Some(id);
        let mut unseen = entries
            .iter()
            .filter(|entry| {
                let middle = (entry.start_ms + entry.end_ms) / 2;
                !same_meeting
                    || !self.copied[cursor_slot(entry.source)]
                        .iter()
                        .any(|&(start, end)| start <= middle && middle <= end)
            })
            .cloned()
            .collect::<Vec<_>>();
        unseen.sort_by_key(|entry| entry.start_ms);
        unseen
    }

    /// Number of coalesced speaker turns available since the last committed
    /// copy or paste for `id`.
    pub fn pending_turn_count(&self, id: &str, entries: &[TranscriptEntry]) -> usize {
        coalesce_transcript(self.unseen(id, entries)).len()
    }

    /// Formats unseen entries for `id`, runs `commit_output`, and advances the
    /// cursor only if `commit_output` succeeds.
    pub fn consume_entries(
        &mut self,
        id: &str,
        entries: &[TranscriptEntry],
        commit_output: impl FnOnce(&str) -> Result<()>,
    ) -> Result<String> {
        let unseen = self.unseen(id, entries);
        let text = format_meeting_delta(&unseen)?;
        commit_output(&text)?;
        if self.meeting_id.as_deref() != Some(id) {
            self.meeting_id = Some(id.to_string());
            self.copied = Default::default();
        }
        for entry in unseen {
            let spans = &mut self.copied[cursor_slot(entry.source)];
            spans.push((entry.start_ms, entry.end_ms));
            spans.sort_unstable();
            let mut merged: Vec<(u64, u64)> = Vec::with_capacity(spans.len());
            for &(start, end) in spans.iter() {
                match merged.last_mut() {
                    Some(last) if start <= last.1 + PASTE_SPAN_JOIN_MS => last.1 = last.1.max(end),
                    _ => merged.push((start, end)),
                }
            }
            *spans = merged;
        }
        Ok(text)
    }
}

fn cursor_slot(source: MeetingSource) -> usize {
    match source {
        MeetingSource::System => 0,
        MeetingSource::Microphone => 1,
    }
}

/// Speaker turns of `entries`, which must be in time order.
pub fn format_meeting_delta(entries: &[TranscriptEntry]) -> Result<String> {
    if entries.is_empty() {
        return Err(eyre!("no new meeting transcript is available"));
    }
    Ok(coalesce_transcript(entries.iter().cloned())
        .into_iter()
        .map(|entry| format!("{}: {}", entry.source.label(), entry.text))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// How long a copy of new turns waits for the recording's audio up to the
/// click to be transcribed. That is the next audio packet (milliseconds), a
/// meeting chunk already running (well under a second on Metal), and each
/// source's last few seconds (a few hundred milliseconds each), so it
/// usually takes under a second; three seconds leaves room for a slower Mac
/// or a short backlog while still feeling like a response to the click. A
/// dictation in progress keeps the model, so the copy may run out of time
/// instead.
pub const COPY_TRANSCRIBE_WAIT: Duration = Duration::from_secs(3);

/// What a copy of new turns reads for meeting `id`: while this process
/// records it, the transcript through the click, waiting up to
/// [`COPY_TRANSCRIBE_WAIT`] for it; past that, or otherwise, the in-call
/// or final transcript as it is.
pub fn entries_for_copy(id: &str) -> Result<Vec<TranscriptEntry>> {
    match transcript::transcribe_up_to_now(id, COPY_TRANSCRIBE_WAIT) {
        transcript::Flushed::Fresh(entries) => return Ok(entries),
        transcript::Flushed::TimedOut => tracing::warn!(
            meeting = id,
            "the latest audio was not transcribed in time; copying what is ready"
        ),
        transcript::Flushed::NotRecording => {}
    }
    TranscriptReader::default().read(id)
}

/// Consumes new turns for `id` using the shared cursor, advancing only if
/// `commit_output` succeeds. While recording they come from the transcript
/// through now ([`entries_for_copy`]), afterwards from the final one.
pub fn consume_delta_for_id(
    id: &str,
    commit_output: impl FnOnce(&str) -> Result<()>,
) -> Result<String> {
    let entries = entries_for_copy(id)?;
    shared_paste_cursor()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .consume_entries(id, &entries, commit_output)
}

/// Like [`consume_delta_for_id`] over entries the caller already read, for the
/// main thread: it fails rather than wait while Option-Control-V holds the
/// cursor through its paste.
pub fn try_consume_delta_from(
    id: &str,
    entries: &[TranscriptEntry],
    commit_output: impl FnOnce(&str) -> Result<()>,
) -> Result<String> {
    let mut cursor = match shared_paste_cursor().try_lock() {
        Ok(cursor) => cursor,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            return Err(eyre!("a meeting paste is in progress"));
        }
    };
    cursor.consume_entries(id, entries, commit_output)
}

/// Speaker turns in `entries` that the shared cursor has not handed out.
pub fn pending_delta_turns(id: &str, entries: &[TranscriptEntry]) -> usize {
    shared_paste_cursor()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .pending_turn_count(id, entries)
}

/// Consumes new completed turns for the active or most recent meeting using
/// the shared cursor, advancing only if `commit_output` succeeds.
pub fn consume_active_or_latest_delta(
    commit_output: impl FnOnce(&str) -> Result<()>,
) -> Result<String> {
    let meetings = list()?;
    let selected =
        active_or_latest(&meetings).ok_or_else(|| eyre!("no meeting transcript is available"))?;
    consume_delta_for_id(&selected.id, commit_output)
}

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
