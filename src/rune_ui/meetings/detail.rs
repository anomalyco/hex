//! A meeting's detail: its title, meta line and actions, the summary and
//! chapters, the live or final transcript as turns, and the ⋯ menu.

use super::*;
use crate::meeting::{Label, MeetingSource, Tracks};

/// The meta line's source icon, a step under the 16 pt list icons.
const SOURCE_ICON: f32 = 14.0;

pub(super) fn detail(ui: &Ui, p: Paint, meeting: &Meeting, layer: &mut Vec<El>) -> El {
    let pane = &ui.meetings;
    let active = meeting.status.is_active();
    let copied = |what: &str| {
        pane.copied
            .as_ref()
            .is_some_and(|(id, copied)| id == &meeting.id && copied == what)
    };
    let armed = pane.delete_armed.as_deref() == Some(meeting.id.as_str());
    let renaming = pane.renaming.as_deref() == Some(meeting.id.as_str());
    let can_select = selectable(ui, meeting);
    let selecting = can_select && pane.selection.active();
    let mut actions = Vec::new();
    let mut confirm = None;
    if armed && !active && !meeting.notes_running {
        // The confirmation sits under the title until it is answered.
        let id = meeting.id.clone();
        confirm = Some(
            row()
                .key("delete-confirm")
                .gap(8.0)
                .pt(12.0)
                .child(
                    button(
                        p,
                        "meeting-confirm-delete",
                        if meeting.reveal.is_markdown() {
                            tr("Trash recording & Markdown")
                        } else {
                            tr("Trash recording")
                        },
                        ButtonKind::Danger,
                        move |ui: &mut Ui, _| trash(ui, id.clone()),
                    )
                    .default_button(),
                )
                .child(button(
                    p,
                    "meeting-cancel-delete",
                    tr("Cancel"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| ui.meetings.delete_armed = None,
                )),
        );
    } else if !renaming && !selecting {
        // The title field brings its own Save and Cancel. The copies, Select
        // lines, then More (Rename lives there), as quiet icons in the bar.
        if !meeting.turns().is_empty() {
            actions.extend(copy_actions(p, pane, meeting, [None, None]));
        }
        if can_select {
            actions.push(selection::action(p));
        }
        actions.extend(overflow(ui, p, meeting, layer));
    }

    let title = if renaming {
        let title_h = ld::CARD_HEADING.line_px();
        row()
            .key("rename")
            .w_full()
            .h(title_h)
            .gap(6.0)
            .center_y()
            .child(
                TextInput::new(&pane.rename)
                    .label(tr("Name"))
                    .width(Some(220.0))
                    .height(title_h)
                    .on_edit(|ui: &mut Ui| &mut ui.meetings.rename)
                    .on_submit(|ui: &mut Ui, _| finish_rename(ui))
                    .build(p.k)
                    .key("rename-field"),
            )
            .child(
                Button::new(tr("Save"))
                    .variant(ButtonVariant::Neutral)
                    .control_size(ControlSize::Small)
                    .on_press(|ui: &mut Ui| finish_rename(ui))
                    .build(p.k)
                    .key("meeting-save-title"),
            )
            .child(
                Button::new(tr("Cancel"))
                    .variant(ButtonVariant::Neutral)
                    .control_size(ControlSize::Small)
                    .on_press(|ui: &mut Ui| ui.meetings.renaming = None)
                    .build(p.k)
                    .key("meeting-cancel-title"),
            )
    } else {
        ld::card_heading(p, shown_title(meeting))
    };
    // The source first, after its small icon, then when and how long.
    let mut meta_line: Vec<String> = meeting.app.clone().into_iter().collect();
    meta_line.extend([
        day_label(&meeting.day).to_string(),
        meeting.clock.clone(),
        length(meeting.duration_ms),
    ]);
    let mut meta: Vec<El> = status_tag(p, meeting.status, pane.stop_pending)
        .into_iter()
        .collect();
    meta.push(
        row()
            .key("meta-source")
            .gap(6.0)
            .center_y()
            .shrink()
            .min_w(0.0)
            .child(
                rune::icon(platform_icon(meeting), SOURCE_ICON)
                    .key("source-icon")
                    .color(p.k.c.icon_muted)
                    .a11y_hidden(),
            )
            .child(ld::meta_text(p, &meta_line.join(" · "))),
    );

    let errors = [
        pane.action_error
            .as_deref()
            .map(|error| ("action-error", tr("Meeting could not be updated."), error)),
        meeting
            .error
            .as_deref()
            .map(|error| ("recording-error", tr("Recording failed."), error)),
        meeting
            .export_error
            .as_deref()
            .map(|error| ("export-error", tr("Markdown not saved."), error)),
        meeting
            .notes_error
            .as_deref()
            .filter(|_| !meeting.notes_running)
            .map(|error| ("notes-error", tr("Notes could not be written."), error)),
        meeting
            .live_error
            .as_deref()
            .filter(|_| active)
            .map(|error| ("live-error", tr("Live transcript unavailable."), error)),
    ];
    // The title leads the document and scrolls with it; the bar stays.
    let mut body = col()
        .key("detail-body")
        .gap(24.0)
        .pt(4.0)
        .pb(16.0)
        .child(title);
    for (key, title, error) in errors.into_iter().flatten() {
        body = body.child(error_panel(p, key, title, error, Tone::Danger));
    }
    if !active && meeting.microphone_warning == Some(crate::meeting::TrackWarning::DigitalSilence) {
        body = body.child(error_panel(
            p,
            "microphone-silence",
            tr("Microphone is silent"),
            tr("Your microphone was silent for part of this call. Another app may have been using it with voice processing, so your side may be missing."),
            Tone::Warning,
        ));
    }
    if meeting.notes_running || meeting.summary.is_some() {
        let mut notes =
            col()
                .key("notes")
                .gap(8.0)
                .child(body_heading(p, "summary-label", tr("Summary")));
        if meeting.notes_running {
            // Writing or regenerating: the spinner and the writer's line
            // above whatever summary is already shown (#2556).
            notes = notes.child(
                row()
                    .key("notes-running")
                    .gap(8.0)
                    .center_y()
                    .child(Loader::new().build(p.k))
                    .child(
                        text(notes_running_label(ui))
                            .font(DESCRIPTION)
                            .color(p.faint()),
                    ),
            );
        }
        if let Some(summary) = &meeting.summary {
            let summary = summary.trim();
            notes = notes.child(
                text(if summary.is_empty() {
                    tr("Nothing to summarize")
                } else {
                    summary
                })
                .key("summary")
                .font(p.k.ty.body)
                .color(if summary.is_empty() {
                    p.faint()
                } else {
                    p.text()
                })
                .wrap(),
            );
        }
        body = body.child(notes);
    }
    if !meeting.chapters.is_empty() {
        body = body.child(chapters(p, meeting, &copied));
    }
    let transcript_label = match meeting.status {
        Status::Starting | Status::Recording if !pane.stop_pending => tr("Live transcript"),
        Status::Starting | Status::Recording | Status::Transcribing => tr("Draft transcript"),
        Status::Complete => tr("Transcript"),
        Status::Interrupted | Status::Failed => tr("Recovered draft"),
    };
    body = body.child(
        col()
            .key("transcript-block")
            .gap(12.0)
            .child(body_heading(p, "transcript-label", transcript_label))
            .children(selecting.then(|| selection::bar(ui, p, meeting)))
            .child(transcript(ui, p, meeting, pane.stop_pending, selecting)),
    );
    col()
        .key("meeting-detail")
        .grow(1.0)
        .shrink()
        .child(ld::utility_bar(meta, actions))
        .children(confirm)
        .child(ld::card_scroll_sticky(
            p,
            "meeting-transcript",
            body,
            follows_live_transcript(ui, meeting),
        ))
}

/// A recording meeting's transcript keeps its newest line in view while
/// scrolled to the bottom. Any meeting not recording scrolls only by hand.
pub(super) fn follows_live_transcript(ui: &Ui, meeting: &Meeting) -> bool {
    matches!(meeting.status, Status::Starting | Status::Recording) && !ui.meetings.stop_pending
}

/// The meeting's chapters, each with its start, name, line, and Copy
/// section.
pub(super) fn chapters(p: Paint, meeting: &Meeting, copied: &dyn Fn(&str) -> bool) -> El {
    let mut list = col().key("chapters").gap(12.0).pb(16.0).child(body_heading(
        p,
        "chapters-label",
        tr("Chapters"),
    ));
    for (index, chapter) in meeting.chapters.iter().enumerate() {
        let what = format!("chapter{index}");
        let id = meeting.id.clone();
        let mut words = col().key("words").gap(2.0).grow(1.0).shrink().child(
            text(chapter.title.clone())
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .wrap(),
        );
        if !chapter.summary.trim().is_empty() {
            words = words.child(
                text(chapter.summary.trim())
                    .key("summary")
                    .font(DESCRIPTION)
                    .color(p.muted())
                    .wrap(),
            );
        }
        list = list.child(
            row()
                .key(format!("chapter{index}"))
                .gap(12.0)
                .child(
                    text(crate::meeting::format_duration(chapter.start_ms))
                        .key("clock")
                        .font(p.tabular(p.description()))
                        .color(p.faint())
                        .pt(1.0),
                )
                .child(words)
                .child(
                    ld::ghost_icon(
                        p,
                        "copy-section",
                        if copied(&what) {
                            p.k.glyph.check_small
                        } else {
                            ld::COPY_ICON
                        },
                        tr("Copy section"),
                        move |ui: &mut Ui, tx: &mut Tx| {
                            if let Some(markdown) = ui
                                .meetings
                                .meeting(&id)
                                .and_then(|meeting| meeting.chapters.get(index))
                                .map(|chapter| chapter.markdown.clone())
                            {
                                copy(ui, tx, id.clone(), what.clone(), markdown);
                            }
                        },
                    )
                    .mt(-4.0),
                ),
        );
    }
    list
}

/// Less common actions behind the ellipsis, Delete last.
pub(super) fn overflow(ui: &Ui, p: Paint, meeting: &Meeting, layer: &mut Vec<El>) -> Option<El> {
    let active = meeting.status.is_active();
    let mut menu = KitMenu::new("meeting-actions")
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .min_width(200.0)
        .open(ui.open == Some(Menu::MeetingActions))
        .on_close(|ui: &mut Ui| ui.open = None);
    let mut items = 0;
    // Rename sits here rather than as its own icon.
    if !active {
        let id = meeting.id.clone();
        menu = menu.item(MenuItem::new(tr("Rename…")).on_select(move |ui: &mut Ui| {
            ui.open = None;
            start_rename(ui, id.clone());
        }));
        items += 1;
    }
    if let Some(summary) = meeting.summary.clone() {
        let id = meeting.id.clone();
        let copied = ui
            .meetings
            .copied
            .as_ref()
            .is_some_and(|(copied, what)| copied == &meeting.id && what == "notes");
        // Stays open so "Copied" confirms the copy.
        menu = menu.item(
            MenuItem::new(if copied {
                tr("Copied")
            } else {
                tr("Copy summary")
            })
            .on_select_tx(move |ui: &mut Ui, tx: &mut Tx| {
                copy(ui, tx, id.clone(), "notes", summary.trim().to_string());
                ui.open = Some(Menu::MeetingActions);
            }),
        );
        items += 1;
    }
    if !active {
        // Reveal Markdown appears only while the export exists; the
        // recording folder stays one item away either way.
        if meeting.reveal.is_markdown() {
            let reveal = meeting.reveal.clone();
            menu = menu.item(
                MenuItem::new(tr("Reveal Markdown")).on_select(move |ui: &mut Ui| {
                    ui.open = None;
                    ask(ui, MeetingRequest::Reveal(reveal.clone()));
                }),
            );
            items += 1;
        }
        let folder = meeting.folder.clone();
        menu = menu.item(
            MenuItem::new(tr("Reveal files")).on_select(move |ui: &mut Ui| {
                ui.open = None;
                ask(
                    ui,
                    MeetingRequest::Reveal(crate::meeting::RevealTarget::Folder(folder.clone())),
                );
            }),
        );
        items += 1;
    }
    if meeting.status == Status::Complete && notes_available(ui) {
        if meeting.notes_running {
            menu = menu.item(MenuItem::new(tr("Summarizing…")).disabled(true));
        } else {
            let id = meeting.id.clone();
            menu = menu.item(
                MenuItem::new(if meeting.summary.is_some() {
                    tr("Regenerate summary")
                } else {
                    tr("Summarize")
                })
                .on_select(move |ui: &mut Ui| {
                    ui.open = None;
                    ask(ui, MeetingRequest::Summarize(id.clone()));
                }),
            );
        }
        items += 1;
    }
    if !active && !meeting.notes_running {
        if items > 0 {
            menu = menu.separator();
        }
        let id = meeting.id.clone();
        menu = menu
            .item(MenuItem::new(tr("Delete…")).on_select(move |ui: &mut Ui| trash(ui, id.clone())));
        items += 1;
    }
    if items == 0 {
        return None;
    }
    let trigger = ld::quiet_action(
        p,
        "meeting-more",
        &MORE_ICON,
        None,
        tr("More"),
        Some(ui.open == Some(Menu::MeetingActions)),
        |ui: &mut Ui, _| {
            ui.open = (ui.open != Some(Menu::MeetingActions)).then_some(Menu::MeetingActions);
        },
    );
    Some(menu.trigger(trigger).build(p.k, layer))
}

pub(super) fn transcript(
    ui: &Ui,
    p: Paint,
    meeting: &Meeting,
    stop_pending: bool,
    selecting: bool,
) -> El {
    let empty = |copy: &str| {
        text(copy)
            .key("transcript-empty")
            .font(DESCRIPTION)
            .color(p.faint())
            .wrap()
    };
    let turns = match &meeting.transcript {
        Transcript::Unavailable(error) => {
            return error_panel(
                p,
                "transcript-error",
                tr("Transcript unavailable."),
                error,
                Tone::Danger,
            );
        }
        Transcript::Loaded(turns) => turns,
    };
    if turns.is_empty() {
        return empty(match meeting.status {
            Status::Starting => tr("Preparing local audio capture…"),
            Status::Recording if meeting.live_error.is_some() => {
                tr("Live transcription is unavailable. Audio is still recording safely.")
            }
            Status::Recording if stop_pending => tr("Building the final transcript…"),
            Status::Recording => tr("Listening for speech…"),
            Status::Transcribing => tr("Building the final transcript…"),
            Status::Complete => tr("No speech was transcribed."),
            Status::Interrupted | Status::Failed => tr("No recoverable transcript is available."),
        });
    }
    let mut lines = col().key("turns").gap(16.0);
    for (index, turn) in turns.iter().enumerate() {
        if selecting {
            lines = lines.child(selection::turn_row(
                p,
                index,
                turn,
                meeting.imported,
                ui.meetings.selection.contains(turn),
            ));
        } else {
            lines = lines.children(turn_line(p, index, turn, meeting.imported));
        }
    }
    if meeting.status == Status::Recording && !stop_pending {
        lines = lines.child(
            text(tr("Listening…"))
                .key("listening")
                .font(p.k.ty.small)
                .color(p.faint()),
        );
    }
    lines
}

/// A section of the detail's body (Summary, Chapters, Transcript): a muted
/// label, as a settings section's, so it frames the 14 pt words under it
/// rather than reading as more of them.
pub(super) fn body_heading(p: Paint, key: &str, label: &str) -> El {
    text(label)
        .key(key)
        .role("heading")
        .level(3)
        .font(p.label())
        .color(p.muted())
}

impl Turn {
    /// What the turn is called, by the rule every transcript reader shares.
    pub(super) fn label(&self, imported: bool) -> Label<'_> {
        let source = if self.mine {
            MeetingSource::Microphone
        } else {
            MeetingSource::System
        };
        let tracks = if imported {
            Tracks::Imported
        } else {
            Tracks::Call
        };
        Label::of(source, self.name.as_ref(), tracks)
    }

    /// The label in the interface's language. A person's name is shown as
    /// the call showed it.
    pub(in crate::rune_ui) fn speaker(&self, imported: bool) -> &str {
        match self.label(imported) {
            Label::You => tr("You"),
            Label::Them => tr("Them"),
            Label::Speaker => tr("Speaker"),
            Label::Named(name) => name.as_str(),
        }
    }
}

/// One turn of a transcript: the pause before it, if any, then the speaker,
/// the time, and the words. An imported meeting has one track: Speaker.
pub(super) fn turn_line(p: Paint, index: usize, turn: &Turn, imported: bool) -> Vec<El> {
    let mut els = Vec::new();
    if let Some(pause) = turn.pause_before_ms {
        els.push(
            text(tr_fill("{} pause", &length(pause)))
                .key(format!("pause{index}"))
                .font(p.k.ty.small)
                .color(p.faint()),
        );
    }
    els.push(
        col()
            .key(format!("turn{index}"))
            .gap(2.0)
            .child(
                row()
                    .key("speaker")
                    .gap(8.0)
                    .child(
                        text(turn.speaker(imported))
                            .key("name")
                            .font(p.k.ty.small_medium)
                            .color(p.muted()),
                    )
                    .child(
                        text(crate::meeting::format_duration(turn.start_ms))
                            .key("clock")
                            .font(p.tabular(p.meta()))
                            .color(p.faint()),
                    ),
            )
            .child(
                text(turn.text.clone())
                    .key("text")
                    .font(p.k.ty.body)
                    .color(p.text())
                    .wrap(),
            ),
    );
    els
}
