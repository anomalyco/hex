//! The recording pill and the ready panel: the listening field and timer at
//! rest, then Stop and the three copies, and the finished meeting's title
//! and summary over the same copies.

use rune::{Cursor, El, Icon, SpringPair, TextStyle, Transition, Tx, col, el, icon, row, text};
use rune_theme_opencode::icons;

use super::super::notes_writer::NotesWriter;
use super::*;

/// What a recording says beside its meter, in place of its title.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Notice {
    /// The microphone is digitally silent (#1518).
    Silent,
    /// For its first seconds, an automatic recording says so.
    Automatic,
}

impl Notice {
    fn text(self) -> &'static str {
        match self {
            Notice::Silent => tr("Microphone is silent"),
            Notice::Automatic => tr("Started automatically"),
        }
    }

    fn color(self, p: Paint) -> rune::Color {
        match self {
            Notice::Silent => p.warning(),
            Notice::Automatic => p.faint(),
        }
    }
}

pub(super) fn recording_notice(shown: &Recording) -> Option<Notice> {
    if shown.warning.is_some() {
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

/// The timer: elapsed while recording, the length once finished.
fn timer_style() -> TextStyle {
    TextStyle::new(13.0, 500.0).mono().lh(18.0)
}

/// "4 min new", "Up to date", "Transcribing…", or ✓ Copied: what Copy new
/// would do now.
fn copy_new_hint(p: Paint, live: &Live, recording: bool) -> Option<El> {
    if live.copying {
        return Some(hint_text(p, tr("Transcribing…").to_owned()));
    }
    if live.copied(LiveCopy::New) {
        return Some(copied_hint(p));
    }
    let meeting::PendingDelta { turns, span_ms } = live.pending;
    // The ready panel reads its transcript after it shows.
    if turns == 0 && live.entries.is_empty() && !recording {
        return None;
    }
    Some(hint_text(
        p,
        if turns > 0 && span_ms >= 60_000 {
            tr_fill("{} min new", &(span_ms / 60_000).to_string())
        } else if turns > 0 {
            tr_fill("{} sec new", &(span_ms / 1_000).max(1).to_string())
        } else if !live.entries.is_empty() {
            tr("Up to date").to_owned()
        } else {
            tr("No speech yet").to_owned()
        },
    ))
}

fn hint_text(p: Paint, hint: String) -> El {
    text(hint)
        .key("hint")
        .font(p.k.ty.small.features("'tnum' 1"))
        .color(p.faint())
        .truncate()
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
    let row_el = if first {
        row_el.fill(p.k.c.layer_02).hover(p.k.c.hover).mb(ROW_GAP)
    } else {
        row_el.hover(p.k.c.hover)
    };
    if enabled {
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
            .opacity(presence)
    } else {
        row_el.disabled(true).opacity(0.5 * presence)
    }
}

/// Copy new, Copy all, and Copy agent prompt, for the recording or the
/// ready meeting.
fn copy_rows(p: Paint, capsule: &Capsule, recording: bool) -> Vec<El> {
    let live = &capsule.live;
    let presence = |row| capsule.action_presence(row).opacity;
    // While recording, reading through the click can find speech the ticker
    // hasn't shown yet.
    let new_enabled = !live.copying
        && (live.pending.turns > 0
            || live.copied(LiveCopy::New)
            || (recording && capsule.copy_new.is_some()));
    let copied = |kind| live.copied(kind).then(|| copied_hint(p));
    vec![
        panel_row(
            p,
            "copy-new",
            &icons::MENU,
            tr("Copy new"),
            copy_new_hint(p, live, recording),
            true,
            new_enabled,
            presence(1),
            |c, tx| c.press_copy_new(tx),
        ),
        panel_row(
            p,
            "copy-all",
            &icons::OUTLINE_COPY,
            tr("Copy all"),
            copied(LiveCopy::All),
            false,
            !recording || !live.entries.is_empty(),
            presence(2),
            |c, tx| c.copy_all(tx),
        ),
        panel_row(
            p,
            "copy-prompt",
            &icons::OUTLINE_CONSOLE,
            tr("Copy agent prompt"),
            copied(LiveCopy::Prompt),
            false,
            true,
            presence(3),
            |c, tx| c.copy_prompt(tx),
        ),
    ]
}

/// The panel's rounded body; only it takes the pointer, not the window's
/// margin around it.
fn panel_body(p: Paint, title: &str) -> El {
    col()
        .key("pill")
        .role("status")
        .label(title)
        .w(PANEL_WIDTH)
        .radius(PANEL_RADIUS)
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_base)
        .clip_rounded(PANEL_RADIUS)
        .hit()
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

/// The finished meeting stays where its pill was; other panels take the saved corner.
pub(super) fn show_corner(
    kind: &Kind,
    movable: bool,
    last: Option<&(String, MeetingIndicatorCorner)>,
    saved: impl FnOnce() -> MeetingIndicatorCorner,
) -> MeetingIndicatorCorner {
    if let Kind::Ready(manifest) = kind {
        return last
            .filter(|(id, _)| *id == manifest.id)
            .map(|(_, corner)| *corner)
            .unwrap_or_else(saved);
    }
    if movable {
        saved()
    } else {
        MeetingIndicatorCorner::TopRight
    }
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

/// The finished meeting: its title and length, the summary when there is
/// one, and the same copies as the recording.
pub(super) fn ready(p: Paint, capsule: &Capsule, manifest: &MeetingManifest) -> El {
    result(
        p,
        capsule,
        meeting::display_title(manifest),
        manifest.duration_ms,
        Some(manifest),
    )
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
            .child(
                text(meeting::format_duration(duration))
                    .key("elapsed")
                    .font(timer_style())
                    .color(p.muted()),
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
    // The raised copy is as far from the title's row as from the next copy.
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
        .font(TextStyle::new(13., 600.).lh(20.))
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

/// The time's width after `minutes`: its 13 pt monospaced digits each
/// occupy 8 pt at layout resolution.
fn timer_width(minutes: u64) -> f32 {
    let extra = minutes.max(1).ilog10().saturating_sub(1);
    TIMER_WIDTH + extra as f32 * 8.
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
        Some(Kind::Recording(shown)) => shown.started.elapsed().as_secs() / 60,
        Some(Kind::Finalizing { duration_ms }) => duration_ms / 60_000,
        Some(Kind::Ready(manifest)) => manifest.duration_ms.unwrap_or(0) / 60_000,
        _ => 0,
    }
}

/// The header's room for the field in a surface `width` wide: from the
/// rows' inset to the time. Another digit on the time takes three columns.
fn field_room(capsule: &Capsule, width: f32) -> f32 {
    width - 2. * REST_PX - METER_GAP - timer_width(minutes_shown(capsule))
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
        Some(Kind::Recording(_)) => capsule.hovered || capsule.stopping,
        Some(Kind::Finalizing { .. } | Kind::Ready(_)) => true,
        _ => false,
    };
    if open {
        field_room(capsule, PANEL_WIDTH).max(waveform::WIDTH)
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
    row()
        .key("meter")
        .h(REST_HEIGHT)
        .gap(METER_GAP)
        .center_y()
        // The same meter in every state: it never arrives a second time.
        .transition(Transition::none())
        .child(waveform::slot(field_width(capsule)))
        .child(
            text(elapsed)
                .key("elapsed")
                .font(timer_style())
                .color(p.muted()),
        )
}

/// The open recording's height: its header, Stop, any notice and the copies.
pub(super) fn open_height(shown: &Recording) -> f32 {
    open_height_without_notice() + notice_height(shown)
}

/// The room the recording's notice line takes, when it says anything.
fn notice_height(shown: &Recording) -> f32 {
    if recording_notice(shown).is_some() {
        NOTICE_HEIGHT
    } else {
        0.0
    }
}

/// The open panel a finished meeting grows from.
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
    /// How much of action row `row` (Stop, then the three copies) shows.
    /// Whole wherever the surface isn't in motion.
    pub(super) fn action_presence(&self, row: usize) -> stage::Presence {
        let (Some(o), Some(Kind::Recording(shown))) = (self.stage.outline(), self.drawn()) else {
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
    let panel_height = open_height(shown);
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
pub(super) fn recording(p: Paint, capsule: &Capsule, shown: &Recording) -> El {
    let open = capsule.hovered || capsule.stopping;
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
    let field = field_width(capsule);
    let wide = field_room(capsule, PANEL_WIDTH);
    let covered = if wide > waveform::WIDTH {
        ((field - waveform::WIDTH) / (wide - waveform::WIDTH)).clamp(0., 1.)
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
    let action = actions.then(|| {
        // Pressed, the same row says so in place: its glyph and first
        // letters stay where they were and it stops taking the pointer.
        let stopping = capsule.stopping;
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
    let panel_height = open_height(shown);
    let (target_w, target_h, target_r) = if open {
        (PANEL_WIDTH, panel_height, PANEL_RADIUS)
    } else {
        (rest_width(shown), REST_HEIGHT, REST_HEIGHT / 2.0)
    };
    // The stage owns the outline. Before its first step the pill is at rest.
    let (width, height, radius) = capsule
        .stage
        .outline()
        .map_or((target_w, target_h, target_r), |o| {
            (stage::grid(o.w), stage::grid(o.h), o.radius)
        });
    let header = meter_band();
    let spacer = el().key("space").grow(1.0);
    let header = if right {
        header.children(accessory).child(spacer).child(meter)
    } else {
        header.child(meter).child(spacer).children(accessory)
    };
    let header = bands(top, header, action).absolute(
        0.,
        if top {
            0.
        } else {
            panel_height - if actions { OPEN_HEADER } else { REST_HEIGHT }
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
    if actions {
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
    let body = col()
        .key("pill")
        .role("status")
        .label(format!("{} · {}", tr("Recording"), shown.title))
        .radius(radius)
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_base)
        .size(width, height)
        .child(veil(capsule, width, height).child(contents))
        .clip_rounded(radius)
        // The stage steps every frame; nothing here eases a second time.
        .motion(Spring::NONE)
        .on_hover_change(|c: &mut Capsule, inside, tx| {
            c.set_hovered(inside);
            c.stir(tx);
        })
        // Only the drawn pill takes the pointer; the rest of the window,
        // as large as the open panel, lets clicks through. On Windows the
        // window is only as large as the pill it draws instead.
        .hit();
    let body = if capsule.presentation.hold_hover {
        body.on_pointer_move(|c: &mut Capsule, point| {
            if c.presentation
                .pointer
                .is_some_and(|(x, y)| (point.x - x).abs().max((point.y - y).abs()) > 4.)
            {
                c.presentation.hold_hover = false;
                c.set_hovered(true);
            } else if c.presentation.pointer.is_none() {
                c.presentation.pointer = Some((point.x, point.y));
            }
        })
    } else {
        body
    };
    in_corner(capsule, presence::anchored(capsule, body))
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

/// This Hex's executable, to run its `hex meetings` commands.
pub(super) fn hex_command() -> String {
    std::env::current_exe()
        .map(|path| shell_word(&path.to_string_lossy()))
        .unwrap_or_else(|_| "hex".to_owned())
}

/// Tells a coding agent how to read meeting `id` with `hex meetings`.
/// `recording` is how far its transcript reaches while it still records.
pub(super) fn agent_prompt(hex: &str, id: &str, title: &str, recording: Option<u64>) -> String {
    let id = shell_word(id);
    let search = format!(
        "Find a topic, then read around the time it came up:\n  \
         {hex} meetings search \"<words>\"\n  \
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
/// `noticed`), then the open panel. Elsewhere it stays at its largest, and
/// only the `.hit()` pill takes the pointer.
#[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
pub(super) fn recording_window_size(open: bool, shown: Option<&Recording>) -> (f32, f32) {
    #[cfg(target_os = "windows")]
    {
        if !open {
            let width = shown.map_or(REST_WIDTH, rest_width) + 2. * CAPSULE_INSET;
            return (width, REST_HEIGHT + 2.0 * CAPSULE_INSET);
        }
        (
            RECORDING_WINDOW.0,
            open_height_without_notice() + 2. * CAPSULE_INSET + shown.map_or(0., notice_height),
        )
    }
    #[cfg(not(target_os = "windows"))]
    RECORDING_WINDOW
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
/// is being written, and the three copies.
#[cfg(target_os = "windows")]
pub(super) fn ready_window_size(manifest: &MeetingManifest) -> (f32, f32) {
    let summary = if ready_summary(manifest).is_some() {
        SUMMARY_LINE * SUMMARY_LINES as f32 + ROW_PX + ROW_GAP
    } else {
        0.0
    };
    (
        PANEL_WIDTH + 2.0 * CAPSULE_INSET,
        RESULT_HEADER_MAX + summary + ROW_GAP + COPIES_HEIGHT + PANEL_PAD + 2.0 * CAPSULE_INSET,
    )
}
