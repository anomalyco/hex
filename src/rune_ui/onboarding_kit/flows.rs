//! The onboarding options, each composed from the kit's pieces on the
//! full-window page.

use rune::{Beat, El, Sequence, Transition, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, glyphs};

use super::super::controls::{Paint, PushSize};
use super::cards::{ADAM, DAX, FRANK, JAY, Notice, Reply, floating, notice};
use super::fixture::Download;
use super::hex_meter::hex_meter;
use super::landing::{LEAD, arrives, heading, keys, language_pick, ready};
use super::mic::mic_level;
use super::mic_line::mic_line;
use super::pill::{FLY, PILL, pill, slot};
use super::stories::{Flow, Onboard, Step};
use super::words::shout;

const TOP_BAR: f32 = 48.0;
const ACTION_H: f32 = 32.0;
const LINE_FONT: rune::TextStyle = rune::TextStyle::new(20.0, 500.0);
const STAGGER: f64 = 0.07;
const SENTENCE_W: f32 = 640.0;
const METER: f32 = 132.0;
const ARRIVE: &[Beat] = &[Beat::new(0.0).opacity(0.0).scale(0.96), Beat::new(0.32)];

pub(super) fn view(p: Paint, s: &Onboard) -> El {
    let page = match s.flow {
        Flow::Pill => pill_flow(p, s),
        Flow::Fill => fill_flow(p, s),
        Flow::Shout => shout_flow(p, s),
        Flow::Versus => super::versus::view(p, s),
        Flow::FirstRun => super::first_run::view(p, s),
    };
    col()
        .key("onboarding")
        .w_full()
        .h_full()
        .fill(p.k.c.bg_deep)
        .children(page)
}

fn top_bar() -> El {
    row().key("top-bar").w_full().h(TOP_BAR).px(20.0).center_y()
}

fn middle() -> El {
    col().key("middle").grow(1.0).shrink().w_full().center()
}

fn action(content: El) -> El {
    row().key("action").h(ACTION_H).center().child(content)
}

fn drift(s: &Onboard) -> Option<f64> {
    (!s.reduced.get()).then_some(s.now)
}

fn download(s: &Onboard) -> Download {
    s.download.expect("the download starts at the pick")
}

/// Primary onward button (`Continue →`), matching the tour's forward CTA.
pub(super) fn continue_button(p: Paint, s: &Onboard) -> El {
    let button = Button::new("Continue")
        .variant(ButtonVariant::Contrast)
        .label_with(|font, ink| {
            row()
                .gap(6.0)
                .center_y()
                .child(text("Continue").key("label").font(font).color(ink))
                .child(icon(&glyphs::ARROW_RIGHT, 15.0).key("arrow").color(ink))
        });
    let button = if s.live.is_some() {
        button.on_press(super::live::press_continue)
    } else {
        button.on_press(Onboard::press_continue)
    };
    button.build_push(p.k).key("continue").transition(arrives())
}

/// Starts `"Without Hex"` once the user is watching.
pub(super) fn watch_button(p: Paint, s: &Onboard) -> El {
    let button = Button::new("Watch what happens")
        .variant(ButtonVariant::Contrast)
        .label_with(|font, ink| {
            row()
                .gap(7.0)
                .center_y()
                .child(icon(&glyphs::TRIANGLE_RIGHT, 13.0).key("play").color(ink))
                .child(
                    text("Watch what happens")
                        .key("label")
                        .font(font)
                        .color(ink),
                )
        });
    let button = if s.live.is_some() {
        button.on_press(super::live::on_ui(Onboard::start_scene))
    } else {
        button.on_press(Onboard::start_scene)
    };
    button.build_push(p.k).key("watch").transition(arrives())
}

/// `Skip →`: a quiet way out once the comedy plays.
pub(super) fn skip_button(p: Paint, s: &Onboard) -> El {
    let font = p.k.ty.small;
    let button = Button::new("Skip")
        .variant(ButtonVariant::GhostFaint)
        .label_with(move |_, ink| {
            row()
                .gap(4.0)
                .center_y()
                .child(text("Skip").key("label").font(font).color(ink))
                .child(icon(&glyphs::ARROW_RIGHT, 12.0).key("arrow").color(ink))
        });
    let button = if s.live.is_some() {
        button.on_press(super::live::on_ui(|s| {
            s.skip_scene();
        }))
    } else {
        button.on_press(|s: &mut Onboard| {
            s.skip_scene();
        })
    };
    button
        .build_small_push(p.k)
        .key("skip")
        .transition(Transition::opacity().springs(rune::SpringPair::new(
            rune::Spring::new(0.6, 0.0),
            rune::Spring::new(0.2, 0.0),
        )))
}

fn hold(p: Paint, s: &Onboard, rest: &str) -> El {
    row()
        .key("hold")
        .gap(8.0)
        .center_y()
        .transition(Transition::opacity())
        .child(text("Hold").key("hold").font(LEAD).color(p.muted()))
        .child(keys(p, "keys", &s.caps, s.step == Step::Hearing))
        .child(text(rest).key("rest").font(LEAD).color(p.muted()))
}

fn jay_reply<'a>(s: &'a Onboard, hint: &'a str) -> Reply<'a> {
    match s.step {
        Step::YourTurn => Reply::Ask {
            keys: &s.caps,
            hint,
        },
        Step::Hearing => Reply::Hearing {
            partial: s.partial,
            meter: s.meter,
        },
        Step::Heard | Step::Done | Step::Ready => Reply::Sent(s.partial),
        _ => Reply::Empty,
    }
}

fn example(s: &Onboard, index: usize) -> Reply<'_> {
    match s.example(index) {
        ("", _) => Reply::Empty,
        (typed, false) => Reply::Typing(typed),
        (typed, true) => Reply::Sent(typed),
    }
}

fn pill_flow(p: Paint, s: &Onboard) -> Vec<El> {
    if s.step == Step::Ready {
        return vec![
            top_bar()
                .justify_center()
                .child(pill(p, download(s), s.closed)),
            middle().pb(TOP_BAR).child(ready(
                p,
                &s.caps,
                None,
                s.ready_age(),
                s.now,
                s.reduced.get(),
                s.motion,
            )),
        ];
    }
    let parked = s.step >= Step::Parked;
    let top = top_bar().justify_end().child(if parked {
        pill(p, download(s), s.closed)
    } else {
        slot()
    });
    let cards =
        col()
            .key("cards")
            .gap(12.0)
            .items_center()
            .child(floating(
                notice(p, &FRANK, example(s, 0), true),
                drift(s),
                0.0,
            ))
            .child(floating(
                notice(p, &ADAM, example(s, 1), true),
                drift(s),
                0.4,
            ))
            .children(parked.then(|| {
                notice(p, &JAY, jay_reply(s, "to answer Jay"), false).transition(arrives())
            }))
            .child(mic_line(p, parked && s.can_hear(), s.listening_peak()));
    let words = if s.can_hear() {
        "Hold a key and talk. Hex types it wherever your cursor is."
    } else {
        "Hex starts listening once the speech model has downloaded."
    };
    let (key, title, lead) = match s.step {
        Step::Pick | Step::Started => ("intro", "Talk instead of typing", words),
        Step::Parked if !s.can_hear() => ("wait", "Almost ready", words),
        Step::Heard | Step::Done => (
            "heard",
            "That's it",
            "Hex pastes wherever your cursor is, in any app.",
        ),
        _ => ("turn", "Your turn", "Answer Jay with your voice."),
    };
    let act = match s.step {
        Step::Pick => language_pick(p, Onboard::pick),
        Step::Started => pill(p, download(s), s.closed),
        Step::Done => continue_button(p, s),
        Step::Heard => text("Finishing the download…")
            .key("finishing")
            .font(p.k.ty.small)
            .color(p.faint())
            .transition(Transition::opacity()),
        _ => el().key("none"),
    };
    vec![
        top,
        middle()
            .pb(TOP_BAR)
            .gap(28.0)
            .child(cards)
            .child(heading(p, key, title, lead))
            .child(action(act)),
    ]
}

fn arriving(card: El, index: usize) -> El {
    card.sequence(Sequence::new(ARRIVE).delay(index as f64 * STAGGER))
        .transition(Transition::scale(0.96))
}

fn fill_flow(p: Paint, s: &Onboard) -> Vec<El> {
    if s.step == Step::Pick {
        return vec![
            top_bar(),
            middle()
                .pb(TOP_BAR)
                .gap(28.0)
                .child(shared_meter(p, 96.0, 0.0, 0.0))
                .child(heading(
                    p,
                    "intro",
                    "Talk instead of typing",
                    "Hold a key and talk. Hex types it wherever your cursor is.",
                ))
                .child(action(language_pick(p, Onboard::pick))),
        ];
    }
    if s.step == Step::Ready {
        return vec![
            top_bar(),
            middle().pb(TOP_BAR).child(ready(
                p,
                &s.caps,
                Some(PILL),
                s.ready_age(),
                s.now,
                s.reduced.get(),
                s.motion,
            )),
        ];
    }
    let d = download(s);
    let hearing = s.step == Step::Hearing;
    let card = |n: &Notice, index: usize, reply: Reply<'_>| {
        let dim = hearing && n.key != JAY.key;
        arriving(
            floating(notice(p, n, reply, false), drift(s), index as f64 * 0.5),
            index,
        )
        .opacity(if dim { 0.45 } else { 1.0 })
    };
    let meter = col()
        .key("meter-column")
        .gap(12.0)
        .items_center()
        .child(shared_meter(p, METER, d.fraction(), s.closed))
        .child(if d.done() {
            super::super::lists::allowed(p, "status", "Ready").transition(Transition::opacity())
        } else {
            text(format!("{}%", (d.fraction() * 100.0).round() as u32))
                .key("status")
                .font(p.k.ty.small.features("'tnum' 1"))
                .color(p.muted())
        });
    let lead = if s.can_hear() {
        hold(p, s, "to answer any of these.")
    } else {
        text("You can answer these once the speech model has downloaded.")
            .key("lead")
            .font(LEAD)
            .color(p.muted())
            .transition(Transition::opacity())
    };
    let head = col()
        .key("head")
        .gap(10.0)
        .items_center()
        .child(
            text("Answer one while it downloads")
                .key("title")
                .font(super::landing::title_font(p))
                .color(p.text()),
        )
        .child(row().key("lead-row").h(ACTION_H).center().child(lead));
    let cards = row()
        .key("cards")
        .w_full()
        .px(24.0)
        .flex_wrap()
        .gaps(16.0, 16.0)
        .justify_center()
        .child(card(&JAY, 0, jay_reply(s, "to answer")))
        .child(card(&DAX, 1, Reply::Empty));
    vec![
        top_bar(),
        middle()
            .pb(TOP_BAR)
            .gap(24.0)
            .child(meter)
            .child(head)
            .child(
                col()
                    .key("try")
                    .w_full()
                    .gap(12.0)
                    .items_center()
                    .child(cards)
                    .child(mic_line(p, s.can_hear(), s.listening_peak())),
            ),
    ]
}

fn shared_meter(p: Paint, size: f32, progress: f32, closed: f32) -> El {
    el().key(PILL)
        .matched(PILL)
        .motion(FLY)
        .child(hex_meter(p, "meter", size, progress, closed))
}

fn sentence(p: Paint, s: &Onboard, finished: bool) -> El {
    shout(p, "words", s.partial, finished, LINE_FONT, 28.0)
        .w_full()
        .max_w(SENTENCE_W)
        .px(32.0)
}

fn shout_flow(p: Paint, s: &Onboard) -> Vec<El> {
    let parked = s.step >= Step::Parked;
    let top = top_bar().justify_end().child(if parked {
        pill(p, download(s), s.closed)
    } else {
        slot()
    });
    let stage = match s.step {
        Step::Pick | Step::Started => el().key("empty"),
        Step::Parked | Step::YourTurn => {
            mic_level(p, "mic", 56.0, s.meter.0, s.meter.1, false).transition(arrives())
        }
        Step::Hearing => col()
            .key("hearing")
            .gap(16.0)
            .items_center()
            .w_full()
            .child(sentence(p, s, false))
            .child(mic_level(p, "mic", 20.0, s.meter.0, s.meter.1, true)),
        _ => sentence(p, s, true),
    };
    let below = match s.step {
        Step::Ready => ready(
            p,
            &s.caps,
            None,
            s.ready_age(),
            s.now,
            s.reduced.get(),
            s.motion,
        ),
        Step::Pick | Step::Started => heading(
            p,
            "intro",
            "Say something",
            "Hold a key and talk. Hex types it wherever your cursor is.",
        ),
        Step::Parked if !s.can_hear() => heading(
            p,
            "wait",
            "Almost ready",
            "Words start once the speech model has downloaded.",
        ),
        Step::Heard | Step::Done => heading(
            p,
            "heard",
            "That's it",
            "Hex pastes wherever your cursor is, in any app.",
        ),
        _ => heading(p, "turn", "Say anything", "It doesn't have to be good."),
    };
    let act = match s.step {
        Step::Pick => language_pick(p, Onboard::pick),
        Step::Started => pill(p, download(s), s.closed),
        Step::Parked if !s.can_hear() => el().key("none"),
        Step::Parked | Step::YourTurn | Step::Hearing => hold(p, s, "and talk."),
        _ => el().key("none"),
    };
    vec![
        top,
        middle()
            .pb(TOP_BAR)
            .gap(28.0)
            .child(col().key("stage").h(150.0).w_full().center().child(stage))
            .child(below)
            .child(
                col()
                    .key("try")
                    .gap(12.0)
                    .items_center()
                    .child(action(act))
                    .child(mic_line(
                        p,
                        parked && s.can_hear() && s.step < Step::Ready,
                        s.listening_peak(),
                    )),
            ),
    ]
}
