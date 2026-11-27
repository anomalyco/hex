//! Timed subtitles for `hex meetings show --srt|--vtt`.

use crate::meeting::{Labels, TranscriptEntry};

/// A timed subtitle format for `hex meetings show --srt|--vtt`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SubtitleFormat {
    Srt,
    Vtt,
}

impl SubtitleFormat {
    pub(super) fn from_flags(srt: bool, vtt: bool) -> Option<Self> {
        match (srt, vtt) {
            (true, _) => Some(Self::Srt),
            (false, true) => Some(Self::Vtt),
            (false, false) => None,
        }
    }
}

/// One cue per transcript entry (a speaker turn), timed by the entry's
/// offsets from the meeting's start and labelled You, Them, or the remote
/// speaker's name.
pub(super) fn render_subtitles<'a>(
    entries: impl IntoIterator<Item = &'a TranscriptEntry>,
    labels: Labels<'_>,
    format: SubtitleFormat,
) -> String {
    let escaped = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut out = String::new();
    if format == SubtitleFormat::Vtt {
        out.push_str("WEBVTT\n\n");
    }
    let mut number = 0_usize;
    for entry in entries {
        let text = cue_text(&entry.text);
        if text.is_empty() {
            continue;
        }
        number += 1;
        let start = entry.start_ms;
        let end = entry.end_ms.max(start);
        let speaker = labels.of(entry);
        match format {
            SubtitleFormat::Srt => {
                out.push_str(&format!(
                    "{number}\n{} --> {}\n{speaker}: {text}\n\n",
                    subtitle_time(start, ','),
                    subtitle_time(end, ',')
                ));
            }
            SubtitleFormat::Vtt => {
                out.push_str(&format!(
                    "{} --> {}\n<v {}>{}\n\n",
                    subtitle_time(start, '.'),
                    subtitle_time(end, '.'),
                    escaped(speaker.as_str()),
                    escaped(&text)
                ));
            }
        }
    }
    out
}

/// The entry's text with blank lines and `-->` removed, so it can't end
/// or break its cue.
fn cue_text(text: &str) -> String {
    text.replace("-->", "->")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `HH:MM:SS,mmm` (SRT) or `HH:MM:SS.mmm` (VTT); hours grow past 99.
pub(super) fn subtitle_time(ms: u64, separator: char) -> String {
    let hours = ms / 3_600_000;
    let minutes = ms / 60_000 % 60;
    let seconds = ms / 1_000 % 60;
    let millis = ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}{separator}{millis:03}")
}
