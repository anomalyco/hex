//! The meetings previews and scenes show: one recording, one with notes, one
//! without, and one that failed, plus variants with more call services and
//! live chapters (Labs).

use super::{Chapter, Meeting, MeetingsPane, Status, Transcript, Turn, transcript_markdown};

impl MeetingsPane {
    /// One recording, one with notes, one without, and one that failed.
    pub(in crate::rune_ui) fn fixture() -> Self {
        let turn = |mine, seconds: u64, text: &str| Turn {
            mine,
            start_ms: seconds * 1_000,
            text: text.into(),
            pause_before_ms: None,
        };
        let mut conversation = vec![
            turn(
                false,
                1,
                "Basically, I want Anomaly Arcade to be one login where you add friends and jump into any game.",
            ),
            turn(false, 4, "I need to finish my StarCraft game, too."),
            turn(true, 9, "Oh nice. How far along is it?"),
            turn(
                false,
                13,
                "Close. I feel like over a couple of months we could make twenty to thirty games.",
            ),
            turn(true, 20, "That's basically a new version of Steam."),
            turn(
                false,
                24,
                "If we're seeing Roblox, the idea should be to become Roblox.",
            ),
            turn(
                true,
                31,
                "Apple has a new thing where you hold two phones together and there's this pulse and gravity well. Could be a nice mini gravity well for the transitions.",
            ),
            turn(false, 40, "Prototype it in WebGPU, whatever's faster."),
            turn(
                true,
                95,
                "The build screen looks rough, by the way. We need a proposal for it.",
            ),
            turn(
                false,
                101,
                "Agreed. Write it up and I'll review it tomorrow.",
            ),
        ];
        conversation[8].pause_before_ms = Some(55_000);
        fn preview_folder(id: &str) -> std::path::PathBuf {
            format!("~/Library/Application Support/hex/meetings/{id}").into()
        }
        let meeting =
            |id: &str, title: &str, status, day: &str, clock: &str, minutes: u64| Meeting {
                id: id.into(),
                title: title.into(),
                status,
                day: day.into(),
                clock: clock.into(),
                duration_ms: minutes * 60_000 + 20_000,
                app: None,
                summary: None,
                notes_running: false,
                reveal: crate::meeting::RevealTarget::Folder(preview_folder(id)),
                folder: preview_folder(id),
                error: None,
                export_error: None,
                notes_error: None,
                live_error: None,
                microphone_warning: None,
                transcript: Transcript::Loaded(Vec::new()),
                chapters: Vec::new(),
                imported: false,
                kept_for: Some("24 hours"),
            };
        let mut live = meeting(
            "preview-live",
            "Zoom call — 14:05",
            Status::Recording,
            "Today",
            "14:05",
            7,
        );
        live.app = Some("Zoom".into());
        live.transcript = Transcript::Loaded(conversation[..6].to_vec());
        let mut notes = meeting(
            "preview-notes",
            "Arcade launch plan",
            Status::Complete,
            "Today",
            "11:12",
            38,
        );
        notes.app = Some("Zoom".into());
        notes.reveal = crate::meeting::RevealTarget::Markdown(
            "~/Documents/Hex Meetings/Arcade launch plan.md".into(),
        );
        notes.summary = Some(
            "One login for every Anomaly Arcade game, aiming for twenty to thirty games over the next couple of months. The build screen needs a proposal before more polish."
                .into(),
        );
        notes.transcript = Transcript::Loaded(conversation.clone());
        let mut design = meeting(
            "preview-design",
            "Design review",
            Status::Complete,
            "Yesterday",
            "16:30",
            24,
        );
        design.app = Some("FaceTime".into());
        design.transcript = Transcript::Loaded(conversation[2..6].to_vec());
        let mut failed = meeting(
            "preview-failed",
            "Meeting — 09:58",
            Status::Failed,
            "Yesterday",
            "09:58",
            2,
        );
        failed.error = Some("ScreenCaptureKit stopped the capture.".into());
        Self {
            meetings: vec![live, notes, design, failed],
            selected: Some("preview-notes".into()),
            ..Self::default()
        }
    }

    /// The fixture with recordings from more call services: a Zoom call, a
    /// Google Meet tab, a Teams website call, a Slack huddle, FaceTime, and
    /// one started by hand.
    #[cfg(all(test, not(target_os = "macos")))]
    pub(super) fn platforms_fixture() -> Self {
        let mut pane = Self::fixture();
        let mut more = Vec::new();
        for (id, title, app) in [
            ("preview-meet", "Weekly sync", "Google Meet"),
            ("preview-teams", "Vendor check-in", "teams.microsoft.com"),
            ("preview-huddle", "Release huddle", "Slack Huddle"),
        ] {
            let mut meeting = pane.meetings[2].clone();
            meeting.id = id.into();
            meeting.title = title.into();
            meeting.app = Some(app.into());
            more.push(meeting);
        }
        pane.meetings.splice(2..2, more);
        pane
    }

    /// The fixture with live chapters (Labs) on the
    /// meeting with notes, its title extended by them.
    pub(in crate::rune_ui) fn chapters_fixture() -> Self {
        let mut pane = Self::fixture();
        let chapters = [
            (
                "One login",
                "One login for every Arcade game, twenty to thirty games in a couple of months.",
                0,
            ),
            (
                "Gravity well",
                "A pulse and gravity well for transitions, prototyped in WebGPU.",
                31_000,
            ),
            (
                "Build screen",
                "The build screen needs a written proposal.",
                95_000,
            ),
        ];
        if let Some(notes) = pane.meetings.iter_mut().find(|m| m.id == "preview-notes") {
            notes.title = "Arcade launch plan: One login, Gravity well, Build screen".into();
            let turns = notes.turns().to_vec();
            let imported = notes.imported;
            notes.chapters = chapters
                .iter()
                .enumerate()
                .map(|(index, &(title, summary, start_ms))| {
                    let end_ms = chapters.get(index + 1).map(|next| next.2);
                    let inside = turns
                        .iter()
                        .filter(|turn| {
                            turn.start_ms >= start_ms
                                && end_ms.is_none_or(|end| turn.start_ms < end)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    Chapter {
                        title: title.into(),
                        summary: summary.into(),
                        start_ms,
                        markdown: format!(
                            "## {title}\n\n{summary}\n\n{}",
                            transcript_markdown(&inside, imported)
                        )
                        .trim_end()
                        .into(),
                    }
                })
                .collect();
        }
        pane
    }
}
