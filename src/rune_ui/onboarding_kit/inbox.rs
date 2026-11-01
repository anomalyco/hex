//! The Launch Room's row in a Messages list, waiting before `"Without
//! Hex"`: Jay's messages land on it with a ding each, its red badge counting
//! them, and it pulses until the user clicks it open.

use rune::{Anim, Cursor, El, Spring, SpringPair, TextStyle, Transition, col, el, row, text};

use super::super::controls::Paint;
use super::cards::material;
use super::scene_script::Script;
use super::stories::Onboard;

/// When each message before the button lands on the row, after it parks.
pub(crate) const DINGS: [f64; 2] = [0.55, 1.35];
pub(crate) const ROW_W: f32 = 340.0;
const ROW_H: f32 = 68.0;
const FACE: f32 = 22.0;
const PULSE: f64 = 1.8;

/// The row `since` seconds after the stage parked; a click opens the
/// conversation.
pub(crate) fn launch_row(p: Paint, s: &Onboard, since: f64) -> El {
    let k = p.k;
    let reduced = s.reduced.get();
    let script = Script::chosen();
    let asked: Vec<_> = s
        .scene
        .lines()
        .iter()
        .filter(|line| line.at < 0.0)
        .collect();
    let landed = DINGS.iter().filter(|&&at| since >= at).count();
    let count = landed.min(asked.len()).max(1);
    let newest = asked[count - 1];
    let from = match newest.who {
        super::typing::Who::Them(who) => script.person(who).name,
        _ => "",
    };
    let preview = if from.is_empty() {
        newest.text.clone()
    } else {
        format!("{from}: {}", newest.text)
    };

    // The badge pops each time a message lands.
    let last = DINGS[..landed.max(1)].last().copied().unwrap_or(0.0);
    let pop = if reduced || landed == 0 {
        1.0
    } else {
        1.0 + 0.35 * (-(since - last).max(0.0) / 0.14).exp() as f32
    };
    let badge = el()
        .key("badge")
        .h(18.0)
        .min_w(18.0)
        .px(5.0)
        .radius(9.0)
        .center()
        .fill(rune_theme_opencode::ember(k.dark, 9))
        .scale(pop)
        .scale_motion(Anim::INSTANT)
        .opacity(if landed == 0 { 0.0 } else { 1.0 })
        .opacity_motion(Spring::new(0.2, 0.0))
        .child(
            text(landed.max(1).to_string())
                .key("count")
                .font(TextStyle::new(11.0, 650.0).lh(13.0))
                .color(k.c.white),
        );

    let faces = script.people.iter().take(3).enumerate().fold(
        el().key("faces").size(44.0, 44.0),
        |faces, (i, person)| {
            let [x, y] = [[0.0, 2.0], [20.0, 6.0], [9.0, 22.0]][i];
            faces.child(
                face(p, person.name, person.colour, FACE)
                    .key(format!("f{i}"))
                    .absolute(x, y),
            )
        },
    );

    let title = row()
        .key("top")
        .w_full()
        .center_y()
        .child(
            text(script.title)
                .key("title")
                .grow(1.0)
                .font(TextStyle {
                    weight: 640.0,
                    ..k.ty.ui_medium
                })
                .color(p.text()),
        )
        .child(text("now").key("when").font(k.ty.small).color(p.faint()));
    let bottom = row()
        .key("bottom")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            text(preview)
                .key("preview")
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .font(k.ty.small)
                .color(p.muted())
                .truncate(),
        )
        .child(badge);

    // A soft ring keeps leaving the row while it waits.
    let u = (since / PULSE).fract() as f32;
    let ring = el()
        .key("ring")
        .absolute(0.0, 0.0)
        .size(ROW_W, ROW_H)
        .radius(16.0)
        .continuous_corners()
        .border(1.5, rune_theme_opencode::scale("cobalt", k.dark, 8))
        .scale(1.0 + 0.07 * u)
        .scale_motion(Anim::INSTANT)
        .opacity(if reduced { 0.0 } else { 0.55 * (1.0 - u) })
        .opacity_motion(Anim::INSTANT);
    let breathe = if reduced {
        1.0
    } else {
        1.0 + 0.012 * (since * std::f64::consts::TAU / PULSE).sin() as f32
    };
    let (fill, rim) = material(p);
    let card = row()
        .key("card")
        .absolute(0.0, 0.0)
        .size(ROW_W, ROW_H)
        .px(12.0)
        .gap(12.0)
        .center_y()
        .radius(16.0)
        .continuous_corners()
        .fill(fill)
        .border(p.hair, rim)
        .child(faces)
        .child(
            col()
                .key("text")
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(3.0)
                .child(title)
                .child(bottom),
        );
    let card = k.elevate(card, &k.elevation.floating);
    let open = Transition::opacity()
        .combined(Transition::scale(1.06))
        .combined(Transition::offset(0.0, -6.0));
    let enter = Transition::opacity().combined(Transition::offset(0.0, 12.0));
    col()
        .key("inbox")
        .gap(10.0)
        .items_center()
        .transition(Transition::asymmetric(enter, open).springs(SpringPair::new(
            Spring::new(0.4, 0.0),
            Spring::new(0.22, 0.0),
        )))
        .child({
            let row = el()
                .key("row")
                .size(ROW_W, ROW_H)
                .scale(breathe)
                .scale_motion(Anim::INSTANT)
                .cursor(Cursor::Pointer)
                .child(ring)
                .child(card);
            if s.live.is_some() {
                row.on_press(super::live::on_ui(Onboard::start_scene))
            } else {
                row.on_press(Onboard::start_scene)
            }
        })
        .child(
            text("Click to open")
                .key("hint")
                .font(k.ty.small)
                .color(p.faint())
                .opacity(if landed >= DINGS.len() { 1.0 } else { 0.0 })
                .opacity_motion(Spring::new(0.5, 0.0)),
        )
}

/// A contact's round face with their initial, in their colour.
pub(crate) fn face(p: Paint, name: &str, colour: &str, size: f32) -> El {
    let dark = p.k.dark;
    let initial: String = name.chars().take(1).collect();
    el().size(size, size)
        .radius(size * 0.5)
        .center()
        .fill(rune_theme_opencode::scale(
            colour,
            dark,
            if dark { 6 } else { 5 },
        ))
        .border(p.hair, p.k.c.border_weak)
        .child(
            text(initial)
                .key("initial")
                .font(TextStyle::new(size * 0.46, 620.0).lh(size * 0.6))
                .color(rune_theme_opencode::scale(colour, dark, 11)),
        )
}
