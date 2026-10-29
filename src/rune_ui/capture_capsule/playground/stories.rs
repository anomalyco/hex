//! The capsule's states as stories in Hex's own window playground (the
//! `playground` experiment): each of [`SCENES`] on the capsule's overlay
//! frame, driven by the same fake host, with the states, tuning, and log
//! beside it. Sound and its volume stay Hex's.

use std::time::Duration;

use rune::{Cx, El, OverlaySpec, Play, Story, col, el, row, text};

use super::super::notch::{self, DISPLAY_27, MACBOOK_PRO_14, Screen};
use super::super::surface::{Anchor, PANEL_H, PANEL_W};
use super::super::{PANEL_MARGIN, capsule};
use super::{
    CapsuleFlow, CapsuleState, Input, Paint, SCENES, SLOW, Step, Tuning, init, log, poll, states,
    tuning,
};

/// The side column's content width (Rune's 264 pt column, less padding).
const SIDE_W: f32 = 240.0;
/// Leaves room under the side column for the story's triggers.
const SIDE_H: f32 = 440.0;
const GROUP: &str = "Prototypes/Capture capsule";

// One story per scene below; adding a scene means adding its index too.
const _: () = assert!(SCENES.len() == 9);

/// `playground` with the whole flow and every capsule scene as stories.
pub(crate) fn add<S: 'static>(playground: rune::Playground<S>) -> rune::Playground<S> {
    playground
        .story(
            story("Prototypes/Capture capsule/whole flow", init_flow).trigger(
                'r',
                "Play the whole flow",
                play,
            ),
        )
        .story(scene::<0>())
        .story(scene::<1>())
        .story(scene::<2>())
        .story(scene::<3>())
        .story(scene::<4>())
        .story(scene::<5>())
        .story(scene::<6>())
        .story(scene::<7>())
        .story(scene::<8>())
        .story(notch_story())
        .story(super::family::story())
}

fn scene<const I: usize>() -> Story<CapsuleState> {
    let name = Box::leak(format!("{GROUP}/{}", SCENES[I].0).into_boxed_str());
    story(name, init_scene::<I>).trigger('p', "Play the whole flow", play)
}

/// A story on the capsule's overlay frame. Its first variant keeps the
/// flow the Flow card picked; the rest are each flow after a capture, so a
/// link names one (`?variant=toast`).
fn story(name: &'static str, init: fn() -> CapsuleState) -> Story<CapsuleState> {
    let frame = OverlaySpec::new("Hex capture", (PANEL_W, PANEL_H), stage);
    let mut story = controls(Story::overlay(name, &frame, init)).variant("tuned", |_| {});
    for (variant, flow) in FLOWS {
        story = story.variant(variant, move |s| {
            toggle(s, |pg| pg.set_tuning(Tuning { flow, ..pg.tuning }))
        });
    }
    story
}

/// Each flow after a capture, by the variant that shows it.
const FLOWS: [(&str, CapsuleFlow); 5] = [
    ("list", CapsuleFlow::List),
    ("toast", CapsuleFlow::Toast),
    ("menu-bar", CapsuleFlow::MenuBar),
    ("cursor", CapsuleFlow::Cursor),
    ("notch", CapsuleFlow::Notch),
];
const _: () = assert!(FLOWS.len() == CapsuleFlow::ALL.len());

/// The capsule's notch flow on a MacBook's notch and on a flat display's
/// pretend one, at their real sizes in points.
fn notch_story() -> Story<CapsuleState> {
    let pick = |screens| move |s: &mut CapsuleState| toggle(s, |pg| pg.screens = screens);
    controls(Story::new(
        "Prototypes/Capture capsule/notch",
        init_notch,
        screens,
    ))
    .trigger('r', "Play the whole flow", play)
    .variant("Side by side", pick(Screens::Both))
    .variant(MACBOOK_PRO_14.name, pick(Screens::One(MACBOOK_PRO_14)))
    .variant(DISPLAY_27.name, pick(Screens::One(DISPLAY_27)))
}

/// The triggers and side column every capsule story has.
fn controls(story: Story<CapsuleState>) -> Story<CapsuleState> {
    story
        .poll(poll)
        .trigger('s', "Slow motion", |s, _| {
            toggle(s, |pg| pg.slow = !pg.slow)
        })
        .trigger('l', "Loop", |s, play| {
            toggle(s, |pg| {
                pg.looping = !pg.looping;
                if pg.looping && pg.queue.is_empty() {
                    pg.play();
                }
            });
            play.wake_after(Duration::ZERO);
        })
        .trigger('g', "Saved rule", |s, _| toggle(s, |pg| pg.rule = !pg.rule))
        .trigger('h', "Hide", |s, play| {
            toggle(s, |pg| pg.at(0.0, Step::Input(Input::Dismiss)));
            play.wake_after(Duration::ZERO);
        })
        .side(side)
}

fn init_flow() -> CapsuleState {
    let mut state = init();
    state.backdrop = false;
    state
}

fn init_notch() -> CapsuleState {
    let mut state = init_flow();
    toggle(&mut state, |pg| {
        pg.set_tuning(Tuning {
            flow: CapsuleFlow::Notch,
            ..pg.tuning
        })
    });
    state
}

fn init_scene<const I: usize>() -> CapsuleState {
    let mut state = init_flow();
    (SCENES[I].1)(&mut state);
    state
}

fn play(s: &mut CapsuleState, play: &mut Play<'_, CapsuleState>) {
    toggle(s, super::Playground::play);
    play.wake_after(Duration::ZERO);
}

fn toggle(s: &mut CapsuleState, f: impl FnOnce(&mut super::Playground)) {
    if let Some(pg) = s.playground.as_mut() {
        f(pg);
    }
}

/// The capsule alone, as its overlay draws it: clear around its shape.
fn stage(s: &CapsuleState, cx: &mut Cx<CapsuleState>) -> El {
    let p = clock(s, cx);
    capsule(p, &s.surface, &s.live, cx)
}

/// Slow motion and Reduce Motion for the frame, and its paint.
fn clock(s: &CapsuleState, cx: &mut Cx<CapsuleState>) -> Paint {
    if s.playground.as_ref().is_some_and(|pg| pg.slow) {
        cx.time_scale(SLOW as f32);
    }
    s.reduced.set(cx.is_reduced_motion());
    Paint::new(cx.system_dark(), cx.env().scale)
}

/// Which screens the notch story shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Screens {
    /// The top of the MacBook's and the flat display's, side by side.
    #[default]
    Both,
    /// One whole screen.
    One(Screen),
}

/// How much of each screen's top the side-by-side shows: the capsule's
/// whole panel.
const TOP_OF: (f32, f32) = (PANEL_W, 300.0);
/// The black glass around a screen.
const BEZEL: f32 = 8.0;

fn screens(s: &CapsuleState, cx: &mut Cx<CapsuleState>) -> El {
    let p = clock(s, cx);
    match s.playground.as_ref().map_or(Screens::Both, |pg| pg.screens) {
        Screens::Both => row()
            .key("screens")
            .gap(32.0)
            .child(top_of(p, s, MACBOOK_PRO_14, cx))
            .child(top_of(p, s, DISPLAY_27, cx)),
        Screens::One(screen) => {
            let (w, h) = screen.size;
            el().key("bezel")
                .size(w + 2.0 * BEZEL, h + 2.0 * BEZEL)
                .radius(BEZEL * 2.0)
                .fill(p.k.c.black)
                .clip_rounded(BEZEL * 2.0)
                .child(desktop(p, s, screen, cx).absolute(BEZEL, BEZEL))
        }
    }
}

/// The top middle of `screen` under its bezel, and what it is.
fn top_of(p: Paint, s: &CapsuleState, screen: Screen, cx: &mut Cx<CapsuleState>) -> El {
    let (w, h) = TOP_OF;
    let notch = if screen.real {
        "its notch"
    } else {
        "a pretend notch"
    };
    col()
        .key(screen.name)
        .gap(10.0)
        .child(
            el().key("top")
                .size(w, BEZEL + h)
                .fill(p.k.c.black)
                .clip_rounded(0.0)
                .child(desktop(p, s, screen, cx).absolute((w - screen.size.0) * 0.5, BEZEL)),
        )
        .child(
            text(format!(
                "{}, {notch}: {} × {} pt",
                screen.name,
                notch::WIDTH,
                screen.notch
            ))
            .key("name")
            .font(p.k.ty.ui_medium.lh(16.0))
            .color(p.k.c.text_muted),
        )
}

/// A whole screen: its desktop, menu bar and notch, and the capsule
/// hanging from the notch (or under the menu bar in the other flows).
fn desktop(p: Paint, s: &CapsuleState, screen: Screen, cx: &mut Cx<CapsuleState>) -> El {
    let k = p.k;
    let (w, h) = screen.size;
    let wash = |name, step| rune_theme_opencode::scale(name, k.dark, step);
    let left = (w - PANEL_W) * 0.5;
    let at_notch = s.surface.anchor == Anchor::Notch;
    let top = if at_notch {
        screen.notch
    } else {
        screen.bar + PANEL_MARGIN
    };
    let mut desktop = el()
        .key("desktop")
        .size(w, h)
        .fill(wash("cobalt", 3))
        .linear_gradient(
            std::f32::consts::PI * 0.75,
            [(0.0, wash("cobalt", 3)), (1.0, wash("lilac", 5))],
        )
        .child(menu_bar(p, screen.size.0, screen.bar))
        .child(notch::notch("notch", k.c.black, w * 0.5, 0.0, screen.notch))
        .child(capsule(p, &s.surface, &s.live, cx).absolute(left, top));
    if at_notch {
        let [x, _, sw, _] = s.surface.shell;
        desktop = desktop.children(notch::ears(
            "capsule-ears",
            k.c.black,
            left + x,
            sw,
            0.0,
            s.surface.shell_motion,
        ));
    }
    desktop
}

/// A menu bar's placeholders: the app's menus at the left, status items
/// at the right, clear of the notch.
pub(super) fn menu_bar(p: Paint, w: f32, bar: f32) -> El {
    let k = p.k;
    let ink = k.alpha(k.c.text_base, 0.28);
    let item = |width: f32| el().size(width, 8.0).radius(4.0).fill(ink);
    let menus = row()
        .key("menus")
        .absolute(20.0, 0.0)
        .h(bar)
        .center_y()
        .gap(20.0)
        .child(el().size(13.0, 13.0).radius(3.0).fill(ink))
        .children([34.0, 28.0, 30.0, 40.0, 50.0, 30.0].map(item));
    let status = row()
        .key("status")
        .absolute(w - 300.0, 0.0)
        .w(280.0)
        .h(bar)
        .center_y()
        .justify_end()
        .gap(16.0)
        .children([14.0, 14.0, 22.0, 14.0, 14.0].map(item))
        .child(item(90.0));
    el().key("menu-bar")
        .absolute(0.0, 0.0)
        .size(w, bar)
        .fill(k.alpha(k.c.bg_base, 0.35))
        .child(menus)
        .child(status)
}

fn side(s: &CapsuleState, cx: &mut Cx<CapsuleState>) -> El {
    let Some(pg) = s.playground.as_deref() else {
        return col();
    };
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let on: Vec<&str> = [
        (pg.slow, "Slow motion"),
        (pg.looping, "Loop"),
        (pg.rule, "Saved rule"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    let body = col()
        .key("capsule-side")
        .w(SIDE_W)
        .gap(14.0)
        .children((!on.is_empty()).then(|| {
            text(format!("On: {}", on.join(", ")))
                .key("capsule-on")
                .font(p.k.ty.ui_medium.lh(16.0))
                .color(p.k.c.text_muted)
        }))
        .child(states(p).px(0.0))
        .child(tuning(p, pg, SIDE_W - 2.0 * super::CARD_PAD, false))
        .child(log(p, pg).px(0.0));
    col()
        .key("capsule-side-scroll")
        .w(SIDE_W)
        .h(SIDE_H)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .child(body)
}
