//! The capsule's states as stories in Hex's own window playground (the
//! `playground` experiment): each of [`SCENES`] on the capsule's overlay
//! frame, driven by the same fake host, with the states, tuning, and log
//! beside it. Sound and its volume stay Hex's.

use std::time::Duration;

use rune::{Cx, El, OverlaySpec, Play, Story, col, text};

use super::super::capsule;
use super::super::surface::{PANEL_H, PANEL_W};
use super::{CapsuleState, Input, Paint, SCENES, SLOW, Step, init, log, poll, states, tuning};

/// The side column's content width (Rune's 264 pt column, less padding).
const SIDE_W: f32 = 240.0;
/// Leaves room under the side column for the story's triggers.
const SIDE_H: f32 = 440.0;
const GROUP: &str = "Capture capsule";

// One story per scene below; adding a scene means adding its index too.
const _: () = assert!(SCENES.len() == 9);

/// `playground` with the whole flow and every capsule scene as stories.
pub(crate) fn add<S: 'static>(playground: rune::Playground<S>) -> rune::Playground<S> {
    playground
        .story(story("Capture capsule/whole flow", init_flow).trigger(
            'r',
            "Play the whole flow",
            play,
        ))
        .story(scene::<0>())
        .story(scene::<1>())
        .story(scene::<2>())
        .story(scene::<3>())
        .story(scene::<4>())
        .story(scene::<5>())
        .story(scene::<6>())
        .story(scene::<7>())
        .story(scene::<8>())
}

fn scene<const I: usize>() -> Story<CapsuleState> {
    let name = Box::leak(format!("{GROUP}/{}", SCENES[I].0).into_boxed_str());
    story(name, init_scene::<I>).trigger('p', "Play the whole flow", play)
}

fn story(name: &'static str, init: fn() -> CapsuleState) -> Story<CapsuleState> {
    let frame = OverlaySpec::new("Hex capture", (PANEL_W, PANEL_H), stage);
    Story::overlay(name, &frame, init)
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
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    if s.playground.as_ref().is_some_and(|pg| pg.slow) {
        cx.time_scale(SLOW as f32);
    }
    s.reduced.set(cx.is_reduced_motion());
    capsule(p, &s.surface, &s.live, cx)
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
