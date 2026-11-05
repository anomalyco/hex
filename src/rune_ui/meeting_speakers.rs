//! Labs › Speakers in meetings, in the Meetings pane: the told-apart voices
//! of a meeting's system audio ("Speaker 1", "Speaker 2", …) label its
//! turns, and the meeting's Speakers section renames them.

use std::collections::BTreeMap;

use rune::{El, TextEdit, col, row, text};
use rune_kit::TextInput;

use super::Ui;
use super::controls::{ButtonKind, DESCRIPTION, Paint, button};
use super::list_detail as ld;
use super::lists::section_label;
use super::meetings::{Meeting, Turn, ask};
use super::platform::MeetingRequest;
use crate::desktop::i18n::{tr, tr_fill};

/// A meeting's told-apart voices, as the manifest records them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Speakers {
    pub(super) count: u32,
    pub(super) names: BTreeMap<u32, String>,
}

impl Speakers {
    fn name(&self, speaker: u32) -> String {
        self.names
            .get(&speaker)
            .cloned()
            .unwrap_or_else(|| tr_fill("Speaker {}", &speaker.to_string()))
    }
}

/// Who said a turn, as the pane shows it.
pub(super) fn label(speakers: Option<&Speakers>, turn: &Turn) -> String {
    match (turn.mine, turn.speaker) {
        (true, _) => tr("You").into(),
        (false, Some(speaker)) => match speakers {
            Some(speakers) => speakers.name(speaker),
            None => tr_fill("Speaker {}", &speaker.to_string()),
        },
        (false, None) => tr("Them").into(),
    }
}

/// Who said a turn in copied Markdown, untranslated like the exported file.
pub(super) fn markdown_label(speakers: Option<&Speakers>, turn: &Turn) -> String {
    match (turn.mine, turn.speaker) {
        (true, _) => "You".into(),
        (false, Some(speaker)) => speakers
            .and_then(|speakers| speakers.names.get(&speaker).cloned())
            .unwrap_or_else(|| format!("Speaker {speaker}")),
        (false, None) => "Them".into(),
    }
}

/// The meeting's speakers could not be told apart, unless a run is going.
pub(super) fn error(meeting: &Meeting) -> Option<(&'static str, &'static str, &str)> {
    meeting
        .speakers_error
        .as_deref()
        .filter(|_| !meeting.speakers_running)
        .map(|error| {
            (
                "speakers-error",
                tr("Speakers could not be told apart."),
                error,
            )
        })
}

/// The Speakers section: each voice with its name and Rename, or the run
/// still going.
pub(super) fn section(ui: &Ui, p: Paint, meeting: &Meeting) -> Option<El> {
    let block =
        col()
            .key("speakers")
            .gap(8.0)
            .child(section_label(p, "speakers-label", tr("Speakers")));
    if meeting.speakers_running {
        return Some(
            block.child(
                text(tr("Telling the speakers apart…"))
                    .key("speakers-running")
                    .font(DESCRIPTION)
                    .color(p.faint()),
            ),
        );
    }
    let speakers = meeting
        .speakers
        .as_ref()
        .filter(|speakers| speakers.count > 0)?;
    let mut list = col().key("speaker-list").gap(4.0);
    for speaker in 1..=speakers.count {
        let editing = ui.meetings.renaming_speaker.as_ref() == Some(&(meeting.id.clone(), speaker));
        list = list.child(if editing {
            editor(ui, p, speaker)
        } else {
            let id = meeting.id.clone();
            row()
                .key(format!("speaker{speaker}"))
                .gap(6.0)
                .center_y()
                .child(
                    text(speakers.name(speaker))
                        .key("name")
                        .font(p.k.ty.body)
                        .color(p.text()),
                )
                .child(ld::ghost_icon(
                    p,
                    "rename-speaker",
                    ld::EDIT_ICON,
                    tr("Rename speaker"),
                    move |ui: &mut Ui, _| start(ui, id.clone(), speaker),
                ))
        });
    }
    Some(block.child(list))
}

fn editor(ui: &Ui, p: Paint, speaker: u32) -> El {
    row()
        .key(format!("speaker{speaker}"))
        .gap(8.0)
        .center_y()
        .child(
            TextInput::new(&ui.meetings.speaker_name)
                .label(tr("Name"))
                .placeholder(tr_fill("Speaker {}", &speaker.to_string()))
                .width(Some(200.0))
                .on_edit(|ui: &mut Ui| &mut ui.meetings.speaker_name)
                .on_submit(|ui: &mut Ui, _| finish(ui))
                .build(p.k)
                .key("speaker-name-field"),
        )
        .child(button(
            p,
            "speaker-save",
            tr("Save"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| finish(ui),
        ))
        .child(button(
            p,
            "speaker-cancel",
            tr("Cancel"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.meetings.renaming_speaker = None,
        ))
}

fn start(ui: &mut Ui, id: String, speaker: u32) {
    let name = ui
        .meetings
        .meeting(&id)
        .and_then(|meeting| meeting.speakers.as_ref())
        .and_then(|speakers| speakers.names.get(&speaker).cloned())
        .unwrap_or_default();
    ui.meetings.speaker_name = TextEdit::new(name);
    ui.meetings.renaming_speaker = Some((id, speaker));
    ui.open = None;
}

fn finish(ui: &mut Ui) {
    let Some((id, speaker)) = ui.meetings.renaming_speaker.take() else {
        return;
    };
    let name = ui.meetings.speaker_name.text.trim().to_string();
    if ask(
        ui,
        MeetingRequest::RenameSpeaker {
            id: id.clone(),
            speaker,
            name: name.clone(),
        },
    ) && ui.host.is_fixture()
        && let Some(speakers) = ui
            .meetings
            .meetings
            .iter_mut()
            .find(|meeting| meeting.id == id)
            .and_then(|meeting| meeting.speakers.as_mut())
    {
        if name.is_empty() {
            speakers.names.remove(&speaker);
        } else {
            speakers.names.insert(speaker, name);
        }
    }
}

/// The fixture's meeting with notes, its other side told apart into two
/// voices, the second renamed.
pub(super) fn fixture() -> super::meetings::MeetingsPane {
    let mut pane = super::meetings::MeetingsPane::fixture();
    if let Some(notes) = pane
        .meetings
        .iter_mut()
        .find(|meeting| meeting.id == "preview-notes")
    {
        if let super::meetings::Transcript::Loaded(turns) = &mut notes.transcript {
            for (index, turn) in turns.iter_mut().filter(|turn| !turn.mine).enumerate() {
                turn.speaker = Some(if index % 3 == 1 { 2 } else { 1 });
            }
        }
        notes.speakers = Some(Speakers {
            count: 2,
            names: BTreeMap::from([(2, "Sam".to_string())]),
        });
    }
    pane
}
