//! A meeting's chapters with their notes, as the recording capsule and the
//! Meetings pane would show them: one concept, so a chapter (live chapters'
//! name, line, and start) holds the decisions, to-dos, questions, and points
//! heard in it. Pressing a note opens the transcript lines it came from.
//! Ported from #201's `meeting_outline`, with its topics folded into
//! chapters. Platform-neutral: a host converts its store into [`Outline`].

use rune::{Edge, El, Transition, col, el, row, text};

use super::super::controls::{DESCRIPTION, Paint};
use super::super::lists::{Tone, tone_tag};
use super::pipeline::clock;
use super::script::NoteKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExcerptLine {
    pub(super) mine: bool,
    pub(super) start_ms: u64,
    pub(super) text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Note {
    /// Stable across passes and boundary moves: the story's note index.
    pub(super) id: usize,
    pub(super) kind: NoteKind,
    pub(super) text: String,
    pub(super) owner: Option<String>,
    pub(super) at_ms: u64,
    pub(super) excerpt: Vec<ExcerptLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Section {
    pub(super) title: String,
    pub(super) summary: String,
    pub(super) start_ms: u64,
    pub(super) notes: Vec<Note>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Outline {
    pub(super) sections: Vec<Section>,
    /// A departure heard once, waiting for the next pass to confirm it.
    pub(super) maybe_next: Option<String>,
}

/// How much an outline shows: the capsule's panel keeps to names and
/// notes; the pane adds each chapter's line and the notes' times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Density {
    Capsule,
    Pane,
}

pub(super) fn kind_label(kind: NoteKind) -> Option<&'static str> {
    match kind {
        NoteKind::Point => None,
        NoteKind::Decision => Some("Decision"),
        NoteKind::Action => Some("To do"),
        NoteKind::Question => Some("Question"),
    }
}

/// Each kind's mark: a dot in its status colour, so a column of notes
/// scans by kind before it's read.
fn kind_color(p: Paint, kind: NoteKind) -> rune::Color {
    match kind {
        NoteKind::Point => p.faint(),
        NoteKind::Decision => p.success(),
        NoteKind::Action => p.accent(),
        NoteKind::Question => p.warning(),
    }
}

/// The outline's chapters, oldest first. `live` marks the newest "Now";
/// `toggle` opens or closes a note's lines.
pub(super) fn outline<S: 'static>(
    p: Paint,
    outline: &Outline,
    open: Option<usize>,
    live: bool,
    density: Density,
    toggle: impl Fn(&mut S, usize) + Clone + 'static,
) -> El {
    let c = &p.k.c;
    let last = outline.sections.len().saturating_sub(1);
    let mut column = col().key("outline").gap(2.0);
    for (index, section) in outline.sections.iter().enumerate() {
        let current = live && index == last;
        let mut heading = row()
            .key("heading")
            .gap(GUTTER_GAP)
            .center_y()
            .child(
                gutter("clock").child(
                    text(clock(section.start_ms))
                        .key(format!("clock:{}", section.start_ms))
                        .font(p.k.ty.mono)
                        .color(p.faint())
                        // A boundary the pass after Stop moves.
                        .transition(Transition::blur_replace()),
                ),
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
            heading = heading.child(tone_tag(p, "now", "Now", Tone::Danger));
        }
        let mut block = col()
            .key(format!("chapter{index}"))
            .gap(2.0)
            .px(8.0)
            .py(6.0)
            .radius(p.k.radius.md)
            .child(heading)
            .transition(Transition::reveal(Edge::Top));
        if current {
            block = block.fill(c.layer_01);
        }
        if density == Density::Pane && !section.summary.is_empty() {
            block = block.child(
                text(section.summary.clone())
                    .key("summary")
                    .ml(TEXT_X)
                    .font(DESCRIPTION)
                    .color(p.muted())
                    .wrap(),
            );
        }
        if !section.notes.is_empty() {
            let mut notes = col().key("notes").pt(2.0).gap(1.0);
            for note in &section.notes {
                let toggle = toggle.clone();
                let id = note.id;
                notes = notes.child(
                    note_row(p, note, open == Some(id), density)
                        .key(format!("note{id}"))
                        .on_press(move |state: &mut S| toggle(state, id)),
                );
            }
            block = block.child(notes);
        }
        column = column.child(block);
    }
    if let Some(next) = &outline.maybe_next {
        column = column.child(
            row()
                .key(format!("maybe:{next}"))
                .px(8.0)
                .py(4.0)
                .gap(GUTTER_GAP)
                .child(gutter("gap"))
                .child(
                    text(format!("Maybe a new chapter: {next}"))
                        .key("label")
                        .grow(1.0)
                        .min_w(0.0)
                        .truncate()
                        .font(p.k.ty.small)
                        .color(p.faint()),
                )
                .transition(Transition::opacity()),
        );
    }
    column
}

/// The gutter the chapters' clocks and the notes' marks end on, so every
/// name, line, note, and excerpt starts on one edge after it.
const GUTTER: f32 = 42.0;
const GUTTER_GAP: f32 = 8.0;
const TEXT_X: f32 = GUTTER + GUTTER_GAP;

fn gutter(key: &str) -> El {
    row().key(key).w(GUTTER).justify_end().center_y()
}

pub(super) fn note_row(p: Paint, note: &Note, open: bool, density: Density) -> El {
    let c = &p.k.c;
    let label = match (kind_label(note.kind), &note.owner) {
        (Some(label), Some(owner)) => Some(format!("{label} · {owner}")),
        (Some(label), None) => Some(label.to_string()),
        (None, _) => None,
    };
    let mut words = col().key("words").grow(1.0).min_w(0.0);
    if let Some(label) = label {
        words = words.child(
            text(label)
                .key("kind")
                .font(p.k.ty.small_medium)
                .color(kind_color(p, note.kind)),
        );
    }
    words = words.child(
        text(note.text.clone())
            .key("text")
            .font(p.k.ty.ui)
            .color(p.text())
            .wrap(),
    );
    let mut line = row()
        .key("line")
        .gap(GUTTER_GAP)
        .items_start()
        .child(
            // On the first line: the kind's label, or the note itself.
            gutter("mark")
                .h(if note.kind == NoteKind::Point {
                    18.0
                } else {
                    16.0
                })
                .child(
                    el().key("dot")
                        .size(6.0, 6.0)
                        .radius(3.0)
                        .fill(kind_color(p, note.kind)),
                ),
        )
        .child(words);
    if density == Density::Pane {
        line = line.child(
            text(clock(note.at_ms))
                .key("clock")
                .font(p.k.ty.small)
                .color(p.faint()),
        );
    }
    let mut item = col()
        .key("note")
        .role("button")
        .label(note.text.clone())
        .gap(6.0)
        .pr(6.0)
        .py(4.0)
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .transition(Transition::opacity().combined(Transition::offset(0.0, 4.0)));
    item = item.child(line);
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
                    text(format!(
                        "{} · {}",
                        if line.mine { "You" } else { "Them" },
                        clock(line.start_ms)
                    ))
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
        .ml(TEXT_X)
        .mb(4.0)
        .gap(10.0)
        .items_stretch()
        .transition(Transition::reveal(Edge::Top))
        .child(el().key("rule").w(2.0).radius(1.0).fill(c.border_base))
        .child(column)
}
