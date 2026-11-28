//! How the stack draws: each card where its flight has it, heading for
//! its slot, the newest in the corner. Its flight places it every frame,
//! so the layout's own springs stay out of the way.

use rune::{Anim, Cursor, Cx, El, PopupItem, PopupMenu, Spring, SpringPair, Transition, Tx, el};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::flight::{lifted, mix};
use super::{
    CLOSE_INSET, CLOSE_RADIUS, CardKey, Cards, Entry, FADE, INSET, LEAVE, Lent, PILL_H, RADIUS,
    REDUCED, TRAVEL_SCALE,
};
use crate::desktop::i18n::tr;
use crate::rune_ui::controls::Paint;

static CLOSE: &rune::Icon = &rune_theme_opencode::icons::CLOSE;

pub(crate) fn view(s: &Cards, cx: &mut Cx<Cards>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let (sw, sh) = s.screen;
    let reduced = cx.is_reduced_motion();
    s.reduced.set(reduced);
    let mut root = el()
        .key("screenshot-thumbnail")
        .size(sw, sh)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT);
    for card in &s.cards {
        root = root.child(entry(p, card, s.hovered == Some(card.id), reduced));
    }
    root
}

/// One card where its flight has it: flat and square as the shot it was
/// on the display, a card with its corner and shadow once it has lifted
/// off. A card the editor draws stays in its slot, unseen and out of the
/// pointer's way.
fn entry(p: Paint, card: &Entry, hovered: bool, reduced: bool) -> El {
    let k = p.k;
    let id = card.id;
    let rect = card.flight.rect();
    let [x, y, w, h] = rect;
    let lift = card.flight.lift();
    let away = card.lent == Lent::Away;
    let mut shell = el()
        .key(format!("card-{id}"))
        .absolute(x, y)
        .size(w, h)
        .radius(mix(0.0, RADIUS, lift))
        .fill(k.c.bg_base)
        .opacity(if away { 0.0 } else { 1.0 })
        .opacity_motion(if reduced {
            Anim::from(REDUCED)
        } else {
            Anim::INSTANT
        });
    for shadow in &k.elevation.floating {
        shell = shell.box_shadow(lifted(*shadow, lift));
    }
    let image = card.sharp.as_ref().unwrap_or(&card.shot.image);
    let shell = shell
        .pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
        .child(
            el().absolute(0.0, 0.0)
                .size(w, h)
                .radius(mix(0.0, RADIUS, lift))
                .border(p.hair, k.c.border_weak)
                .opacity(lift),
        )
        // Its flight places it each frame; Reduce Motion fades it.
        .motion(Spring::NONE)
        .transition(travel(rect, reduced));
    if away {
        return shell;
    }
    // Its labels come in as it arrives: none while it's still twice its
    // card's width.
    let tw = card.flight.target()[2].max(1.0);
    let arrived = (2.0 - w / tw).clamp(0.0, 1.0);
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
            .opacity(arrived)
    });
    let recording = card.shot.recording.is_some();
    let shell = shell
        .cursor(Cursor::Pointer)
        .on_press_tx(move |s: &mut Cards, tx| s.open(id, tx))
        .on_secondary_press(move |_: &mut Cards, press, tx| menu(id, recording, press, tx))
        .drag_out(move |s: &Cards| {
            s.get(id).map(|card| {
                rune::DragItem::new()
                    .file(card.shot.path.clone())
                    .within_app()
            })
        })
        .hit()
        .on_hover_change(move |s: &mut Cards, on, tx| {
            if on {
                s.hover(Some(id));
            } else if s.hovered == Some(id) {
                s.hover(None);
            }
            tx.animated();
        })
        .child(close);
    match pill {
        Some(pill) => shell.child(pill),
        None => shell,
    }
}

/// How a card comes and goes. It comes as its flight brings it, so it
/// shows whole from its first frame; it goes past the display's right
/// edge, leaning back about the corner it rests in. Reduce Motion only
/// fades.
fn travel([_, _, w, _]: [f32; 4], reduced: bool) -> Transition {
    if reduced {
        Transition::opacity().springs(SpringPair::new(REDUCED, REDUCED))
    } else {
        let leave = Transition::offset(w + INSET, 0.0).combined(Transition::scale_anchor(
            TRAVEL_SCALE,
            rune::Vec2::new(1.0, 1.0),
        ));
        Transition::asymmetric(Transition::identity(), leave).springs(SpringPair::new(FADE, LEAVE))
    }
}

/// The card's right-click menu.
fn menu(id: u64, recording: bool, press: rune::PointerPress, tx: &mut Tx) {
    let mut menu = PopupMenu::new().item(PopupItem::new("edit", tr("Open in Editor")));
    if !recording {
        menu = menu.item(PopupItem::new("preview", super::files::preview_label()));
    }
    let menu = menu
        .item(PopupItem::new("copy", tr("Copy")))
        .item(PopupItem::new("reveal", tr("Show in Finder")))
        .separator()
        .item(PopupItem::new("trash", tr("Move to Trash")))
        .item(PopupItem::new("close", tr("Close")));
    tx.popup_menu(menu, press, move |s: &mut Cards, picked, tx| {
        let key = match picked {
            Some("edit") => CardKey::Edit,
            Some("preview") => CardKey::Preview,
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
