//! Without Hex, then with it — aligned on one central vertical spine:
//!
//! 1. Staggered entrance (never sitting on stage during `"Setting up Hex"`):
//!    the heading settles in, Jay's centred notification card glides into
//!    place, and the 3D-tilted keyboard rises beneath it just before typing.
//! 2. Someone types Jay's reply slowly (~8.5 s), stumbling over five typos
//!    and arrowing back to fix `"snyc"`, while a centred stopwatch capsule
//!    counts up above the card.
//! 3. For `"With Hex"`, the reply field clears cleanly, the 3D keyboard dims
//!    back leaving `⌥` backlit on its front row, the thin single-hexagon
//!    voice scope rises on the centre spine, and spoken words arc upward into
//!    the card in 3.0 s (`"Three times faster"`).

use rune::{Anim, Beat, El, Sequence, Transition, col, el, row, text};

use super::super::controls::Paint;
use super::cards::{CARD_W, FIELD_FONT, JAY, Reply, field_text_origin, lines_card_height, notice};
use super::field_text::{self, slots};
use super::fixture::ANSWER_JAY;
use super::flight::{Flight, caret_x, flicker, flying, typeset};
use super::flows::continue_button;
use super::keyboard::{self, keyboard};
use super::landing::{
    LEAD, UI_SPRING, arrives, clean_heading, keys, language_pick, ready, title_font,
};
use super::mic_line::mic_line;
use super::pill::{pill, slot};
use super::stories::{Flow, Onboard, Step};
use super::typing;

const STAGE_W: f32 = 640.0;
const STAGE_H: f32 = 388.0;
/// Single central spine shared by stopwatch, card, voice scope, and keyboard.
const CENTRE_X: f32 = STAGE_W * 0.5;
const CARD_X: f32 = CENTRE_X - CARD_W * 0.5;
const CARD_TOP: f32 = 32.0;
/// Voice scope sits in the open corridor (`y = 178..224`) between Jay's card
/// (`bottom = 174`) and the 3D keyboard (`top = 226`), never overlapping keys.
const MOUTH: [f32; 2] = [CENTRE_X, 201.0];
const MOUTH_D: f32 = 42.0;
const CALM: f32 = 0.14;
const BURST: f64 = 0.05;
const ACTION_H: f32 = 32.0;
const TOP_BAR: f32 = 48.0;

fn reply_lines() -> usize {
    field_text::wrap(ANSWER_JAY.text()).len()
}

fn card_bottom() -> f32 {
    CARD_TOP + lines_card_height(reply_lines())
}

fn field_origin() -> [f32; 2] {
    field_text_origin([CARD_X, card_bottom()], reply_lines())
}

fn typing_elapsed(s: &Onboard) -> Option<f64> {
    s.typing_from.map(|from| s.now - from)
}

fn typed_done(s: &Onboard) -> bool {
    typing_elapsed(s).is_some_and(|e| e >= typing::duration())
}

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

fn flights(s: &Onboard) -> Vec<(Flight, field_text::Slot<'static>)> {
    let Some(from) = s.take_from else {
        return Vec::new();
    };
    let origin = field_origin();
    slots(ANSWER_JAY.text())
        .into_iter()
        .zip(heard_at(s.streams()))
        .enumerate()
        .map(|(i, (slot, at))| {
            let to = [origin[0] + slot.centre[0], origin[1] + slot.centre[1]];
            (
                Flight::new(MOUTH, to, from + at, CALM, FIELD_FONT.size, i),
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
            // Never sit the keyboard on stage before the demo actually starts.
            .children(parked.then(|| stage(p, s)))
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

pub(super) fn heading(p: Paint, s: &Onboard) -> El {
    let line = |key: &str, copy: String| -> El {
        text(copy)
            .key(key.to_string())
            .font(LEAD)
            .color(p.muted())
            .transition(Transition::opacity())
    };
    let (key, title, lead) = match s.step {
        Step::Pick | Step::Started => (
            "intro",
            "Talk instead of typing",
            line(
                "lead-intro",
                "Hold a key and talk. Hex types it wherever your cursor is.".into(),
            ),
        ),
        Step::Parked if !typed_done(s) => (
            "without",
            "Without Hex",
            line("lead-usual", "Answering Jay the usual way.".into()),
        ),
        Step::Parked if s.demo_hears() => (
            "without",
            "Without Hex",
            line(
                "lead-sent",
                format!("Five typos and {} later.", seconds(typed_secs())),
            ),
        ),
        Step::Parked => (
            "with",
            "With Hex",
            line(
                "lead-wait",
                "Hex starts listening once the speech model has downloaded.".into(),
            ),
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
        _ if s.flow == Flow::FirstRun => (
            "heard",
            "Three times faster",
            line(
                "lead-done",
                format!(
                    "Typed in {}. Spoken in {}.",
                    seconds(typed_secs()),
                    seconds(ANSWER_JAY.release)
                ),
            ),
        ),
        _ => (
            "heard",
            "That's it",
            line(
                "lead-done",
                format!(
                    "Typed in {}. Spoken in {}.",
                    seconds(typed_secs()),
                    seconds(ANSWER_JAY.release)
                ),
            ),
        ),
    };
    clean_heading(
        col()
            .key(key)
            .gap(8.0)
            .items_center()
            .child(text(title).key("title").font(title_font(p)).color(p.text()))
            .child(row().key("lead-row").h(24.0).center().child(lead)),
    )
}

pub(super) fn stage(p: Paint, s: &Onboard) -> El {
    const CARD_IN: &[Beat] = &[
        Beat::new(0.0).opacity(0.0).scale(0.975),
        Beat::new(0.34)
            .opacity(1.0)
            .scale(1.0)
            .easing(rune::Easing::EASE_OUT),
    ];
    const KB_IN: &[Beat] = &[
        Beat::new(0.0).opacity(0.0).scale(0.985),
        Beat::new(0.42)
            .opacity(1.0)
            .scale(1.0)
            .easing(rune::Easing::EASE_OUT),
    ];

    let (kw, kh) = keyboard::size();
    let nudge = landing_nudge(s);
    let voice_phase = s.step >= Step::YourTurn;

    // Smooth vertical rise for the 3D keyboard arriving 0.24 s into the demo.
    let kb_rise = if s.reduced.get() || voice_phase {
        0.0
    } else {
        match typing_elapsed(s) {
            Some(te) => {
                // `typing_from` starts ~0.6 s after `Step::Parked` begins.
                let since_parked = (te + 0.6).max(0.0);
                let u = ((since_parked - 0.22) / 0.42).clamp(0.0, 1.0) as f32;
                let eased = 1.0 - (1.0 - u).powi(3);
                20.0 * (1.0 - eased)
            }
            None => 0.0,
        }
    };

    let card = el()
        .key("card-wrap")
        .absolute(CARD_X, CARD_TOP)
        .w(CARD_W)
        .sequence(Sequence::new(CARD_IN).delay(0.08))
        .transition(Transition::offset(0.0, 12.0))
        .child(
            notice(p, &JAY, jay_reply(s), false)
                .offset(nudge[0], nudge[1])
                .offset_motion(Anim::INSTANT),
        );

    let kb_dim = if voice_phase {
        0.28
    } else if typed_done(s) {
        0.55
    } else {
        1.0
    };
    let kb_shift = if voice_phase { 6.0 } else { 0.0 };
    let keys_el = el()
        .key("keys")
        .absolute((STAGE_W - kw) * 0.5, STAGE_H - kh + kb_shift + kb_rise)
        .size(kw, kh)
        .offset_motion(Anim::INSTANT)
        .sequence(Sequence::new(KB_IN).delay(0.22))
        .child(keyboard(
            p,
            "keyboard",
            kb_dim,
            |id| key_depth(s, id),
            |id| key_glow(s, id),
        ));

    let show_scope = matches!(s.step, Step::YourTurn | Step::Hearing);
    el().key("stage")
        .size(STAGE_W, STAGE_H)
        .child(stopwatch_pill(p, s))
        .child(keys_el)
        .children(show_scope.then(|| mouth(p, s).transition(arrives())))
        .child(card)
        .child(words(p, s, nudge))
}

fn shortcut_keys(s: &Onboard) -> impl Iterator<Item = &'static str> + '_ {
    s.caps.iter().filter_map(|cap| keyboard::modifier(cap))
}

fn key_depth(s: &Onboard, id: &str) -> f32 {
    if s.step == Step::Hearing && shortcut_keys(s).any(|k| k == id) {
        return 1.0;
    }
    match (s.step, typing_elapsed(s)) {
        (Step::Parked, Some(elapsed)) if elapsed >= 0.0 => typing::depth(elapsed, id),
        _ => 0.0,
    }
}

fn key_glow(s: &Onboard, id: &str) -> f32 {
    if !shortcut_keys(s).any(|k| k == id) {
        return 0.0;
    }
    match s.step {
        Step::YourTurn => 0.65,
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
            let active = typing_elapsed(s).filter(|&e| e >= 0.0);
            let stroke = active.and_then(typing::at);
            let typed = stroke.map_or("", |st| st.text.as_str());
            Reply::Lines {
                text: typed,
                caret: active.map(|_| stroke.map_or(0, |st| st.caret)),
                lines,
                placeholder: typed.is_empty(),
                focus: active.is_some(),
                sent: false,
            }
        }
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
        _ => Reply::Lines {
            text: "",
            caret: None,
            lines,
            placeholder: false,
            focus: false,
            sent: true,
        },
    }
}

/// Centred tabular stopwatch capsule above Jay's card: wakes once typing
/// starts, then places the spoken time right beside it for direct comparison.
fn stopwatch_pill(p: Paint, s: &Onboard) -> El {
    let k = p.k;
    let typed_raw = typing_elapsed(s).filter(|&e| e >= 0.0);
    let Some(typed_e) = typed_raw else {
        return el().key("stopwatch-empty");
    };
    let typed = typed_e.clamp(0.0, typed_secs());
    let said = s
        .take_from
        .filter(|_| s.step >= Step::Hearing)
        .map(|from| (s.now - from).clamp(0.0, ANSWER_JAY.release));
    let voice_stage = s.step >= Step::YourTurn;
    let finished = s.step >= Step::Heard;

    let num_font = k.ty.small_medium.features("'tnum' 1");
    let mut pill = row()
        .key("stopwatch-pill")
        .h(24.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .motion(UI_SPRING)
        .transition(arrives())
        .child(
            row()
                .key("typed-stat")
                .gap(5.0)
                .center_y()
                .child(
                    text("Typing")
                        .key("typed-lbl")
                        .font(k.ty.small)
                        .color(p.faint()),
                )
                .child(
                    text(seconds(typed))
                        .key("typed-val")
                        .font(num_font)
                        .color(if voice_stage { p.muted() } else { p.text() }),
                ),
        );

    if voice_stage {
        let voice_color = if finished { p.success() } else { p.text() };
        pill = pill
            .child(el().key("sep").size(p.hair, 10.0).fill(k.c.border_base))
            .child(
                row()
                    .key("voice-stat")
                    .gap(5.0)
                    .center_y()
                    .transition(Transition::opacity())
                    .child(
                        text("With Hex")
                            .key("voice-lbl")
                            .font(k.ty.small)
                            .color(if finished { p.success() } else { p.muted() }),
                    )
                    .child(
                        text(seconds(said.unwrap_or(0.0)))
                            .key("voice-val")
                            .font(num_font)
                            .color(voice_color),
                    ),
            );
    }

    row()
        .key("stopwatch-row")
        .absolute(0.0, 0.0)
        .w(STAGE_W)
        .h(24.0)
        .center()
        .child(k.elevate(pill, &k.elevation.raised))
}

fn mouth(p: Paint, s: &Onboard) -> El {
    super::mic::mouth(p, "mouth", MOUTH, MOUTH_D, s, s.step == Step::Hearing)
}

fn landing_nudge(s: &Onboard) -> [f32; 2] {
    if s.reduced.get() {
        return [0.0, 0.0];
    }
    flights(s).iter().fold([0.0, 0.0], |[x, y], (f, _)| {
        let [dx, dy] = f.nudge(s.now);
        [x + dx, y + dy]
    })
}

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
    let weight = 500.0 + 240.0 * CALM;
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
        }
        if reduced || s.now >= flight.arrives() - 0.10 {
            if slot.line != line {
                targets.clear();
                line = slot.line;
            }
            targets.push((flight.arrives() - 0.10, slot.right + 1.5));
        }
        if let Some(pose) = flight.pose(s.now, reduced) {
            out = out.child(flying(p, &format!("f{i}"), slot.word, weight, pose));
        }
    }
    if matches!(s.step, Step::YourTurn | Step::Hearing) {
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
    fn card_and_voice_scope_share_the_central_spine() {
        assert!(((CARD_X + CARD_W * 0.5) - STAGE_W * 0.5).abs() < 0.01);
        assert!((MOUTH[0] - STAGE_W * 0.5).abs() < 0.01);
        let [x, y] = field_origin();
        assert!(x > CARD_X && x < STAGE_W);
        assert!(y > CARD_TOP && y < card_bottom());
        assert!(card_bottom() < MOUTH[1] - MOUTH_D * 0.5);
        assert!(MOUTH[1] + MOUTH_D * 0.5 < STAGE_H - keyboard::size().1);
    }
}
