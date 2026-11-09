//! How the stack draws: each card in its slot, the newest in the corner.

use rune::{Anim, Cursor, Cx, El, PopupItem, PopupMenu, Spring, SpringPair, Transition, Tx, el};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::{
    CLOSE_INSET, CLOSE_RADIUS, CardKey, Cards, ENTER, Entry, FADE, INSET, LEAVE, PILL_H, RADIUS,
    REDUCED, TRAVEL_SCALE, ZOOM,
};
use crate::desktop::i18n::tr;
use crate::rune_ui::controls::Paint;

static CLOSE: &rune::Icon = &rune_theme_opencode::icons::CLOSE;

pub(crate) fn view(s: &Cards, cx: &mut Cx<Cards>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let (sw, sh) = s.screen;
    let reduced = cx.is_reduced_motion();
    let mut root = el()
        .key("screenshot-thumbnail")
        .size(sw, sh)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT);
    for (card, (id, rect)) in s.cards.iter().zip(s.slots()) {
        debug_assert_eq!(card.id, id);
        root = root.child(entry(p, card, rect, s.hovered == Some(id), reduced));
    }
    root
}

/// One card at `rect`.
fn entry(p: Paint, card: &Entry, rect: [f32; 4], hovered: bool, reduced: bool) -> El {
    let k = p.k;
    let id = card.id;
    let [x, y, w, h] = rect;
    let mut shell = el()
        .key(format!("card-{id}"))
        .absolute(x, y)
        .size(w, h)
        .radius(RADIUS)
        .fill(k.c.bg_base);
    for shadow in &k.elevation.floating {
        shell = shell.box_shadow(*shadow);
    }
    let close = IconButton::new(CLOSE)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .label(tr("Close"))
        .on_press_tx(move |s: &mut Cards, tx| s.close(id, tx))
        .build(k)
        .key("close")
        .absolute(CLOSE_INSET, CLOSE_INSET)
        .opacity(if hovered { 1.0 } else { 0.0 })
        .opacity_motion(FADE);
    // A note ("Copied") stands in for a recording's length while it shows.
    let pill = match (&card.note, &card.shot.recording) {
        (Some((note, _)), _) => Some(label(p, note.clone())),
        (None, Some(recording)) => Some(label(
            p,
            super::editor::clip::duration_label(recording.duration_ms),
        )),
        (None, None) => None,
    }
    .map(|pill| {
        pill.key("pill")
            .absolute(CLOSE_INSET, h - CLOSE_INSET - PILL_H)
    });
    let shell = shell
        .pixels_fit(card.shot.image.pixels(), rune::SurfaceFit::Fit)
        .cursor(Cursor::Pointer)
        .on_press_tx(move |s: &mut Cards, tx| s.open(id, tx))
        .on_secondary_press(move |_: &mut Cards, press, tx| menu(id, press, tx))
        .drag_out(move |s: &Cards| {
            s.get(id)
                .map(|card| rune::DragItem::new().file(card.shot.path.clone()))
        })
        .child(
            el().absolute(0.0, 0.0)
                .size(w, h)
                .radius(RADIUS)
                .border(p.hair, k.c.border_weak),
        )
        .hit()
        .on_hover_change(move |s: &mut Cards, on, tx| {
            if on {
                s.hover(Some(id));
            } else if s.hovered == Some(id) {
                s.hover(None);
            }
            tx.animated();
        })
        // The older cards make room on the HUD's spring; Reduce Motion
        // moves them at once.
        .motion(if reduced { Spring::NONE } else { ENTER })
        .transition(travel(card, rect, reduced))
        .child(close);
    match pill {
        Some(pill) => shell.child(pill),
        None => shell,
    }
}

/// How a card comes and goes. A shot grows in from the rectangle it was
/// taken from, at that rectangle's size and place, and shrinks into its
/// slot; without one (a whole display, a recording, or once it has been in
/// a moment) it comes and goes past the display's right edge, leaning back
/// about the corner it rests in. Reduce Motion only fades.
fn travel(card: &Entry, [x, y, w, _]: [f32; 4], reduced: bool) -> Transition {
    if reduced {
        Transition::opacity().springs(SpringPair::new(REDUCED, REDUCED))
    } else if let Some([cx0, cy0, cw, _]) = card.capture {
        let scale = (cw / w).max(0.02);
        Transition::offset(cx0 - x, cy0 - y)
            .combined(Transition::scale_anchor(scale, rune::Vec2::new(0.0, 0.0)))
            .springs(SpringPair::new(ZOOM, LEAVE))
    } else {
        Transition::offset(w + INSET, 0.0)
            .combined(Transition::scale_anchor(
                TRAVEL_SCALE,
                rune::Vec2::new(1.0, 1.0),
            ))
            .springs(SpringPair::new(ENTER, LEAVE))
    }
}

/// The card's right-click menu.
fn menu(id: u64, press: rune::PointerPress, tx: &mut Tx) {
    let menu = PopupMenu::new()
        .item(PopupItem::new("edit", tr("Open in Editor")))
        .item(PopupItem::new("copy", tr("Copy")))
        .item(PopupItem::new("reveal", tr("Show in Finder")))
        .separator()
        .item(PopupItem::new("trash", tr("Move to Trash")))
        .item(PopupItem::new("close", tr("Close")));
    tx.popup_menu(menu, press, move |s: &mut Cards, picked, tx| {
        let key = match picked {
            Some("edit") => CardKey::Edit,
            Some("copy") => CardKey::Copy,
            Some("reveal") => CardKey::Reveal,
            Some("trash") => CardKey::Trash,
            Some("close") => CardKey::Close,
            _ => return,
        };
        s.act(id, key, tx);
    });
}

/// A short label in the card's bottom-left corner: a recording's length,
/// or a note.
fn label(p: Paint, text: String) -> El {
    let k = p.k;
    let mut pill = rune::row()
        .h(PILL_H)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(CLOSE_RADIUS)
        .fill(k.c.bg_base);
    for shadow in &k.elevation.floating {
        pill = pill.box_shadow(*shadow);
    }
    pill.child(
        rune::text(text)
            .font(crate::rune_ui::capture_capsule::surface::small())
            .color(p.text())
            .single_line(),
    )
}
