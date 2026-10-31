//! Without Hex, then with it, in one Messages conversation:
//!
//! 1. `"Watch what happens"` starts it once the user is looking.
//! 2. In the `"Launch Room"` group, Jay (CEO) has asked whether we're ready
//!    to ship, investors in the room. You type the answer by hand, faster as
//!    panic builds, and a finger slips onto Return: `"Yse, we're dead"`.
//!    Silence, then `"Read"`. Every fix makes it worse until Jay removes you
//!    from the group mid-word ([`typing::Scene`]).
//! 3. `"With Hex"` in two beats: the disaster rewinds back to Jay's question,
//!    then the keyboard melts away to `⌥`, Hex's real `Dot scope` HUD
//!    (`rune_hud.wgsl`) rises, and the answer lands in about 1.5 seconds.
//!    Jay is delighted.
//! 4. The score: what answering by hand cost against Hex's time.
//!
//! One thing has the eye at a time; the rest dims, deeply only when the
//! story freezes.

use rune::{Anim, El, Spring, SpringPair, Transition, col, el, row, text};

use super::super::controls::Paint;
use super::cards::{
    CHAT_W, FIELD_FONT, LAUNCH_ROOM, Reply, Side, bubble, chat, chat_height, field_text_origin,
    receipt, system_line, typing_dots,
};
use super::field_text::{self, CHAT};
use super::fixture::ANSWER_JAY;
use super::flight::{Flight, flicker, flying, typeset};
use super::flows::continue_button;
use super::hud_stage::voice_or_idle_slot;
use super::keyboard::{self, keyboard_lit};
use super::landing::{LEAD, UI_SPRING, keys, language_pick, ready, title_font};
use super::mic_line::mic_line;
use super::pill::{pill, slot};
use super::stories::{Flow, Onboard, Step};
use super::typing::{Focus, Line, TYPOS, Timing, Who};

const STAGE_W: f32 = 500.0;
const STAGE_H: f32 = 516.0;
const CENTRE_X: f32 = STAGE_W * 0.5;
const CARD_X: f32 = CENTRE_X - CHAT_W * 0.5;
const CARD_TOP: f32 = 0.0;
/// Over the keyboard's upper rows, which melt away, above the lit `⌥`.
const HUD_CENTRE: [f32; 2] = [CENTRE_X, 392.0];
const HUD_SCALE: f32 = 1.05;
const CALM: f32 = 0.20;
const BURST: f64 = 0.04;
const ACTION_H: f32 = 32.0;
const TOP_BAR: f32 = 48.0;
/// The last few lines, while they aren't the focus.
const RECENT: f32 = 0.80;
/// Older lines.
const OLDER: f32 = 0.65;
/// Everything but the focus while the story freezes.
const FROZEN: f32 = 0.28;
/// The message being answered while you type.
const CONTEXT: f32 = 0.9;
/// `"Without Hex"`'s field holds one line.
const WITHOUT_LINES: usize = 1;
/// From the stage settling to the button's clock: Jay's question arrives
/// as the stage settles ([`typing::Scene`]'s lines before the button).
const PRE_ROLL: f64 = 1.2;
/// `"With Hex"`'s first beat: the disaster zips back out, newest first.
pub(crate) const REWIND: f64 = 0.55;
const REWIND_STAGGER: f64 = 0.035;
/// The second beat: the keyboard melts to `⌥` and the HUD rises; then `⌥`
/// goes down this long after `"With Hex"` begins.
pub(crate) const BEFORE_KEY: f64 = 1.5;
const MELT: f64 = 0.9;
/// The keys start melting as the rewind settles, so the keyboard never
/// brightens on its way out.
const MELT_FROM: f64 = 0.1;
/// The HUD rises once the keys have mostly melted out of its way.
const HUD_RISES: f64 = MELT_FROM + MELT * 0.6;
/// Keys other than the shortcut once they've melted away.
const MELTED: f32 = 0.10;

/// The least time Jay's delighted reply holds before the score.
const REPLY_HOLD: f64 = 1.75;
/// How long a landed word takes to settle into the field.
const SETTLE: f64 = 0.15;
/// Jay, delighted.
pub(crate) const HEX_REPLY: &str = "perfect, they're clapping 👏";

fn reply_lines() -> usize {
    CHAT.wrap(ANSWER_JAY.text()).len()
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

/// The thread's time: [`scene_time`], or before the button, Jay's question
/// arriving as the stage settles.
fn thread_time(s: &Onboard) -> Option<f64> {
    scene_time(s).or_else(|| {
        s.parked_from
            .map(|from| (s.now - from - PRE_ROLL).min(-1e-3))
    })
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
    CHAT.slots(ANSWER_JAY.text())
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
        // Long enough for the applause to land.
        done: reply + timing.read(HEX_REPLY).max(REPLY_HOLD),
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
                "Investors are waiting. One quick reply by hand.".into(),
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
                    "Typing: {} seconds, {TYPOS} typos, and one job.",
                    s.scene.score_secs().round()
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

/// Seconds into `"With Hex"`, from the rewind.
fn hex_time(s: &Onboard) -> Option<f64> {
    (s.step >= Step::YourTurn)
        .then_some(s.voice_from)
        .flatten()
        .map(|from| s.now - from)
}

pub(super) fn stage(p: Paint, s: &Onboard) -> El {
    let (kw, kh) = keyboard::size();
    let voice_phase = s.step >= Step::YourTurn;
    let reduced = s.reduced.get();
    let t = scene_time(s);
    let focus = t.map_or(Focus::Waiting, |t| s.scene.focus(t));
    let frozen = t.is_some_and(|t| s.scene.frozen(t));
    let since = hex_time(s);

    // A single zero-bounce spring carries the deck and its key sweep.
    let enter_u = if reduced {
        1.0
    } else {
        s.parked_from
            .map_or(1.0, |from| s.motion.keyboard_progress(s.now - from))
    };
    // As the rewind settles, every key but the shortcut melts away.
    let melt_u = match since {
        Some(_) if reduced => 1.0,
        Some(since) => smoothstep(((since - MELT_FROM) / MELT) as f32),
        None => 0.0,
    };

    // While you type the keyboard comes up, and recedes while Jay talks; it
    // drops away only when the story freezes, after Return's flash.
    let flash = |id: &str| match t {
        Some(t) if !voice_phase => s.scene.flash(t, id),
        _ => 0.0,
    };
    // On `"With Hex"` the deck comes back up only as its keys melt, so only
    // the shortcut brightens.
    let keys_opacity = match focus {
        _ if since.is_some_and(|since| since < MELT_FROM + MELT * 0.5) => 0.52,
        _ if voice_phase => 1.0,
        Focus::Field => 0.92,
        _ if flash("return") > 0.0 => 0.92,
        _ if frozen => FROZEN,
        _ => 0.52,
    };
    let kb_el = keyboard_lit(
        p,
        "keyboard",
        enter_u,
        1.0 + (MELTED - 1.0) * melt_u,
        |id| key_depth(s, id),
        |id| key_glow(s, id, melt_u),
        flash,
    );
    let kb_shift = (1.0 - enter_u) * 14.0 + melt_u * 6.0;
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
    // unfolds as it rises into the space the keys melted out of, above `⌥`.
    let hud_el = since
        .filter(|&since| reduced || since >= HUD_RISES)
        .and_then(|_| {
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
        .map(|hud| hud.transition(s.motion.roll(10.0)));

    let nudge = landing_nudge(s);
    let knock = match t {
        Some(t) if !reduced && !voice_phase => s.scene.knock(t),
        _ => 0.0,
    };
    // One card throughout, so `"With Hex"` rewinds the same thread.
    let card = match since {
        Some(since) => with_hex_card(p, s, since),
        None => without_hex_card(p, s, focus, frozen),
    };
    let card = el()
        .key("card")
        .absolute(CARD_X, CARD_TOP)
        .w(CHAT_W)
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
        .opacity(if waiting_to_start(s) { 0.85 } else { 1.0 })
        .opacity_motion(Spring::new(0.4, 0.0))
        .child(keys_el)
        .children(hud_el)
        .child(card)
        .child(words(p, s, nudge))
}

/// Why the field is gone once Jay removes you.
const NO_LONGER: &str = "You are no longer in this conversation.";

/// `"Without Hex"`'s thread at `t`: every line so far and the `"Read"`
/// receipt, each at `opacity(line index, line)`, the receipt at `read`, and
/// Jay's typing dots.
fn thread_at(
    p: Paint,
    s: &Onboard,
    t: f64,
    opacity: impl Fn(usize, &Line) -> f32,
    read: f32,
) -> Vec<El> {
    let reduced = s.reduced.get();
    let scene = &s.scene;
    let lines = &scene.lines()[..scene.shown(t)];
    let receipt_at = scene.read(t);
    let mut thread = Vec::new();
    let mut sent = 0;
    for (i, line) in lines.iter().enumerate() {
        let o = opacity(i, line);
        thread.push(match line.who {
            Who::Jay => {
                // The sender's name over the first of a run, as in a group.
                let named = i == 0 || lines[i - 1].who != Who::Jay;
                bubble(
                    p,
                    format!("l{i}"),
                    Side::Them,
                    named.then_some("Jay"),
                    &line.text,
                    o,
                    reduced,
                )
            }
            Who::You => {
                sent += 1;
                bubble(
                    p,
                    format!("sent-bubble-{}", sent - 1),
                    Side::You,
                    None,
                    &line.text,
                    o,
                    reduced,
                )
            }
            Who::System => system_line(p, format!("l{i}"), &line.text, o, reduced),
        });
        if receipt_at == Some(i) {
            thread.push(receipt(p, read, reduced));
        }
    }
    if scene.dots(t).is_some() {
        thread.push(typing_dots(p, "dots".into(), s.now, 1.0, reduced));
    }
    thread
}

/// The conversation as `"Without Hex"` stands now, Jay's question already
/// there before the button.
fn without_hex_card(p: Paint, s: &Onboard, focus: Focus, frozen: bool) -> El {
    let scene = &s.scene;
    let t = thread_time(s).unwrap_or(-1e-3);
    let shown = scene.shown(t);
    let answering = scene.lines()[..shown]
        .iter()
        .rposition(|l| l.who == Who::Jay);
    let opacity = |i: usize, _: &Line| match focus {
        Focus::Waiting => 1.0,
        Focus::Line(j) | Focus::Read(j) if i == j => 1.0,
        _ if frozen => FROZEN,
        Focus::Field if Some(i) == answering => CONTEXT,
        _ if shown - i <= 3 => RECENT,
        _ => OLDER,
    };
    let read = match focus {
        Focus::Read(_) => 1.0,
        _ if frozen => FROZEN,
        _ => RECENT,
    };
    let thread = thread_at(p, s, t, opacity, read);
    if scene.removed(t) {
        return chat(p, &LAUNCH_ROOM, thread, Reply::Locked(NO_LONGER), 1.0);
    }
    let (draft, caret) = scene.draft(t);
    chat(
        p,
        &LAUNCH_ROOM,
        thread,
        Reply::Lines {
            text: draft,
            caret,
            lines: WITHOUT_LINES,
            placeholder: draft.is_empty(),
            focus: caret.is_some(),
            sent: false,
        },
        if focus == Focus::Field { 1.0 } else { 0.6 },
    )
}

/// The same question, answered by voice, `since` seconds into `"With Hex"`.
/// It opens on the rewind: the disaster zips back out, newest first, until
/// Jay's question is all that's left.
fn with_hex_card(p: Paint, s: &Onboard, since: f64) -> El {
    let reduced = s.reduced.get();
    let beats = hex_beats(s.streams(), &s.timing);
    let t = take_time(s);
    let sent = t.is_some_and(|t| t >= beats.send);
    let replied = t.is_some_and(|t| t >= beats.reply);
    let typing = t.is_some_and(|t| t >= beats.send + 0.4 && t < beats.reply);
    let composing = !sent;
    let asked = s.scene.lines().iter().filter(|l| l.at < 0.0).count();
    let mut thread = if since < REWIND && !reduced {
        let end = s.scene.duration();
        let mut all = thread_at(p, s, end, |i, _| if i < asked { 1.0 } else { 0.5 }, 0.5);
        let gone = ((since / REWIND_STAGGER) as usize + 1).min(all.len() - asked);
        all.truncate(all.len() - gone);
        all
    } else {
        let question = if composing { 1.0 } else { OLDER };
        thread_at(p, s, -1e-3, |_, _| question, 0.0)
    };
    if sent {
        thread.push(bubble(
            p,
            "hex-bubble".into(),
            Side::You,
            None,
            ANSWER_JAY.text(),
            if replied { RECENT } else { 1.0 },
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
            Some("Jay"),
            HEX_REPLY,
            1.0,
            reduced,
        ));
    }
    let flights = flights(s);
    let started = flights.iter().any(|(f, _)| s.now >= f.start());
    // The field grows a line only once a word heads for it.
    let lines = flights
        .iter()
        .filter(|(f, _)| s.now >= f.start())
        .map(|(_, slot)| slot.line + 1)
        .max()
        .unwrap_or(1);
    chat(
        p,
        &LAUNCH_ROOM,
        thread,
        Reply::Lines {
            text: "",
            caret: None,
            lines: if sent { 1 } else { lines },
            placeholder: !started || sent,
            focus: composing,
            sent: false,
        },
        if composing { 1.0 } else { 0.6 },
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

fn key_glow(s: &Onboard, id: &str, melt_u: f32) -> f32 {
    if !shortcut_keys(s).any(|k| k == id) {
        return 0.0;
    }
    match s.step {
        Step::YourTurn => 0.85 * melt_u,
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
    // Just before the placeholder's first glyph until a word lands.
    let mut caret = -1.5;
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
        if reduced || s.now >= flight.arrives() {
            line = slot.line;
            caret = slot.right + 1.5;
        }
        if let Some(pose) = flight.pose(s.now, reduced) {
            out = out.child(flying(p, &format!("f{i}"), slot.word, weight, pose));
        }
    }
    // The words take over from the caret while they fly in and settle, so
    // it never sits under one; it waits after the last that landed.
    let settling = !reduced
        && flights
            .iter()
            .any(|(f, _)| s.now >= f.start() && s.now < f.arrives() + SETTLE);
    if matches!(s.step, Step::YourTurn | Step::Hearing) && !settling {
        out = out.child(
            el().key("caret-at")
                .absolute(origin[0], origin[1])
                .child(field_text::caret(p, "caret", caret, line)),
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
        assert_eq!(at.len(), 7);
        assert_eq!(at[0], 0.16);
        assert!(at.windows(2).all(|w| w[0] <= w[1]));
        assert!(*at.last().unwrap() <= ANSWER_JAY.release);
        let burst = heard_at(false);
        assert_eq!(burst[0], ANSWER_JAY.release);
    }

    #[test]
    fn card_keyboard_and_voice_hud_share_the_central_spine() {
        let (_, kh) = keyboard::size();
        assert!(((CARD_X + CHAT_W * 0.5) - STAGE_W * 0.5).abs() < 0.01);
        assert!((CHAT_W - keyboard::size().0).abs() < 0.01, "one rail");
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
