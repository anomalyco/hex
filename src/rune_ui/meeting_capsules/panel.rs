//! The recording pill and the ready panel: the listening field and timer at
//! rest, then Stop and the two copies, and the finished meeting's title
//! and summary over the same copies.

use super::super::notes_writer::NotesWriter;
use super::*;
use rune::{Cursor, El, Icon, SpringPair, Transition, Tx, col, el, icon, row, text};
use rune_theme_opencode::icons;

mod finished;
mod peek;

use finished::discard_confirm;
pub(super) use finished::ready;
use finished::ready_height;
#[cfg(test)]
pub(super) use peek::{LINK_FOOT, PEEK_FOOT, WORDS_FOOT};

/// What a recording says beside its meter, in place of its title.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Notice {
    /// Hex leaves the microphone out of the recording (Labs › Mute the
    /// microphone while recording).
    Muted,
    /// The microphone is digitally silent (#1518).
    Silent,
    /// For its first seconds, an automatic recording says so.
    Automatic,
}

impl Notice {
    fn text(self) -> &'static str {
        match self {
            Notice::Muted => tr("Microphone muted"),
            Notice::Silent => tr("Microphone is silent"),
            Notice::Automatic => tr("Started automatically"),
        }
    }

    fn color(self, p: Paint) -> rune::Color {
        match self {
            Notice::Muted => p.muted(),
            Notice::Silent => p.warning(),
            Notice::Automatic => p.faint(),
        }
    }
}

/// A capsule ✕'s hit box: the open recording's discard and the ready
/// panel's close.
const CLOSE_BOX: f32 = CAPSULE.close;
/// The panel's icon spine: the centre of its rows' 16 pt glyphs, where every
/// ✕ is centred too.
pub(super) const SPINE: f32 = CAPSULE.icon_spine();
/// How far the discard ✕ hangs past the header's inset, so its centre lands
/// on the spine.
const CLOSE_NUDGE: f32 = REST_PX + CLOSE_BOX / 2.0 - SPINE;
/// What the discard ✕ takes from the open header's field, on its free side.
pub(super) const DISCARD_ROOM: f32 = CLOSE_BOX - CLOSE_NUDGE;
/// The panel while it asks "Discard recording?": its header, the question
/// and its consequence, and Keep and Discard (§2.4 of the design spec).
pub(super) const CONFIRM_HEIGHT: f32 = REST_HEIGHT + CONFIRM_BODY;
/// The question under the header: 4 + title 20 + 2 + detail 16 + 12 +
/// buttons 28 + 8.
const CONFIRM_BODY: f32 = 4.0 + 20.0 + 2.0 + 16.0 + 12.0 + CONFIRM_BUTTON + CONFIRM_INSET;
const CONFIRM_BUTTON: f32 = 28.0;
/// Keep and Discard sit this far in from the panel's sides and its edge.
const CONFIRM_INSET: f32 = CAPSULE.button_inset;
/// The ready panel's title band: its link's padding round the title and length.
const TITLE_PY: f32 = 8.0;
/// Clear space under the title link's hover fill.
const TITLE_CLEAR: f32 = 8.0;
/// Between the summary and the copies.
const SUMMARY_GAP: f32 = 12.0;

pub(super) fn recording_notice(shown: &Recording) -> Option<Notice> {
    // The microphone's own switch says more than the silence warning that
    // would otherwise stand in its place while Hex leaves it out.
    if crate::meeting::microphone_muted() {
        Some(Notice::Muted)
    } else if shown.warning.is_some() {
        Some(Notice::Silent)
    } else if shown.automatic && shown.started.elapsed() < AUTOMATIC_LABEL {
        Some(Notice::Automatic)
    } else {
        None
    }
}

/// Whether Apple Intelligence writes meeting notes on this computer, which
/// the Meetings pane reads from its platform host.
fn apple_intelligence_here() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::apple_intelligence::availability().is_available()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// The ready panel's line under its title, and its colour: the summary, the
/// summary still being written by whichever writer the Meetings pane names,
/// or why the Markdown wasn't saved.
pub(super) fn ready_summary(manifest: &MeetingManifest) -> Option<(String, bool)> {
    if meeting::notes_in_progress(&manifest.id) {
        let writer = NotesWriter::for_choice(
            crate::app_settings::meeting_summaries(),
            apple_intelligence_here(),
        );
        return Some((writer.running_label().to_owned(), false));
    }
    if let Some(summary) = manifest
        .notes
        .as_ref()
        .map(|notes| notes.summary.trim())
        .filter(|summary| !summary.is_empty())
    {
        return Some((summary.to_owned(), true));
    }
    manifest
        .export_error
        .as_ref()
        .map(|error| (tr_fill("Markdown not saved: {}", error), false))
}

fn copied_hint(p: Paint) -> El {
    row()
        .key("hint")
        .gap(4.0)
        .center_y()
        .child(icon(&icons::CHECK, 12.0).color(p.success()))
        .child(
            text(tr("Copied").to_owned())
                .font(p.k.ty.small)
                .color(p.success()),
        )
}

/// One action across its whole visible row, including the trailing whitespace.
#[allow(clippy::too_many_arguments)]
fn panel_row(
    p: Paint,
    key: &'static str,
    glyph: &'static Icon,
    label: &str,
    hint: Option<El>,
    first: bool,
    enabled: bool,
    presence: f32,
    press: impl Fn(&mut Capsule, &mut Tx) + 'static,
) -> El {
    let (ink, font) = if first {
        (p.text(), p.k.ty.ui_medium)
    } else {
        (p.muted(), p.k.ty.ui)
    };
    let row_el = row()
        .key(key)
        .h(ROW_HEIGHT)
        .px(ROW_PX)
        .gap(8.0)
        .center_y()
        .radius(ROW_RADIUS)
        .role("button")
        // An enabled row is a button; a disabled one only says what it would
        // do, and keeps the arrow.
        .cursor(if enabled {
            Cursor::Pointer
        } else {
            Cursor::Default
        })
        .focus_ring(2., -2., p.k.c.border_focus)
        // The surface steps each row's presence; nothing eases it twice.
        .transition(Transition::none())
        .label(label)
        .child(icon(glyph, 16.0).key("glyph").color(ink))
        .child(
            text(label.to_owned())
                .key("label")
                .font(font)
                .color(ink)
                .truncate()
                .grow(1.0)
                .min_w(0.0)
                .a11y_hidden(),
        )
        .children(hint);
    // No row is filled at rest: the lead action leads by its ink and weight,
    // and every row fills only under the pointer.
    let row_el = row_el.hover(p.k.c.hover);
    let row_el = if first { row_el.mb(ROW_GAP) } else { row_el };
    if enabled {
        pressed(row_el, press).opacity(presence)
    } else {
        row_el.disabled(true).opacity(0.5 * presence)
    }
}

/// Makes a wide row a button: pressed, unless the press swiped across it.
fn pressed(row_el: El, press: impl Fn(&mut Capsule, &mut Tx) + 'static) -> El {
    row_el
        .focusable(true)
        .no_window_drag()
        // Rune buttons accept a release anywhere inside their rectangle.
        // Track press travel only: no window move, drag item or grab cursor.
        .on_drag(|c: &mut Capsule, d, tx| {
            if d.phase == Phase::Down {
                c.live.row_swiped = false;
            }
            c.live.row_swiped |= d.dx.hypot(d.dy) > 4.;
            if d.phase == Phase::Up
                && (d.cancelled() || d.x < 0. || d.x >= d.w || d.y < 0. || d.y >= d.h)
            {
                c.live.row_swiped = false;
            }
            tx.live_only();
        })
        .on_press_tx(move |c: &mut Capsule, tx| {
            if !std::mem::take(&mut c.live.row_swiped) {
                press(c, tx);
            }
        })
}

/// Copy agent prompt and Copy transcript, for the recording or the ready meeting.
fn copy_rows(p: Paint, capsule: &Capsule, recording: bool) -> Vec<El> {
    let live = &capsule.live;
    let presence = |row| capsule.action_presence(row).opacity;
    let copied = |kind| live.copied(kind).then(|| copied_hint(p));
    vec![
        panel_row(
            p,
            "copy-prompt",
            &icons::OUTLINE_CONSOLE,
            tr("Copy agent prompt"),
            copied(LiveCopy::Prompt),
            true,
            true,
            presence(1),
            |c, tx| c.copy_prompt(tx),
        ),
        panel_row(
            p,
            "copy-all",
            &icons::OUTLINE_COPY,
            tr("Copy transcript"),
            copied(LiveCopy::All),
            false,
            !recording || !live.entries.is_empty(),
            presence(2),
            |c, tx| c.copy_all(tx),
        ),
    ]
}

/// The panel's rounded body; only it takes the pointer, not the window's
/// margin around it.
fn panel_body(p: Paint, title: &str) -> El {
    let mut body = col()
        .key("pill")
        .role("status")
        .label(title)
        .w(PANEL_WIDTH)
        .radius(PANEL_RADIUS)
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_base)
        .clip_rounded(PANEL_RADIUS)
        .hit();
    for shadow in &p.k.elevation.overlay {
        body = body.box_shadow(*shadow);
    }
    body
}

/// A line replaced in place leaves sooner than it arrives, even under
/// Reduce Motion.
const REDUCED_EXIT: f32 = 0.15;

fn replace(c: &Capsule) -> Transition {
    let reduced = c.reduced.get();
    // A whole line replaced in place softens; under Reduce Motion it only fades.
    let look = if reduced {
        Transition::opacity()
    } else {
        Transition::blur(2.0)
    };
    let (enter, exit) = if reduced {
        (stage::REDUCED, Spring::new(REDUCED_EXIT, 0.))
    } else {
        (stage::ENTER, stage::EXIT)
    };
    look.springs(SpringPair::new(enter, exit))
}

/// The finished meeting stays where its pill was, on that display too; other
/// panels take the saved corner.
pub(super) fn show_anchor(
    kind: &Kind,
    last: Option<&(String, screen::Anchor)>,
    saved: impl FnOnce() -> screen::Anchor,
) -> screen::Anchor {
    if let Kind::Ready(manifest) = kind {
        return last
            .filter(|(id, _)| *id == manifest.id)
            .map(|(_, anchor)| anchor.clone())
            .unwrap_or_else(saved);
    }
    saved()
}

/// Places a capsule's body in its window's corner.
fn in_corner(capsule: &Capsule, body: El) -> El {
    let column = col()
        .key("capsule")
        .w_full()
        .h_full()
        .padding(CAPSULE_INSET);
    let column = if capsule.corner.is_right() {
        column.items_end()
    } else {
        column.items_start()
    };
    if capsule.corner.is_top() {
        column.justify_start()
    } else {
        column.justify_end()
    }
    .child(body)
}

pub(super) fn finalizing(p: Paint, capsule: &Capsule, duration_ms: u64) -> El {
    result(
        p,
        capsule,
        tr("Transcribing…").to_owned(),
        Some(duration_ms),
        None,
    )
}

/// The finished meeting's ✕, in its header.
const CLOSE_WIDTH: f32 = 32.0;

fn result(
    p: Paint,
    capsule: &Capsule,
    title: String,
    duration: Option<u64>,
    manifest: Option<&MeetingManifest>,
) -> El {
    let top = capsule.corner.is_top();
    let right = capsule.corner.is_right();
    // The words drawn follow the title a few frames behind: the old ones
    // go before the new ones arrive.
    let reduced = capsule.reduced.get();
    let (words, strength, soft) = capsule.surface.title.line(title.clone(), reduced);
    let label = title_label(p, capsule, &words)
        .opacity(strength)
        .opacity_motion(Anim::INSTANT)
        .blur(soft)
        .blur_motion(Anim::INSTANT);
    let field = field_width(capsule);
    let clock = duration.map(|duration| {
        row()
            .key("meter")
            .h(REST_HEIGHT)
            .gap(METER_GAP)
            .center_y()
            .transition(Transition::none())
            // The field cools after capture ends; it never cuts. It sits
            // beside the time without moving it.
            .children(capsule.waveform.is_some().then(|| {
                waveform::slot(field).absolute(
                    -(field + METER_GAP),
                    (REST_HEIGHT - crate::rune_hud::dot_scope::PLATE_HEIGHT) / 2.,
                )
            }))
            // The recording's clock, so the time is the same node. Capture
            // has ended: the dot fades where it was and the time stays put.
            .child(
                row()
                    .key("clock")
                    .gap(METER_GAP)
                    .center_y()
                    .child(rec_dot(p).opacity(0.))
                    .child(
                        text(meeting::format_duration(duration))
                            .key("elapsed")
                            .w(timer_text_width(duration / 60_000))
                            .align(Align::End)
                            .font(p.clock())
                            .color(p.muted()),
                    ),
            )
    });
    let close = row()
        .key("close-slot")
        .w(CLOSE_WIDTH)
        .h(REST_HEIGHT)
        .center()
        .children(manifest.map(|_| dismiss_button(p).transition(replace(capsule))));
    let meta = meter_band();
    let space = el().key("space").grow(1.);
    // ✕ sits before the time in every corner. In a left corner the time
    // stays after the field's free end, wherever the wall has left it.
    let meta = match (right, clock) {
        (false, Some(clock)) => {
            let lead = el().key("field-room").w(field + METER_GAP - CLOSE_WIDTH);
            meta.child(lead).child(close).child(clock).child(space)
        }
        (_, clock) => meta.child(space).child(close).children(clock),
    };
    // Stop leaves from where it was pressed while the title arrives.
    let stop = capsule.stage.presence(stage::Layer::Action(0));
    let ghost = stop.shows().then(|| {
        panel_row(
            p,
            "stop",
            &icons::STOP,
            tr("Stopping…"),
            None,
            false,
            false,
            stop.opacity,
            |_, _| {},
        )
        .absolute(PANEL_PAD, 0.)
        .w(PANEL_WIDTH - 2. * PANEL_PAD)
        // The stage steps it; eased again it would outstay the title's wait.
        .opacity(stop.opacity)
        .opacity_motion(Anim::INSTANT)
        .blur(if capsule.reduced.get() {
            0.
        } else {
            2. * (1. - stop.opacity)
        })
        .blur_motion(Anim::INSTANT)
        .inert()
        .a11y_hidden()
    });
    let label = stop_slot().child(label).children(ghost);
    let header = bands(top, meta, Some(label));
    let summary = manifest.and_then(ready_summary).map(|now| {
        let ((summary, written), strength, soft) = capsule.surface.summary.line(now, reduced);
        text(summary)
            .key("summary")
            .opacity(strength)
            .opacity_motion(Anim::INSTANT)
            .blur(soft)
            .blur_motion(Anim::INSTANT)
            // The words wipe on behind the growing edge, soft at their front.
            .transition(if capsule.reduced.get() {
                replace(capsule)
            } else {
                Transition::reveal(if top {
                    rune::Edge::Top
                } else {
                    rune::Edge::Bottom
                })
                // Slower than the panel grows, so the edge is always ahead
                // of the words and never cuts them.
                .springs(SpringPair::new(Spring::new(0.6, 0.0), stage::EXIT))
            })
            .font(p.k.ty.text_small)
            .color(if written { p.muted() } else { p.faint() })
            .max_lines(SUMMARY_LINES)
            .mx(ROW_PX + PANEL_PAD)
            .mt(2.0)
            .mb(ROW_PX)
    });
    // Reveal the words within the growing surface, never over the copy buttons.
    let summary = col().key("summary-lane").clip_rounded(0.).children(summary);
    let rows = col()
        .key("rows")
        .px(PANEL_PAD)
        .children(copy_rows(p, capsule, false));
    // The lead copy is as far from the title's row as from the next copy.
    let rows = if top {
        rows.pt(ROW_GAP).pb(PANEL_PAD)
    } else {
        rows.pt(PANEL_PAD).pb(ROW_GAP)
    };
    // The copies dim and return on their own spring, wherever the surface is.
    let rows = if manifest.is_none() {
        rows.disabled(true).opacity(0.48)
    } else {
        rows
    }
    .motion(RECORDING_OPEN);
    let contents = col()
        .key("panel-contents")
        .w(PANEL_WIDTH)
        .transition(Transition::none());
    // New notes grow from the free edge, not between the pointer and its
    // copy row. A bottom corner mirrors the bands, not their reading order.
    let contents = if top {
        contents.child(header).child(rows).child(summary)
    } else {
        contents.child(summary).child(rows).child(header)
    };
    let body = panel_body(p, &title);
    // A surface still on its way here keeps its own motion until it lands;
    // from then on the panel's layout grows it for a title or a summary.
    let arriving = capsule
        .stage
        .travelling()
        .then(|| capsule.stage.outline())
        .flatten();
    let body = match arriving {
        Some(o) => {
            let body = body
                .size(stage::grid(o.w), stage::grid(o.h))
                .radius(o.radius)
                .clip_rounded(o.radius)
                .motion(Spring::NONE);
            let body = if right {
                body.items_end()
            } else {
                body.items_start()
            };
            if top {
                body.justify_start()
            } else {
                body.justify_end()
            }
        }
        None => body.motion(RECORDING_OPEN),
    };
    // The same nodes the recording drew, so nothing that stays is mounted twice.
    let body = body
        .child(
            el().key("veil")
                .transition(Transition::none())
                .child(contents),
        )
        .on_hover_change(|c: &mut Capsule, inside, _| c.set_hovered(inside));
    in_corner(capsule, presence::anchored(capsule, body))
}

/// A fast worker can report every phase within a few frames. Keep one text
/// run instead of leaving several fading titles in the same place. Only its
/// slot moves: the words wrap at their destination width from the first frame.
fn title_label(p: Paint, c: &Capsule, title: &str) -> El {
    let label = text(title.to_owned())
        .key("title")
        .w_full()
        .motion(Spring::NONE)
        .font(p.strong())
        .color(p.text())
        .wrap_anywhere()
        .max_lines(TITLE_LINES)
        .a11y_hidden();
    col()
        .key("caption")
        .min_w(0.)
        .min_h(ROW_HEIGHT)
        .py(TITLE_INSET)
        .px(PANEL_PAD + ROW_PX)
        .justify_center()
        .transition(replace(c))
        .child(label)
}

/// Only the meter band owns the window drag, never an ancestor of an action
/// row, so the grab hand stops where the rows' pointer begins.
fn meter_band() -> El {
    row()
        .key("meta")
        .h(REST_HEIGHT)
        .px(REST_PX)
        .center_y()
        .transition(Transition::none())
        .cursor(Cursor::Grab)
        .on_drag(|c: &mut Capsule, d, tx| c.drag(d, tx))
}

fn bands(top: bool, meta: El, action: Option<El>) -> El {
    let header = col()
        .key("header")
        .w(PANEL_WIDTH)
        .transition(Transition::none());
    if top {
        header.child(meta).children(action)
    } else {
        header.children(action).child(meta)
    }
}

/// Width reserved for the timer's digits after `minutes`: `TIMER_TEXT_WIDTH`
/// for five characters (`00:00`), plus 8 pt for each extra minute digit.
fn timer_text_width(minutes: u64) -> f32 {
    let extra = minutes.max(1).ilog10().saturating_sub(1);
    TIMER_TEXT_WIDTH + extra as f32 * 8.
}

/// The time's width after `minutes`, dot and gap included.
fn timer_width(minutes: u64) -> f32 {
    timer_text_width(minutes) + REC_DOT + METER_GAP
}

/// Only grow the compact header for a notice or another minute digit.
pub(super) fn rest_width(shown: &Recording) -> f32 {
    if recording_notice(shown).is_some() {
        return PANEL_WIDTH;
    }
    let minutes = shown.started.elapsed().as_secs() / 60;
    (REST_WIDTH - TIMER_WIDTH + timer_width(minutes)).min(PANEL_WIDTH)
}

/// The minutes on the time the capsule draws.
fn minutes_shown(capsule: &Capsule) -> u64 {
    match capsule.drawn() {
        Some(State::Recording {
            recording: shown, ..
        }) => shown.started.elapsed().as_secs() / 60,
        Some(State::Finalizing { duration_ms, .. }) => duration_ms / 60_000,
        Some(State::Ready { manifest, .. }) => manifest.duration_ms.unwrap_or(0) / 60_000,
        _ => 0,
    }
}

/// The header's room for the field in a surface `width` wide: from the
/// rows' inset to the time. Another digit on the time takes three columns.
fn field_room(capsule: &Capsule, width: f32) -> f32 {
    let room = width - 2. * REST_PX - METER_GAP - timer_width(minutes_shown(capsule));
    room - discard_room(capsule).min((room - waveform::WIDTH).max(0.))
}

/// The room the discard ✕ holds in the header this frame: none at rest, all
/// of it once the surface is a row taller, so the field's free end and a
/// left corner's time ride the wall with the ✕ beyond them. Finishing keeps
/// it, so nothing in the header moves at Stop. It never takes the resting
/// field's own width (see [`field_room`]).
fn discard_room(capsule: &Capsule) -> f32 {
    let open = match capsule.drawn() {
        Some(State::Recording {
            hovered, stopping, ..
        }) => *hovered || *stopping,
        Some(State::Finalizing { .. } | State::Ready { .. }) => true,
        _ => return 0.,
    };
    match capsule.stage.outline() {
        Some(o) => DISCARD_ROOM * ((stage::grid(o.h) - REST_HEIGHT) / ROW_HEIGHT).clamp(0., 1.),
        None if open => DISCARD_ROOM,
        None => 0.,
    }
}

/// The soft edge the widening field wipes the header's notice with, pt, and
/// how far across the header the field is when the notice has gone.
const NOTICE_WIPE: f32 = 12.0;
const NOTICE_GONE: f32 = 0.5;

/// The fewest points the field is ever drawn across.
const FIELD_LEAST: f32 = 40.0;

/// How wide the listening field wants to be: its resting width, and the
/// whole header once the panel is open.
pub(super) fn field_wanted(capsule: &Capsule) -> f32 {
    let open = match capsule.drawn() {
        Some(State::Recording {
            hovered, stopping, ..
        }) => *hovered || *stopping,
        Some(State::Finalizing { .. } | State::Ready { .. }) => true,
        _ => false,
    };
    if open {
        let room = PANEL_WIDTH - 2. * REST_PX - METER_GAP - timer_width(minutes_shown(capsule));
        (room - DISCARD_ROOM).max(waveform::WIDTH)
    } else {
        waveform::WIDTH
    }
}

/// How wide the listening field is this frame. It opens on the outline's
/// own width spring and never takes more room than a wall still on its way
/// leaves it, so what ends the header is the wall's motion, 14 pt inside
/// it: the field's free end on the right, the time beyond it on the left.
pub(super) fn field_width(capsule: &Capsule) -> f32 {
    let Some(outline) = capsule.stage.outline() else {
        return field_wanted(capsule);
    };
    // A digit that arrives on the time takes its room at once, before the
    // pill has grown for it: the field gives up columns, never the inset.
    stage::grid(capsule.stage.field())
        .min(field_room(capsule, stage::grid(outline.w)))
        .max(FIELD_LEAST)
}

/// The recording's meter: the field and the time it has run.
pub(super) fn meter(p: Paint, capsule: &Capsule, shown: &Recording) -> El {
    // A recording that has left the surface stops counting as it goes.
    let run = capsule
        .surface
        .clock()
        .saturating_duration_since(shown.started);
    let elapsed = meeting::format_duration(run.as_millis() as u64);
    let minutes = run.as_secs() / 60;
    row()
        .key("meter")
        .h(REST_HEIGHT)
        .gap(METER_GAP)
        .center_y()
        // The same meter in every state: it never arrives a second time.
        .transition(Transition::none())
        .child(waveform::slot(field_width(capsule)))
        .child(
            row()
                .key("clock")
                .gap(METER_GAP)
                .center_y()
                .child(rec_dot(p))
                .child(
                    text(elapsed)
                        .key("elapsed")
                        .w(timer_text_width(minutes))
                        .align(Align::End)
                        .font(p.clock())
                        .color(p.muted()),
                ),
        )
}

/// The red dot before a recording's time.
fn rec_dot(p: Paint) -> El {
    el().key("rec-dot")
        .size(REC_DOT, REC_DOT)
        .radius(REC_DOT / 2.)
        .fill(p.danger())
        .a11y_hidden()
}

/// The open recording's rows: its header, Stop, any notice and the copies.
fn rows_height(shown: &Recording) -> f32 {
    open_height_without_notice() + notice_height(shown)
}

/// The open recording's height: its rows and the foot under them.
pub(super) fn open_height(capsule: &Capsule, shown: &Recording) -> f32 {
    rows_height(shown) + peek::height(capsule)
}

/// The room the open recording's foot takes under its copies.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn foot_height(capsule: &Capsule) -> f32 {
    peek::height(capsule)
}

/// The room the recording's notice line takes, when it says anything.
fn notice_height(shown: &Recording) -> f32 {
    if recording_notice(shown).is_some() {
        NOTICE_HEIGHT
    } else {
        0.0
    }
}

/// The open panel a finished meeting grows from: no notice and no foot.
pub(super) fn open_height_without_notice() -> f32 {
    OPEN_HEADER + ROW_GAP + COPIES_HEIGHT + PANEL_PAD
}

/// Where Stop is while recording and where the title is afterwards: the same
/// place, so Stop leaves from the row it was pressed in as the title arrives.
fn stop_slot() -> El {
    col()
        .key("caption-slot")
        .w_full()
        .min_h(ROW_HEIGHT)
        .transition(Transition::none())
}

/// How far short of the open panel's width its rows start to show.
const UNCOVER: f32 = 32.0;

/// What a moving edge of the surface crosses fades under it: the sides away
/// from the saved corner, by as much as the stage says they are moving.
pub(super) fn veil(capsule: &Capsule, width: f32, height: f32) -> El {
    let (x, y) = capsule.stage.veil();
    let (right, top) = (capsule.corner.is_right(), capsule.corner.is_top());
    el().key("veil")
        .absolute(0.0, 0.0)
        .size(width, height)
        .transition(Transition::none())
        .clip_fade_linear(
            if right { x } else { 0.0 },
            if top { 0.0 } else { y },
            if right { 0.0 } else { x },
            if top { y } else { 0.0 },
        )
}

impl Capsule {
    /// How much of action row `row` (Stop, the two copies, then the foot) shows.
    /// Whole wherever the surface isn't in motion.
    pub(super) fn action_presence(&self, row: usize) -> stage::Presence {
        let (
            Some(o),
            Some(State::Recording {
                recording: shown, ..
            }),
        ) = (self.stage.outline(), self.drawn())
        else {
            return stage::Presence::flat(1.0);
        };
        let mut presence = self.stage.presence(stage::Layer::Action(row));
        // A row shows only as far as the outline has uncovered it, so an
        // edge in flight never crosses words at full strength: across, as
        // the far side clears the row's start; along, as the edge passes it.
        let notice = notice_height(shown);
        let near = match row {
            0 => REST_HEIGHT,
            1 => OPEN_HEADER + notice + ROW_GAP,
            row => OPEN_HEADER + notice + 2.0 * ROW_GAP + (row - 1) as f32 * ROW_HEIGHT,
        };
        let smooth = |t: f32| {
            let t = t.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        presence.opacity *=
            smooth((o.w - (PANEL_WIDTH - UNCOVER)) / UNCOVER) * smooth((o.h - near) / ROW_HEIGHT);
        presence
    }
}

/// The resting recording's header inside a surface `width` by `height`
/// that is still arriving from its offer: the nodes [`recording`] draws.
pub(super) fn arriving_header(
    p: Paint,
    capsule: &Capsule,
    shown: &Recording,
    width: f32,
    height: f32,
    presence: f32,
) -> El {
    let (right, top) = (capsule.corner.is_right(), capsule.corner.is_top());
    let panel_height = rows_height(shown);
    let meter = meter(p, capsule, shown).opacity(presence);
    let spacer = el().key("space").grow(1.0);
    let meta = row()
        .key("meta")
        .h(REST_HEIGHT)
        .px(REST_PX)
        .center_y()
        .transition(Transition::none());
    let meta = if right {
        meta.child(spacer).child(meter)
    } else {
        meta.child(meter).child(spacer)
    };
    col()
        .key("panel-contents")
        .size(PANEL_WIDTH, panel_height)
        .transition(Transition::none())
        .absolute(
            if right { width - PANEL_WIDTH } else { 0.0 },
            if top { 0.0 } else { height - panel_height },
        )
        .child(
            bands(top, meta, None).absolute(0., if top { 0. } else { panel_height - REST_HEIGHT }),
        )
}

/// The recording: at rest only its levels and time; under the
/// pointer it springs open into Stop and the copies, its header nearest the
/// screen edge.
/// How the recording's pill is held open, and whether it asks to discard.
#[derive(Clone, Copy)]
pub(super) struct Open {
    pub(super) hovered: bool,
    pub(super) stopping: bool,
    pub(super) confirming: bool,
}

pub(super) fn recording(p: Paint, capsule: &Capsule, shown: &Recording, held: Open) -> El {
    let Open {
        hovered,
        stopping,
        confirming,
    } = held;
    let open = hovered || stopping;
    // The question replaces Stop and the copies while it is asked.
    let asking = open && confirming;
    // Rows that are leaving stay drawn until they have gone.
    let actions = open || (0..stage::ACTIONS).any(|row| capsule.action_presence(row).shows());
    let notice = recording_notice(shown);
    let notice_text = |notice: Notice| {
        text(notice.text().to_owned())
            .key("notice")
            .font(p.k.ty.text_xsmall)
            .color(notice.color(p))
            .truncate()
            .min_w(0.0)
    };
    let right = capsule.corner.is_right();
    let top = capsule.corner.is_top();
    let live = if capsule.stage.outline().is_some() {
        capsule.stage.presence(stage::Layer::Meter).opacity
    } else {
        1.0
    };
    let meter = meter(p, capsule, shown).opacity(live);
    // The open field takes the notice's place in the header. The words keep
    // their place and their cut while the field's free end wipes over them,
    // or in a left corner the time beyond it: they are never squeezed, and
    // nothing is drawn on one.
    // The notice is measured against the resting header, so the ✕ arriving
    // never recuts it; it has gone once the field is as wide as it opens.
    let field = field_width(capsule);
    let wide = PANEL_WIDTH - 2. * REST_PX - METER_GAP - timer_width(minutes_shown(capsule));
    let widest = wide - DISCARD_ROOM;
    let covered = if widest > waveform::WIDTH {
        ((field - waveform::WIDTH) / (widest - waveform::WIDTH)).clamp(0., 1.)
    } else {
        f32::from(u8::from(open))
    };
    let accessory = notice.filter(|_| covered < 1.).map(|notice| {
        let gone = (covered / NOTICE_GONE).min(1.);
        let wipe = (field - waveform::WIDTH).min(NOTICE_WIPE);
        let lane = row()
            .key("notice-lane")
            .size(wide - field, REST_HEIGHT)
            .center_y()
            .transition(Transition::none());
        let words = notice_text(notice)
            .w(wide - waveform::WIDTH)
            .min_w(wide - waveform::WIDTH);
        // In a left corner the words end at the inset, away from the time.
        let (lane, words) = if right {
            (lane.clip_fade(0., 0., wipe, 0.), words)
        } else {
            (
                lane.justify_end().clip_fade(wipe, 0., 0., 0.),
                words.align(rune::Align::End),
            )
        };
        lane.opacity(1. - gone * gone * (3. - 2. * gone))
            .opacity_motion(Anim::INSTANT)
            .child(words)
    });
    let stop = capsule.action_presence(0);
    let action = (actions && !asking).then(|| {
        // Pressed, the same row says so in place: its glyph and first
        // letters stay where they were and it stops taking the pointer.
        let row = panel_row(
            p,
            "stop",
            &icons::STOP,
            if stopping {
                tr("Stopping…")
            } else {
                tr("Stop")
            },
            None,
            false,
            !stopping && open && stop.takes_input(),
            stop.opacity,
            |c, tx| {
                c.request_stop();
                c.stir(tx);
            },
        )
        .absolute(PANEL_PAD, 0.)
        .w(PANEL_WIDTH - 2. * PANEL_PAD);
        let row = if stopping || !open {
            row.opacity(stop.opacity)
        } else {
            row
        };
        stop_slot().child(row)
    });
    // The rows' own box, laid out as the finished meeting's is; the foot is
    // a layer past its free edge, so nothing in the box moves for it.
    let (panel_height, foot_room) = if asking {
        (CONFIRM_HEIGHT, 0.)
    } else {
        (rows_height(shown), peek::height(capsule))
    };
    let (target_w, target_h) = if open {
        (PANEL_WIDTH, panel_height + foot_room)
    } else {
        (rest_width(shown), REST_HEIGHT)
    };
    // The stage owns the outline. Before its first step the pill is at rest.
    // At rest and open it keeps every capsule's corners.
    let (width, height, radius) = capsule
        .stage
        .outline()
        .map_or((target_w, target_h, PANEL_RADIUS), |o| {
            (stage::grid(o.w), stage::grid(o.h), o.radius)
        });
    let header = meter_band();
    let spacer = el().key("space").grow(1.0);
    // The ✕ keeps its place on the icon spine, over the header rather than
    // in its row, so nothing beside it moves as it arrives; while the
    // question is asked (or the discard is under way) it fades and takes no
    // input.
    let discard = (open && (!stopping || confirming)).then(|| {
        let x = if right {
            SPINE - CLOSE_BOX / 2.
        } else {
            PANEL_WIDTH - SPINE - CLOSE_BOX / 2.
        };
        let slot = row()
            .key("discard-slot")
            .size(CLOSE_BOX, CLOSE_BOX)
            .absolute(x, (REST_HEIGHT - CLOSE_BOX) / 2.)
            .center()
            .child(
                IconButton::new(&icons::CLOSE_SMALL)
                    .variant(IconButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .label(tr("Discard recording"))
                    .build(p.k)
                    .key("discard")
                    .cursor(Cursor::Pointer)
                    .no_window_drag()
                    .on_press_tx(|c: &mut Capsule, tx| {
                        c.ask_discard();
                        c.stir(tx);
                    }),
            );
        if asking {
            slot.opacity(0.0).inert()
        } else {
            slot.opacity(stop.opacity)
        }
    });
    let header = if right {
        header
            .children(discard)
            .children(accessory)
            .child(spacer)
            .child(meter)
    } else {
        header
            .child(meter)
            .child(spacer)
            .children(accessory)
            .children(discard)
    };
    let header = bands(top, header, action).absolute(
        0.,
        if top {
            0.
        } else {
            panel_height
                - if actions && !asking {
                    OPEN_HEADER
                } else {
                    REST_HEIGHT
                }
        },
    );
    // The full-size layout exists before the first hover. Its origin cancels
    // the moving clip's origin so neither the rows nor the meter ride its edge.
    let mut contents = col()
        .key("panel-contents")
        .size(PANEL_WIDTH, panel_height)
        .transition(Transition::none())
        .absolute(
            if right { width - PANEL_WIDTH } else { 0.0 },
            if top { 0.0 } else { height - panel_height },
        )
        .child(header);
    if asking {
        contents = contents.child(
            discard_confirm(p, top, stopping).absolute(0.0, if top { REST_HEIGHT } else { 0.0 }),
        );
    } else if actions {
        if let Some(notice) = notice {
            contents = contents.child(
                row()
                    .key("notice-line")
                    .absolute(
                        0.0,
                        if top {
                            OPEN_HEADER
                        } else {
                            panel_height - OPEN_HEADER - NOTICE_HEIGHT
                        },
                    )
                    .w(PANEL_WIDTH)
                    .h(NOTICE_HEIGHT)
                    .px(PANEL_PAD + ROW_PX)
                    .center_y()
                    .opacity(stop.opacity)
                    .child(notice_text(notice)),
            );
        }
        let rows = col()
            .key("rows")
            .absolute(
                0.0,
                if top {
                    panel_height - (COPIES_HEIGHT + PANEL_PAD)
                } else {
                    PANEL_PAD
                },
            )
            .w(PANEL_WIDTH)
            .px(PANEL_PAD)
            .children(copy_rows(p, capsule, true));
        // Leaving rows are drawn, not pressed.
        contents = contents.child(if open { rows } else { rows.inert() });
    }
    // The foot is on the panel's free edge, a gap past the copies, so Stop
    // stays a row from the pill and the copies keep their place as it grows.
    let foot = (actions && !asking).then(|| {
        let foot = peek::foot(p, capsule, shown)
            .absolute(
                PANEL_PAD + if right { width - PANEL_WIDTH } else { 0. },
                if top {
                    panel_height - PANEL_PAD + ROW_GAP
                } else {
                    height - panel_height + PANEL_PAD - foot_room
                },
            )
            .w(PANEL_WIDTH - 2. * PANEL_PAD);
        if open { foot } else { foot.inert() }
    });
    let mut body = col()
        .key("pill")
        .role("status")
        .label(format!("{} · {}", tr("Recording"), shown.title))
        .radius(radius)
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_base)
        .size(width, height)
        .child(veil(capsule, width, height).child(contents).children(foot))
        .clip_rounded(radius)
        // The stage steps every frame; nothing here eases a second time.
        .motion(Spring::NONE)
        // Only the drawn pill takes the pointer; the rest of the window,
        // as large as the open panel, lets clicks through. On Windows the
        // window is only as large as what it draws, so the pointer is
        // tracked on the window instead (see below).
        .hit();
    for shadow in &p.k.elevation.overlay {
        body = body.box_shadow(*shadow);
    }
    // A recording in its offer's window is larger than the pill it draws, so
    // Windows tracks that window as it always has; one the recording owns is
    // opened by the pill and kept open by the window.
    #[cfg(target_os = "windows")]
    let body = if capsule.presentation.inherits_offer_window() {
        recording_pointer(body)
    } else {
        recording_opener(body)
    };
    #[cfg(not(target_os = "windows"))]
    let body = recording_pointer(body);
    let body = if capsule.presentation.hold_hover {
        body.on_pointer_move_local(|c: &mut Capsule, point, tx| {
            let (x, y) = (point.window_x, point.window_y);
            if c.presentation
                .pointer
                .is_some_and(|(px, py)| (x - px).abs().max((y - py).abs()) > 4.)
            {
                c.presentation.hold_hover = false;
                c.set_hovered(true);
                // The panel opens on this frame, not the next event's.
                c.stir(tx);
            } else if c.presentation.pointer.is_none() {
                c.presentation.pointer = Some((x, y));
            }
        })
    } else {
        body
    };
    let surface = in_corner(capsule, presence::anchored(capsule, body));
    // Windows sizes this window to what it draws, so it is only as large as
    // the pill at rest: the drawn pill opens the recording, and the whole
    // window keeps it open. A pointer that outruns the opening pill, or
    // rests in the window's margin, neither closes it nor shrinks the
    // window out from under itself, and a click where a closed panel was
    // stays a click on what is beneath.
    #[cfg(target_os = "windows")]
    let surface = if capsule.presentation.inherits_offer_window() {
        surface
    } else {
        recording_window_pointer(surface).hit()
    };
    surface
}

/// Opens the recording while the pointer is on it, and closes it when the
/// pointer leaves: where the pointer's whole is the pill it draws.
fn recording_pointer(target: El) -> El {
    target.on_hover_change(|c: &mut Capsule, inside, tx| {
        c.set_hovered(inside);
        c.stir(tx);
    })
}

/// Windows: the drawn pill opens the recording. Leaving the pill is not a
/// leave while the window still holds the pointer, so this never closes it.
#[cfg(target_os = "windows")]
fn recording_opener(target: El) -> El {
    target.on_hover_change(|c: &mut Capsule, inside, tx| {
        if inside {
            c.set_hovered(true);
            c.stir(tx);
        }
    })
}

/// Windows: the window keeps the recording open and lets it close only when
/// the pointer leaves the whole window. Entering the window where the pill
/// isn't (the area a closed panel covered) opens nothing.
#[cfg(target_os = "windows")]
fn recording_window_pointer(target: El) -> El {
    target.on_hover_change(|c: &mut Capsule, inside, tx| {
        if !inside {
            c.set_hovered(false);
            c.stir(tx);
        }
    })
}

/// `path` as one shell word.
pub(super) fn shell_word(path: &str) -> String {
    let plain = !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+:@%".contains(c));
    if plain {
        path.to_owned()
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

/// Formats an executable path for invoking `hex` with arguments in a shell:
/// POSIX single-quoting on Unix, and PowerShell's call operator `& '...'` on
/// Windows when the path needs quoting (paths with only ASCII alphanumerics
/// and `\ / . _ - : + @ %` run unquoted in both PowerShell and cmd.exe).
pub(super) fn command_for_path(path: &str, windows: bool) -> String {
    if !windows {
        return shell_word(path);
    }
    let plain = !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || r"\/._-:+@%".contains(c));
    if plain {
        path.to_owned()
    } else {
        format!("& '{}'", path.replace('\'', "''"))
    }
}

/// This Hex's executable, to run its `hex meetings` commands.
pub(super) fn hex_command() -> String {
    std::env::current_exe()
        .map(|path| command_for_path(&path.to_string_lossy(), cfg!(target_os = "windows")))
        .unwrap_or_else(|_| "hex".to_owned())
}

/// Tells a coding agent how to read meeting `id` with this Hex executable.
pub(in crate::rune_ui) fn meeting_agent_prompt(
    id: &str,
    title: &str,
    recording: Option<u64>,
) -> String {
    agent_prompt(&hex_command(), id, title, recording)
}

/// Tells a coding agent how to read meeting `id` with `hex meetings`.
/// `recording` is how far its transcript reaches while it still records.
pub(super) fn agent_prompt(hex: &str, id: &str, title: &str, recording: Option<u64>) -> String {
    let id = shell_word(id);
    let search = format!(
        "Find a topic, then read around the time it came up:\n  \
         {hex} meetings search --meeting {id} \"<words>\"\n  \
         {hex} meetings show {id} --from <mm:ss> --to <mm:ss> --md\n"
    );
    match recording {
        Some(reached_ms) => format!(
            "Use my Hex meeting {title:?} as context. It is still recording.\n\n\
             Read the transcript so far:\n  {hex} meetings show {id} --md\n\n\
             Catch up on what was said since this prompt (the first turn may repeat):\n  \
             {hex} meetings show {id} --from {} --md\n\n{search}",
            meeting::format_duration(reached_ms),
        ),
        None => format!(
            "Use my Hex meeting {title:?} as context.\n\n\
             Read its notes and transcript:\n  {hex} meetings show {id} --md\n\n{search}"
        ),
    }
}

/// The recording capsule's window. On Windows it is the size of what it
/// draws: the resting pill (as wide as the open panel while it says
/// `noticed`), then the open panel with its foot `open` tall. Elsewhere it
/// stays at its largest, and only the `.hit()` pill takes the pointer.
pub(super) fn recording_window_size(open: Option<f32>, shown: Option<&Recording>) -> (f32, f32) {
    if cfg!(target_os = "windows") {
        match open {
            Some(foot) => open_window(shown, foot),
            None => rest_window(shown),
        }
    } else {
        RECORDING_WINDOW
    }
}

/// The window a resting recording draws: the pill and the inset around it.
pub(super) fn rest_window(shown: Option<&Recording>) -> (f32, f32) {
    (
        shown.map_or(REST_WIDTH, rest_width) + 2. * CAPSULE_INSET,
        REST_HEIGHT + 2. * CAPSULE_INSET,
    )
}

/// The window an open recording draws: the open panel, its inset, its foot,
/// and room for the notice when it says anything.
pub(super) fn open_window(shown: Option<&Recording>, foot: f32) -> (f32, f32) {
    (
        RECORDING_WINDOW.0,
        open_height_without_notice() + foot + 2. * CAPSULE_INSET + shown.map_or(0., notice_height),
    )
}

/// The finished meeting's window while it transcribes: the panel it grew
/// from, without a notice. Elsewhere the window stays at its largest, and
/// only the `.hit()` panel takes the pointer.
pub(super) fn finalizing_window_size() -> (f32, f32) {
    #[cfg(target_os = "windows")]
    {
        (
            RECORDING_WINDOW.0,
            open_height_without_notice() + 2. * CAPSULE_INSET,
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        RECORDING_WINDOW
    }
}

/// Elsewhere the window stays at its largest and only the `.hit()` panel
/// takes the pointer.
#[cfg(not(target_os = "windows"))]
pub(super) fn ready_window_size(_: &MeetingManifest) -> (f32, f32) {
    RECORDING_WINDOW
}

/// The ready panel's window: its header, a summary while there is one or one
/// is being written, and the two copies.
#[cfg(target_os = "windows")]
pub(super) fn ready_window_size(manifest: &MeetingManifest) -> (f32, f32) {
    ready_body(manifest)
}

/// What the ready panel draws, on every OS: its header, a summary while there
/// is one or one is being written, and the two copies, with the inset.
pub(super) fn ready_body(manifest: &MeetingManifest) -> (f32, f32) {
    (
        PANEL_WIDTH + 2.0 * CAPSULE_INSET,
        ready_height(manifest).max(open_height_without_notice()) + 2.0 * CAPSULE_INSET,
    )
}

/// The recording window's size on Windows, where it follows what the capsule
/// draws. Growth is taken the frame it is asked for; a shrink waits for the
/// surface to land, because a window that shrinks first cuts the panel it is
/// still drawing.
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct WindowFit {
    size: Option<(f32, f32)>,
}

#[cfg(any(target_os = "windows", test))]
impl WindowFit {
    /// The size to ask the host for now: `want` when the window has to grow
    /// or it has no size yet, only at rest when it shrinks, and nothing while
    /// it already has that size.
    pub(super) fn next(&mut self, want: (f32, f32), at_rest: bool) -> Option<(f32, f32)> {
        if self.size == Some(want) {
            return None;
        }
        if !at_rest
            && self
                .size
                .is_some_and(|size| want.0 < size.0 || want.1 < size.1)
        {
            return None;
        }
        self.size = Some(want);
        Some(want)
    }

    /// The size the host was last given, once this fit has asked for one.
    pub(super) fn size(&self) -> Option<(f32, f32)> {
        self.size
    }

    /// A different window, whose size this fit did not ask for.
    pub(super) fn reset(&mut self) {
        self.size = None;
    }
}

/// Windows: the recording capsule's window is only as large as what it
/// draws, so it follows the capsule: the open panel while the pointer or Stop
/// holds it, and the resting pill once the closing surface has landed.
#[cfg(target_os = "windows")]
impl Capsule {
    /// Asks for the window what the capsule shows now needs. A shrink waits
    /// for the surface; growth is taken at once.
    pub(super) fn fit_recording_window(&mut self) {
        self.ask_recording_window(false);
    }

    /// The surface has landed: a window still larger than what is drawn can
    /// shrink to it now.
    pub(super) fn settle_recording_window(&mut self) {
        self.ask_recording_window(true);
    }

    fn ask_recording_window(&mut self, at_rest: bool) {
        // A recording an offer opened keeps that window while it records;
        // once capture ends the finished panel sizes its own.
        if matches!(self.state, State::Recording { .. })
            && self.presentation.inherits_offer_window()
        {
            return;
        }
        let Some(want) = self.recording_window_want() else {
            return;
        };
        let Some(resize) = &self.resize else {
            return;
        };
        if let Some(size) = self.recording_window.next(want, at_rest) {
            resize(size);
        }
    }

    /// The window this capsule's surface needs: the pill, the open panel,
    /// the finalizing panel, or the ready panel.
    fn recording_window_want(&self) -> Option<(f32, f32)> {
        match &self.state {
            State::Recording {
                recording: shown,
                hovered,
                stopping,
                ..
            } => Some(recording_window_size(
                (*hovered || *stopping).then(|| foot_height(self)),
                Some(shown),
            )),
            State::Finalizing { .. } => Some(finalizing_window_size()),
            State::Ready { manifest, .. } => Some(ready_window_size(manifest)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest() -> (f32, f32) {
        rest_window(None)
    }

    /// Open before the call has words: the plain link is its foot.
    fn open() -> (f32, f32) {
        open_window(None, peek::LINK_FOOT)
    }

    fn noticed() -> Recording {
        Recording {
            id: "noticed".into(),
            title: "Design sync".into(),
            started: Instant::now(),
            automatic: true,
            warning: None,
        }
    }

    #[test]
    fn the_resting_window_is_the_pill_and_the_open_one_the_panel() {
        // The 168 pt pill (its field, the red dot, and the time) and its inset.
        assert_eq!(rest(), (176., 44.));
        assert_eq!(open(), (260., 224.));
        // With words, the foot is their last lines.
        assert_eq!(open_window(None, peek::PEEK_FOOT), (260., 252.));
        // The discard question fits inside the open window.
        assert!(CONFIRM_HEIGHT + 2. * CAPSULE_INSET <= open().1);
        // A notice widens the resting pill to the panel, and adds its line
        // to the open one.
        let noticed = noticed();
        assert_eq!(rest_window(Some(&noticed)), (260., 44.));
        assert_eq!(open_window(Some(&noticed), peek::LINK_FOOT), (260., 244.));
    }

    #[test]
    fn a_growing_window_is_taken_at_once_and_a_shrinking_one_at_rest() {
        let mut fit = WindowFit::default();
        // Nothing asked yet: the first size is taken whatever it is.
        assert_eq!(fit.next(rest(), false), Some(rest()));
        assert_eq!(fit.next(open(), false), Some(open()));
        // The pointer left: the close is still drawn at the open size.
        assert_eq!(fit.next(rest(), false), None);
        assert_eq!(fit.size(), Some(open()));
        // The surface landed.
        assert_eq!(fit.next(rest(), true), Some(rest()));
        assert_eq!(fit.next(rest(), true), None);
        assert_eq!(fit.size(), Some(rest()));
    }

    #[test]
    fn a_pointer_that_returns_before_the_close_lands_keeps_the_open_window() {
        let mut fit = WindowFit::default();
        fit.next(open(), false);
        // The pointer left, but the window may not shrink yet.
        assert_eq!(fit.next(rest(), false), None);
        // Back inside: the window never shrank, so nothing is asked.
        assert_eq!(fit.next(open(), false), None);
        assert_eq!(fit.size(), Some(open()));
        // It leaves again and the close lands.
        assert_eq!(fit.next(rest(), false), None);
        assert_eq!(fit.next(rest(), true), Some(rest()));
    }

    #[test]
    fn a_window_that_grows_one_axis_and_shrinks_the_other_waits_for_rest() {
        let mut fit = WindowFit::default();
        let noticed = noticed();
        let open_noticed = open_window(Some(&noticed), peek::LINK_FOOT);
        fit.next(open_noticed, false);
        // The notice line goes: narrower in height, the same width.
        assert_eq!(fit.next(open(), false), None);
        assert_eq!(fit.size(), Some(open_noticed));
        assert_eq!(fit.next(open(), true), Some(open()));
    }

    #[test]
    fn a_new_window_forgets_the_size_the_last_one_had() {
        let mut fit = WindowFit::default();
        fit.next(open(), false);
        fit.reset();
        assert_eq!(fit.size(), None);
        // Its first ask may be smaller than the window it replaced.
        assert_eq!(fit.next(rest(), false), Some(rest()));
    }
}
