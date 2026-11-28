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

/// The ready title link's padding round the title and length: the title's
/// line centres on the ✕ box beside it.
const READY_TITLE_PY: f32 = (CLOSE_BOX - 20.0) / 2.0;

/// The finished meeting: its title and length as a link that opens it in
/// Hex, then Copy agent prompt over Copy transcript, every row 32 pt on the
/// panel's 6 pt inset. The ✕ is a 28 pt box pinned 6 pt in from the top
/// and side, and a rounded-rect ring round it drains over its stay, held
/// while the pointer is inside. No summary: the link opens the meeting.
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
        .py(READY_TITLE_PY)
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
        .px(PANEL_PAD)
        .gap(2.0)
        .items_start()
        .child(link)
        .children(close);
    let live = &capsule.live;
    let copied = |kind| live.copied(kind).then(|| copied_hint(p));
    let rows = col()
        .key("rows")
        .px(PANEL_PAD)
        .child(panel_row(
            p,
            "copy-prompt",
            &icons::PROMPT,
            tr("Copy agent prompt"),
            copied(LiveCopy::Prompt),
            true,
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
            false,
            true,
            1.0,
            |c, tx| c.copy_all(tx),
        ));
    // The title band sits on the anchored edge, a row's gap from the
    // copies, as the copies are from each other.
    let contents = col().key("panel-contents").w(PANEL_WIDTH).py(PANEL_PAD);
    let contents = if top {
        contents.child(header.mb(ROW_GAP)).child(rows)
    } else {
        contents.child(rows).child(header.mt(ROW_GAP))
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

/// The ready panel's height: the title band and the two copies.
pub(super) fn ready_height(_: &MeetingManifest) -> f32 {
    let band = 2.0 * READY_TITLE_PY + 20.0 + 2.0 + 16.0;
    2.0 * PANEL_PAD + band + ROW_GAP + COPIES_HEIGHT
}

/// The ready panel's ✕: the capsules' 28 pt concentric close box, inside
/// the rounded-rect ring that drains over its stay.
fn close_with_ring(p: Paint, stay: &stay::Stay) -> El {
    el().key("close-slot")
        .size(CLOSE_BOX, CLOSE_BOX)
        .child(super::super::auto_started::dismiss_button(p))
        .child(stay::ring(p, stay))
}
