//! The router's list hanging under the open capsule: rows, hints, badges
//! and footer.
use super::*;

/// Between the search field and the first section.
const FIELD_GAP: f32 = 4.0;

/// The list hanging under the open capsule, `x` and `w` its shell's:
/// searching, its field on top, then the suggestions and the recent
/// sessions under their own headers, every row 32 pt on the 6 pt inset.
/// The highlighted row says what ⏎ does there.
pub(super) fn list_view(
    p: Paint,
    list: &List,
    typed: &rune::TextEdit,
    x: f32,
    w: f32,
    reduced: bool,
) -> El {
    let k = p.k;
    let h = list_height(list);
    // Rows and headers keep their identity across a filter: the ones that
    // stay slide to their new slot sharp, the rest soften out in place.
    let blur = if reduced { 0.0 } else { PRESENCE_BLUR };
    let scale = if reduced { 1.0 } else { 0.96 };
    let t = tuning::get();
    let springs = SpringPair::new(t.content_in, t.content_out);
    let in_place = Transition::blur(blur)
        .combined(Transition::scale(scale))
        .springs(springs);
    let mut body = col().absolute(0.0, 0.0).w(w).px(LIST_PAD).pt(LIST_PAD);
    let suggesting = list.suggesting();
    let saving = list.always.as_ref().is_some_and(|(_, shift)| *shift);
    if !suggesting {
        body = body.child(search_field(p, typed).mb(FIELD_GAP));
    }
    let split = list.suggested.min(list.rows.len());
    let sections = [
        (tr("Suggested"), 0..split),
        (tr("Recent"), split..list.rows.len()),
    ];
    for (title, range) in sections {
        if range.is_empty() {
            continue;
        }
        body = body.child(
            row()
                .key(format!("section-{title}"))
                .items_end()
                .h(HEADER_H)
                .px(ROW_PX)
                .pb(HEADER_PB)
                .child(label(title, k.ty.small_medium, p.faint()))
                .transition(in_place),
        );
        for n in range {
            let dest = &list.rows[n];
            let highlighted = !suggesting && n == list.selected;
            let send_w = hint_width(tr("Send"), &["⏎"]);
            // The name keeps its width; a long reason is cut first, to no
            // less than two fifths of the row.
            let content = w - 2.0 * LIST_PAD - 2.0 * ROW_PX - 2.0 * ROW_PX;
            let why_room = (content - measure(&dest.name, medium())).max(content * 0.4);
            let why = ellipsize(&dest.why, small(), why_room);
            let trail = if highlighted {
                send_w
            } else {
                measure(&why, small())
            };
            let max = content - trail;
            let reason = label(
                &why,
                small(),
                if saving {
                    if list.target == Some(n) {
                        p.accent()
                    } else {
                        p.muted()
                    }
                } else if n == 0 {
                    p.accent()
                } else {
                    p.muted()
                },
            )
            .offset(0.0, (medium().size - small().size) * BASELINE_DROP);
            let mut entry = row()
                .key(format!("row-{}", dest.name))
                .trace_name("row")
                .center_y()
                .h(ROW_H)
                .px(ROW_PX)
                .gap(ROW_PX)
                .radius(ROW_RADIUS)
                .press(k.c.pressed)
                .child(label(
                    &ellipsize(&dest.name, medium(), max),
                    medium(),
                    p.text(),
                ))
                .child(el().grow(1.0))
                // The highlighted row says what ⏎ does; the rest say why.
                .child(if highlighted {
                    hint(p, tr("Send"), &["⏎"]).key("send-hint")
                } else {
                    reason
                })
                .role("button")
                .label(dest.name.clone())
                .hit()
                .on_press_tx(press(Id::Row(n)))
                .on_pointer_move_local(move |s: &mut CapsuleState, _, tx| s.point(n, true, tx))
                .on_hover_change(move |s: &mut CapsuleState, on, tx| {
                    if on {
                        tx.live_only();
                    } else {
                        s.point(n, false, tx);
                    }
                })
                .transition(in_place);
            if highlighted {
                entry = entry.fill(k.c.layer_02);
            } else {
                entry = entry.hover(k.c.hover);
            }
            body = body.child(entry);
        }
    }
    if unmatched(list) {
        body = body.child(
            row()
                .key("unmatched")
                .center_y()
                .h(ROW_H)
                .px(ROW_PX)
                .child(label(tr("No sessions match"), medium(), p.faint()))
                .transition(in_place),
        );
    }
    let footer = match (&list.always, list.footer) {
        (_, Footer::Suggesting) => Some(suggesting_footer(p, list.queued)),
        (Some((site, true)), _) => Some(saving_hint(p, list, site, w)),
        // Send is on the highlighted row; the footer keeps the rules.
        (_, Footer::Search { .. }) => Some(search_hints(p, false, false, list.queued, w)),
    };
    list_panel(p, body, footer, x, w, h, blur)
}

/// The search field at the top of the open list: the kit's plain search
/// input in a calm recessed well, a 32 pt row whose corners match the rows'.
/// It is always typed into, so it never wears a focus ring: the caret says
/// it. It takes its own clicks, so a click in it never reaches the app
/// behind.
fn search_field(p: Paint, typed: &rune::TextEdit) -> El {
    let c = &p.k.c;
    let mut edit = typed.clone();
    edit.focused = true;
    let input = TextInput::new(&edit)
        .placeholder(tr("Search sessions…"))
        .label(tr("Send to"))
        .leading_icon(p.k.glyph.search)
        .plain()
        .width(None)
        .height(ROW_H)
        .build(p.k);
    row()
        .key("field")
        .trace_name("field")
        .h(ROW_H)
        .w_full()
        .radius(ROW_RADIUS)
        .fill(c.bg_deep)
        .border(p.hair * 2.0, c.border_weak)
        .box_shadow(rune::Shadow {
            y: 1.0,
            blur: 2.0,
            color: p.k.alpha(c.black, if p.k.dark { 0.35 } else { 0.06 }),
            inset: true,
            ..Default::default()
        })
        .child(input.grow(1.0))
        .hit()
}

/// A quiet clickable footer action in the Send-to list.
fn footer_action(p: Paint, key: &str, text_label: &'static str, id: Id) -> El {
    row()
        .key(key.to_owned())
        .h(22.0)
        .px(6.0)
        .center_y()
        .radius(ROW_RADIUS - 2.0)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .role("button")
        .label(text_label)
        .hit()
        .on_press_tx(press(id))
        .child(label(text_label, small(), p.muted()))
}

/// Footer for the suggesting Send-to list: optional `Cancel send` (when opened
/// from Queued) and `All rules…` opening `Modal::RoutingRules`.
fn suggesting_footer(p: Paint, queued: bool) -> El {
    row()
        .center_y()
        .gap(HINTS_GAP)
        .children(queued.then(|| footer_action(p, "cancel-queued", tr("Cancel send"), Id::Undo)))
        .child(el().grow(1.0))
        .child(footer_action(
            p,
            "all-rules",
            tr("All rules…"),
            Id::OpenRules,
        ))
}

/// The search's hints, as one quiet line: what each key does, then the
/// key ("Send ⏎"); the highlighted row already says where. `All rules…`
/// (and `Cancel send` when opened from Queued) sits on the right.
pub(super) fn search_hints(p: Paint, remember: bool, send: bool, queued: bool, w: f32) -> El {
    const SEND: [&str; 1] = ["⏎"];
    const REMEMBER: [&str; 2] = ["⇧", "⏎"];
    let room = w - 2.0 * LIST_PAD - 2.0 * ROW_PX;
    let rules_w = measure(tr("All rules…"), small()) + 12.0;
    let cancel_w = if queued {
        measure(tr("Cancel send"), small()) + 12.0 + HINTS_GAP
    } else {
        0.0
    };
    let right_w = rules_w + cancel_w;
    let used = if send {
        hint_width(tr("Send"), &SEND) + HINTS_GAP
    } else {
        0.0
    };
    let remember =
        remember && used + hint_width(tr("Remember"), &REMEMBER) + HINTS_GAP + right_w <= room;
    // Send and Remember share a growing row, so the gaps drawn are the
    // gaps measured: no spacer with a gap on each side.
    let left = row()
        .key("left")
        .center_y()
        .gap(HINTS_GAP)
        .grow(1.0)
        .children(send.then(|| hint(p, tr("Send"), &SEND).key("send")))
        .children(remember.then(|| hint(p, tr("Remember"), &REMEMBER).key("remember")));
    row()
        .center_y()
        .gap(HINTS_GAP)
        .child(left)
        .children(queued.then(|| footer_action(p, "cancel-queued", tr("Cancel send"), Id::Undo)))
        .child(footer_action(
            p,
            "all-rules",
            tr("All rules…"),
            Id::OpenRules,
        ))
}

/// Shift down: the footer says where the rule will send `site`, each in
/// a badge with its kind's icon: "Always send [🌐 site] to [💬 SEER]",
/// following the row under the pointer, else the highlighted one.
pub(super) fn saving_hint(p: Paint, list: &List, site: &str, w: f32) -> El {
    let room = w - 2.0 * LIST_PAD - 2.0 * ROW_PX;
    let to = list.target_dest().map(|dest| dest.name.as_str());
    let words = measure(tr("Always send"), small()) + measure(tr("to"), small()) + 3.0 * BADGE_GAP;
    let badges =
        if to.is_some() { 2.0 } else { 1.0 } * (2.0 * BADGE_PX + BADGE_ICON + BADGE_ICON_GAP);
    let left = (room - words - badges).max(0.0);
    let to_w = to.map_or(0.0, |to| measure(to, small()));
    let site_w = measure(site, small());
    let (site_room, to_room) = if site_w + to_w <= left {
        (site_w, to_w)
    } else {
        let to_room = to_w.min(left * 0.4);
        (left - to_room, to_room)
    };
    let mut hint = row()
        .center_y()
        .gap(BADGE_GAP)
        .key("saving")
        .child(label(tr("Always send"), small(), p.muted()))
        .child(
            badge(
                p,
                Badge::of_site(site),
                &ellipsize(site, small(), site_room),
            )
            .key("site"),
        );
    hint = match to {
        Some(to) => hint
            .child(label(tr("to"), small(), p.muted()))
            .child(badge(p, Badge::Chat, &ellipsize(to, small(), to_room)).key("to")),
        None => hint.child(label(tr("to…"), small(), p.muted())),
    };
    hint.child(el().grow(1.0))
}

pub(super) const BADGE_H: f32 = 20.0;
pub(super) const BADGE_PX: f32 = 6.0;
pub(super) const BADGE_RADIUS: f32 = 5.0;
pub(super) const BADGE_GAP: f32 = 6.0;
pub(super) const BADGE_ICON: f32 = 12.0;
pub(super) const BADGE_ICON_GAP: f32 = 4.0;

/// What a [`badge`] names, which picks its icon.
#[derive(Clone, Copy)]
pub(super) enum Badge {
    Website,
    App,
    Chat,
}

impl Badge {
    /// A rule's key is a site's host or an app's name.
    fn of_site(site: &str) -> Self {
        if site.contains('.') && !site.contains(char::is_whitespace) {
            Self::Website
        } else {
            Self::App
        }
    }
}

/// A name in a rounded rect after its kind's icon: what a Shift pick's
/// rule ties together.
pub(super) fn badge(p: Paint, kind: Badge, name: &str) -> El {
    let k = p.k;
    row()
        .center()
        .gap(BADGE_ICON_GAP)
        .h(BADGE_H)
        .px(BADGE_PX)
        .radius(BADGE_RADIUS)
        .fill(k.alpha(k.c.text_base, 0.09))
        .child(
            canvas(badge_icon(kind, mark_stroke(BADGE_ICON), p.muted()))
                .size(BADGE_ICON, BADGE_ICON),
        )
        .child(label(name, small(), p.text()))
}

/// A badge's line icon on the 24 × 24 grid: a globe, an app window, or a
/// chat bubble.
pub(super) fn badge_icon(kind: Badge, width: f32, color: Color) -> Shape {
    let line = |a: [f32; 2], b: [f32; 2]| Path::new().move_to(a).line_to(b);
    let paths = match kind {
        Badge::Website => vec![
            rounded(3.0, 3.0, 18.0, 18.0, 9.0),
            line([3.0, 12.0], [21.0, 12.0]),
            Path::new()
                .move_to([12.0, 3.0])
                .cubic_to([7.5, 8.0], [7.5, 16.0], [12.0, 21.0])
                .cubic_to([16.5, 16.0], [16.5, 8.0], [12.0, 3.0])
                .close(),
        ],
        Badge::App => vec![
            rounded(3.0, 3.5, 18.0, 17.0, 3.0),
            line([3.0, 8.5], [21.0, 8.5]),
        ],
        Badge::Chat => vec![
            Path::new()
                .move_to([7.0, 4.0])
                .line_to([17.0, 4.0])
                .cubic_to([19.2, 4.0], [21.0, 5.8], [21.0, 8.0])
                .line_to([21.0, 13.0])
                .cubic_to([21.0, 15.2], [19.2, 17.0], [17.0, 17.0])
                .line_to([11.0, 17.0])
                .line_to([6.5, 20.5])
                .line_to([6.5, 17.0])
                .cubic_to([4.6, 16.8], [3.0, 15.1], [3.0, 13.0])
                .line_to([3.0, 8.0])
                .cubic_to([3.0, 5.8], [4.8, 4.0], [7.0, 4.0])
                .close(),
        ],
    };
    paths
        .iter()
        .fold(Shape::new().view_box(24.0, 24.0), |shape, path| {
            shape.stroke_path(path, width, LineCap::Round, color)
        })
}

/// The footer's height, and the list's below its last row.
pub(super) fn footer_height() -> f32 {
    match tuning::get().list_footer {
        ListFooter::Band => LIST_PAD + BAND_H,
        ListFooter::Well => LIST_PAD + WELL_H + LIST_PAD,
    }
}

/// The hints set apart from the rows: (A) a full-width band on a quiet
/// fill under a rule, or (B) a rounded well inset like the rows.
pub(super) fn footer_box(p: Paint, hints: El, w: f32, h: f32) -> El {
    let k = p.k;
    match tuning::get().list_footer {
        ListFooter::Band => el()
            .key("footer")
            .absolute(0.0, h - BAND_H)
            .size(w, BAND_H)
            .fill(k.c.layer_01)
            .child(
                el().key("rule")
                    .absolute(0.0, 0.0)
                    .size(w, p.hair)
                    .fill(k.c.border_weak),
            )
            .child(
                row()
                    .absolute(0.0, 0.0)
                    .size(w, BAND_H)
                    .center_y()
                    .px(LIST_PAD + ROW_PX)
                    .child(hints.grow(1.0)),
            ),
        ListFooter::Well => el()
            .key("footer")
            .absolute(LIST_PAD, h - LIST_PAD - WELL_H)
            .size(w - 2.0 * LIST_PAD, WELL_H)
            .radius(ROW_RADIUS)
            .fill(k.c.layer_01)
            .child(
                row()
                    .absolute(0.0, 0.0)
                    .size(w - 2.0 * LIST_PAD, WELL_H)
                    .center_y()
                    .px(ROW_PX)
                    .child(hints.grow(1.0)),
            ),
    }
}

pub(super) fn list_panel(
    p: Paint,
    body: El,
    hints: Option<El>,
    x: f32,
    w: f32,
    h: f32,
    blur: f32,
) -> El {
    let k = p.k;
    let mut panel = el()
        .key("list")
        .trace_name("list")
        .absolute(x, LIST_Y)
        .size(w, h)
        .radius(LIST_RADIUS)
        .fill(k.c.bg_base);
    for shadow in &k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    // The clip is on an inner box: clipping the panel itself would cut off
    // its own ring and shadow.
    panel = panel.child(
        el().absolute(0.0, 0.0)
            .size(w, h)
            .radius(LIST_RADIUS)
            .clip_rounded(LIST_RADIUS)
            .child(body)
            .children(hints.map(|hints| footer_box(p, hints, w, h))),
    );
    if k.dark {
        panel = panel.child(rim(k, w, h, LIST_RADIUS));
    }
    panel
        .motion(tuning::get().island)
        .role("list")
        .label(tr("Send to"))
        .hit()
        // Hex's tap reports a press on the list before the list hears it:
        // the pointer being here keeps it from reading as a click away.
        .on_hover_change(|s: &mut CapsuleState, on, tx| s.hover(Part::List, on, tx))
        .on_pointer_move_local(|s: &mut CapsuleState, _, tx| s.pointer_moved(tx))
        .transition(
            Transition::offset(0.0, -LIST_DROP)
                .combined(Transition::opacity())
                .combined(Transition::blur(blur))
                .springs(SpringPair::new(
                    tuning::get().content_in,
                    tuning::get().content_out,
                )),
        )
}

/// A search that matches nothing: it says so rather than show no rows.
pub(super) fn unmatched(list: &List) -> bool {
    !list.suggesting() && list.rows.is_empty()
}

/// Whether the list shows its header: over rows, or the room a search
/// keeps for them.
pub(super) fn headed(list: &List) -> bool {
    !list.rows.is_empty() || list.min_rows > 0
}

/// The list's height: its header, rows (at least the room it keeps), and
/// the search's footer.
pub(crate) fn list_height(list: &List) -> f32 {
    let split = list.suggested.min(list.rows.len());
    let sections = usize::from(split > 0) + usize::from(list.rows.len() > split);
    let header = if headed(list) {
        sections.max(1) as f32 * HEADER_H
    } else {
        0.0
    };
    let field = if list.suggesting() {
        0.0
    } else {
        ROW_H + FIELD_GAP
    };
    let rows = list
        .rows
        .len()
        .max(list.min_rows)
        .max(usize::from(unmatched(list))) as f32;
    let footer = footer_height();
    LIST_PAD + field + header + rows * ROW_H + footer
}
