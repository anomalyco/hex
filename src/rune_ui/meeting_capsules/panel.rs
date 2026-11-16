//! The recording pill and the ready panel: the listening field and timer at
//! rest, then Stop and the three copies, and the finished meeting's title
//! and summary over the same copies.

use rune::{Cursor, El, Icon, SpringPair, TextStyle, Transition, Tx, col, el, icon, row, text};
use rune_theme_opencode::icons;

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

/// The ready panel's line under its title, and its colour: the summary, the
/// summary still being written, or why the Markdown wasn't saved.
pub(super) fn ready_summary(manifest: &MeetingManifest) -> Option<(String, bool)> {
    if meeting::notes_in_progress(&manifest.id) {
        return Some((
            tr("Writing a title and summary with Apple Intelligence…").to_owned(),
            false,
        ));
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

/// One of the panel's copies: an icon, its label, and on the right what it
/// would copy or that it just did. The first row is raised.
#[allow(clippy::too_many_arguments)]
fn panel_row(
    p: Paint,
    key: &'static str,
    glyph: &'static Icon,
    label: &str,
    hint: Option<El>,
    first: bool,
    enabled: bool,
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
        .label(label)
        .child(icon(glyph, 16.0).color(ink))
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
        row_el.fill(p.k.c.layer_02).mb(FIRST_ROW_GAP)
    } else {
        row_el.hover(p.k.c.hover)
    };
    if enabled {
        row_el.focusable(true).on_press_tx(press)
    } else {
        row_el.disabled(true).opacity(0.5)
    }
}

/// Copy new, Copy all, and Copy agent prompt, for the recording or the
/// ready meeting.
fn copy_rows(p: Paint, capsule: &Capsule, recording: bool) -> Vec<El> {
    let live = &capsule.live;
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

fn replace(c: &Capsule) -> Transition {
    let (enter, exit) = if c.reduced.get() {
        (0.18, 0.15)
    } else {
        (0.3, 0.24)
    };
    Transition::opacity().springs(SpringPair::new(
        Spring::new(enter, 0.),
        Spring::new(exit, 0.),
    ))
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

fn result(
    p: Paint,
    capsule: &Capsule,
    title: String,
    duration: Option<u64>,
    manifest: Option<&MeetingManifest>,
) -> El {
    let top = capsule.corner.is_top();
    let right = capsule.corner.is_right();
    let header = row()
        .key("header")
        .min_h(OPEN_HEADER)
        .clip_rounded(0.)
        .px(REST_PX)
        .gap(6.0)
        .center_y();
    // The title has its final column even before Close is available. It may
    // grow vertically without moving the time off the recording baseline.
    let label = text(title.clone())
        .key(if manifest.is_some() {
            "title"
        } else {
            "finalizing"
        })
        .transition(replace(capsule))
        .font(TextStyle::new(13.0, 600.0).lh(20.0))
        .color(p.text())
        .wrap_anywhere()
        .max_lines(TITLE_LINES)
        .grow(1.0)
        .min_w(0.0)
        .my(TITLE_INSET)
        .a11y_hidden();
    let label = if top {
        label.self_start()
    } else {
        label.self_end()
    };
    let clock = duration.map(|duration| {
        let meter = row().key("meter").h(REST_HEIGHT).center_y().child(
            text(meeting::format_duration(duration))
                .key("elapsed")
                .font(timer_style())
                .color(p.muted()),
        );
        if top {
            meter.self_start()
        } else {
            meter.self_end()
        }
    });
    let close = row()
        .key("close-slot")
        .w(32.)
        .h(REST_HEIGHT)
        .center()
        .children(manifest.map(|_| dismiss_button(p).transition(replace(capsule))));
    let close = if top {
        close.self_start()
    } else {
        close.self_end()
    };
    let header = if right {
        header.child(label).child(close).children(clock)
    } else {
        header.children(clock).child(close).child(label)
    };
    let summary = manifest.and_then(ready_summary).map(|(summary, written)| {
        text(summary)
            .key("summary")
            .transition(replace(capsule))
            .font(p.k.ty.text_small)
            .color(if written { p.muted() } else { p.faint() })
            .max_lines(SUMMARY_LINES)
            .mx(ROW_PX + PANEL_PAD)
            .mt(2.0)
            .mb(ROW_PX)
    });
    // The measured lane and the following rows move on the same spring.
    // Reveal the words inside the growing gap, never over the copy buttons.
    let summary = col().key("summary-lane").clip_rounded(0.).children(summary);
    let rows = col()
        .key("rows")
        .px(PANEL_PAD)
        .children(copy_rows(p, capsule, false));
    let rows = if top {
        rows.pb(PANEL_PAD)
    } else {
        rows.pt(PANEL_PAD)
    };
    let rows = if manifest.is_none() {
        rows.disabled(true).opacity(0.48)
    } else {
        rows
    };
    let contents = col().key("panel-contents").w(PANEL_WIDTH);
    // New notes grow from the free edge, not between the pointer and its
    // copy row. A bottom corner mirrors the bands, not their reading order.
    let contents = if top {
        contents.child(header).child(rows).child(summary)
    } else {
        contents.child(summary).child(rows).child(header)
    };
    let body = panel_body(p, &title)
        .motion(RECORDING_OPEN)
        .child(contents)
        .on_hover_change(|c: &mut Capsule, inside, _| c.set_hovered(inside))
        .cursor(Cursor::Grab)
        .on_drag(|c: &mut Capsule, drag, tx| c.drag(drag, tx));
    in_corner(capsule, body)
}

/// The recording: at rest only its status, levels and time; under the
/// pointer it springs open into Stop and the copies, its header nearest the
/// screen edge.
pub(super) fn recording(p: Paint, capsule: &Capsule, shown: &Recording) -> El {
    let open = capsule.hovered || capsule.stopping;
    let notice = recording_notice(shown);
    let elapsed = meeting::format_duration(shown.started.elapsed().as_millis() as u64);
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
    let meter = row()
        .key("meter")
        .h(REST_HEIGHT)
        .gap(6.0)
        .center_y()
        .child(status_dot(p.danger()).size(6.0, 6.0))
        .child(waveform::slot())
        .child(
            text(elapsed)
                .key("elapsed")
                .font(timer_style())
                .color(p.muted()),
        );
    let meter = if top {
        meter.self_start()
    } else {
        meter.self_end()
    };
    let accessory = if capsule.stopping {
        Some(
            text(tr("Stopping…").to_owned())
                .key("stopping")
                .font(p.k.ty.text_xsmall)
                .color(p.muted())
                .mx(ROW_PX)
                .transition(replace(capsule)),
        )
    } else if open {
        Some(
            row()
                .key("stop")
                .h(STOP_TARGET)
                .min_w(STOP_TARGET)
                .px(ROW_PX + 4.)
                .gap(6.0)
                .center_y()
                .radius(ROW_RADIUS)
                .hover(p.k.c.hover)
                .label(tr("Stop"))
                .role("button")
                .focusable(true)
                .on_press(Capsule::request_stop)
                .child(icon(&icons::STOP, 12.0).color(p.muted()))
                .child(
                    text(tr("Stop").to_owned())
                        .font(p.k.ty.text_xsmall)
                        .color(p.muted())
                        .a11y_hidden(),
                ),
        )
    } else {
        notice.map(notice_text)
    };
    let panel_height = OPEN_HEADER
        + 3.0 * ROW_HEIGHT
        + FIRST_ROW_GAP
        + PANEL_PAD
        + if notice.is_some() { NOTICE_HEIGHT } else { 0.0 };
    let width = if open || notice.is_some() {
        PANEL_WIDTH
    } else {
        REST_WIDTH
    };
    let height = if open { panel_height } else { REST_HEIGHT };
    // The full-size layout exists before the first hover. Its origin cancels
    // the moving clip's origin so neither the rows nor the meter ride its edge.
    let header = row()
        .key("header")
        .absolute(0.0, if top { 0.0 } else { panel_height - OPEN_HEADER })
        .w(PANEL_WIDTH)
        .h(OPEN_HEADER)
        .center_y()
        .gap(8.0);
    let spacer = el().key("space").grow(1.0);
    let header = if right {
        header
            .pl(if open { PANEL_PAD } else { REST_PX })
            .pr(REST_PX)
            .children(accessory)
            .child(spacer)
            .child(meter)
    } else {
        header
            .pl(REST_PX)
            .pr(if open { PANEL_PAD } else { REST_PX })
            .child(meter)
            .child(spacer)
            .children(accessory)
    };
    let mut contents = col()
        .key("panel-contents")
        .size(PANEL_WIDTH, panel_height)
        .absolute(
            if right { width - PANEL_WIDTH } else { 0.0 },
            if top { 0.0 } else { height - panel_height },
        )
        .child(header);
    if open {
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
                    .child(notice_text(notice)),
            );
        }
        let rows = col()
            .key("rows")
            .absolute(
                0.0,
                if top {
                    panel_height - (3.0 * ROW_HEIGHT + FIRST_ROW_GAP + PANEL_PAD)
                } else {
                    PANEL_PAD
                },
            )
            .w(PANEL_WIDTH)
            .px(PANEL_PAD)
            .children(copy_rows(p, capsule, true));
        contents = contents.child(rows);
    }
    let radius = if open {
        PANEL_RADIUS
    } else {
        REST_HEIGHT / 2.0
    };
    let body = col()
        .key("pill")
        .role("status")
        .label(&shown.title)
        .radius(radius)
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_base)
        .size(width, height)
        .child(contents)
        .clip_rounded(radius)
        .cursor(Cursor::Grab)
        .motion(if open {
            RECORDING_OPEN
        } else {
            RECORDING_CLOSE
        })
        .on_hover_change(|c: &mut Capsule, inside, _| c.set_hovered(inside))
        .on_drag(|c: &mut Capsule, drag, tx| c.drag(drag, tx))
        // Only the drawn pill takes the pointer; the rest of the window,
        // as large as the open panel, lets clicks through. On Windows the
        // window is only as large as the pill it draws instead.
        .hit();
    in_corner(capsule, body)
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
pub(super) fn recording_window_size(open: bool, noticed: bool) -> (f32, f32) {
    #[cfg(target_os = "windows")]
    {
        if !open {
            let width = if noticed {
                RECORDING_WINDOW.0
            } else {
                REST_WINDOW_WIDTH
            };
            return (width, REST_HEIGHT + 2.0 * CAPSULE_INSET);
        }
        (
            RECORDING_WINDOW.0,
            OPEN_HEADER
                + 3. * ROW_HEIGHT
                + FIRST_ROW_GAP
                + PANEL_PAD
                + 2. * CAPSULE_INSET
                + if noticed { NOTICE_HEIGHT } else { 0. },
        )
    }
    #[cfg(not(target_os = "windows"))]
    RECORDING_WINDOW
}

/// The ready panel's window: its header, a summary while there is one or one
/// is being written, and the three copies.
#[cfg(not(target_os = "windows"))]
pub(super) fn ready_window_size(_: &MeetingManifest) -> (f32, f32) {
    RECORDING_WINDOW
}

#[cfg(target_os = "windows")]
pub(super) fn ready_window_size(manifest: &MeetingManifest) -> (f32, f32) {
    let summary = if ready_summary(manifest).is_some() {
        SUMMARY_LINE * SUMMARY_LINES as f32 + ROW_PX + FIRST_ROW_GAP
    } else {
        0.0
    };
    (
        PANEL_WIDTH + 2.0 * CAPSULE_INSET,
        RESULT_HEADER_MAX
            + summary
            + 3.0 * ROW_HEIGHT
            + FIRST_ROW_GAP
            + PANEL_PAD
            + 2.0 * CAPSULE_INSET,
    )
}
