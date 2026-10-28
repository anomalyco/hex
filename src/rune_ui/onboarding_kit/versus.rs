//! Without Hex, then with it: while the model downloads, someone answers
//! Jay on the keyboard, slowly, fixing typos as they go; then they hold the
//! shortcut and say the same reply, and its words fly into the field the
//! way they do on the site's hero ([`super::flight`]).
//!
//! Everything on the stage is placed by hand, so the words in flight know
//! where the microphone and the field are without reading back layout.

use rune::{Anim, El, Transition, col, el, row, text};

use super::super::controls::Paint;
use super::cards::{CARD_W, FIELD_FONT, JAY, PEEK, Reply, field_text_origin, notice, stacked};
use super::field_text::{self, slots};
use super::fixture::ANSWER_JAY;
use super::flight::{Flight, caret_x, flicker, flying, typeset};
use super::flows::continue_button;
use super::keyboard::{self, keyboard};
use super::landing::{LEAD, TITLE, arrives, keys, language_pick, ready};
use super::mic::mic_level;
use super::mic_line::mic_line;
use super::pill::{pill, slot};
use super::stories::{Onboard, Step};
use super::typing;

const STAGE_W: f32 = 640.0;
const STAGE_H: f32 = 352.0;
/// The card's left edge, over the keyboard's right edge, and its bottom,
/// with the cards behind it peeking out just above the keys.
const CARD_X: f32 = 600.0 - CARD_W;
const CARD_BOTTOM: f32 = 156.0;
/// Cards behind Jay's, as Notification Center stacks them.
const BEHIND: usize = 2;
/// The microphone the words leave from, left of the card.
const MOUTH: [f32; 2] = [124.0, 60.0];
const MOUTH_D: f32 = 64.0;
/// The hero opens calm; words fly small and quick.
const CALM: f32 = 0.12;
/// Without a streaming model the whole reply arrives at release; its words
/// leave this far apart.
const BURST: f64 = 0.05;
const ACTION_H: f32 = 32.0;
const TOP_BAR: f32 = 48.0;

/// How many lines the reply takes in the field.
fn reply_lines() -> usize {
    field_text::wrap(ANSWER_JAY.text()).len()
}

/// Where the field's text starts on the stage.
fn field_origin() -> [f32; 2] {
    field_text_origin([CARD_X, CARD_BOTTOM], reply_lines())
}

/// Seconds since typing started, if it has.
fn typing_elapsed(s: &Onboard) -> Option<f64> {
    s.typing_from.map(|from| s.now - from)
}

fn typed_done(s: &Onboard) -> bool {
    typing_elapsed(s).is_some_and(|e| e >= typing::duration())
}

/// When each word of the reply is heard, in seconds after the key went
/// down: as the partials stream, or all at release.
pub(crate) fn heard_at(streams: bool) -> Vec<f64> {
    let count = ANSWER_JAY.text().split_whitespace().count();
    (0..count)
        .map(|i| {
            if streams {
                ANSWER_JAY
                    .updates
                    .iter()
                    .find(|(_, partial)| partial.split_whitespace().count() > i)
                    .map_or(ANSWER_JAY.release, |(at, _)| *at)
            } else {
                ANSWER_JAY.release + i as f64 * BURST
            }
        })
        .collect()
}

/// Every word's flight into the field, from when the key went down.
fn flights(s: &Onboard) -> Vec<(Flight, field_text::Slot<'static>)> {
    let Some(from) = s.take_from else {
        return Vec::new();
    };
    let origin = field_origin();
    let mouth = MOUTH;
    slots(ANSWER_JAY.text())
        .into_iter()
        .zip(heard_at(s.streams()))
        .enumerate()
        .map(|(i, (slot, at))| {
            let to = [origin[0] + slot.centre[0], origin[1] + slot.centre[1]];
            (
                Flight::new(mouth, to, from + at, CALM, FIELD_FONT.size, i),
                slot,
            )
        })
        .collect()
}

pub(super) fn view(p: Paint, s: &Onboard) -> Vec<El> {
    if s.step == Step::Ready {
        return vec![
            top_bar()
                .justify_center()
                .child(pill(p, download(s), s.closed)),
            middle().pb(TOP_BAR).child(ready(p, &s.caps, None)),
        ];
    }
    let parked = s.step >= Step::Parked;
    let top = top_bar().justify_end().child(if parked {
        pill(p, download(s), s.closed)
    } else {
        slot()
    });
    let act = match s.step {
        Step::Pick => language_pick(p, Onboard::pick),
        Step::Started => pill(p, download(s), s.closed),
        Step::Heard if !s.downloaded() => text("Finishing the download…")
            .key("finishing")
            .font(p.k.ty.small)
            .color(p.faint())
            .transition(Transition::opacity()),
        Step::Done => continue_button(p),
        _ => el().key("none"),
    };
    vec![
        top,
        middle()
            .pb(TOP_BAR)
            .gap(20.0)
            .child(heading(p, s))
            .child(stage(p, s))
            .child(
                col()
                    .key("try")
                    .gap(12.0)
                    .items_center()
                    .child(row().key("action").h(ACTION_H).center().child(act))
                    .child(mic_line(
                        p,
                        s.can_hear() && s.step >= Step::YourTurn,
                        s.listening_peak(),
                    )),
            ),
    ]
}

fn top_bar() -> El {
    row().key("top-bar").w_full().h(TOP_BAR).px(20.0).center_y()
}

fn middle() -> El {
    col().key("middle").grow(1.0).shrink().w_full().center()
}

fn download(s: &Onboard) -> super::fixture::Download {
    s.download.expect("the download starts at the pick")
}

fn seconds(secs: f64) -> String {
    format!("{secs:.1} s")
}

fn typed_secs() -> f64 {
    typing::duration()
}

fn heading(p: Paint, s: &Onboard) -> El {
    let line = |copy: String| -> El {
        text(copy)
            .key("lead")
            .font(LEAD)
            .color(p.muted())
            .transition(Transition::opacity())
    };
    let (key, title, lead) = match s.step {
        Step::Pick | Step::Started => (
            "intro",
            "Talk instead of typing",
            line("Hold a key and talk. Hex types it wherever your cursor is.".into()),
        ),
        Step::Parked if !typed_done(s) => (
            "without",
            "Without Hex",
            line("Answering Jay the usual way.".into()),
        ),
        Step::Parked if s.can_hear() => (
            "without",
            "Without Hex",
            line(format!("Sent, after {}.", seconds(typed_secs()))),
        ),
        Step::Parked => (
            "with",
            "With Hex",
            line("Hex starts listening once the speech model has downloaded.".into()),
        ),
        Step::YourTurn | Step::Hearing => (
            "with",
            "With Hex",
            row()
                .key("hold")
                .gap(8.0)
                .center_y()
                .transition(Transition::opacity())
                .child(text("Hold").key("hold").font(LEAD).color(p.muted()))
                .child(keys(p, "keys", &s.caps, s.step == Step::Hearing))
                .child(text("and say it.").key("rest").font(LEAD).color(p.muted())),
        ),
        Step::Heard | Step::Done | Step::Ready => (
            "heard",
            "That's it",
            line(format!(
                "Typed in {}. Said in {}.",
                seconds(typed_secs()),
                seconds(ANSWER_JAY.release)
            )),
        ),
    };
    col()
        .key(key)
        .gap(10.0)
        .items_center()
        .transition(Transition::blur_replace())
        .child(text(title).key("title").font(TITLE).color(p.text()))
        .child(row().key("lead-row").h(22.0).center().child(lead))
}

fn stage(p: Paint, s: &Onboard) -> El {
    let (kw, kh) = keyboard::size();
    let nudge = landing_nudge(s);
    let card = stacked(p, "stack", notice(p, &JAY, jay_reply(s), false), BEHIND)
        .w(CARD_W)
        .pin(
            Some(CARD_X),
            None,
            None,
            Some(STAGE_H - CARD_BOTTOM - PEEK * BEHIND as f32),
        )
        .offset(nudge[0], nudge[1])
        .offset_motion(Anim::INSTANT);
    el().key("stage")
        .size(STAGE_W, STAGE_H)
        .child(
            el().key("keys")
                .absolute((STAGE_W - kw) / 2.0, STAGE_H - kh)
                .size(kw, kh)
                .child(keyboard(
                    p,
                    "keyboard",
                    if s.step >= Step::YourTurn || typed_done(s) {
                        0.6
                    } else {
                        1.0
                    },
                    |id| key_depth(s, id),
                    |id| key_glow(s, id),
                )),
        )
        .child(clocks(p, s))
        .children((s.step >= Step::YourTurn).then(|| mouth(p, s).transition(arrives())))
        .child(card)
        .child(words(p, s, nudge))
}

/// The shortcut's keys on the drawn keyboard.
fn shortcut_keys(s: &Onboard) -> impl Iterator<Item = &'static str> + '_ {
    s.caps.iter().filter_map(|cap| keyboard::modifier(cap))
}

fn key_depth(s: &Onboard, id: &str) -> f32 {
    if s.step == Step::Hearing && shortcut_keys(s).any(|k| k == id) {
        return 1.0;
    }
    match (s.step, typing_elapsed(s)) {
        (Step::Parked, Some(elapsed)) => typing::depth(elapsed, id),
        _ => 0.0,
    }
}

fn key_glow(s: &Onboard, id: &str) -> f32 {
    if !shortcut_keys(s).any(|k| k == id) {
        return 0.0;
    }
    match s.step {
        Step::YourTurn => 0.5,
        Step::Hearing => 1.0,
        _ => 0.0,
    }
}

fn jay_reply(s: &Onboard) -> Reply<'static> {
    let lines = reply_lines();
    let empty = Reply::Lines {
        text: "",
        caret: None,
        lines,
        placeholder: true,
        focus: false,
        sent: false,
    };
    match s.step {
        Step::Pick | Step::Started => empty,
        Step::Parked if !typed_done(s) => {
            let stroke = typing_elapsed(s).and_then(typing::at);
            let typed = stroke.map_or("", |st| st.text.as_str());
            Reply::Lines {
                text: typed,
                caret: Some(stroke.map_or(0, |st| st.caret)),
                lines,
                placeholder: typed.is_empty(),
                focus: true,
                sent: false,
            }
        }
        // Waiting for the model with the typed reply sent.
        Step::Parked => Reply::Lines {
            text: ANSWER_JAY.text(),
            caret: None,
            lines,
            placeholder: false,
            focus: false,
            sent: true,
        },
        Step::YourTurn | Step::Hearing => Reply::Lines {
            text: "",
            caret: None,
            lines,
            placeholder: !flights(s).iter().any(|(f, _)| s.now >= f.start()),
            focus: true,
            sent: false,
        },
        Step::Heard | Step::Done | Step::Ready => Reply::Lines {
            text: "",
            caret: None,
            lines,
            placeholder: false,
            focus: false,
            sent: true,
        },
    }
}

/// How long typing took, then how long saying it took, counting up.
fn clocks(p: Paint, s: &Onboard) -> El {
    let stat = |key: &str, secs: Option<f64>, label: &str| -> El {
        col()
            .key(key.to_string())
            .w(84.0)
            .gap(2.0)
            .items_center()
            .opacity(if secs.is_some() { 1.0 } else { 0.0 })
            .child(
                text(seconds(secs.unwrap_or(0.0)))
                    .key("secs")
                    .font(
                        rune::TextStyle::new(22.0, 600.0)
                            .lh(26.0)
                            .features("'tnum' 1"),
                    )
                    .color(p.text()),
            )
            .child(
                text(label.to_string())
                    .key("label")
                    .font(p.k.ty.small)
                    .color(p.muted()),
            )
    };
    let typed = typing_elapsed(s).map(|e| e.clamp(0.0, typed_secs()));
    let said = s
        .take_from
        .map(|from| (s.now - from).clamp(0.0, ANSWER_JAY.release));
    row()
        .key("clocks")
        .absolute(MOUTH[0] - 92.0, 106.0)
        .w(184.0)
        .gap(16.0)
        .justify_center()
        .child(stat("typed", typed, "typing"))
        .child(stat("said", said, "saying"))
}

/// The microphone the words leave from, its level live while held.
fn mouth(p: Paint, s: &Onboard) -> El {
    let k = p.k;
    let live = s.step == Step::Hearing;
    el().key("mouth")
        .absolute(MOUTH[0] - MOUTH_D / 2.0, MOUTH[1] - MOUTH_D / 2.0)
        .size(MOUTH_D, MOUTH_D)
        .radius(MOUTH_D / 2.0)
        .center()
        .fill(k.c.surface_raised)
        .border(
            p.hair,
            if live {
                k.c.border_focus
            } else {
                k.c.border_weak
            },
        )
        .child(mic_level(p, "mic", 26.0, s.meter.0, s.meter.1, live))
}

/// How the landings so far push the card, as the site's calm words do.
fn landing_nudge(s: &Onboard) -> [f32; 2] {
    if s.reduced.get() {
        return [0.0, 0.0];
    }
    flights(s).iter().fold([0.0, 0.0], |[x, y], (f, _)| {
        let [dx, dy] = f.nudge(s.now);
        [x + dx, y + dy]
    })
}

/// The words in flight and in their slots, and the caret after them, over
/// everything on the stage.
fn words(p: Paint, s: &Onboard, nudge: [f32; 2]) -> El {
    let mut out = el()
        .key("words")
        .absolute(0.0, 0.0)
        .size(STAGE_W, STAGE_H)
        .instant_changes();
    let reduced = s.reduced.get();
    let origin = field_origin();
    let font = FIELD_FONT.lh(super::cards::FIELD_LINE);
    let flights = flights(s);
    let weight = 520.0 + 300.0 * CALM;
    let mut targets = Vec::new();
    let mut line = 0;
    for (i, (flight, slot)) in flights.iter().enumerate() {
        if let Some(landed) = flight.landed(s.now, reduced) {
            let at = [
                origin[0] + slot.centre[0] + nudge[0],
                origin[1] + slot.centre[1] + nudge[1],
            ];
            out = out.children(typeset(
                p,
                &format!("w{i}"),
                slot.word,
                font,
                at,
                landed,
                flicker(s.now, i),
            ));
            if slot.line != line {
                targets.clear();
                line = slot.line;
            }
            targets.push((flight.arrives(), slot.right + 1.0));
        }
        if let Some(pose) = flight.pose(s.now, reduced) {
            out = out.child(flying(p, &format!("f{i}"), slot.word, weight, pose));
        }
    }
    let typing = matches!(s.step, Step::YourTurn | Step::Hearing);
    if typing {
        let x = if reduced {
            targets.last().map_or(0.0, |t| t.1)
        } else {
            caret_x(0.0, &targets, s.now)
        };
        out = out.child(
            el().key("caret-at")
                .absolute(origin[0], origin[1])
                .child(field_text::caret(p, "caret", x, line)),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_words_leave_as_they_are_heard() {
        let at = heard_at(true);
        assert_eq!(at.len(), 10);
        assert_eq!(at[0], 0.45);
        assert!(at.windows(2).all(|w| w[0] <= w[1]));
        let burst = heard_at(false);
        assert_eq!(burst[0], ANSWER_JAY.release);
    }

    #[test]
    fn the_field_sits_inside_the_stage() {
        let [x, y] = field_origin();
        assert!(x > CARD_X && x < STAGE_W);
        assert!(y > 0.0 && y < CARD_BOTTOM);
        const { assert!(MOUTH[0] + MOUTH_D / 2.0 < CARD_X) };
    }
}
