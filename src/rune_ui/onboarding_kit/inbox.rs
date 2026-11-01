//! The Launch Room's row in a Messages list, waiting before `"Without
//! Hex"`: Jay's messages land on it with a ding each, its red badge counting
//! them, a ring leaving it with each, and then a pointer comes in and
//! clicks it open. The click grows the row into the conversation, its faces,
//! title, and Jay's question flying over as themselves ([`MATCHED`]); a
//! click of the user's own opens it sooner.

use rune::{
    Anim, Cursor, El, LineCap, Path, Shape, Spring, SpringPair, TextStyle, Transition, canvas, col,
    el, row, text,
};

use super::super::controls::Paint;
use super::cards::{FIELD_FONT, material};
use super::scene_script::Script;
use super::stories::Onboard;

/// When each message before the button lands on the row, after it parks.
pub(crate) const DINGS: [f64; 2] = [0.55, 1.35];
pub(crate) const ROW_W: f32 = 340.0;
pub(crate) const ROW_H: f32 = 68.0;
const FACE: f32 = 22.0;
/// The ring each ding sends out: how long, and how far past the row.
const RING: f64 = 0.9;
const RING_OUT: f32 = 12.0;
/// The pointer comes in, glides onto the row, presses it, and clicks.
const POINTER_IN: f64 = 1.6;
const GLIDE: f64 = 0.62;
const PRESS_AT: f64 = 2.3;
const PRESS: f64 = 0.12;
/// When the pointer's click opens the conversation, after the row parks.
pub(crate) const CLICK_AT: f64 = 2.5;
/// How far a press sinks the row.
pub(crate) const PRESSED: f32 = 0.97;
/// Where on the row the pointer clicks, and where it comes from.
const AIM: [f32; 2] = [ROW_W * 0.64, ROW_H * 0.6];
const FROM: [f32; 2] = [AIM[0] + 120.0, AIM[1] + 150.0];

/// The ids the row's faces, title, and Jay's question share with the
/// conversation they fly into ([`rune::El::matched`]).
pub(crate) const MATCHED: [&str; 3] = ["launch-room-faces", "launch-room-title", "launch-room-ask"];

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
    // The newest line is the one that flies into the thread, so it keeps
    // the bubble's size.
    let preview_font = FIELD_FONT.lh(16.0);
    let preview = |copy: String, key: &str| {
        text(copy)
            .key(key.to_string())
            .font(preview_font)
            .color(p.muted())
            .shrink()
            .min_w(0.0)
            .truncate()
    };
    let asking = preview(newest.text.clone(), "preview");
    let asking = if count == asked.len() {
        asking.matched(MATCHED[2])
    } else {
        asking
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

    let title = row()
        .key("top")
        .w_full()
        .center_y()
        .child(
            text(script.title)
                .key("title")
                .matched(MATCHED[1])
                .font(TextStyle {
                    weight: 640.0,
                    ..k.ty.ui_medium
                })
                .color(p.text()),
        )
        .child(el().key("gap").grow(1.0))
        .child(text("now").key("when").font(k.ty.small).color(p.faint()));
    let bottom = row()
        .key("bottom")
        .w_full()
        .gap(8.0)
        .center_y()
        .children((!from.is_empty()).then(|| preview(format!("{from}:"), "from")))
        .child(asking)
        .child(el().key("gap").grow(1.0))
        .child(badge);

    // Each ding sends one ring out from the row, the same distance on every
    // side.
    let rings = DINGS.iter().enumerate().filter_map(|(i, &at)| {
        let u = (since - at) / RING;
        (!reduced && (0.0..1.0).contains(&u)).then(|| {
            let u = u as f32;
            let out = RING_OUT * (1.0 - (1.0 - u).powi(3));
            el().key(format!("ring{i}"))
                .absolute(-out, -out)
                .size(ROW_W + 2.0 * out, ROW_H + 2.0 * out)
                .radius(16.0 + out)
                .continuous_corners()
                .border(1.5, rune_theme_opencode::scale("cobalt", k.dark, 8))
                .opacity(0.5 * (1.0 - u).powf(1.6))
                .instant_changes()
                .transition(Transition::identity())
        })
    });
    let pressed = pressed(since, reduced);
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
        .child(group_faces(p, FACE).matched(MATCHED[0]))
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
    let enter = Transition::opacity().combined(Transition::offset(0.0, 12.0));
    // The conversation grows out of the row, which goes at once; under
    // Reduce Motion they cross-fade.
    let goes = Spring::new(if reduced { 0.15 } else { 0.05 }, 0.0);
    let row = el()
        .key("row")
        .size(ROW_W, ROW_H)
        .scale(1.0 - (1.0 - PRESSED) * pressed)
        .scale_motion(Anim::INSTANT)
        .cursor(Cursor::Pointer)
        .children(rings)
        .child(card);
    let row = if s.live.is_some() {
        row.on_press(super::live::on_ui(Onboard::start_scene))
    } else {
        row.on_press(Onboard::start_scene)
    };
    col()
        .key("inbox")
        .w(ROW_W)
        .transition(
            Transition::asymmetric(enter, Transition::opacity())
                .springs(SpringPair::new(Spring::new(0.4, 0.0), goes)),
        )
        .child(row)
}

/// How far the pointer's press has sunk the row, `0..=1`.
fn pressed(since: f64, reduced: bool) -> f32 {
    if reduced {
        0.0
    } else {
        smooth(((since - PRESS_AT) / PRESS) as f32)
    }
}

/// The pointer that clicks the row open, over the row's top-left corner
/// at `origin`: it fades in on its way, glides onto the row on one ease,
/// dips with the press, and once the conversation has been open `opened`
/// seconds, lifts and fades.
pub(crate) fn pointer(
    origin: [f32; 2],
    since: f64,
    opened: Option<f64>,
    reduced: bool,
) -> Option<El> {
    if since < POINTER_IN || opened.is_some_and(|opened| opened > 0.6) {
        return None;
    }
    let lifted = opened.map_or(0.0, |opened| smooth((opened / 0.1) as f32));
    let gone = opened.map_or(0.0, |opened| smooth(((opened - 0.15) / 0.4) as f32));
    let u = if reduced {
        1.0
    } else {
        smooth(((since - POINTER_IN) / GLIDE) as f32)
    };
    let at = [
        FROM[0] + (AIM[0] - FROM[0]) * u,
        FROM[1] + (AIM[1] - FROM[1]) * u,
    ];
    let arrow = Path::new()
        .move_to([1.0, 1.0])
        .line_to([1.0, 16.0])
        .line_to([5.0, 12.5])
        .line_to([7.6, 18.2])
        .line_to([10.2, 17.0])
        .line_to([7.7, 11.6])
        .line_to([12.6, 11.2])
        .close();
    let shape = Shape::new()
        .view_box(14.0, 20.0)
        .fill_path(&arrow, rune::Color::BLACK)
        .stroke_path(&arrow, 1.2, LineCap::Round, rune::Color::WHITE);
    let pressed = pressed(since, reduced) * (1.0 - lifted);
    Some(
        canvas(shape)
            .key("pointer")
            .absolute(origin[0] + at[0] - 1.0, origin[1] + at[1] - 1.0)
            .size(14.0 * 1.3, 20.0 * 1.3)
            .origin(0.0, 0.0)
            .scale(1.0 - 0.12 * pressed)
            .opacity(((since - POINTER_IN) / 0.18).clamp(0.0, 1.0) as f32 * (1.0 - gone))
            .instant_changes()
            .raise(),
    )
}

fn smooth(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// The group's faces, overlapping: three of them, `face` points across.
pub(crate) fn group_faces(p: Paint, face: f32) -> El {
    let script = Script::chosen();
    let k = face / FACE;
    script.people.iter().take(3).enumerate().fold(
        el().key("faces").size(44.0 * k, 44.0 * k),
        |faces, (i, person)| {
            let [x, y] = [[0.0, 2.0], [20.0, 6.0], [9.0, 22.0]][i];
            faces.child(
                // Its own entrance would fade it in under the flight that
                // carries the group.
                self::face(p, person.name, person.colour, face)
                    .key(format!("f{i}"))
                    .absolute(x * k, y * k)
                    .transition(Transition::identity()),
            )
        },
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
                .color(rune_theme_opencode::scale(colour, dark, 11))
                .transition(Transition::identity()),
        )
}
