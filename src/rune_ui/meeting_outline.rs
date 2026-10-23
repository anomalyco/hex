//! A meeting's live notes as the recording capsule and the Meetings pane
//! show them: topic sections with timestamped notes, the current topic
//! marked "Now", and a pressed note opening the transcript lines it came
//! from. The store is macOS-only, so hosts convert `meeting::LiveNotes`
//! into [`Outline`]; scenes use [`Outline::fixture`].

use rune::{El, Transition, col, el, row, text};

use super::controls::Paint;
use super::lists::{Tone, tone_tag};
use crate::desktop_i18n::tr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NoteKind {
    Point,
    Decision,
    Action,
    Question,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExcerptLine {
    pub(super) mine: bool,
    pub(super) start_ms: u64,
    pub(super) text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Note {
    pub(super) kind: NoteKind,
    pub(super) text: String,
    pub(super) owner: Option<String>,
    pub(super) at_ms: u64,
    pub(super) excerpt: Vec<ExcerptLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Section {
    pub(super) title: String,
    pub(super) start_ms: u64,
    pub(super) notes: Vec<Note>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Outline {
    pub(super) sections: Vec<Section>,
}

/// The note whose excerpt is open. Notes move between sections and give way
/// to others as the outline grows, so they're known by time and text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NoteId {
    at_ms: u64,
    text: String,
}

pub(super) type OpenNote = Option<NoteId>;

impl Note {
    pub(super) fn id(&self) -> NoteId {
        NoteId {
            at_ms: self.at_ms,
            text: self.text.clone(),
        }
    }
}

/// Opens `note`, or closes it when it is already open.
pub(super) fn toggle(open: &mut OpenNote, note: NoteId) {
    *open = (open.as_ref() != Some(&note)).then_some(note);
}

#[cfg(target_os = "macos")]
impl Outline {
    pub(super) fn topic(&self) -> Option<&str> {
        self.sections.last().map(|section| section.title.as_str())
    }
}

impl Outline {
    pub(super) fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// Three topics from a design call, the last with one note so far.
    pub(super) fn fixture() -> Self {
        let line = |mine, seconds: u64, text: &str| ExcerptLine {
            mine,
            start_ms: seconds * 1_000,
            text: text.into(),
        };
        let note = |kind, text: &str, owner: Option<&str>, seconds: u64, excerpt| Note {
            kind,
            text: text.into(),
            owner: owner.map(Into::into),
            at_ms: seconds * 1_000,
            excerpt,
        };
        Self {
            sections: vec![
                Section {
                    title: "Arcade login and accounts".into(),
                    start_ms: 12_000,
                    notes: vec![
                        note(
                            NoteKind::Decision,
                            "One login across every Arcade game",
                            None,
                            41,
                            vec![
                                line(false, 41, "So every game shares the same account, right?"),
                                line(true, 45, "Yes, one login. We agreed on that last week."),
                            ],
                        ),
                        note(
                            NoteKind::Point,
                            "Twenty to thirty games over the next two months",
                            None,
                            96,
                            vec![line(
                                false,
                                96,
                                "We're aiming for twenty to thirty games over the next couple of months.",
                            )],
                        ),
                    ],
                },
                Section {
                    title: "Build screen polish".into(),
                    start_ms: 214_000,
                    notes: vec![
                        note(
                            NoteKind::Action,
                            "Write a proposal for the build screen",
                            Some("You"),
                            236,
                            vec![
                                line(false, 231, "The build screen still feels unfinished."),
                                line(true, 236, "I'll write a proposal before we polish it more."),
                            ],
                        ),
                        note(
                            NoteKind::Question,
                            "Should builds stream logs or show steps?",
                            None,
                            262,
                            vec![line(
                                false,
                                262,
                                "Do we stream the raw logs, or just show the steps?",
                            )],
                        ),
                    ],
                },
                Section {
                    title: "Launch checklist and owners".into(),
                    start_ms: 371_000,
                    notes: vec![note(
                        NoteKind::Action,
                        "Draft the launch post and changelog",
                        Some("Them"),
                        402,
                        vec![line(
                            false,
                            402,
                            "I can draft the launch post and the changelog this week.",
                        )],
                    )],
                },
            ],
        }
    }
}

#[cfg(target_os = "macos")]
impl From<&crate::meeting::LiveNotes> for Outline {
    fn from(notes: &crate::meeting::LiveNotes) -> Self {
        use crate::meeting::{LiveNoteKind, MeetingSource};
        let sections = notes
            .sections
            .iter()
            .map(|section| Section {
                title: section.title.clone(),
                start_ms: section.start_ms,
                notes: section
                    .bullets
                    .iter()
                    .map(|bullet| Note {
                        kind: match bullet.kind {
                            LiveNoteKind::Point => NoteKind::Point,
                            LiveNoteKind::Decision => NoteKind::Decision,
                            LiveNoteKind::Action => NoteKind::Action,
                            LiveNoteKind::Question => NoteKind::Question,
                        },
                        text: bullet.text.clone(),
                        owner: bullet.owner.clone(),
                        at_ms: bullet.at_ms,
                        excerpt: bullet
                            .excerpt
                            .iter()
                            .map(|entry| ExcerptLine {
                                mine: entry.source == MeetingSource::Microphone,
                                start_ms: entry.start_ms,
                                text: entry.text.clone(),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();
        Self { sections }
    }
}

/// `mm:ss`, as `meeting::format_duration`.
fn clock(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn kind_label(kind: NoteKind) -> Option<&'static str> {
    match kind {
        NoteKind::Point => None,
        NoteKind::Decision => Some(tr("Decision")),
        NoteKind::Action => Some(tr("To do")),
        NoteKind::Question => Some(tr("Question")),
    }
}

fn speaker(mine: bool) -> &'static str {
    if mine { tr("You") } else { tr("Them") }
}

/// The outline's sections. `live` marks the newest section "Now";
/// `toggle` opens or closes a note's excerpt.
pub(super) fn outline<S: 'static>(
    p: Paint,
    outline: &Outline,
    open: &OpenNote,
    live: bool,
    toggle: impl Fn(&mut S, NoteId) + Clone + 'static,
) -> El {
    let c = &p.k.c;
    let last = outline.sections.len().saturating_sub(1);
    let mut column = col().key("outline").gap(4.0);
    for (section_index, section) in outline.sections.iter().enumerate() {
        let current = live && section_index == last;
        let mut heading = row()
            .key("heading")
            .gap(8.0)
            .center_y()
            .child(
                text(clock(section.start_ms))
                    .key("clock")
                    .font(p.k.ty.mono)
                    .color(p.faint()),
            )
            .child(
                text(section.title.clone())
                    .key("title")
                    .grow(1.0)
                    .min_w(0.0)
                    .truncate()
                    .font(p.k.ty.ui_medium)
                    .color(p.text()),
            );
        if current {
            heading = heading.child(tone_tag(p, "now", tr("Now"), Tone::Danger));
        }
        let mut block = col()
            .key(format!("section{section_index}"))
            .gap(2.0)
            .px(8.0)
            .py(6.0)
            .radius(p.k.radius.md)
            .child(heading)
            .transition(Transition::reveal(rune::Edge::Top));
        if current {
            block = block.fill(c.layer_01);
        }
        for (note_index, note) in section.notes.iter().enumerate() {
            let id = note.id();
            let is_open = open.as_ref() == Some(&id);
            let toggle = toggle.clone();
            block = block.child(
                note_row(p, note, is_open)
                    .key(format!("note{note_index}"))
                    .on_press(move |state: &mut S| toggle(state, id.clone())),
            );
        }
        column = column.child(block);
    }
    column
}

fn note_row(p: Paint, note: &Note, open: bool) -> El {
    let c = &p.k.c;
    let label = match (kind_label(note.kind), &note.owner) {
        (Some(label), Some(owner)) => Some(format!("{label} · {owner}")),
        (Some(label), None) => Some(label.to_string()),
        (None, _) => None,
    };
    let mut line = row()
        .key("line")
        .gap(8.0)
        .items_start()
        .child(text("•").key("bullet").font(p.k.ty.ui).color(p.faint()));
    let mut words = col().key("words").grow(1.0).min_w(0.0);
    if let Some(label) = label {
        words = words.child(text(label).key("kind").font(p.k.ty.small_medium).color(
            match note.kind {
                NoteKind::Action => p.accent(),
                NoteKind::Decision => p.success(),
                _ => p.muted(),
            },
        ));
    }
    words = words.child(
        text(note.text.clone())
            .key("text")
            .font(p.k.ty.ui)
            .color(p.text())
            .wrap(),
    );
    line = line.child(words).child(
        text(clock(note.at_ms))
            .key("clock")
            .font(p.k.ty.mono)
            .color(p.faint()),
    );
    let mut item = col()
        .key("note")
        .role("button")
        .label(note.text.clone())
        .gap(6.0)
        .px(6.0)
        .py(4.0)
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .child(line);
    if open {
        item = item.fill(c.layer_02).child(excerpt(p, &note.excerpt));
    }
    item
}

fn excerpt(p: Paint, lines: &[ExcerptLine]) -> El {
    let c = &p.k.c;
    let mut column = col().key("lines").grow(1.0).min_w(0.0).gap(6.0);
    for (index, line) in lines.iter().enumerate() {
        column = column.child(
            col()
                .key(format!("line{index}"))
                .child(
                    text(format!("{} · {}", speaker(line.mine), clock(line.start_ms)))
                        .key("speaker")
                        .font(p.k.ty.small_medium)
                        .color(if line.mine { p.accent() } else { p.faint() }),
                )
                .child(
                    text(line.text.clone())
                        .key("text")
                        .font(p.k.ty.small)
                        .color(p.muted())
                        .wrap(),
                ),
        );
    }
    row()
        .key("excerpt")
        .ml(14.0)
        .gap(10.0)
        .items_stretch()
        .transition(Transition::reveal(rune::Edge::Top))
        .child(el().key("rule").w(2.0).radius(1.0).fill(c.border_base))
        .child(column)
}
