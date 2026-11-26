//! The finished meeting's panel (a title link over its copies, closed by a
//! ring round ✕ that drains over its stay) and the open recording's
//! "Discard recording?" question.

use super::*;

/// The second step of the open recording's ✕: the question and what it
/// costs on one text rail, then Keep and Discard at the panel's button
/// inset. No icon: Discard is the one red mark. `top` when the header is
/// above it, so the buttons sit on the panel's free edge.
pub(super) fn discard_confirm(p: Paint, top: bool, stopping: bool) -> El {
    super::super::super::controls::capsule_confirm(
        p,
        tr("Discard recording?"),
        tr("Deletes its audio and transcript"),
        top,
        stopping,
        (tr("Keep"), |c: &mut Capsule, tx| {
            c.keep_recording();
            c.stir(tx);
        }),
        (tr("Discard"), |c: &mut Capsule, tx| {
            c.request_discard();
            c.stir(tx);
        }),
    )
    .h(CONFIRM_BODY)
}

/// The finished meeting: its title and length as a link that opens it in
/// Hex, the summary when there is one, then Copy agent prompt over Copy
/// transcript. A ring round its ✕ drains over its stay, held while the pointer
/// is inside.
pub(in crate::rune_ui::meeting_capsules) fn ready(
    p: Paint,
    capsule: &Capsule,
    manifest: &MeetingManifest,
) -> El {
    let top = capsule.corner.is_top();
    let title = meeting::display_title(manifest);
    // A renamed meeting's old words go before the new ones arrive.
    let reduced = capsule.reduced.get();
    let (words, strength, soft) = capsule.surface.title.line(title.clone(), reduced);
    let id = manifest.id.clone();
    let link = row()
        .key("open-meeting")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .px(ROW_PX)
        .py(TITLE_PY)
        .gap(6.0)
        .center_y()
        .radius(ROW_RADIUS)
        .role("link")
        .label(tr_fill("Open {} in Hex", &title))
        .cursor(Cursor::Pointer)
        .focusable(true)
        .focus_ring(2., -2., p.k.c.border_focus)
        .no_window_drag()
        .hover(p.k.c.hover)
        .hover_group()
        .on_press_tx(move |c: &mut Capsule, tx| {
            if let Some(open) = &c.open_meeting {
                open(id.clone());
            }
            c.close();
            c.stir(tx);
        })
        .child(
            col()
                .key("title-block")
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(2.0)
                // The cue trails the title's words on their line, not the ✕.
                .child(
                    row()
                        .key("title-line")
                        .gap(6.0)
                        .center_y()
                        .child(
                            text(words)
                                .key("title")
                                .font(p.k.ty.ui_medium)
                                .color(p.text())
                                .truncate()
                                .shrink()
                                .min_w(0.0)
                                .opacity(strength)
                                .opacity_motion(Anim::INSTANT)
                                .blur(soft)
                                .blur_motion(Anim::INSTANT)
                                .a11y_hidden(),
                        )
                        .child(
                            icon(&icons::OUTLINE_ARROW_UP_RIGHT, 14.0)
                                .key("open-cue")
                                .color(p.muted())
                                // Quiet at rest, so the title reads as a link
                                // before the pointer finds it.
                                .group_opacity(0.6, 1.0)
                                .a11y_hidden(),
                        ),
                )
                .children(manifest.duration_ms.map(|duration| {
                    text(meeting::format_duration(duration))
                        .key("elapsed")
                        .font(p.tabular(p.meta()))
                        .color(p.muted())
                })),
        );
    let close = match &capsule.state {
        State::Ready { stay, .. } => Some(stay),
        _ => None,
    }
    .map(|stay| close_with_ring(p, stay));
    let header = row()
        .key("ready-header")
        .pl(PANEL_PAD)
        .pr(SPINE - CLOSE_BOX / 2.0)
        .gap(2.0)
        .center_y()
        .child(link)
        .children(close);
    let summary = ready_summary(manifest).map(|(summary, written)| {
        text(summary)
            .key("summary")
            .font(p.description())
            .color(if written { p.muted() } else { p.faint() })
            .max_lines(SUMMARY_LINES)
            .px(CAPSULE.text_rail())
    });
    let live = &capsule.live;
    let copied = |kind| live.copied(kind).then(|| copied_hint(p));
    let rows = col()
        .key("rows")
        .px(PANEL_PAD)
        .child(panel_row(
            p,
            "copy-prompt",
            &icons::OUTLINE_CONSOLE,
            tr("Copy agent prompt"),
            copied(LiveCopy::Prompt),
            true,
            true,
            1.0,
            |c, tx| c.copy_prompt(tx),
        ))
        .child(panel_row(
            p,
            "copy-all",
            &icons::OUTLINE_COPY,
            tr("Copy transcript"),
            copied(LiveCopy::All),
            false,
            true,
            1.0,
            |c, tx| c.copy_all(tx),
        ));
    // The title band sits on the anchored edge; 8 pt stay clear under its
    // hover fill, and the summary keeps 12 pt from the copies.
    let contents = col().key("panel-contents").w(PANEL_WIDTH).py(PANEL_PAD);
    let contents = if top {
        contents
            .child(header.mb(TITLE_CLEAR))
            .children(summary.map(|summary| summary.mb(SUMMARY_GAP)))
            .child(rows)
    } else {
        contents
            .child(rows)
            .children(summary.map(|summary| summary.mt(SUMMARY_GAP)))
            .child(header.mt(TITLE_CLEAR))
    };
    // Dragged from anywhere its rows and link leave free, like the pill.
    let body = panel_body(p, &title)
        .cursor(Cursor::Grab)
        .on_drag(|c: &mut Capsule, d, tx| c.drag(d, tx))
        .motion(RECORDING_OPEN)
        .child(contents)
        .on_hover_change(|c: &mut Capsule, inside, tx| {
            c.set_hovered(inside);
            c.wake_for_stay(tx);
            c.stir(tx);
        });
    in_corner(capsule, presence::anchored(capsule, body))
}

/// The ready panel's height for `manifest`: the title band, the summary
/// while there is one, and the two copies.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn ready_height(manifest: &MeetingManifest) -> f32 {
    let band = 2.0 * TITLE_PY + 20.0 + 2.0 + 12.0;
    let summary = if ready_summary(manifest).is_some() {
        16.0 * SUMMARY_LINES as f32 + SUMMARY_GAP
    } else {
        0.0
    };
    2.0 * PANEL_PAD + band + TITLE_CLEAR + summary + COPIES_HEIGHT
}

/// The ready panel's ✕, centred on the icon spine inside the ring that
/// drains over its stay.
fn close_with_ring(p: Paint, stay: &stay::Stay) -> El {
    el().key("close-slot")
        .size(CLOSE_BOX, CLOSE_BOX)
        .child(stay::ring(p, stay))
        .child(
            row()
                .key("close-box")
                .size(CLOSE_BOX, CLOSE_BOX)
                .center()
                .child(
                    IconButton::new(&icons::CLOSE_SMALL)
                        .variant(IconButtonVariant::GhostMuted)
                        .control_size(ControlSize::Small)
                        .label(tr("Close"))
                        .build(p.k)
                        .key("dismiss")
                        .cursor(Cursor::Pointer)
                        .no_window_drag()
                        .on_press_tx(|c: &mut Capsule, tx| {
                            c.close();
                            c.stir(tx);
                        }),
                ),
        )
}
