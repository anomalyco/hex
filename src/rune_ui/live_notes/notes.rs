//! One note in its chapter under the recording capsule: a mark in its
//! kind's colour, the note, and who owns it. Pressing it opens the
//! transcript lines it came from. Platform-neutral: a host converts its
//! store into [`Note`].

use rune::{Edge, El, Transition, col, el, row, text};

use super::super::controls::Paint;
use super::pipeline::clock;
use super::script::{NoteKind, Script};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExcerptLine {
    pub(super) mine: bool,
    pub(super) start_ms: u64,
    pub(super) text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Note {
    /// Stable across passes: the story's note index.
    pub(super) id: usize,
    pub(super) kind: NoteKind,
    pub(super) text: String,
    pub(super) owner: Option<String>,
    pub(super) excerpt: Vec<ExcerptLine>,
}

impl Note {
    pub(super) fn from_script(script: &Script, id: usize) -> Self {
        let spec = &script.notes[id];
        Self {
            id,
            kind: spec.kind,
            text: spec.text.into(),
            owner: spec.owner.map(Into::into),
            excerpt: spec
                .lines
                .iter()
                .map(|&line| {
                    let line = &script.lines[line];
                    ExcerptLine {
                        mine: line.mine,
                        start_ms: line.start_ms,
                        text: line.text.clone(),
                    }
                })
                .collect(),
        }
    }
}

/// Each kind's mark, so a stack of notes scans by kind before it's read.
pub(super) fn kind_color(p: Paint, kind: NoteKind) -> rune::Color {
    match kind {
        NoteKind::Point => p.faint(),
        NoteKind::Decision => p.success(),
        NoteKind::Action => p.accent(),
        NoteKind::Question => p.warning(),
    }
}

/// Pinned to the first line of the note, beside the mark.
const MARK_H: f32 = 20.0;
const MARK_W: f32 = 14.0;

pub(super) fn row_for(p: Paint, note: &Note, open: bool) -> El {
    let c = &p.k.c;
    let mut words = col().key("words").grow(1.0).min_w(0.0).gap(1.0).child(
        text(note.text.clone())
            .key("text")
            .font(p.k.ty.ui)
            .color(p.text())
            .wrap(),
    );
    if let Some(owner) = &note.owner {
        words = words.child(
            text(owner.clone())
                .key("owner")
                .font(p.k.ty.small)
                .color(p.faint()),
        );
    }
    let mut card = col()
        .key("card")
        .role("button")
        .label(note.text.clone())
        .w_full()
        .gap(8.0)
        .pl(8.0)
        .pr(10.0)
        .py(5.0)
        .radius(p.k.radius.md)
        .hover(c.hover)
        .press(c.pressed)
        .child(
            row()
                .key("line")
                .gap(6.0)
                .items_start()
                .child(
                    row().key("mark").w(MARK_W).h(MARK_H).center().child(
                        el().key("dot")
                            .size(6.0, 6.0)
                            .radius(3.0)
                            .fill(kind_color(p, note.kind)),
                    ),
                )
                .child(words),
        );
    if open {
        card = card.child(excerpt(p, &note.excerpt));
    }
    card
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
                    .color(p.faint()),
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
        .ml(MARK_W + 6.0)
        .gap(10.0)
        .items_stretch()
        .transition(Transition::reveal(Edge::Top))
        .child(el().key("rule").w(2.0).radius(1.0).fill(c.border_base))
        .child(column)
}
