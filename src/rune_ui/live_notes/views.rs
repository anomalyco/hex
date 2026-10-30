//! The story's stage: a mock screen with the recording capsule in a corner
//! and the notes floating under it, and the side column's clock and pass
//! log.

use rune::{Cursor, Cx, Drag, Edge, El, Phase as Gesture, Transition, Tx, col, el, row, text};

use super::super::controls::{DESCRIPTION, Paint, pulsing_dot, status_dot};
use super::super::lists::section_label;
use super::copy::Copy;
use super::notes::{Note, row_for};
use super::pipeline::clock;
use super::script::{NoteKind, lantern};
use super::{Corner, LiveNotes, Phase, SPEEDS};

const PILL_H: f32 = 36.0;
/// The capsule's column: the notes' width, and the most the pill takes.
const COLUMN_W: f32 = 340.0;
const COLUMN_GAP: f32 = 6.0;
const SCREEN_PAD: f32 = 16.0;
const CHAPTER_H: f32 = 30.0;
const CHIP_H: f32 = 22.0;
const STOP_SIZE: f32 = 22.0;
const SIDE_W: f32 = 240.0;

impl LiveNotes {
    /// The call's title before a pass has named anything, then the
    /// provisional one.
    fn base_title(&self) -> &'static str {
        if self.phase == Phase::Done || self.pipeline.passes > 0 {
            "Lantern design sync"
        } else {
            lantern().base_title
        }
    }

    fn current_chapter(&self) -> Option<&'static str> {
        let chapter = self.pipeline.chapters.last()?;
        Some(lantern().topics[chapter.topic].name)
    }

    /// The chapter showing its notes: one pressed open, else the current
    /// one while recording.
    fn open_chapter_index(&self) -> Option<usize> {
        self.open_chapter.or_else(|| {
            (self.phase == Phase::Recording)
                .then(|| self.pipeline.chapters.len().checked_sub(1))
                .flatten()
        })
    }

    /// The notes found in chapter `index`, in the order they were said.
    fn chapter_notes(&self, index: usize) -> Vec<usize> {
        let script = lantern();
        let chapters = &self.pipeline.chapters;
        let start = chapters[index].start_ms;
        let end = chapters
            .get(index + 1)
            .map_or(u64::MAX, |next| next.start_ms);
        let mut notes: Vec<usize> = self
            .pipeline
            .found
            .iter()
            .copied()
            .filter(|&note| (start..end).contains(&script.note_at(note)))
            .collect();
        notes.sort_by_key(|&note| script.note_at(note));
        notes
    }
}

fn toggle_note(s: &mut LiveNotes, note: usize) {
    s.take_over();
    s.open_note = (s.open_note != Some(note)).then_some(note);
}

fn toggle_chapter(s: &mut LiveNotes, index: usize) {
    s.take_over();
    s.open_note = None;
    s.open_chapter = (s.open_chapter_index() != Some(index)).then_some(index);
}

fn copy(what: Copy) -> impl Fn(&mut LiveNotes, &mut Tx) + 'static {
    move |s: &mut LiveNotes, tx: &mut Tx| {
        s.take_over();
        s.copy(what, tx);
    }
}

pub(super) fn stage(s: &LiveNotes, cx: &mut Cx<LiveNotes>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let screen_rect = cx.layout("screen");
    let column_rect = cx.layout("capsule");
    col()
        .key("live-notes")
        .w_full()
        .h_full()
        .fill(p.k.c.bg_base)
        .padding(12.0)
        .child(screen(s, p, screen_rect, column_rect))
}

/// The screen, with a window on it for the capsule to float over.
fn screen(s: &LiveNotes, p: Paint, screen: Option<[f32; 4]>, column: Option<[f32; 4]>) -> El {
    let c = &p.k.c;
    let mut lane = row().key("lane").w_full();
    if s.corner.right {
        lane = lane.justify_end();
    }
    let mut screen_el = col()
        .key("screen")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(SCREEN_PAD)
        .radius(p.k.radius.xl)
        .fill(c.bg_deep)
        .border(p.hair, c.border_weak)
        .clip_rounded(p.k.radius.xl)
        .child(window(p))
        .child(lane.child(capsule_column(s, p, screen, column)));
    if s.corner.bottom {
        screen_el = screen_el.justify_end();
    }
    screen_el
}

/// A stand-in for whatever the user is working in.
fn window(p: Paint) -> El {
    let c = &p.k.c;
    let dot = |key: &str| el().key(key).size(8.0, 8.0).radius(4.0).fill(c.border_base);
    let bar = |key: &str, w: f32| el().key(key).h(8.0).w(w).radius(4.0).fill(c.border_weak);
    col()
        .key("window")
        .pin(Some(96.0), Some(84.0), Some(96.0), Some(72.0))
        .radius(p.k.radius.lg)
        .fill(c.layer_01)
        .border(p.hair, c.border_weak)
        .child(
            row()
                .key("titlebar")
                .h(28.0)
                .px(12.0)
                .gap(6.0)
                .center_y()
                .child(dot("close"))
                .child(dot("min"))
                .child(dot("zoom")),
        )
        .child(
            col()
                .key("content")
                .padding(20.0)
                .gap(12.0)
                .child(bar("bar1", 180.0))
                .child(bar("bar2", 320.0))
                .child(bar("bar3", 260.0))
                .child(bar("bar4", 120.0)),
        )
}

/// The capsule and what floats under it (over it in a bottom corner),
/// moving together while the capsule is dragged.
fn capsule_column(
    s: &LiveNotes,
    p: Paint,
    screen: Option<[f32; 4]>,
    column: Option<[f32; 4]>,
) -> El {
    let mut under: Vec<El> = Vec::new();
    if s.phase == Phase::Done {
        under.push(summary(s, p));
    }
    if s.notes_open && !s.pipeline.chapters.is_empty() {
        let enter = Transition::opacity().combined(Transition::offset(
            0.0,
            if s.corner.bottom { 8.0 } else { -8.0 },
        ));
        under.push(chapters(s, p).transition(enter));
    }
    if s.corner.bottom {
        under.reverse();
    }
    let pill = pill(s, p).on_drag(move |s: &mut LiveNotes, drag: &mut Drag, tx: &mut Tx| {
        drag_capsule(s, drag, tx, screen, column);
    });
    let mut column_el = col()
        .key("capsule")
        .w(COLUMN_W)
        .gap(COLUMN_GAP)
        .offset(s.drag.0, s.drag.1);
    column_el = if s.corner.right {
        column_el.items_end()
    } else {
        column_el.items_start()
    };
    if s.corner.bottom {
        column_el.children(under).child(pill)
    } else {
        column_el.child(pill).children(under)
    }
}

/// The chapters so far, oldest first, with the open one's notes under it.
fn chapters(s: &LiveNotes, p: Paint) -> El {
    let c = &p.k.c;
    let script = lantern();
    let open = s.open_chapter_index();
    let last = s.pipeline.chapters.len() - 1;
    let mut panel = col()
        .key("chapters")
        .w_full()
        .gap(2.0)
        .padding(4.0)
        .radius(p.k.radius.lg)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak);
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    for (index, chapter) in s.pipeline.chapters.iter().enumerate() {
        let name = script.topics[chapter.topic].name;
        let current = index == last && s.phase == Phase::Recording;
        let hovered = s.hovered_chapter == Some(index);
        let trailing = if hovered {
            let mut actions = row().key("actions").gap(2.0).child(chip(
                p,
                "copy",
                "Copy",
                copy(Copy::Chapter(index)),
            ));
            // From the last chapter on is the chapter itself.
            if index < last {
                actions = actions.child(chip(p, "from", "From here", copy(Copy::From(index))));
            }
            actions
        } else {
            row().key("time").child(
                text(clock(chapter.start_ms))
                    .key("start")
                    .font(p.k.ty.mono)
                    .color(p.faint()),
            )
        };
        let mut title = row()
            .key("title")
            .grow(1.0)
            .min_w(0.0)
            .h(CHAPTER_H)
            .gap(8.0)
            .center_y()
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut LiveNotes| toggle_chapter(s, index));
        if current {
            title = title.child(status_dot(p.danger()));
        }
        title = title.child(
            text(name)
                .key("name")
                .shrink()
                .min_w(0.0)
                .truncate()
                .font(if open == Some(index) {
                    p.k.ty.ui_medium
                } else {
                    p.k.ty.ui
                })
                .color(if open == Some(index) {
                    p.text()
                } else {
                    p.muted()
                }),
        );
        let mut chapter_el = col().key(format!("chapter{index}")).w_full().child(
            row()
                .key("row")
                .w_full()
                .pl(8.0)
                .pr(4.0)
                .gap(8.0)
                .center_y()
                .radius(p.k.radius.md)
                .hover(c.hover)
                .on_hover_change(move |s: &mut LiveNotes, inside, _| {
                    if inside {
                        s.hovered_chapter = Some(index);
                    } else if s.hovered_chapter == Some(index) {
                        s.hovered_chapter = None;
                    }
                })
                .child(title)
                .child(trailing),
        );
        if open == Some(index) {
            let mut notes = col()
                .key("notes")
                .w_full()
                .pb(4.0)
                .transition(Transition::reveal(Edge::Top));
            for id in s.chapter_notes(index) {
                let note = Note::from_script(script, id);
                notes = notes.child(
                    row_for(p, &note, s.open_note == Some(id))
                        .key(format!("note{id}"))
                        .on_press(move |s: &mut LiveNotes| toggle_note(s, id))
                        .transition(Transition::opacity()),
                );
            }
            chapter_el = chapter_el.child(notes);
        }
        panel = panel.child(chapter_el);
    }
    panel
}

/// A small quiet text button.
fn chip(
    p: Paint,
    key: &str,
    label: &'static str,
    press: impl Fn(&mut LiveNotes, &mut Tx) + 'static,
) -> El {
    let c = &p.k.c;
    row()
        .key(key)
        .role("button")
        .label(label)
        .h(CHIP_H)
        .px(8.0)
        .center()
        .radius(CHIP_H / 2.0)
        .fill(c.layer_02)
        .hover(c.hover)
        .press(c.pressed)
        .cursor(Cursor::Pointer)
        .on_press_tx(press)
        .child(
            text(label)
                .key("label")
                .font(p.k.ty.small_medium)
                .color(p.text()),
        )
}

/// The capsule follows the pointer and, let go, settles in the screen
/// corner nearest it. A cancelled drag goes back where it was.
fn drag_capsule(
    s: &mut LiveNotes,
    drag: &Drag,
    tx: &mut Tx,
    screen: Option<[f32; 4]>,
    column: Option<[f32; 4]>,
) {
    match drag.phase {
        Gesture::Down => s.take_over(),
        Gesture::Move => s.drag = (drag.dx, drag.dy),
        Gesture::Up => {
            if !drag.cancelled()
                && let (Some(screen), Some(column)) = (screen, column)
            {
                let x = column[0] + column[2] / 2.0 + drag.dx;
                let y = column[1] + PILL_H / 2.0 + drag.dy;
                s.corner = Corner {
                    right: x > screen[0] + screen[2] / 2.0,
                    bottom: y > screen[1] + screen[3] / 2.0,
                };
            }
            s.drag = (0.0, 0.0);
            tx.animated();
        }
    }
}

/// Dot, title, and on the right the elapsed time. Under the pointer the
/// time gives way to copying, hiding the notes, and stopping: Copy all,
/// then, once something is copied, Copy new beside it. What a copy did
/// shows for a moment where the time was. The title is keyed by its words,
/// so a new chapter blurs in over the old.
fn pill(s: &LiveNotes, p: Paint) -> El {
    let c = &p.k.c;
    let (dot, title) = match s.phase {
        Phase::Recording => (
            pulsing_dot(p.danger()),
            s.current_chapter().unwrap_or(s.base_title()),
        ),
        Phase::Transcribing | Phase::Writing => (pulsing_dot(p.muted()), "Finishing notes…"),
        Phase::Done => (status_dot(p.success()), s.base_title()),
    };
    let actions = s.hovered && matches!(s.phase, Phase::Recording | Phase::Done);
    let mut pill = row()
        .key("pill")
        .h(PILL_H)
        .max_w(COLUMN_W)
        .gap(8.0)
        .pl(14.0)
        .pr(if actions { 7.0 } else { 14.0 })
        .center_y()
        .radius(999.0)
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .cursor(Cursor::Grab)
        .on_hover_change(|s: &mut LiveNotes, inside, _| s.hovered = inside)
        .child(dot.key("dot"))
        .child(
            text(title)
                .key(format!("title:{title}"))
                .shrink()
                .min_w(0.0)
                .truncate()
                .font(p.k.ty.ui_medium)
                .color(p.text())
                .transition(Transition::blur_replace()),
        );
    for shadow in &p.k.elevation.overlay {
        pill = pill.box_shadow(*shadow);
    }
    if let Some((flash, _)) = &s.flash {
        pill = pill.child(
            text(flash.clone())
                .key("flash")
                .font(p.k.ty.small_medium)
                .color(p.success())
                .transition(Transition::opacity()),
        );
    } else if actions {
        if s.copied_through.is_some() {
            pill = pill.child(chip(p, "copy-new", "Copy new", copy(Copy::New)));
        }
        pill = pill.child(chip(p, "copy-all", "Copy all", copy(Copy::All)));
    } else if s.phase == Phase::Recording {
        pill = pill.child(
            text(clock(s.clock_ms))
                .key("clock")
                .font(p.k.ty.mono)
                .color(p.faint()),
        );
    }
    if actions && !s.pipeline.chapters.is_empty() {
        pill = pill.child(hide(s, p));
    }
    if actions && s.phase == Phase::Recording {
        pill = pill.child(stop(p));
    }
    pill
}

/// Folds the notes away toward the capsule, or brings them back.
fn hide(s: &LiveNotes, p: Paint) -> El {
    let c = &p.k.c;
    // The chevron points where the notes go.
    let toward_pill = s.notes_open != s.corner.bottom;
    let glyph = if toward_pill {
        p.k.glyph.chevron_up
    } else {
        p.k.glyph.chevron_down
    };
    row()
        .key("hide")
        .role("button")
        .label(if s.notes_open {
            "Hide notes"
        } else {
            "Show notes"
        })
        .size(STOP_SIZE, STOP_SIZE)
        .center()
        .radius(STOP_SIZE / 2.0)
        .fill(c.layer_02)
        .hover(c.hover)
        .press(c.pressed)
        .cursor(Cursor::Pointer)
        .on_press(|s: &mut LiveNotes| {
            s.take_over();
            s.notes_open = !s.notes_open;
        })
        .child(rune::icon(glyph, 14.0).key("chevron").color(c.icon_muted))
}

fn stop(p: Paint) -> El {
    let c = &p.k.c;
    row()
        .key("stop")
        .role("button")
        .label("Stop")
        .size(STOP_SIZE, STOP_SIZE)
        .center()
        .radius(STOP_SIZE / 2.0)
        .fill(c.layer_02)
        .hover(c.hover)
        .press(c.pressed)
        .cursor(Cursor::Pointer)
        .transition(Transition::opacity())
        .on_press(|s: &mut LiveNotes| {
            s.take_over();
            s.stop();
        })
        .child(
            el().key("square")
                .size(8.0, 8.0)
                .radius(2.0)
                .fill(p.danger()),
        )
}

/// Once the notes are written: the summary and what they hold.
fn summary(s: &LiveNotes, p: Paint) -> El {
    let c = &p.k.c;
    let mut card = col()
        .key("summary")
        .w_full()
        .gap(6.0)
        .px(14.0)
        .py(10.0)
        .radius(p.k.radius.lg)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        .transition(Transition::opacity())
        .child(
            text(lantern().summary)
                .key("text")
                .font(p.k.ty.ui)
                .color(p.text())
                .wrap(),
        )
        .child(
            text(counts(s))
                .key("counts")
                .font(p.k.ty.small)
                .color(p.faint()),
        );
    for shadow in &p.k.elevation.overlay {
        card = card.box_shadow(*shadow);
    }
    card
}

/// "3 decisions · 6 to-dos · 2 questions".
fn counts(s: &LiveNotes) -> String {
    let script = lantern();
    let count = |kind| {
        s.pipeline
            .found
            .iter()
            .filter(|&&note| script.notes[note].kind == kind)
            .count()
    };
    let mut parts = Vec::new();
    for (kind, one, many) in [
        (NoteKind::Decision, "decision", "decisions"),
        (NoteKind::Action, "to-do", "to-dos"),
        (NoteKind::Question, "question", "questions"),
    ] {
        match count(kind) {
            0 => {}
            1 => parts.push(format!("1 {one}")),
            n => parts.push(format!("{n} {many}")),
        }
    }
    parts.join(" · ")
}

/// The clock, the speed, what's playing, and what each pass did.
pub(super) fn side(s: &LiveNotes, cx: &mut Cx<LiveNotes>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let script = lantern();
    let state = match s.phase {
        Phase::Recording if s.playing => format!("Playing at {}×", SPEEDS[s.speed]),
        Phase::Recording if s.clock_ms == 0 => "Press R to play".into(),
        Phase::Recording => "Paused".into(),
        Phase::Transcribing => "Stopped: transcribing".into(),
        Phase::Writing => "Stopped: writing the title and summary".into(),
        Phase::Done => "Done".into(),
    };
    let what = if s.notes_open {
        "The chapters so far float under the capsule, the current one open to its notes."
    } else {
        "The notes are hidden; the capsule's chevron brings them back."
    };
    let mut log = col().key("log").gap(4.0);
    for (index, entry) in s.pipeline.log.iter().enumerate().rev().take(8) {
        log = log.child(
            text(entry.clone())
                .key(format!("entry{index}"))
                .font(p.k.ty.small)
                .color(p.muted())
                .wrap(),
        );
    }
    col()
        .key("live-notes-side")
        .w(SIDE_W)
        .gap(12.0)
        .child(
            text(format!(
                "{} / {}",
                clock(s.clock_ms),
                clock(script.end_ms())
            ))
            .key("clock")
            .font(p.k.ty.mono)
            .color(p.text()),
        )
        .child(
            text(state)
                .key("state")
                .font(p.k.ty.small_medium)
                .color(p.muted()),
        )
        .child(
            text(what)
                .key("what")
                .font(DESCRIPTION)
                .color(p.faint())
                .wrap(),
        )
        .child(
            text(
                "Hover the capsule to copy the transcript (Copy new shows after a first copy), \
                 or a chapter to copy it or everything from it on. Drag the capsule to any corner.",
            )
            .key("how")
            .font(DESCRIPTION)
            .color(p.faint())
            .wrap(),
        )
        .child(el().key("rule").h(p.hair).w_full().fill(p.k.c.border_weak))
        .child(section_label(p, "log-label", "Passes, newest first"))
        .child(log)
}
