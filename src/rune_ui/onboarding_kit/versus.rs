//! Without Hex, then with it, in one Messages conversation:
//!
//! 1. `"Watch what happens"` starts it once the user is looking.
//! 2. Jay (CEO) asks whether we're ready to ship, investors in the room. You
//!    type the answer by hand, faster as panic builds, and a finger slips
//!    onto Return: `"Yse, we're dead"`. Silence, then `"Read"`. Every fix
//!    makes it worse until Jay removes you from #launch ([`typing::Scene`]).
//! 3. A clean cut to `"With Hex"`: the same question, the keyboard recedes,
//!    Hex's real `Dot scope` HUD (`rune_hud.wgsl`) lights `⌥`, and the answer
//!    lands in about 1.5 seconds. Jay is delighted.
//! 4. The score: typing time and typos against Hex's time.
//!
//! One thing has the eye at a time; the rest dims.

use rune::{Anim, El, Spring, SpringPair, Transition, col, el, row, text};

use super::super::controls::Paint;
use super::cards::{
    CARD_W, FIELD_FONT, JAY, Reply, Side, bubble, chat, chat_height, field_text_origin, receipt,
    system_line, typing_dots,
};
use super::field_text::{self, slots};
use super::fixture::ANSWER_JAY;
use super::flight::{Flight, caret_x, flicker, flying, typeset};
use super::flows::continue_button;
use super::hud_stage::voice_or_idle_slot;
use super::keyboard::{self, keyboard_lit};
use super::landing::{LEAD, UI_SPRING, keys, language_pick, ready, title_font};
use super::mic_line::mic_line;
use super::pill::{pill, slot};
use super::stories::{Flow, Onboard, Step};
use super::typing::{Focus, TYPOS, Timing, Who};

const STAGE_W: f32 = 500.0;
const STAGE_H: f32 = 452.0;
const CENTRE_X: f32 = STAGE_W * 0.5;
const CARD_X: f32 = CENTRE_X - CARD_W * 0.5;
const CARD_TOP: f32 = 0.0;
/// Over the keyboard's upper rows, which recede, above the lit `⌥`.
const HUD_CENTRE: [f32; 2] = [CENTRE_X, 330.0];
const HUD_SCALE: f32 = 1.05;
const CALM: f32 = 0.20;
const BURST: f64 = 0.04;
const ACTION_H: f32 = 32.0;
const TOP_BAR: f32 = 48.0;
/// What isn't the focus fades to this.
const DIM: f32 = 0.34;
/// The message being answered while you type.
const CONTEXT: f32 = 0.72;
/// `"Without Hex"`'s field holds one line.
const WITHOUT_LINES: usize = 1;

/// Jay's question again, on `"With Hex"`.
const HEX_QUESTION: [&str; 2] = ["The investors are in the room.", "Are we ready to ship?"];
/// Jay, delighted.
pub(crate) const HEX_REPLY: &str = "perfect, they're clapping 👏";

fn reply_lines() -> usize {
    field_text::wrap(ANSWER_JAY.text()).len()
}

fn card_bottom(lines: usize) -> f32 {
    CARD_TOP + chat_height(lines)
}

fn field_origin() -> [f32; 2] {
    let lines = reply_lines();
    field_text_origin([CARD_X, card_bottom(lines)], lines)
}

/// Seconds into `"Without Hex"`, once it has started.
fn scene_time(s: &Onboard) -> Option<f64> {
    s.typing_from.map(|from| s.now - from)
}

fn scene_over(s: &Onboard) -> bool {
    scene_time(s).is_some_and(|t| t >= s.scene.duration())
}

/// `"Without Hex"` is waiting for `"Watch what happens"`.
pub(crate) fn waiting_to_start(s: &Onboard) -> bool {
    s.step == Step::Parked && s.typing_from.is_none()
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

fn flights_from(streams: bool, from: f64) -> Vec<(Flight, field_text::Slot<'static>)> {
    let origin = field_origin();
    slots(ANSWER_JAY.text())
        .into_iter()
        .zip(heard_at(streams))
        .enumerate()
        .map(|(i, (slot, at))| {
            let to = [origin[0] + slot.centre[0], origin[1] + slot.centre[1]];
            (
                Flight::new(HUD_CENTRE, to, from + at, CALM, FIELD_FONT.size, i),
                slot,
            )
        })
        .collect()
}

fn flights(s: &Onboard) -> Vec<(Flight, field_text::Slot<'static>)> {
    s.take_from
        .map_or_else(Vec::new, |from| flights_from(s.streams(), from))
}

/// `"With Hex"`'s beats after the shortcut goes down: the answer sends, Jay
/// replies, and his reply has been read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HexBeats {
    pub(crate) send: f64,
    pub(crate) reply: f64,
    pub(crate) done: f64,
}

pub(crate) fn hex_beats(streams: bool, timing: &Timing) -> HexBeats {
    let landed = flights_from(streams, 0.0)
        .iter()
        .map(|(f, _)| f.arrives())
        .fold(ANSWER_JAY.release, f64::max);
    let send = landed + timing.hex_send;
    let reply = send + timing.hex_reply;
    HexBeats {
        send,
        reply,
        done: reply + timing.read(HEX_REPLY),
    }
}

/// From `Heard` to the score.
pub(crate) fn reply_secs(s: &Onboard) -> f64 {
    hex_beats(s.streams(), &s.timing).done - (ANSWER_JAY.release + 0.1)
}

/// Seconds since the shortcut went down on `"With Hex"`.
fn take_time(s: &Onboard) -> Option<f64> {
    matches!(s.step, Step::Hearing | Step::Heard | Step::Done)
        .then_some(s.take_from)
        .flatten()
        .map(|from| s.now - from)
}

/// `"With Hex"`'s answer has left the field.
pub(crate) fn hex_sent(s: &Onboard) -> bool {
    take_time(s).is_some_and(|t| t >= hex_beats(s.streams(), &s.timing).send)
}

/// How many `"Without Hex"` replies have been sent.
pub(super) fn sent_count(s: &Onboard) -> usize {
    match scene_time(s) {
        Some(t) if s.step == Step::Parked => s.scene.sent(t),
        _ => 0,
    }
}

pub(super) fn view(p: Paint, s: &Onboard) -> Vec<El> {
    if s.step == Step::Ready {
        return vec![
            top_bar()
                .justify_center()
                .child(pill(p, download(s), s.closed)),
            middle()
                .pb(TOP_BAR)
                .child(ready(p, &s.caps, None, s.now, s.reduced.get(), s.motion)),
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
        Step::Done => continue_button(p, s),
        Step::Parked if waiting_to_start(s) => super::flows::watch_button(p, s),
        _ => el().key("none"),
    };
    vec![
        top,
        middle()
            .pb(TOP_BAR)
            .gap(16.0)
            .child(heading(p, s))
            .children(parked.then(|| stage(p, s)))
            .child(
                col()
                    .key("try")
                    .gap(10.0)
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

pub(super) fn heading(p: Paint, s: &Onboard) -> El {
    let line = |key: &str, copy: String| -> El {
        text(copy)
            .key(key.to_string())
            .font(LEAD)
            .color(p.muted())
            .transition(Transition::opacity())
    };
    let hold = || {
        row()
            .key("hold")
            .gap(8.0)
            .center_y()
            .transition(Transition::opacity())
            .child(text("Hold").key("hold").font(LEAD).color(p.muted()))
            .child(keys(p, "keys", &s.caps, s.step == Step::Hearing))
            .child(text("and say it.").key("rest").font(LEAD).color(p.muted()))
    };
    let (key, title, lead) = match s.step {
        Step::Pick | Step::Started => (
            "intro",
            "Talk instead of typing".to_string(),
            line(
                "lead-intro",
                "Hold a key and talk. Hex types it wherever your cursor is.".into(),
            ),
        ),
        Step::Parked if !scene_over(s) || s.demo_hears() => (
            "without",
            "Without Hex".to_string(),
            line(
                "lead-usual",
                "Jay needs an answer. Type it the usual way.".into(),
            ),
        ),
        Step::Parked => (
            "with",
            "With Hex".to_string(),
            line(
                "lead-wait",
                "Hex starts listening once the speech model has downloaded.".into(),
            ),
        ),
        Step::YourTurn | Step::Hearing | Step::Heard => ("with", "With Hex".to_string(), hold()),
        _ if s.flow == Flow::FirstRun || s.flow == Flow::Versus => (
            "score",
            format!("{:.1} seconds, no typos", ANSWER_JAY.release),
            line(
                "lead-score",
                format!(
                    "Typing: {:.1} seconds, {TYPOS} typos, and one job.",
                    s.scene.typing_secs()
                ),
            ),
        ),
        _ => (
            "heard",
            "That's it".to_string(),
            line("lead-done", "The same reply, typed and then spoken.".into()),
        ),
    };
    s.motion.heading(
        col()
            .key(key)
            .gap(8.0)
            .items_center()
            .child(text(title).key("title").font(title_font(p)).color(p.text()))
            .child(row().key("lead-row").h(24.0).center().child(lead)),
    )
}

fn smoothstep(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

pub(super) fn stage(p: Paint, s: &Onboard) -> El {
    let (kw, kh) = keyboard::size();
    let voice_phase = s.step >= Step::YourTurn;
    let reduced = s.reduced.get();
    let t = scene_time(s);
    let focus = t.map_or(Focus::Waiting, |t| s.scene.focus(t));

    // A single zero-bounce spring carries the deck and its key sweep.
    let enter_u = if reduced {
        1.0
    } else {
        s.parked_from
            .map_or(1.0, |from| s.motion.keyboard_progress(s.now - from))
    };
    let voice_u = if reduced {
        if voice_phase { 1.0 } else { 0.0 }
    } else {
        s.voice_from
            .filter(|_| voice_phase)
            .map_or(0.0, |from| smoothstep(((s.now - from) / 0.85) as f32))
    };

    // The keyboard is the focus only while you type; otherwise it recedes.
    let keys_opacity = match (voice_phase, focus) {
        (true, _) if s.step == Step::Done || hex_sent(s) => 0.28,
        (true, _) | (false, Focus::Field) => 1.0,
        (false, Focus::Waiting) => 0.6,
        _ => 0.28,
    };
    let kb_el = keyboard_lit(
        p,
        "keyboard",
        enter_u,
        1.0 + (0.34 - 1.0) * voice_u,
        |id| key_depth(s, id),
        |id| key_glow(s, id, voice_u),
    );
    let kb_shift = (1.0 - enter_u) * 14.0 + voice_u * 6.0;
    let keys_el = el()
        .key("keys")
        .transition(
            Transition::opacity()
                .springs(SpringPair::new(UI_SPRING, Spring::new(s.motion.exit, 0.0))),
        )
        .absolute((STAGE_W - kw) * 0.5, STAGE_H - kh)
        .offset(0.0, kb_shift)
        .size(kw, kh)
        .offset_motion(Anim::INSTANT)
        .opacity(keys_opacity)
        .opacity_motion(Spring::new(0.36, 0.0))
        .child(kb_el);

    // Real `Dot scope` phosphor HUD (`rune_hud.wgsl`) during `With Hex`: it
    // unfolds as it rises a little off the keyboard, where `⌥` lights.
    let hud_el = voice_phase
        .then(|| {
            voice_or_idle_slot(
                "onboard-dot-hud",
                HUD_CENTRE,
                HUD_SCALE,
                s.step == Step::YourTurn,
                &ANSWER_JAY,
                s.take_from,
                s,
            )
            .0
        })
        .flatten()
        .map(|hud| hud.transition(s.motion.roll(10.0)));

    let nudge = landing_nudge(s);
    let knock = match t {
        Some(t) if !reduced && !voice_phase => s.scene.knock(t),
        _ => 0.0,
    };
    let card = if voice_phase {
        with_hex_card(p, s)
    } else {
        without_hex_card(p, s, t, focus)
    };
    let card = el()
        .key(if voice_phase {
            "card-with"
        } else {
            "card-without"
        })
        .absolute(CARD_X, CARD_TOP)
        .w(CARD_W)
        .transition(Transition::opacity().springs(SpringPair::new(
            Spring::new(0.3, 0.0),
            Spring::new(0.16, 0.0),
        )))
        .child(
            card.offset(nudge[0] + knock, nudge[1])
                .offset_motion(Anim::INSTANT),
        );

    el().key("stage")
        .size(STAGE_W, STAGE_H)
        .opacity(if waiting_to_start(s) { 0.62 } else { 1.0 })
        .opacity_motion(Spring::new(0.4, 0.0))
        .child(keys_el)
        .children(hud_el)
        .child(card)
        .child(words(p, s, nudge))
}

/// The conversation as `"Without Hex"` stands at `t`.
fn without_hex_card(p: Paint, s: &Onboard, t: Option<f64>, focus: Focus) -> El {
    let reduced = s.reduced.get();
    let scene = &s.scene;
    let Some(t) = t else {
        return chat(
            p,
            &JAY,
            Vec::new(),
            Reply::Lines {
                text: "",
                caret: None,
                lines: WITHOUT_LINES,
                placeholder: true,
                focus: false,
                sent: false,
            },
            1.0,
        );
    };
    let shown = scene.shown(t);
    let lines = &scene.lines()[..shown];
    let answering = lines.iter().rposition(|l| l.who == Who::Jay);
    let read = scene.read(t);
    let removed = scene.removed(t);
    let mut thread = Vec::new();
    let mut sent = 0;
    for (i, line) in lines.iter().enumerate() {
        let lit = match focus {
            Focus::Line(j) | Focus::Read(j) => i == j,
            _ => false,
        };
        let opacity = if lit {
            1.0
        } else if focus == Focus::Field && Some(i) == answering {
            CONTEXT
        } else {
            DIM
        };
        thread.push(match line.who {
            Who::Jay => bubble(p, format!("l{i}"), Side::Them, &line.text, opacity, reduced),
            Who::You => {
                sent += 1;
                bubble(
                    p,
                    format!("sent-bubble-{}", sent - 1),
                    Side::You,
                    &line.text,
                    opacity,
                    reduced,
                )
            }
            Who::System => system_line(p, format!("l{i}"), &line.text, opacity, reduced),
        });
        if read == Some(i) {
            let lit = matches!(focus, Focus::Read(_));
            thread.push(receipt(p, if lit { 1.0 } else { DIM }, reduced));
        }
    }
    if scene.dots(t).is_some() {
        thread.push(typing_dots(p, "dots".into(), s.now, 1.0, reduced));
    }
    let (draft, caret) = scene.draft(t);
    let field_opacity = if focus == Focus::Field {
        1.0
    } else if removed {
        0.25
    } else {
        0.5
    };
    chat(
        p,
        &JAY,
        thread,
        Reply::Lines {
            text: draft,
            caret,
            lines: WITHOUT_LINES,
            placeholder: draft.is_empty(),
            focus: caret.is_some(),
            sent: false,
        },
        field_opacity,
    )
}

/// The same question, answered by voice.
fn with_hex_card(p: Paint, s: &Onboard) -> El {
    let reduced = s.reduced.get();
    let beats = hex_beats(s.streams(), &s.timing);
    let t = take_time(s);
    let sent = t.is_some_and(|t| t >= beats.send);
    let replied = t.is_some_and(|t| t >= beats.reply);
    let typing = t.is_some_and(|t| t >= beats.send + 0.4 && t < beats.reply);
    let composing = !sent;
    let mut thread = Vec::new();
    for (i, copy) in HEX_QUESTION.iter().enumerate() {
        let opacity = if composing && i == HEX_QUESTION.len() - 1 {
            CONTEXT
        } else {
            DIM
        };
        thread.push(bubble(
            p,
            format!("q{i}"),
            Side::Them,
            copy,
            opacity,
            reduced,
        ));
    }
    if sent {
        let opacity = if replied { DIM } else { 1.0 };
        thread.push(bubble(
            p,
            "hex-bubble".into(),
            Side::You,
            ANSWER_JAY.text(),
            opacity,
            reduced,
        ));
    }
    if typing {
        thread.push(typing_dots(p, "dots".into(), s.now, 1.0, reduced));
    }
    if replied {
        thread.push(bubble(
            p,
            "hex-reply".into(),
            Side::Them,
            HEX_REPLY,
            1.0,
            reduced,
        ));
    }
    let started = flights(s).iter().any(|(f, _)| s.now >= f.start());
    chat(
        p,
        &JAY,
        thread,
        Reply::Lines {
            text: "",
            caret: None,
            lines: reply_lines(),
            placeholder: !started || sent,
            focus: composing,
            sent: false,
        },
        if composing { 1.0 } else { 0.5 },
    )
}

fn shortcut_keys(s: &Onboard) -> impl Iterator<Item = &'static str> + '_ {
    s.caps.iter().filter_map(|cap| keyboard::modifier(cap))
}

fn key_depth(s: &Onboard, id: &str) -> f32 {
    if s.step == Step::Hearing && shortcut_keys(s).any(|k| k == id) {
        return 1.0;
    }
    match (s.step, scene_time(s)) {
        (Step::Parked, Some(t)) if t >= 0.0 => s.scene.depth(t, id),
        _ => 0.0,
    }
}

fn key_glow(s: &Onboard, id: &str, voice_u: f32) -> f32 {
    if !shortcut_keys(s).any(|k| k == id) {
        return 0.0;
    }
    match s.step {
        Step::YourTurn => 0.74 * voice_u,
        Step::Hearing => 1.0,
        Step::Heard => {
            let fade = s.take_from.map_or(0.0, |from| {
                1.0 - smoothstep(((s.now - from - ANSWER_JAY.release) / 0.45) as f32)
            });
            0.70 * fade
        }
        _ => 0.0,
    }
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
    // Once sent, the answer is its bubble.
    let flights = if hex_sent(s) { Vec::new() } else { flights(s) };
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
    fn streamed_words_leave_within_one_and_a_half_seconds() {
        let at = heard_at(true);
        assert_eq!(at.len(), 12);
        assert_eq!(at[0], 0.14);
        assert!(at.windows(2).all(|w| w[0] <= w[1]));
        assert!(*at.last().unwrap() <= ANSWER_JAY.release);
        let burst = heard_at(false);
        assert_eq!(burst[0], ANSWER_JAY.release);
    }

    #[test]
    fn card_keyboard_and_voice_hud_share_the_central_spine() {
        let (_, kh) = keyboard::size();
        assert!(((CARD_X + CARD_W * 0.5) - STAGE_W * 0.5).abs() < 0.01);
        assert!((HUD_CENTRE[0] - STAGE_W * 0.5).abs() < 0.01);
        let [x, y] = field_origin();
        let bottom = card_bottom(reply_lines());
        assert!(x > CARD_X && x < STAGE_W);
        assert!(y > CARD_TOP && y < bottom);
        assert!(bottom < STAGE_H - kh, "the chat clears the keyboard");
        assert!(
            bottom < HUD_CENTRE[1] - 40.0 * HUD_SCALE,
            "the HUD clears the chat"
        );
    }

    #[test]
    fn hex_sends_after_the_words_land_and_jay_replies_after() {
        let beats = hex_beats(true, &Timing::default());
        let landed = flights_from(true, 0.0)
            .iter()
            .map(|(f, _)| f.arrives())
            .fold(0.0, f64::max);
        assert!(beats.send > landed);
        assert!(beats.reply > beats.send + 0.4);
        assert!(beats.done > beats.reply + 1.0);
    }
}
