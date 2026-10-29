//! The story's stage: a screen corner with the recording capsule over the
//! Meetings pane, and the side column's clock and pass log.

use rune::{Cursor, Cx, El, ScrollSpec, Spring, Transition, col, el, row, text};
use rune_kit::{Button, ButtonVariant, IconButton, IconButtonVariant};

use super::super::controls::{DESCRIPTION, Paint, PushSize, pulsing_dot, status_dot};
use super::super::lists::{detail_header, detail_title, section_label};
use super::super::meetings::{Status, Turn, status_tag, turn_bubble};
use super::outline::{Density, ExcerptLine, Note, Outline, Section, kind_label, note_row, outline};
use super::pipeline::clock;
use super::script::{NoteKind, Script, lantern};
use super::{LiveNotes, Phase, SPEEDS};

/// The capsule, as `meeting_capsules` sizes it: compact at rest, open
/// under the pointer.
const PILL_H: f32 = 40.0;
const PILL_COMPACT_W: f32 = 244.0;
/// Room for the chevron beside the compact pill's lines.
const PILL_COMPACT_CHEVRON_W: f32 = 284.0;
const PILL_OPEN_W: f32 = 632.0;
const PILL_OPEN: Spring = Spring::new(0.25, 0.0);
const PILL_CLOSE: Spring = Spring::new(0.2, 0.0);
/// The finishing and ready capsules.
const RESTING_W: f32 = 420.0;
/// The notes under the pill: as wide as the compact pill's corner allows,
/// and at most this tall before they scroll.
const NOTES_W: f32 = 400.0;
const NOTES_MAX_H: f32 = 260.0;
const SCREEN_LABEL_H: f32 = 16.0;
const NOTES_GAP: f32 = 6.0;
const SCREEN_PAD: f32 = 12.0;
const SIDE_W: f32 = 240.0;

impl LiveNotes {
    /// The provisional title once a pass has named something, the call's
    /// before; the generated one once the notes are written.
    fn base_title(&self) -> &'static str {
        if self.phase == Phase::Done || self.pipeline.passes > 0 {
            "Lantern design sync"
        } else {
            lantern().base_title
        }
    }

    /// The title the pane and the ready capsule show: live chapters' rule,
    /// the base plus up to three of the longest chapters once there are two.
    fn display_title(&self) -> String {
        let script = lantern();
        let chapters = &self.pipeline.chapters;
        let base = self.base_title();
        if chapters.len() < 2 {
            return base.into();
        }
        let end = if self.phase == Phase::Recording {
            self.clock_ms
        } else {
            script.end_ms()
        };
        let mut ranked: Vec<(u64, usize)> = chapters
            .iter()
            .enumerate()
            .map(|(index, chapter)| {
                let until = chapters.get(index + 1).map_or(end, |next| next.start_ms);
                (until.saturating_sub(chapter.start_ms), index)
            })
            .collect();
        ranked.sort_by_key(|&(length, index)| (std::cmp::Reverse(length), index));
        let mut picked: Vec<usize> = ranked.into_iter().take(3).map(|(_, i)| i).collect();
        picked.sort_unstable();
        let mut names = Vec::new();
        for index in picked {
            let name = script.topics[chapters[index].topic].name;
            let length = base.len() + names.iter().map(|n: &&str| n.len() + 2).sum::<usize>();
            if length + 2 + name.len() <= 100 {
                names.push(name);
            }
        }
        format!("{base}: {}", names.join(", "))
    }

    fn outline(&self) -> Outline {
        let script = lantern();
        let mut sections: Vec<Section> = self
            .pipeline
            .chapters
            .iter()
            .map(|chapter| {
                let topic = &script.topics[chapter.topic];
                Section {
                    title: topic.name.into(),
                    summary: topic.summary.into(),
                    start_ms: chapter.start_ms,
                    notes: Vec::new(),
                }
            })
            .collect();
        for &id in &self.pipeline.found {
            if let Some(section) = self.pipeline.chapter_at(script.note_at(id)) {
                sections[section].notes.push(note(script, id));
            }
        }
        for section in &mut sections {
            section.notes.sort_by_key(|note| note.at_ms);
        }
        Outline {
            sections,
            maybe_next: self
                .pipeline
                .pending
                .filter(|_| self.phase == Phase::Recording)
                .map(|pending| script.topics[pending.topic].name.into()),
        }
    }

    fn current_chapter(&self) -> Option<&'static str> {
        let chapter = self.pipeline.chapters.last()?;
        Some(lantern().topics[chapter.topic].name)
    }
}

fn note(script: &Script, id: usize) -> Note {
    let spec = &script.notes[id];
    Note {
        id,
        kind: spec.kind,
        text: spec.text.into(),
        owner: spec.owner.map(Into::into),
        at_ms: script.note_at(id),
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

fn toggle(s: &mut LiveNotes, note: usize) {
    s.take_over();
    s.open_note = (s.open_note != Some(note)).then_some(note);
}

pub(super) fn stage(s: &LiveNotes, cx: &mut Cx<LiveNotes>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let c = &p.k.c;
    let outline = s.outline();
    col()
        .key("live-notes")
        .w_full()
        .h_full()
        .fill(c.bg_deep)
        .padding(12.0)
        .gap(12.0)
        .child(screen(s, p, &outline))
        .child(pane(s, p, &outline))
}

/// The screen's corner the capsule sits in, sized for its notes when it
/// has them so the pane below holds still while they open.
fn screen(s: &LiveNotes, p: Paint, outline: &Outline) -> El {
    let notes = s.variant.capsule_outline;
    let height = PILL_H
        + 2.0 * SCREEN_PAD
        + SCREEN_LABEL_H
        + if notes { NOTES_GAP + NOTES_MAX_H } else { 0.0 };
    // Each in a row of its own so it can give up width on a narrow stage.
    let corner = |key: &str, child: El| row().key(key).w_full().justify_end().child(child.shrink());
    let mut screen = col()
        .key("screen")
        .w_full()
        .h(height)
        .padding(SCREEN_PAD)
        .gap(NOTES_GAP)
        .radius(p.k.radius.lg)
        .border(p.hair, p.k.c.border_weak)
        .child(corner("capsule-corner", capsule(s, p)));
    if notes && s.outline_open && s.phase == Phase::Recording {
        screen = screen.child(corner("notes-corner", notes_panel(s, p, outline)));
    }
    screen.child(el().key("spacer").grow(1.0)).child(
        text("Your screen")
            .key("screen-label")
            .font(p.k.ty.small)
            .color(p.faint()),
    )
}

fn capsule(s: &LiveNotes, p: Paint) -> El {
    match s.phase {
        Phase::Recording => recording(s, p),
        Phase::Transcribing | Phase::Writing => resting(
            p,
            pulsing_dot(p.muted()),
            "Finishing notes…",
            &s.display_title(),
        ),
        Phase::Done => resting(p, status_dot(p.success()), &s.display_title(), &counts(s)),
    }
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
    let mut parts = vec!["Notes ready".to_string()];
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

/// The recording capsule: the chapter as its title, crossfading when a new
/// one opens; the elapsed time under it, or for a moment the note just
/// heard; and the chevron for the notes. Under the pointer it opens to its
/// actions, as the real capsule does.
fn recording(s: &LiveNotes, p: Paint) -> El {
    let elapsed = clock(s.clock_ms);
    let base = s.base_title();
    let title = s.current_chapter().unwrap_or(base);
    let subtitle = match s.ticker {
        Some((note, _)) => {
            let spec = &lantern().notes[note];
            match kind_label(spec.kind) {
                Some(label) => format!("{label} · {}", spec.text),
                None => spec.text.to_string(),
            }
        }
        None if s.current_chapter().is_some() => format!("{elapsed} · {base}"),
        None => elapsed,
    };
    let open = s.hovered;
    let chevron = s.variant.capsule_outline.then(|| {
        let up = s.outline_open;
        IconButton::new(if up {
            p.k.glyph.chevron_up
        } else {
            p.k.glyph.chevron_down
        })
        .variant(IconButtonVariant::Ghost)
        .expanded(up)
        .label(if up { "Hide notes" } else { "Show notes" })
        .on_press(|s: &mut LiveNotes| {
            s.take_over();
            s.outline_open = !s.outline_open;
        })
        .build(p.k)
        .key("notes-toggle")
    });
    let actions = open.then(|| {
        row()
            .key("actions")
            .gap(8.0)
            .child(action(p, "copy-new", "Copy new", false, |_| {}))
            .child(action(p, "copy-all", "Copy all", false, |_| {}))
            .child(action(p, "open", "Open", false, |_| {}))
            .child(action(p, "stop", "Stop", true, |s| {
                s.take_over();
                s.stop();
            }))
    });
    let width = match (open, chevron.is_some()) {
        (true, _) => PILL_OPEN_W,
        (false, true) => PILL_COMPACT_CHEVRON_W,
        (false, false) => PILL_COMPACT_W,
    };
    let subtitle_key = s
        .ticker
        .map_or_else(|| "clock".to_string(), |(note, _)| format!("note{note}"));
    body(p, pulsing_dot(p.danger()), title, &subtitle, subtitle_key)
        .pr(if chevron.is_some() { 4.0 } else { 8.0 })
        .children(actions)
        .children(chevron)
        .w(width)
        .clip_rounded(999.0)
        .cursor(Cursor::Grab)
        .motion(if open { PILL_OPEN } else { PILL_CLOSE })
        .on_hover_change(|s: &mut LiveNotes, inside, _| s.hovered = inside)
}

fn resting(p: Paint, dot: El, title: &str, subtitle: &str) -> El {
    body(p, dot, title, subtitle, format!("subtitle:{subtitle}"))
        .w(RESTING_W)
        .transition(Transition::opacity())
}

fn action(p: Paint, key: &str, label: &str, primary: bool, press: fn(&mut LiveNotes)) -> El {
    Button::new(label)
        .variant(if primary {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::Neutral
        })
        .on_press(press)
        .build_push(p.k)
        .key(key)
}

/// The pill: dot, title, and subtitle. The title is keyed by its words and
/// the subtitle by `subtitle_key`, so a new chapter or note blurs in over
/// the old while the ticking clock keeps one key.
fn body(p: Paint, dot: El, title: &str, subtitle: &str, subtitle_key: String) -> El {
    let c = &p.k.c;
    row()
        .key("pill")
        .h(PILL_H)
        .gap(12.0)
        .pl(16.0)
        .center_y()
        .radius(999.0)
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .child(dot)
        .child(
            col()
                .key("lines")
                .grow(1.0)
                .min_w(0.0)
                .child(
                    text(title.to_owned())
                        .key(format!("title:{title}"))
                        .truncate()
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .transition(Transition::blur_replace()),
                )
                .child(
                    text(subtitle.to_owned())
                        .key(subtitle_key)
                        .truncate()
                        .font(p.k.ty.small)
                        .color(p.faint())
                        .transition(Transition::blur_replace()),
                ),
        )
}

/// The running outline under the pill, newest at the bottom and kept in
/// view.
fn notes_panel(s: &LiveNotes, p: Paint, notes: &Outline) -> El {
    let c = &p.k.c;
    let content = if notes.sections.is_empty() {
        text("Notes appear as the call goes on.")
            .key("notes-empty")
            .font(DESCRIPTION)
            .color(p.faint())
            .wrap()
    } else {
        outline(p, notes, s.open_note, true, Density::Capsule, toggle)
    };
    let mut panel = col()
        .key("notes-panel")
        .w(NOTES_W)
        .max_h(NOTES_MAX_H)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .clip_rounded(p.k.radius.xl)
        .transition(Transition::opacity().combined(Transition::offset(0.0, -6.0)))
        .child(
            col()
                .key("notes-scroll")
                .max_h(NOTES_MAX_H)
                .clip_fade(0.0, 10.0, 0.0, 10.0)
                .scroll(ScrollSpec { stick_bottom: true })
                .child(col().key("inset").px(6.0).py(8.0).child(content)),
        );
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    panel
}

/// The Meetings pane's detail for this call: title and status; after Stop
/// the summary and the to-dos; the chapters with their notes; then the
/// transcript so far.
fn pane(s: &LiveNotes, p: Paint, notes: &Outline) -> El {
    let c = &p.k.c;
    let script = lantern();
    let status = match s.phase {
        Phase::Recording => Status::Recording,
        Phase::Transcribing | Phase::Writing => Status::Transcribing,
        Phase::Done => Status::Complete,
    };
    let duration = if s.phase == Phase::Recording {
        s.clock_ms
    } else {
        script.end_ms()
    };
    let mut meta: Vec<El> = status_tag(p, status, false, true).into_iter().collect();
    meta.push(
        text(format!(
            "Today · 10:30 · {} · {}",
            clock(duration),
            script.app
        ))
        .key("meta-line")
        .font(DESCRIPTION)
        .color(p.muted())
        .truncate(),
    );
    let title = s.display_title();
    let header = detail_header(
        detail_title(p, &title).key(format!("detail-title:{title}")),
        meta,
        Vec::new(),
    );

    let mut body = col().key("detail-body").gap(16.0).pt(16.0);
    match s.phase {
        Phase::Done => {
            body = body.child(
                col()
                    .key("summary-block")
                    .gap(6.0)
                    .child(section_label(p, "summary-label", "Summary"))
                    .child(
                        text(script.summary)
                            .key("summary")
                            .font(p.k.ty.body)
                            .color(p.muted())
                            .wrap(),
                    )
                    .transition(Transition::opacity()),
            );
            body = body.child(todos(s, p, notes));
        }
        Phase::Writing => {
            body = body.child(
                text("Writing a title and summary with Apple Intelligence…")
                    .key("notes-running")
                    .font(DESCRIPTION)
                    .color(p.faint()),
            );
        }
        Phase::Recording | Phase::Transcribing => {}
    }
    let mut chapters =
        col()
            .key("chapters")
            .gap(6.0)
            .child(section_label(p, "chapters-label", "Chapters"));
    chapters = if notes.sections.is_empty() {
        chapters.child(
            text(if s.phase == Phase::Recording {
                "The first chapter and its notes come with the first pass."
            } else {
                "No chapters."
            })
            .key("chapters-empty")
            .font(DESCRIPTION)
            .color(p.faint()),
        )
    } else {
        chapters.child(outline(
            p,
            notes,
            s.open_note,
            s.phase == Phase::Recording,
            Density::Pane,
            toggle,
        ))
    };
    body = body.child(chapters);

    let recording = s.phase == Phase::Recording;
    let mut label = row()
        .key("transcript-label")
        .gap(8.0)
        .center_y()
        .child(section_label(
            p,
            "label",
            match s.phase {
                Phase::Recording => "Live transcript",
                Phase::Transcribing => "Draft transcript",
                Phase::Writing | Phase::Done => "Transcript",
            },
        ));
    if recording {
        label = label.child(pulsing_dot(p.danger()));
    }
    let mut turns = col().key("turns").gap(12.0);
    for (index, turn) in self::turns(script, if recording { s.clock_ms } else { u64::MAX })
        .iter()
        .enumerate()
    {
        turns = turns.child(turn_bubble(p, index, turn));
    }
    body = body.child(label).child(turns);

    col()
        .key("pane")
        .grow(1.0)
        .shrink()
        .min_h(0.0)
        .radius(p.k.radius.lg)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        .padding(p.hair)
        .clip_soft(0.0)
        .child(
            col()
                .key("meeting-detail")
                .grow(1.0)
                .shrink()
                .min_h(0.0)
                .px(20.0)
                .pt(16.0)
                .child(header)
                .child(super::super::scroll_body("meeting-transcript", body)),
        )
}

/// After Stop, every to-do in one place, each still opening its lines.
fn todos(s: &LiveNotes, p: Paint, notes: &Outline) -> El {
    // Inset as the chapters' notes are, so the two lists share an edge.
    let mut list = col().key("rows").gap(1.0).px(8.0);
    for note in notes
        .sections
        .iter()
        .flat_map(|section| &section.notes)
        .filter(|note| note.kind == NoteKind::Action)
    {
        let id = note.id;
        list = list.child(
            note_row(p, note, s.open_note == Some(id), Density::Pane)
                .key(format!("todo{id}"))
                .on_press(move |s: &mut LiveNotes| toggle(s, id)),
        );
    }
    col()
        .key("todos")
        .gap(6.0)
        .child(section_label(p, "todos-label", "To-dos"))
        .child(list)
        .transition(Transition::opacity())
}

/// The lines heard by `until_ms`, joined into turns as the pane joins them:
/// one speaker's lines within 3 s of each other.
fn turns(script: &Script, until_ms: u64) -> Vec<Turn> {
    let mut turns: Vec<(Turn, u64)> = Vec::new();
    for line in script.lines.iter().filter(|line| line.end_ms <= until_ms) {
        if let Some((turn, end)) = turns.last_mut()
            && turn.mine == line.mine
            && line.start_ms <= *end + 3_000
        {
            turn.text.push(' ');
            turn.text.push_str(&line.text);
            *end = line.end_ms;
            continue;
        }
        turns.push((
            Turn {
                mine: line.mine,
                start_ms: line.start_ms,
                text: line.text.clone(),
                pause_before_ms: None,
            },
            line.end_ms,
        ));
    }
    turns.into_iter().map(|(turn, _)| turn).collect()
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
    let what = match (s.variant.capsule_outline, s.variant.interval_ms) {
        (true, super::NOTES_INTERVAL_MS) => {
            "A pass every 75 s finds chapters and notes; the capsule's chevron opens them."
        }
        (false, _) => {
            "Same passes; the capsule only names the chapter and shows each new note for a moment."
        }
        _ => {
            "Live chapters' cadence: a pass every 3 min. Chapters come minutes late, and the caves never get one."
        }
    };
    let mut log = col().key("log").gap(4.0);
    for (index, entry) in s.pipeline.log.iter().enumerate().rev().take(10) {
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
        .child(el().key("rule").h(p.hair).w_full().fill(p.k.c.border_weak))
        .child(section_label(p, "log-label", "Passes, newest first"))
        .child(log)
}
