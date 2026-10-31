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

use rune::{Anim, El, Spring, SpringPair, TextStyle, Transition, col, el, row, text};

use super::super::controls::Paint;
use super::cards::{
    App, CHAT_W, Caption, FIELD_FONT, Notice, Notify, Reply, Side, bubble, chat, chat_height,
    field_text_origin, presenting, system_line, typing_dots,
};
use super::field_text::{self, CHAT};
use super::fixture::Take;
use super::flight::{Flight, flicker, flying, typeset};
use super::flows::continue_button;
use super::hud_stage::voice_or_idle_slot;
use super::keyboard::{self, keyboard_lit};
use super::landing::{LEAD, UI_SPRING, keys, language_pick, ready, title_font};
use super::mic_line::mic_line;
use super::pill::{pill, slot};
use super::scene_script::{Script, fill};
use super::stories::{Flow, Onboard, Step};
use super::triumph::{self, HexBeats, Said};
use super::typing::{Focus, Glyph, Line, Timing, Who};

const STAGE_W: f32 = 500.0;
const STAGE_H: f32 = 540.0;
const CENTRE_X: f32 = STAGE_W * 0.5;
const CARD_X: f32 = CENTRE_X - CHAT_W * 0.5;
const CARD_TOP: f32 = 0.0;
/// Over the keyboard's upper rows, which melt away, above the lit `⌥`.
const HUD_CENTRE: [f32; 2] = [CENTRE_X, 416.0];
/// The keys behind the HUD go dark around it, this far out from its centre.
const POCKET: [f32; 2] = [136.0, 58.0];
/// The HUD has collapsed away this long after the answer's release.
const HUD_GONE: f64 = 0.74;
/// How long the pocket takes to fill back in once the HUD has gone.
const REFILL: f64 = 0.3;
const HUD_SCALE: f32 = 1.05;
const CALM: f32 = 0.20;
const BURST: f64 = 0.04;
const ACTION_H: f32 = 32.0;
const TOP_BAR: f32 = 48.0;
/// Room under the demo, kept small so the thread can be tall.
const BOTTOM: f32 = 24.0;
/// The last few lines, while they aren't the focus.
const RECENT: f32 = 0.80;
/// Older lines.
const OLDER: f32 = 0.65;
/// Everything but the focus while the story freezes.
const FROZEN: f32 = 0.28;
/// The message being answered while you type.
const CONTEXT: f32 = 0.9;
/// How many of the newest lines read as one stack, at [`RECENT`].
const STACK: usize = 5;
/// The thread behind the draft in slow motion.
const SLOW_THREAD: f32 = 0.42;
/// How long a line that cuts in while you type stays at full brightness.
const CUT_IN: f64 = 0.8;
/// `"Without Hex"`'s field holds one line.
const WITHOUT_LINES: usize = 1;
/// From the stage settling to the button's clock: Jay's question arrives
/// as the stage settles ([`typing::Scene`]'s lines before the button).
const PRE_ROLL: f64 = 1.2;
/// `"With Hex"`'s first beat: the disaster zips back out, newest first.
pub(crate) const REWIND: f64 = 0.48;
const REWIND_STAGGER: f64 = 0.045;
/// The rewind keeps the field locked until the removal has peeled away.
const UNLOCK: f64 = 0.1;
/// How long the field stays empty after a send, before its placeholder.
const CLEAR: f64 = 0.22;
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
const MELTED: f32 = 0.14;
/// The keyboard while Jay talks, and before the button.
const IDLE: f32 = 0.52;
/// The keyboard while you type.
const TYPING: f32 = 0.92;
/// The shortcut's ember on the score.
const EMBER: f32 = 0.22;

/// How long a landed word takes to settle into the field.
const SETTLE: f64 = 0.15;

/// The scene this run plays; it's chosen once, so the stage's geometry can
/// be worked out without an [`Onboard`].
fn script() -> &'static Script<'static> {
    Script::chosen()
}

/// Your spoken line in `"With Hex"`.
fn take() -> &'static Take {
    script().take()
}

fn reply_lines() -> usize {
    CHAT.wrap(take().text()).len()
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

/// When each of `take`'s words is heard after the shortcut.
fn heard_for(take: &'static Take, streams: bool) -> Vec<f64> {
    let count = take.text().split_whitespace().count();
    (0..count)
        .map(|i| {
            if streams {
                take.updates
                    .iter()
                    .find(|(_, partial)| partial.split_whitespace().count() > i)
                    .map_or(take.release, |(at, _)| *at)
            } else {
                take.release + i as f64 * BURST
            }
        })
        .collect()
}

fn flights_from(streams: bool, from: f64) -> Vec<(Flight, field_text::Slot<'static>)> {
    flights_for(take(), streams, from)
}

fn flights_for(
    take: &'static Take,
    streams: bool,
    from: f64,
) -> Vec<(Flight, field_text::Slot<'static>)> {
    let origin = field_origin();
    CHAT.slots(take.text())
        .into_iter()
        .zip(heard_for(take, streams))
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

/// `"With Hex"`'s beats after the shortcut goes down: the answer sends once
/// its words have landed, then the scene's cheers ([`HexBeats::plan`]).
pub(crate) fn hex_beats(streams: bool, timing: &Timing) -> HexBeats {
    HexBeats::plan(script(), send_at(take(), streams, timing), timing)
}

/// When `take` leaves the field after the shortcut: once its words land.
pub(crate) fn send_at(take: &'static Take, streams: bool, timing: &Timing) -> f64 {
    let landed = flights_for(take, streams, 0.0)
        .iter()
        .map(|(f, _)| f.arrives())
        .fold(take.release, f64::max);
    landed + timing.hex_send
}

/// From `Heard` to the score.
pub(crate) fn reply_secs(s: &Onboard) -> f64 {
    hex_beats(s.streams(), &s.timing).done - (take().release + 0.1)
}

/// Seconds since the shortcut went down on `"With Hex"`.
fn take_time(s: &Onboard) -> Option<f64> {
    matches!(s.step, Step::Hearing | Step::Heard | Step::Done)
        .then_some(s.take_from)
        .flatten()
        .map(|from| s.now - from)
}

/// How long the comedy plays before `Skip →` shows, on a first watch.
const SKIP_AFTER: f64 = 2.0;

/// The comedy is playing and `Skip →` shows: two seconds in, or at once
/// for someone who has seen it before, until the answer sends.
pub(super) fn skippable(s: &Onboard) -> bool {
    let Some(from) = s.typing_from else {
        return false;
    };
    // Once the answer is out there's nothing left to skip.
    matches!(s.step, Step::Parked | Step::YourTurn | Step::Hearing)
        && !hex_sent(s)
        && (s.had_model && s.granted || s.now - from >= SKIP_AFTER)
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
        _ if skippable(s) => super::flows::skip_button(p, s),
        _ => el().key("none"),
    };
    vec![
        top,
        middle()
            .pb(BOTTOM)
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
            line("lead-usual", script().lead.to_string()),
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
        _ if s.flow == Flow::FirstRun || s.flow == Flow::Versus => {
            let with = &script().with;
            let filled = |template| {
                fill(
                    template,
                    take().release,
                    s.scene.score_secs(),
                    s.scene.typos(),
                )
            };
            (
                "score",
                filled(with.score),
                line("lead-score", filled(with.typed)),
            )
        }
        _ => (
            "heard",
            "That's it".to_string(),
            line("lead-done", "The same reply, typed and then spoken.".into()),
        ),
    };
    col()
        .key(key)
        .gap(8.0)
        .items_center()
        .transition(act_break())
        .enter_delay(HEADING_GAP)
        .child(text(title).key("title").font(title_font(p)).color(p.text()))
        .child(row().key("lead-row").h(24.0).center().child(lead))
}

/// How long the outgoing heading has alone before the next sharpens in.
const HEADING_GAP: f32 = 0.09;

/// One heading gives way to the next in place: the outgoing one blurs away
/// fast, mostly gone before the incoming one sharpens in, so two titles
/// never sit on each other.
fn act_break() -> Transition {
    let soft = |scale| Transition::blur(3.5).combined(Transition::scale(scale));
    Transition::asymmetric(soft(0.98), soft(1.02)).springs(SpringPair::new(
        Spring::new(0.28, 0.0),
        Spring::new(0.14, 0.0),
    ))
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

    let flash = |id: &str| match t {
        Some(t) if !voice_phase => s.scene.flash(t, id),
        _ => 0.0,
    };
    // From where the removal left it, the deck only ever goes down as its
    // keys melt; the shortcut alone lights up, and the keys behind the HUD
    // go dark.
    let level = match since {
        Some(_) => FROZEN + (MELTED - FROZEN) * melt_u,
        None => keyboard_level(s),
    };
    // Once the HUD has gone, the keys behind it fill back in.
    let pocket_u = melt_u
        * (1.0
            - take_time(s).map_or(0.0, |take| {
                smoothstep(((take - self::take().release - HUD_GONE) / REFILL) as f32)
            }));
    let deck = [(STAGE_W - kw) * 0.5, STAGE_H - kh];
    let pocket = [HUD_CENTRE[0] - deck[0], HUD_CENTRE[1] - deck[1]];
    let kb_el = keyboard_lit(
        p,
        "keyboard",
        keyboard::Lighting {
            enter: enter_u,
            level,
            depth: &|id| key_depth(s, id),
            glow: &|id| key_glow(s, id, melt_u),
            flash: &flash,
            hollow: &|[x, y, w, h]| {
                let dx = (x + w * 0.5 - pocket[0]) / POCKET[0];
                let dy = (y + h * 0.5 - pocket[1]) / POCKET[1];
                pocket_u * (1.0 - smoothstep(((dx * dx + dy * dy).sqrt() - 1.0) / 0.35))
            },
        },
    );
    let kb_shift = (1.0 - enter_u) * 14.0 + melt_u * 6.0;
    let shudder = match t {
        Some(t) if !reduced && !voice_phase => s.scene.shudder(t),
        _ => [0.0, 0.0],
    };
    let keys_el = el()
        .key("keys")
        .transition(
            Transition::opacity()
                .springs(SpringPair::new(UI_SPRING, Spring::new(s.motion.exit, 0.0))),
        )
        .absolute(deck[0], deck[1])
        .offset(shudder[0], kb_shift + shudder[1])
        .size(kw, kh)
        .offset_motion(Anim::INSTANT)
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
                take(),
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
        .children(t.filter(|_| !voice_phase).and_then(|t| slam(p, s, t)))
        .children(take_time(s).and_then(|t| {
            triumph::effects(
                p,
                &hex_beats(s.streams(), &s.timing),
                t,
                [STAGE_W, STAGE_H],
                reduced,
            )
        }))
}

/// Caps Lock's light once it's on.
const CAPS_LIGHT: f32 = 0.9;
/// The autocorrect title: how long it takes to land, and how far it shakes.
const SLAM_IN: f64 = 0.09;
const SLAM_SHAKE: f32 = 9.0;
const SLAM_COPY: &str = "AUTOCORRECT";

/// The autocorrect title slamming onto the stage like a fighting game's
/// round call: it drops in oversized, punches past its size, shakes the
/// stage with a flash, holds, then snaps away as time comes back. Reduce
/// Motion fades it in and out where it lands.
fn slam(p: Paint, s: &Onboard, t: f64) -> Option<El> {
    let since = s.scene.slam(t)?;
    let reduced = s.reduced.get();
    let k = p.k;
    let u = (since / SLAM_IN).min(1.0) as f32;
    let (scale, shake) = if reduced {
        (1.0, [0.0, 0.0])
    } else {
        // In from 2.4x, overshooting to 0.9x, then a damped punch to rest.
        let settle = (since - SLAM_IN).max(0.0);
        let punch = if since < SLAM_IN {
            2.4 - 1.5 * smoothstep(u)
        } else {
            1.0 - 0.1
                * (std::f64::consts::TAU * settle / 0.16).cos() as f32
                * (-settle / 0.09).exp() as f32
        };
        let decay = (-settle / 0.12).exp() as f32 * (since >= SLAM_IN) as u8 as f32;
        let wobble =
            |f: f64, phase: f64| (std::f64::consts::TAU * (f * settle + phase)).sin() as f32;
        (
            punch,
            [
                SLAM_SHAKE * decay * wobble(23.0, 0.0),
                0.6 * SLAM_SHAKE * decay * wobble(17.0, 0.3),
            ],
        )
    };
    let opacity = if reduced {
        smoothstep(u)
    } else {
        u.min(1.0).sqrt()
    };
    let flash = if reduced {
        0.0
    } else {
        0.55 * (-(since - SLAM_IN).max(0.0) / 0.07).exp() as f32
            * (since >= SLAM_IN * 0.6) as u8 as f32
    };
    let title = text(SLAM_COPY.to_string())
        .key("slam-copy")
        .font(TextStyle::new(52.0, 900.0).lh(56.0).tracking(-1.0))
        .color(k.c.submit_fg);
    let plate = row()
        .key("slam-plate")
        .px(26.0)
        .py(10.0)
        .radius(14.0)
        .continuous_corners()
        .fill(k.c.bg_accent)
        .shadow(8.0, 24.0, k.c.black.alpha(0.55))
        .rotate(-0.07)
        .child(title);
    Some(
        el().key("slam")
            .absolute(0.0, 0.0)
            .size(STAGE_W, STAGE_H)
            .instant_changes()
            // It snaps away as time comes back, not fades.
            .transition(Transition::opacity().springs(SpringPair::new(
                Spring::new(0.05, 0.0),
                Spring::new(0.04, 0.0),
            )))
            .child(
                el().key("slam-flash")
                    .absolute(0.0, 0.0)
                    .size(STAGE_W, STAGE_H)
                    .fill(k.c.white)
                    .opacity(flash),
            )
            .child(
                row()
                    .key("slam-stage")
                    .absolute(0.0, 0.0)
                    .size(STAGE_W, STAGE_H * 0.62)
                    .center()
                    .child(
                        plate
                            .scale(scale)
                            .offset(shake[0], shake[1])
                            .opacity(opacity),
                    ),
            ),
    )
}

/// The red pill under the header, from the first frame, flaring at scene
/// time `t`.
fn banner(p: Paint, s: &Onboard, t: f64) -> Option<El> {
    script()
        .pill
        .map(|copy| presenting(p, copy, s.now, s.scene.flare(t)))
}

/// The conversation's header, named `title`.
fn room(title: &'static str) -> Notice {
    Notice {
        key: "room",
        app: App::Messages,
        from: title,
        place: script().group,
        body: "",
        field: "iMessage",
    }
}

/// A name over the first of a run of theirs, in a group.
fn caption(who: u8, named: bool) -> Caption<'static> {
    let script = script();
    if named && script.group.is_some() {
        let person = script.person(who);
        Caption::From(person.name, person.colour)
    } else {
        Caption::None
    }
}

/// `"Without Hex"`'s thread at `t`: every line so far, each at
/// `opacity(line index, line)` with `"Read"` under the one it reads, and
/// Jay's typing dots.
fn thread_at(p: Paint, s: &Onboard, t: f64, opacity: impl Fn(usize, &Line) -> f32) -> Vec<El> {
    let reduced = s.reduced.get();
    let scene = &s.scene;
    let lines = &scene.lines()[..scene.shown(t)];
    let receipt_at = scene.read(t);
    let mut thread = Vec::new();
    let mut sent = 0;
    for (i, line) in lines.iter().enumerate() {
        let o = opacity(i, line);
        let taps: Vec<Glyph> = scene
            .taps(t)
            .filter(|tap| tap.line == i)
            .map(|tap| tap.glyph)
            .collect();
        thread.push(match line.who {
            Who::Them(who) => {
                // The sender's name over the first of a run, as in a group.
                let named = i == 0 || lines[i - 1].who != line.who;
                bubble(
                    p,
                    format!("l{i}"),
                    Side::Them,
                    caption(who, named),
                    &line.text,
                    &taps,
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
                    if receipt_at == Some(i) {
                        Caption::Read
                    } else {
                        Caption::None
                    },
                    &line.text,
                    &taps,
                    o,
                    reduced,
                )
            }
            Who::System => system_line(p, format!("l{i}"), &line.text, Notify::Danger, o, reduced),
            Who::Status => system_line(p, format!("l{i}"), &line.text, Notify::Status, o, reduced),
        });
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
        .rposition(|l| matches!(l.who, Who::Them(_)));
    let removed = scene.removed(t);
    let slow = scene.slow(t) > 0.0;
    let opacity = |i: usize, line: &Line| match focus {
        Focus::Waiting => 1.0,
        Focus::Line(j) | Focus::Read(j) if i == j => 1.0,
        // Jay's last two lines stay readable over the removal: the firing
        // reads as one stack.
        _ if removed && i + 2 == shown => 0.82,
        _ if removed && i + 3 == shown => 0.55,
        // In slow motion the draft has the eye, but the stack stays legible.
        _ if slow => SLOW_THREAD,
        _ if frozen => FROZEN,
        // Someone cutting in while you type lands bright, then settles.
        Focus::Field if t - line.at < CUT_IN => 1.0,
        Focus::Field if Some(i) == answering => CONTEXT,
        _ if shown - i <= STACK => RECENT,
        _ => OLDER,
    };
    let thread = thread_at(p, s, t, opacity);
    let room = room(scene.title(t));
    if removed {
        let locked = scene.locked();
        return chat(
            p,
            &room,
            banner(p, s, t),
            thread,
            Reply::Locked(&locked),
            1.0,
        );
    }
    let (draft, caret) = scene.draft(t);
    // The field stays empty a moment after a send, so the bubble lifts out
    // of it before the placeholder comes back.
    let just_sent = scene.lines()[..shown]
        .iter()
        .rev()
        .find(|l| l.who == Who::You)
        .is_some_and(|l| t - l.at < CLEAR);
    chat(
        p,
        &room,
        banner(p, s, t),
        thread,
        Reply::Lines {
            text: draft,
            caret,
            lines: WITHOUT_LINES,
            placeholder: draft.is_empty() && !just_sent,
            focus: caret.is_some(),
            sent: false,
            mark: scene.mark(t),
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
    let at = |when: f64| t.is_some_and(|t| t >= when);
    let sent = at(beats.send);
    let replied = beats.lines.first().is_some_and(|&(when, _)| at(when));
    let typing = beats
        .dots
        .is_some_and(|(from, until)| at(from) && !at(until));
    let composing = !sent;
    let asked = s.scene.lines().iter().filter(|l| l.at < 0.0).count();
    let rewinding = since < REWIND && !reduced;
    let mut thread = if rewinding {
        let end = s.scene.duration();
        let mut all = thread_at(p, s, end, |i, _| if i < asked { 1.0 } else { 0.5 });
        let gone = ((since / REWIND_STAGGER) as usize + 1).min(all.len() - asked);
        all.truncate(all.len() - gone);
        all
    } else {
        let question = if composing { 1.0 } else { OLDER };
        thread_at(p, s, -1e-3, |_, _| question)
    };
    if sent {
        let taps: Vec<Glyph> = beats
            .taps
            .iter()
            .filter(|&&(when, _)| at(when))
            .map(|&(_, glyph)| glyph)
            .collect();
        thread.push(bubble(
            p,
            "hex-bubble".into(),
            Side::You,
            Caption::None,
            take().text(),
            &taps,
            if replied { RECENT } else { 1.0 },
            reduced,
        ));
    }
    if typing {
        thread.push(typing_dots(p, "dots".into(), s.now, 1.0, reduced));
    }
    let mut last: Option<u8> = None;
    for (i, (_, said)) in beats
        .lines
        .iter()
        .enumerate()
        .filter(|(_, (when, _))| at(*when))
    {
        let key = format!("cheer-{i}");
        thread.push(match *said {
            Said::Them(who, copy) => {
                let named = last != Some(who);
                last = Some(who);
                bubble(
                    p,
                    key,
                    Side::Them,
                    caption(who, named),
                    copy,
                    &[],
                    1.0,
                    reduced,
                )
            }
            Said::You(copy) => {
                last = None;
                bubble(p, key, Side::You, Caption::None, copy, &[], 1.0, reduced)
            }
            Said::Pill(ref copy) => {
                last = None;
                system_line(p, key, copy, Notify::Cheer, 1.0, reduced)
            }
        });
    }
    let room = room(beats.title(t.unwrap_or(f64::NEG_INFINITY), script().title));
    let flights = flights(s);
    let started = flights.iter().any(|(f, _)| s.now >= f.start());
    // The field grows a line only once a word heads for it.
    let lines = flights
        .iter()
        .filter(|(f, _)| s.now >= f.start())
        .map(|(_, slot)| slot.line + 1)
        .max()
        .unwrap_or(1);
    if rewinding && since < UNLOCK {
        let locked = s.scene.locked();
        return chat(
            p,
            &room,
            banner(p, s, -1e-3),
            thread,
            Reply::Locked(&locked),
            1.0,
        );
    }
    // The field waits, unfocused, until the shortcut goes down.
    let listening = s.step >= Step::Hearing && !sent;
    let cleared = t.is_some_and(|t| t >= beats.send + CLEAR);
    // The pill can change as the payoff lands; gone, it keeps its row.
    let banner =
        script().pill.map(
            |_| match beats.pill(t.unwrap_or(f64::NEG_INFINITY), script().pill) {
                (Some(copy), flare) => presenting(p, copy, s.now, flare),
                (None, _) => el().key("presenting"),
            },
        );
    chat(
        p,
        &room,
        banner,
        thread,
        Reply::Lines {
            text: "",
            caret: None,
            lines: if sent { 1 } else { lines },
            placeholder: !started || cleared,
            focus: listening,
            sent: false,
            mark: None,
        },
        if composing { 1.0 } else { 0.6 },
    )
}

/// What the keyboard is lit to at scene time `t` in `"Without Hex"`: up
/// while you type and through Return's flash, back while Jay talks, and down
/// only when the story freezes.
fn keyboard_target(s: &Onboard, t: f64) -> f32 {
    if t < 0.0 {
        return IDLE;
    }
    let scene = &s.scene;
    if scene.focus(t) == Focus::Field || scene.flash(t, "return") > 0.0 {
        TYPING
    } else if scene.frozen(t) {
        FROZEN
    } else {
        IDLE
    }
}

/// [`keyboard_target`] followed by a critically damped spring, worked out
/// from the scene's clock over the last second so any frame can be drawn
/// on its own.
fn keyboard_level(s: &Onboard) -> f32 {
    const RESPONSE: f64 = 0.36;
    const STEP: f64 = 1.0 / 240.0;
    let Some(t) = scene_time(s) else {
        return IDLE;
    };
    let omega = std::f64::consts::TAU / RESPONSE;
    let mut at = t - 1.0;
    let mut level = f64::from(keyboard_target(s, at));
    let mut speed = 0.0;
    while at < t {
        let goal = f64::from(keyboard_target(s, at));
        speed += (omega * omega * (goal - level) - 2.0 * omega * speed) * STEP;
        level += speed * STEP;
        at += STEP;
    }
    level as f32
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
    // Caps Lock's light, once an elbow knocks it on.
    if id == "caps" && s.step == Step::Parked {
        return match scene_time(s) {
            Some(t) if s.scene.caps(t) => CAPS_LIGHT,
            _ => 0.0,
        };
    }
    if !shortcut_keys(s).any(|k| k == id) {
        return 0.0;
    }
    match s.step {
        Step::YourTurn => 0.85 * melt_u,
        Step::Hearing => 1.0,
        // It cools to an ember that stays through the score.
        Step::Heard => {
            let fade = s.take_from.map_or(0.0, |from| {
                1.0 - smoothstep(((s.now - from - take().release) / 0.45) as f32)
            });
            EMBER + (0.70 - EMBER) * fade
        }
        Step::Done => EMBER,
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
    if s.step == Step::Hearing && !settling {
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
        let at = heard_for(take(), true);
        assert_eq!(at.len(), 7);
        assert_eq!(at[0], 0.16);
        assert!(at.windows(2).all(|w| w[0] <= w[1]));
        assert!(*at.last().unwrap() <= take().release);
        let burst = heard_for(take(), false);
        assert_eq!(burst[0], take().release);
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
        let reply = beats.lines[0].0;
        assert!(reply > beats.send + 0.4);
        assert!(beats.done > reply + 1.0);
        assert!(beats.done <= 8.0, "{}", beats.done);
    }
}
