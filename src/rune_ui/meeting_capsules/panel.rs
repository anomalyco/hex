//! The recording pill and the ready panel: the voice bars and timer at
//! rest, then Stop and the three copies, and the finished meeting's title
//! and summary over the same copies.

use rune::{Cursor, El, Icon, TextStyle, Tx, col, el, icon, row, text};
use rune_theme_opencode::icons;

use super::*;

/// The recording pill's voice bars, set from the bar thread
/// ([`Pill::tick`]) without rebuilding the view.
#[derive(Clone)]
pub(super) struct Bars(pub(super) [rune::Live<[f32; 4]>; BARS]);

impl Default for Bars {
    fn default() -> Self {
        Self(std::array::from_fn(|bar| {
            rune::Live::new(bar_rect(bar, BAR_REST))
        }))
    }
}

impl Bars {
    /// `levels` newest first, each 0 to 1.
    pub(super) fn show(&self, levels: [f32; 3]) {
        for (bar, live) in self.0.iter().enumerate() {
            let level = levels[bar.abs_diff(BARS / 2)];
            let height = BAR_REST + (BAR_MAX - BAR_REST) * level * BAR_SHAPE[bar];
            live.set(bar_rect(bar, height));
        }
    }
}

/// Bar `bar` at `height`, centred in [`BARS_BOX`].
fn bar_rect(bar: usize, height: f32) -> [f32; 4] {
    [
        bar as f32 * BAR_STEP,
        (BARS_BOX.1 - height) / 2.0,
        BAR_WIDTH,
        height,
    ]
}

/// What a recording says beside its bars, in place of its title.
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
    TextStyle::new(14.0, 600.0).mono().lh(18.0)
}

/// The voice bars, moved by [`Bars::show`] without rebuilding the view.
fn voice_bars(p: Paint, bars: &Bars) -> El {
    let color = p.success();
    el().key("bars")
        .size(BARS_BOX.0, BARS_BOX.1)
        .a11y_hidden()
        .children(bars.0.iter().enumerate().map(|(bar, live)| {
            el().key(format!("bar-{bar}"))
                .absolute(0.0, 0.0)
                .size(BAR_WIDTH, BAR_REST)
                .radius(BAR_WIDTH / 2.0)
                .fill(color)
                .live_rect(live.smooth(BAR_SPRING))
        }))
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
    let title = meeting::display_title(manifest);
    let header = row()
        .key("header")
        .h(HEADER_HEIGHT)
        .pl(PANEL_PAD + ROW_PX)
        .pr(REST_PX)
        .gap(8.0)
        .center_y()
        .child(
            text(title.clone())
                .key("title")
                .font(TextStyle::new(13.0, 600.0).lh(20.0))
                .color(p.text())
                .truncate()
                .grow(1.0)
                .min_w(0.0)
                .a11y_hidden(),
        )
        .children(manifest.duration_ms.map(|duration| {
            text(meeting::format_duration(duration))
                .key("duration")
                .font(timer_style())
                .color(p.muted())
        }));
    let summary = ready_summary(manifest).map(|(summary, written)| {
        text(summary)
            .key("summary")
            .font(p.k.ty.text_small)
            .color(if written { p.muted() } else { p.faint() })
            .max_lines(SUMMARY_LINES)
            .mx(ROW_PX + PANEL_PAD)
            .mt(2.0)
            .mb(ROW_PX)
    });
    let rows = col()
        .key("rows")
        .px(PANEL_PAD)
        .pb(PANEL_PAD)
        .children(copy_rows(p, capsule, false));
    let body = panel_body(p, &title)
        .child(header)
        .children(summary)
        .child(rows)
        .on_hover_change(|c: &mut Capsule, inside, _| c.set_hovered(inside));
    let body = if capsule.movable {
        body.cursor(Cursor::Grab)
            .on_drag(|c: &mut Capsule, drag, tx| c.drag(drag, tx))
    } else {
        body
    };
    in_corner(capsule, body)
}

/// The recording: at rest only its voice bars and elapsed time; under the
/// pointer it springs open into Stop and the copies, its header nearest the
/// screen edge.
pub(super) fn recording(p: Paint, capsule: &Capsule, shown: &Recording) -> El {
    let open = capsule.hovered;
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
    let meter = row()
        .key("meter")
        .gap(8.0)
        .center_y()
        .children((!open).then_some(notice).flatten().map(notice_text))
        .child(voice_bars(p, &capsule.bars))
        .child(
            text(elapsed)
                .key("elapsed")
                .font(timer_style())
                .color(p.success()),
        );
    let header = row().key("header").center_y().justify_between().gap(8.0);
    let header = if open {
        let stop = row()
            .key("stop")
            .h(HEADER_HEIGHT - 2.0 * PANEL_PAD)
            .px(ROW_PX)
            .gap(6.0)
            .center_y()
            .radius(ROW_RADIUS)
            .hover(p.k.c.hover)
            .label(tr("Stop"))
            .focusable(true)
            .on_press(|c: &mut Capsule| {
                c.send(ControllerCommand::Stop);
            })
            .child(icon(&icons::STOP, 12.0).color(p.muted()))
            .child(
                text(tr("Stop").to_owned())
                    .font(p.k.ty.text_xsmall)
                    .color(p.muted())
                    .a11y_hidden(),
            );
        header
            .w_full()
            .h(HEADER_HEIGHT)
            .pl(PANEL_PAD)
            .pr(REST_PX)
            .child(stop)
            .child(meter)
    } else {
        header.h(REST_HEIGHT).px(REST_PX).child(meter)
    };
    let top = capsule.corner.is_top();
    let mut parts = vec![header];
    if open {
        if let Some(notice) = notice {
            parts.push(
                row()
                    .key("notice-line")
                    .h(NOTICE_HEIGHT)
                    .px(PANEL_PAD + ROW_PX)
                    .center_y()
                    .child(notice_text(notice)),
            );
        }
        let rows = col()
            .key("rows")
            .px(PANEL_PAD)
            .children(copy_rows(p, capsule, true));
        parts.push(if top {
            rows.pb(PANEL_PAD)
        } else {
            rows.pt(PANEL_PAD)
        });
    }
    if !top {
        parts.reverse();
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
        .children(parts)
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
    in_corner(capsule, if open { body.w(PANEL_WIDTH) } else { body })
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
