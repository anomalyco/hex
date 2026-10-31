//! The Meetings pane's listing, read from the meetings store on every OS,
//! and the meeting requests the Linux and Windows windows perform in their
//! own process (macOS's go through `macos_host`).

#[cfg(not(target_os = "macos"))]
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::meetings::{Chapter, Meeting, Status, Transcript, Turn};
#[cfg(not(target_os = "macos"))]
use super::platform::MeetingRequest;
#[cfg(not(target_os = "macos"))]
use crate::desktop::i18n::tr;

const RESCAN: Duration = Duration::from_secs(1);

/// Linux and Windows: the window and the meeting controller share one
/// process, so the window performs meeting requests itself.
#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub(super) struct LocalMeetings {
    feed: MeetingFeed,
    /// The transcript folder panel, while it is open.
    folder_choice: Option<crate::file_dialog::Pending>,
}

#[cfg(not(target_os = "macos"))]
impl LocalMeetings {
    /// `wake` redraws the window once the folder panel answers.
    pub(super) fn send(
        &mut self,
        request: MeetingRequest,
        wake: impl Fn() + Send + 'static,
    ) -> Result<(), String> {
        use crate::meeting::MeetingRequest as Controller;
        let failed = |error: color_eyre::Report| format!("{error:#}");
        match request {
            MeetingRequest::Start => {
                crate::meeting::runtime::request(Controller::Start { title: None })
            }
            MeetingRequest::Stop => crate::meeting::runtime::request(Controller::Stop),
            MeetingRequest::Summarize(id) => {
                crate::meeting::runtime::request(Controller::Summarize(id))
            }
            MeetingRequest::Rename { id, title } => crate::meeting::rename(&id, &title)
                .map(|_| ())
                .map_err(failed),
            MeetingRequest::Trash(id) => crate::meeting::trash(&id).map_err(failed),
            MeetingRequest::Reveal(id) => {
                crate::meeting::reveal(&crate::meeting::root().map_err(failed)?.join(id));
                Ok(())
            }
            MeetingRequest::RevealFolder => {
                let folder = crate::app_settings::meeting_output_folder();
                std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
                crate::meeting::reveal(&folder);
                Ok(())
            }
            MeetingRequest::ChooseFolder => {
                use crate::file_dialog::{Dialog, Target};
                if self.folder_choice.is_none() {
                    self.folder_choice = Some(crate::file_dialog::open(
                        Dialog {
                            title: tr("Transcript folder").into(),
                            target: Target::Folder,
                            directory: Some(crate::app_settings::meeting_output_folder()),
                        },
                        wake,
                    ));
                }
                Ok(())
            }
        }
    }

    /// The folder the panel answered with since the last call.
    pub(super) fn chosen_folder(&mut self) -> Option<Result<PathBuf, String>> {
        let choice = self.folder_choice.as_ref()?.poll()?;
        self.folder_choice = None;
        choice.transpose()
    }

    pub(super) fn meetings(&mut self) -> Option<Result<Vec<Meeting>, String>> {
        self.feed.poll()
    }
}

/// The Meetings pane's listing from the read-only store query, rescanned at
/// most once a second; unchanged meeting folders come from its cache.
#[derive(Default)]
pub(super) struct MeetingFeed {
    library: Option<crate::meeting::query::Library>,
    scan_at: Option<Instant>,
    shown: Option<Result<Vec<Meeting>, String>>,
}

impl MeetingFeed {
    pub(super) fn poll(&mut self) -> Option<Result<Vec<Meeting>, String>> {
        let now = Instant::now();
        if self.scan_at.is_some_and(|at| now < at) {
            return None;
        }
        self.scan_at = Some(now + RESCAN);
        let listed = self.scan();
        if self.shown.as_ref() == Some(&listed) {
            return None;
        }
        self.shown = Some(listed.clone());
        Some(listed)
    }

    fn scan(&mut self) -> Result<Vec<Meeting>, String> {
        let library = match &self.library {
            Some(library) => library,
            None => {
                let root = crate::meeting::root().map_err(|error| format!("{error:#}"))?;
                self.library
                    .insert(crate::meeting::query::Library::new(root))
            }
        };
        let scan = library.scan().map_err(|error| format!("{error:#}"))?;
        let now_ms = crate::events::now_ms();
        Ok(scan
            .meetings
            .iter()
            .map(|meeting| pane_meeting(meeting, now_ms))
            .collect())
    }
}

/// A stored meeting as the pane shows it.
fn pane_meeting(meeting: &crate::meeting::query::Meeting, now_ms: u64) -> Meeting {
    use crate::meeting::MeetingStatus;
    let manifest = &meeting.manifest;
    let transcript = if meeting.entries.is_empty() && !meeting.warnings.is_empty() {
        Transcript::Unavailable(meeting.warnings.join("\n"))
    } else {
        Transcript::Loaded(turns(&meeting.entries))
    };
    Meeting {
        id: meeting.id.clone(),
        title: meeting.title(),
        status: match meeting.status {
            MeetingStatus::Starting => Status::Starting,
            MeetingStatus::Recording => Status::Recording,
            MeetingStatus::Transcribing => Status::Transcribing,
            MeetingStatus::Complete => Status::Complete,
            MeetingStatus::Interrupted => Status::Interrupted,
            MeetingStatus::Failed => Status::Failed,
        },
        day: crate::meeting::day_label(meeting.start_ms(), now_ms),
        clock: crate::meeting::clock(meeting.start_ms()),
        duration_ms: meeting.duration_ms(now_ms),
        app: manifest.app.clone(),
        summary: manifest
            .notes
            .as_ref()
            .map(|notes| notes.summary.clone())
            .filter(|summary| !summary.is_empty()),
        notes_running: crate::meeting::notes_in_progress(&meeting.id),
        has_markdown: manifest
            .export_path
            .as_ref()
            .is_some_and(|path| path.exists()),
        error: manifest.error.clone(),
        export_error: manifest.export_error.clone(),
        notes_error: manifest.notes_error.clone(),
        live_error: manifest.live_transcription_error.clone(),
        transcript,
        // Sections come from the transcript lines, not the merged turns, as
        // the menu bar and `hex meetings show --chapter` read them.
        kept_for: (!meeting.status.is_active())
            .then(|| {
                crate::meeting::retention_for(manifest, crate::app_settings::meeting_retention())
            })
            .filter(|retention| retention.max_age_ms().is_some())
            .map(crate::app_settings::MeetingRetention::label),
        chapters: meeting
            .chapter_views(now_ms)
            .iter()
            .map(|view| Chapter {
                title: view.chapter.title.clone(),
                summary: view.chapter.summary.clone(),
                start_ms: view.chapter.start_ms,
                markdown: view.markdown(),
            })
            .collect(),
    }
}

/// Speaker turns, marking silences of 30 s or more.
fn turns(entries: &[crate::meeting::TranscriptEntry]) -> Vec<Turn> {
    let mut previous_end: Option<u64> = None;
    crate::meeting::coalesce_transcript(entries.iter().cloned())
        .into_iter()
        .map(|turn| {
            let pause_before_ms = previous_end
                .map(|end| turn.start_ms.saturating_sub(end))
                .filter(|pause| *pause >= 30_000);
            previous_end = Some(previous_end.map_or(turn.end_ms, |end| end.max(turn.end_ms)));
            Turn {
                mine: turn.source == crate::meeting::MeetingSource::Microphone,
                start_ms: turn.start_ms,
                text: turn.text,
                pause_before_ms,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meeting_feed_lists_stored_meetings_once_until_they_change() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings");
        let mut feed = MeetingFeed {
            library: Some(crate::meeting::query::Library::new(root)),
            ..MeetingFeed::default()
        };
        let meetings = feed.poll().expect("the first poll lists").unwrap();
        let ids: Vec<_> = meetings.iter().map(|meeting| meeting.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "1790001000000-1003",
                "1790000500000-1002",
                "1790000000000-1001"
            ],
            "newest first, without the unreadable folder"
        );
        let standup = &meetings[0];
        assert_eq!(standup.status, Status::Interrupted, "an unlocked recording");
        let review = &meetings[1];
        assert_eq!(review.title, "Roadmap review");
        let Transcript::Loaded(turns) = &review.transcript else {
            panic!("the roadmap review has a transcript");
        };
        assert!(!turns[0].mine && turns[1].mine);
        assert_eq!(turns.last().unwrap().pause_before_ms, Some(42_000));
        feed.scan_at = None;
        assert!(
            feed.poll().is_none(),
            "an unchanged listing is not sent again"
        );
    }

    #[test]
    fn copy_section_matches_the_menu_bar_and_the_cli() {
        let root = std::env::temp_dir().join(format!(
            "hex-copy-section-{}-{}",
            std::process::id(),
            crate::events::now_ms()
        ));
        let id = "1790003000000-1005";
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/meetings-chapters")
            .join(id);
        let directory = root.join(id);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::copy(
            fixture.join("manifest.json"),
            directory.join("manifest.json"),
        )
        .unwrap();
        // One speaker's two lines join into one turn across the boundary
        // at 06:30; the second line belongs to the chapter after.
        let line = |start_ms: u64, end_ms: u64, text: &str| crate::meeting::TranscriptEntry {
            source: crate::meeting::MeetingSource::Microphone,
            start_ms,
            end_ms,
            text: text.into(),
        };
        let entries = [
            line(181_000, 188_000, "Next, the motion lint."),
            line(386_000, 389_000, "That settles the lint."),
            line(390_500, 397_000, "Last thing: a name for the chat app."),
        ];
        let ndjson = entries
            .iter()
            .map(|entry| serde_json::to_string(entry).unwrap() + "\n")
            .collect::<String>();
        std::fs::write(directory.join("transcript.ndjson"), ndjson).unwrap();

        let library = crate::meeting::query::Library::new(&root);
        let meeting = library.meeting(id).unwrap();
        let now_ms = crate::events::now_ms();
        let pane = pane_meeting(&meeting, now_ms);
        let chapters = &meeting.manifest.chapters;
        for (index, chapter) in pane.chapters.iter().enumerate() {
            // The menu bar's Copy Section (`meeting::section_for_id`).
            let menu = crate::meeting::section_markdown(
                &chapters[index],
                &crate::meeting::chapter_entries(&entries, chapters, index),
            );
            assert_eq!(chapter.markdown, menu);
            let cli = library.chapter_markdown(id, index + 1, now_ms).unwrap();
            assert!(cli.ends_with(&format!("\n{menu}\n")), "{cli}");
        }
        assert!(pane.chapters[1].markdown.contains("That settles the lint."));
        assert!(!pane.chapters[1].markdown.contains("Last thing"));
        assert!(pane.chapters[2].markdown.contains("Last thing"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
