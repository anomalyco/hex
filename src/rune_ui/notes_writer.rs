//! What writes a meeting's notes now, and the progress line that names it:
//! one rule for the Meetings pane and the finished-meeting capsule, which
//! both say notes are being written (#2062, #1151).

use crate::app_settings::MeetingSummaries;
use crate::desktop::i18n::tr;

/// The notes writer, as `meeting::generator` picks it: Apple Intelligence
/// where it can write, otherwise OpenCode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NotesWriter {
    AppleIntelligence,
    OpenCode,
}

impl NotesWriter {
    /// The writer for a Summaries choice on a computer with (or without)
    /// Apple Intelligence. Summaries › OpenCode always uses OpenCode.
    pub(super) fn for_choice(summaries: MeetingSummaries, apple_intelligence: bool) -> Self {
        if summaries != MeetingSummaries::OpenCode && apple_intelligence {
            Self::AppleIntelligence
        } else {
            Self::OpenCode
        }
    }

    /// What a pane or capsule says while this writer writes the notes.
    pub(super) fn running_label(self) -> &'static str {
        match self {
            Self::AppleIntelligence => tr("Writing a title and summary with Apple Intelligence…"),
            Self::OpenCode => tr("Writing a title and summary with OpenCode…"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MeetingSummaries::{Mac, Off, OpenCode};

    /// The progress line names the writer the generator picks: Apple
    /// Intelligence where it can write, unless Summaries sends notes to
    /// OpenCode.
    #[test]
    fn the_progress_line_names_the_writer_the_generator_picks() {
        for summaries in [Mac, Off] {
            let writer = NotesWriter::for_choice(summaries, true);
            assert_eq!(writer, NotesWriter::AppleIntelligence, "{summaries:?}");
            assert!(writer.running_label().contains("Apple Intelligence"));
        }
        for (summaries, apple_intelligence) in [
            (Mac, false),
            (Off, false),
            (OpenCode, false),
            (OpenCode, true),
        ] {
            let writer = NotesWriter::for_choice(summaries, apple_intelligence);
            assert_eq!(
                writer,
                NotesWriter::OpenCode,
                "{summaries:?}, Apple Intelligence {apple_intelligence}"
            );
            assert!(writer.running_label().contains("OpenCode"));
        }
    }
}
