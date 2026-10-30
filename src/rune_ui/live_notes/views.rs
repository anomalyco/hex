//! The story's stage: a mock screen with the recording capsule in a corner
//! and the notes floating under it, and the side column's clock and pass
//! log.

use rune::{Cursor, Cx, Drag, El, Phase as Gesture, Transition, Tx, col, el, row, text};

use super::super::controls::{DESCRIPTION, Paint, pulsing_dot, status_dot};
use super::super::lists::section_label;
use super::notes::{Note, card};
use super::pipeline::clock;
use super::script::{NoteKind, lantern};
use super::{Corner, LiveNotes, Phase, SPEEDS, Under};

const PILL_H: f32 = 36.0;
/// The capsule's column: the notes' width, and the most the pill takes.
const COLUMN_W: f32 = 300.0;
const COLUMN_GAP: f32 = 6.0;
const SCREEN_PAD: f32 = 16.0;
/// How many notes [`Under::Stack`] keeps.
const STACK: usize = 3;
/// How many [`Under::Hover`] shows under the pointer.
const HOVER_MAX: usize = 6;
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

    /// The notes under the capsule, nearest it first.
    fn floating(&self) -> Vec<usize> {
        if self.phase != Phase::Recording {
            return Vec::new();
        }
        let newest = self.notes_newest_first();
        match self.under {
            Under::Stack => newest.into_iter().take(STACK).collect(),
            Under::Fade => newest
                .into_iter()
                .filter(|&note| {
                    self.fresh.is_some_and(|(fresh, _)| fresh == note)
                        || self.open_note == Some(note)
                })
                .collect(),
            Under::Hover if self.hovered => newest.into_iter().take(HOVER_MAX).collect(),
            Under::Hover => Vec::new(),
        }
    }
}

fn toggle(s: &mut LiveNotes, note: usize) {
    s.take_over();
    s.open_note = (s.open_note != Some(note)).then_some(note);
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
    let mut under: Vec<El> = s
        .floating()
        .into_iter()
        .enumerate()
        .map(|(age, id)| {
            let note = Note::from_script(lantern(), id);
            // Older notes recede into the screen.
            let ink = match age {
                0 => p.text(),
                1 => p.muted(),
                _ => p.faint(),
            };
            let enter = Transition::opacity().combined(Transition::offset(
                0.0,
                if s.corner.bottom { 8.0 } else { -8.0 },
            ));
            card(p, &note, s.open_note == Some(id), ink)
                .key(format!("note{id}"))
                .on_press(move |s: &mut LiveNotes| toggle(s, id))
                .transition(Transition::asymmetric(enter, Transition::opacity()))
        })
        .collect();
    if s.phase == Phase::Done {
        under.push(summary(s, p));
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
        .offset(s.drag.0, s.drag.1)
        .on_hover_change(|s: &mut LiveNotes, inside, _| s.hovered = inside);
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

/// Dot, title, and on the right the elapsed time; under the pointer a stop
/// button joins it. The title is keyed by its words, so a new chapter blurs
/// in over the old.
fn pill(s: &LiveNotes, p: Paint) -> El {
    let c = &p.k.c;
    let (dot, title, trailing) = match s.phase {
        Phase::Recording => (
            pulsing_dot(p.danger()),
            s.current_chapter().unwrap_or(s.base_title()),
            Some(clock(s.clock_ms)),
        ),
        Phase::Transcribing | Phase::Writing => (pulsing_dot(p.muted()), "Finishing notes…", None),
        Phase::Done => (status_dot(p.success()), s.base_title(), None),
    };
    let mut pill = row()
        .key("pill")
        .h(PILL_H)
        .max_w(COLUMN_W)
        .gap(10.0)
        .pl(14.0)
        .pr(if s.hovered && s.phase == Phase::Recording {
            7.0
        } else {
            14.0
        })
        .center_y()
        .radius(999.0)
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .cursor(Cursor::Grab)
        .child(dot)
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
    if let Some(trailing) = trailing {
        pill = pill.child(
            text(trailing)
                .key("clock")
                .font(p.k.ty.mono)
                .color(p.faint()),
        );
    }
    if s.hovered && s.phase == Phase::Recording {
        pill = pill.child(stop(p));
    }
    pill
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
    let what = match s.under {
        Under::Stack => "The three newest notes stay under the capsule.",
        Under::Fade => "Each new note shows for a few seconds, then goes.",
        Under::Hover => "Nothing shows until the pointer is on the capsule.",
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
            text("Drag the capsule to any corner. Press a note for what was said.")
                .key("how")
                .font(DESCRIPTION)
                .color(p.faint())
                .wrap(),
        )
        .child(el().key("rule").h(p.hair).w_full().fill(p.k.c.border_weak))
        .child(section_label(p, "log-label", "Passes, newest first"))
        .child(log)
}
