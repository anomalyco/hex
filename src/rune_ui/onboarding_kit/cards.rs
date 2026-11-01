//! Example macOS notifications to answer by voice, and the Messages
//! group conversation `"Without Hex"` plays out in, drawn strictly with OpenCode
//! theme tokens.

use rune::{
    Color, El, LineCap, Path, Shape, Spring, SpringPair, TextStyle, Transition, canvas, col, el,
    row, text,
};

use super::super::controls::{KeySize, Paint, key_tray, keycap};
use super::super::lists::allowed;
use super::field_text;
use super::mic::mic_level;
use super::typing::Glyph;
use super::words::live_words;

pub(crate) const CARD_W: f32 = 404.0;
const CARD_PAD: f32 = 14.0;
const RADIUS: f32 = 16.0;
const ICON: f32 = 32.0;
const FIELD_H: f32 = 36.0;
const FIELD_PX: f32 = 12.0;
const FIELD_PY: f32 = 7.0;
pub(crate) const FIELD_FONT: TextStyle = TextStyle::new(13.0, 440.0);
pub(crate) const FIELD_LINE: f32 = 18.0;
const SENT_W: f32 = 58.0;
pub(crate) const REPLY_W: f32 = CARD_W - 2.0 * CARD_PAD - 2.0 * FIELD_PX - SENT_W;
/// [`chat`] runs as wide as the keyboard under it.
pub(crate) const CHAT_W: f32 = 472.0;
/// [`chat`]'s field has no `"Sent"` badge.
pub(crate) const CHAT_REPLY_W: f32 = CHAT_W - 2.0 * CARD_PAD - 2.0 * FIELD_PX;
const DRIFT: f32 = 2.5;
const DRIFT_SECS: f64 = 5.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum App {
    Messages,
    Slack,
}

impl App {
    fn name(self) -> &'static str {
        match self {
            Self::Messages => "Messages",
            Self::Slack => "Slack",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Notice {
    pub(crate) key: &'static str,
    pub(crate) app: App,
    pub(crate) from: &'static str,
    pub(crate) place: Option<&'static str>,
    pub(crate) body: &'static str,
    pub(crate) field: &'static str,
}

pub(crate) const FRANK: Notice = Notice {
    key: "frank",
    app: App::Messages,
    from: "Frank",
    place: None,
    body: "your phone is on silent right",
    field: "iMessage",
};
pub(crate) const JAY: Notice = Notice {
    key: "jay",
    app: App::Messages,
    from: "Jay · CEO",
    place: None,
    body: "The investors are in the room. Are we ready to ship?",
    field: "iMessage",
};
pub(crate) const DAX: Notice = Notice {
    key: "dax",
    app: App::Slack,
    from: "Dax",
    place: Some("#opencode"),
    body: "can you look at the effect PR before 5",
    field: "Reply in thread",
};
pub(crate) const ADAM: Notice = Notice {
    key: "adam",
    app: App::Slack,
    from: "Adam",
    place: Some("#hex"),
    body: "hex just typed my standup into figma",
    field: "Reply in thread",
};

#[derive(Clone, Copy)]
pub(crate) enum Reply<'a> {
    Empty,
    Ask {
        keys: &'a [String],
        hint: &'a str,
    },
    Hearing {
        partial: &'a str,
        meter: (f32, f32),
    },
    Typing(&'a str),
    Sent(&'a str),
    Lines {
        sent_bubbles: &'a [String],
        text: &'a str,
        caret: Option<usize>,
        lines: usize,
        placeholder: bool,
        focus: bool,
        sent: bool,
    },
    /// No field any more, only why.
    Locked(&'a str),
}

/// Height of [`notice`] when its reply field uses [`Reply::Lines`] with empty
/// `sent_bubbles` and `lines` wrapped lines.
pub(crate) fn lines_card_height(n: &Notice, lines: usize) -> f32 {
    // Measure how many lines `n.body` takes inside the top message column
    // (`CARD_W - 2*CARD_PAD - ICON - 10.0`).
    let k = rune_theme_opencode::kit(true);
    let body_w = CARD_W - 2.0 * CARD_PAD - ICON - 10.0;
    let (_, bh) = rune::text::measure(n.body, &[], k.ty.ui, Some(body_w));
    let top_h = (12.0 + 1.0 + 20.0 + 1.0 + bh).max(ICON);
    2.0 * CARD_PAD + top_h + 10.0 + 2.0 * FIELD_PY + lines as f32 * FIELD_LINE
}

pub(crate) fn notice(p: Paint, n: &Notice, reply: Reply<'_>, example: bool) -> El {
    let k = p.k;
    let head = row()
        .key("head")
        .gap(6.0)
        .center_y()
        .child(
            text(n.app.name())
                .key("app")
                .font(k.ty.small)
                .color(p.muted()),
        )
        .child(el().key("space").grow(1.0))
        .child(
            text(if example { "example" } else { "now" })
                .key("time")
                .font(k.ty.small)
                .color(p.faint()),
        );
    let title = match n.place {
        Some(place) => format!("{} in {place}", n.from),
        None => n.from.to_string(),
    };
    let message = col()
        .key("message")
        .grow(1.0)
        .gap(1.0)
        .child(head)
        .child(
            text(title)
                .key("title")
                .font(TextStyle {
                    weight: if k.dark { 560.0 } else { 600.0 },
                    ..k.ty.ui_medium
                })
                .color(p.text()),
        )
        .child(
            text(n.body)
                .key("body")
                .font(k.ty.ui)
                .color(p.text())
                .wrap()
                .max_lines(2),
        );

    let bubbles: &[String] = match reply {
        Reply::Lines { sent_bubbles, .. } => sent_bubbles,
        _ => &[],
    };

    let (fill, rim) = material(p);
    let mut card = col()
        .key(n.key)
        .w(CARD_W)
        .padding(CARD_PAD)
        .gap(10.0)
        .radius(RADIUS)
        .continuous_corners()
        .fill(fill)
        .backdrop_blur(24.0)
        .border(p.hair, rim)
        .motion(Spring::new(0.24, 0.0))
        .child(
            row()
                .key("top")
                .gap(10.0)
                .items_start()
                .child(app_icon(p, n.app))
                .child(message),
        );

    if !bubbles.is_empty() {
        card = card.child(sent_thread(p, bubbles));
    }

    // Remove the field in the same layout change as the second sent bubble,
    // not when Return lifts. Otherwise the card first makes room for both,
    // then shrinks while the departing field is already outside its new edge.
    let hide_field = bubbles.len() >= 2 && matches!(reply, Reply::Lines { text: "", .. });
    if !hide_field {
        card = card.child(field(p, n, reply, field_text::CARD, None));
    }

    k.elevate(card, &k.elevation.floating)
}

/// Right-aligned sent bubbles for the accidental double-send typos!
fn sent_thread(p: Paint, bubbles: &[String]) -> El {
    let k = p.k;
    let bubble_bg = rune_theme_opencode::scale("cobalt", k.dark, if k.dark { 4 } else { 3 });
    let bubble_rim = rune_theme_opencode::scale("cobalt", k.dark, if k.dark { 7 } else { 6 });
    let mut list = col().key("sent-thread").w_full().gap(5.0).items_end();
    for (i, msg) in bubbles.iter().enumerate() {
        list = list.child(
            row()
                .key(format!("sent-bubble-{i}"))
                .px(10.0)
                .py(4.0)
                .gap(6.0)
                .center_y()
                .radius(12.0)
                .fill(bubble_bg)
                .border(p.hair, bubble_rim)
                // The travel comes from the launch out of the field
                // (`stories::poll`); the bubble itself only fades up.
                .transition(Transition::opacity())
                .child(
                    text(msg.clone())
                        .key("msg")
                        .font(FIELD_FONT.lh(16.0))
                        .color(p.text()),
                )
                .child(text("Sent").key("tag").font(k.ty.small).color(p.muted())),
        );
    }
    list
}

/// The conversation's visible thread, between its header and the field.
pub(crate) const THREAD_H: f32 = 232.0;
const HEADER_H: f32 = 34.0;
const BUBBLE_MAX: f32 = 328.0;
const BUBBLE_FONT: TextStyle = FIELD_FONT.lh(17.0);
/// Older lines fade out under the header over this much.
const THREAD_FADE: f32 = 28.0;

/// Height of [`chat`] with a reply field `lines` tall.
pub(crate) fn chat_height(lines: usize) -> f32 {
    2.0 * CARD_PAD + HEADER_H + 10.0 + THREAD_H + 10.0 + 2.0 * FIELD_PY + lines as f32 * FIELD_LINE
}

/// A Messages conversation with `n.from` (a group when `n.place` names its
/// members): header, the newest of `thread`'s lines (older ones scroll up
/// under a soft edge), and the reply field.
/// `banner`, such as [`presenting`], sits at the header's right end, level
/// with the title, where Messages puts a live call.
pub(crate) fn chat(
    p: Paint,
    n: &Notice,
    banner: Option<El>,
    thread: Vec<El>,
    reply: Reply<'_>,
    mark: Option<(usize, usize)>,
    field_opacity: f32,
) -> El {
    let k = p.k;
    let title = text(n.from)
        .key("title")
        .font(TextStyle {
            weight: if k.dark { 560.0 } else { 600.0 },
            ..k.ty.ui_medium
        })
        .color(p.text());
    let title = match banner {
        Some(banner) => row()
            .key("title-row")
            .w_full()
            .gap(8.0)
            .center_y()
            .child(col().key("name").grow(1.0).child(title))
            .child(banner),
        None => title,
    };
    let head = row()
        .key("head")
        .h(HEADER_H)
        .gap(10.0)
        .center_y()
        .child(app_icon(p, n.app))
        .child(
            col().key("who").grow(1.0).child(title).child(
                text(n.place.unwrap_or(n.field))
                    .key("app")
                    .font(k.ty.small)
                    .color(p.faint()),
            ),
        );
    let thread = col()
        .key("thread")
        .w_full()
        .h(THREAD_H)
        .justify_end()
        .gap(6.0)
        .clip_fade(0.0, THREAD_FADE, 0.0, 0.0)
        .children(thread);
    let (fill, rim) = material(p);
    let card = col()
        .key(n.key)
        .w(CHAT_W)
        .padding(CARD_PAD)
        .gap(10.0)
        .radius(RADIUS)
        .continuous_corners()
        .fill(fill)
        .backdrop_blur(24.0)
        .border(p.hair, rim)
        .child(head)
        .child(thread)
        .child(
            field(p, n, reply, field_text::CHAT, mark)
                .opacity(field_opacity)
                .opacity_motion(Spring::new(0.32, 0.0)),
        );
    k.elevate(card, &k.elevation.floating)
}

/// Who a [`bubble`] is from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Them,
    You,
}

/// The small print around a [`bubble`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Caption<'a> {
    None,
    /// Theirs in a group, as Messages draws it: the sender's name, small
    /// and grey, over the first of a run, and their face (in their colour,
    /// an OpenCode theme scale such as `"cobalt"`) beside the last.
    From(Sender<'a>),
    /// `"Read"` under one of yours, moving with it.
    Read,
}

/// Who sent one of theirs in a group, and where it sits in their run.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sender<'a> {
    pub(crate) name: &'a str,
    pub(crate) colour: &'a str,
    pub(crate) first: bool,
    pub(crate) last: bool,
}

/// A sender's face beside their messages in a group.
const FACE: f32 = 24.0;
const FACE_GAP: f32 = 7.0;

/// The sender's face beside a line in a group, or the room it would take.
fn face_slot(p: Paint, sender: Sender<'_>) -> El {
    match sender {
        sender if sender.last => {
            super::inbox::face(p, sender.name, sender.colour, FACE).key("face")
        }
        _ => el().key("face").size(FACE, FACE),
    }
}

/// One message in [`chat`]'s thread: theirs on the left, yours on the right
/// in cobalt, with its [`Caption`] and any `taps` on its corner. It glides
/// up as newer lines arrive and fades to `opacity`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bubble(
    p: Paint,
    key: String,
    side: Side,
    caption: Caption<'_>,
    copy: &str,
    taps: &[Glyph],
    opacity: f32,
    reduced: bool,
) -> El {
    let k = p.k;
    let (fill, rim) = match side {
        Side::Them => (
            if k.dark { k.c.layer_02 } else { k.c.layer_01 },
            k.c.border_weak,
        ),
        Side::You => (
            rune_theme_opencode::scale("cobalt", k.dark, if k.dark { 4 } else { 3 }),
            rune_theme_opencode::scale("cobalt", k.dark, if k.dark { 7 } else { 6 }),
        ),
    };
    let bubble = row()
        .key("bubble")
        .max_w(BUBBLE_MAX)
        .px(11.0)
        .py(5.0)
        .radius(13.0)
        .continuous_corners()
        .fill(fill)
        .border(p.hair, rim)
        .child(
            text(copy.to_string())
                .key("msg")
                .font(BUBBLE_FONT)
                .color(p.text())
                .wrap()
                .shrink(),
        )
        .children(tapbacks(p, side, taps, reduced));
    let line = line(key, side, opacity, reduced);
    let line = if taps.is_empty() {
        line
    } else {
        line.mt(TAP_ROOM)
    };
    match caption {
        Caption::None => line.child(bubble.shrink()),
        Caption::From(sender) => line.child(
            row()
                .key("sender")
                .gap(FACE_GAP)
                .items_end()
                .shrink()
                .child(face_slot(p, sender))
                .child(
                    col()
                        .key("named")
                        .gap(2.0)
                        .shrink()
                        .children(sender.first.then(|| {
                            text(sender.name.to_string())
                                .key("from")
                                .font(k.ty.small)
                                .color(p.faint())
                                .ml(12.0)
                        }))
                        .child(bubble.shrink()),
                ),
        ),
        Caption::Read => line.child(
            col()
                .key("read-under")
                .gap(2.0)
                .items_end()
                .shrink()
                .child(bubble.shrink())
                .child(
                    text("Read")
                        .key("read")
                        .font(k.ty.small_medium)
                        .color(p.muted())
                        .transition(Transition::opacity()),
                ),
        ),
    }
}

/// A row of [`chat`]'s thread, aligned to `side`. Theirs rise in; yours
/// arrive whole, launched from the field. Any line leaves by peeling up
/// fast, so a rewind never piles ghosts up. Under Reduce Motion it only
/// fades.
pub(crate) fn line(key: String, side: Side, opacity: f32, reduced: bool) -> El {
    let row = row().key(key).w_full();
    let row = match side {
        Side::Them => row.justify_start(),
        Side::You => row.justify_end(),
    };
    let row = row.opacity(opacity).opacity_motion(Spring::new(0.32, 0.0));
    if reduced {
        return row.transition(Transition::opacity());
    }
    let enter = match side {
        Side::Them => Transition::opacity().combined(Transition::offset(0.0, 10.0)),
        Side::You => Transition::identity(),
    };
    let peel = Transition::opacity()
        .combined(Transition::scale(0.95))
        .combined(Transition::offset(0.0, -8.0));
    row.motion(Spring::new(0.34, 0.0))
        .transition(Transition::asymmetric(enter, peel).springs(SpringPair::new(
            Spring::new(0.34, 0.0),
            Spring::new(0.09, 0.0),
        )))
}

/// Their three typing dots, pulsing in turn at `now`, beside the typer's
/// face in a group.
pub(crate) fn typing_dots(
    p: Paint,
    key: String,
    now: f64,
    opacity: f32,
    reduced: bool,
    typer: Option<Sender<'_>>,
) -> El {
    let k = p.k;
    let dot = |i: usize| {
        let phase = (now * 1.7 - i as f64 * 0.22).rem_euclid(1.0);
        let lit = (1.0 - (phase * 2.0 - 0.5).abs() * 2.0).clamp(0.0, 1.0) as f32;
        el().key(format!("d{i}"))
            .size(6.0, 6.0)
            .radius(3.0)
            .fill(p.muted())
            .opacity(0.35 + 0.65 * lit)
            .opacity_motion(rune::Anim::INSTANT)
    };
    let dots = row()
        .key("dots")
        .h(27.0)
        .px(12.0)
        .gap(4.0)
        .center_y()
        .radius(13.0)
        .continuous_corners()
        .fill(if k.dark { k.c.layer_02 } else { k.c.layer_01 })
        .border(p.hair, k.c.border_weak)
        .child(dot(0))
        .child(dot(1))
        .child(dot(2));
    let line = line(key, Side::Them, opacity, reduced);
    match typer {
        Some(typer) => line.child(
            row()
                .key("sender")
                .gap(FACE_GAP)
                .items_end()
                .child(face_slot(p, typer))
                .child(dots),
        ),
        None => line.child(dots),
    }
}

/// A small red pill in a chat's header saying who is sharing their screen, its dot pulsing
/// like a recording light at `now`; `flare` (`0..=1`) swells it for a beat.
pub(crate) fn presenting(p: Paint, copy: &str, now: f64, flare: f32) -> El {
    let danger = &p.k.c.danger;
    let pulse = 0.5 + 0.5 * (now * std::f64::consts::TAU / 1.6).cos();
    row()
        .key("presenting")
        .scale(1.0 + 0.12 * flare)
        .shadow(0.0, 14.0 * flare, danger.fg.alpha(0.45 * flare))
        .h(18.0)
        .px(7.0)
        .gap(5.0)
        .center_y()
        .radius(9.0)
        .continuous_corners()
        .fill(danger.bg)
        .border(p.hair, danger.border)
        .child(
            el().key("dot")
                .size(6.0, 6.0)
                .radius(3.0)
                .fill(danger.fg)
                .opacity(0.45 + 0.55 * pulse as f32)
                .opacity_motion(rune::Anim::INSTANT),
        )
        .child(
            text(copy.to_string())
                .key("copy")
                .font(p.k.ty.small_medium)
                .color(danger.fg),
        )
}

/// Tapback badges, after Messages: the badge's width and the ring of card
/// cut around it; how far its middle sits out past the bubble's side and up
/// over its top; and how far each older one peeks out behind the newest.
const TAP: f32 = 24.0;
const TAP_RING: f32 = 2.0;
const TAP_OUT: f32 = 1.0;
const TAP_UP: f32 = 6.0;
const TAP_STEP: f32 = 11.0;
/// One tapback's canvas, square, and where the badge's middle sits in it,
/// leaving room for the tail dots trailing out and down.
pub(crate) const TAP_BOX: f32 = 36.0;
const TAP_MID: f32 = 16.0;
/// The room a line keeps above its bubble once a tapback lands on it.
pub(crate) const TAP_ROOM: f32 = TAP_UP + TAP / 2.0 + TAP_RING - 6.0;

/// Tapbacks on a bubble's top corner, as Messages puts them: left on yours,
/// right on theirs. The newest sits on the corner with the tail; older ones
/// slide out behind it. Each pops in on a spring; under Reduce Motion it
/// only fades.
fn tapbacks(p: Paint, side: Side, taps: &[Glyph], reduced: bool) -> Vec<El> {
    let newest = taps.len().saturating_sub(1);
    taps.iter()
        .enumerate()
        .map(|(i, &glyph)| {
            let behind = (newest - i) as f32;
            let out = TAP_MID - TAP_BOX - TAP_OUT - TAP_STEP * behind;
            let top = -TAP_UP - TAP_MID;
            let badge = canvas(tapback(p, glyph, side, false, i == newest))
                .key(format!("tap{i}"))
                .size(TAP_BOX, TAP_BOX);
            let badge = match side {
                Side::You => badge.pin(Some(out), Some(top), None, None),
                Side::Them => badge.pin(None, Some(top), Some(out), None),
            };
            if reduced {
                return badge.transition(Transition::opacity());
            }
            badge.motion(Spring::new(0.3, 0.0)).transition(
                Transition::opacity()
                    .combined(Transition::scale(0.2))
                    .springs(SpringPair::new(
                        Spring::new(0.36, 0.42),
                        Spring::new(0.12, 0.0),
                    )),
            )
        })
        .collect()
}

/// One tapback on a [`TAP_BOX`] canvas: a round badge ringed in the card's
/// colour with `glyph` on it, and with `tail`, two dots trailing out and
/// down, away from `side`'s bubble. Other people's are grey; the reader's
/// `own` are blue. Every glyph is drawn, never set in a font.
pub(crate) fn tapback(p: Paint, glyph: Glyph, side: Side, own: bool, tail: bool) -> Shape {
    let k = p.k;
    let (card, _) = material(p);
    let (fill, ink) = if own {
        (rune_theme_opencode::scale("cobalt", k.dark, 9), k.c.white)
    } else {
        (k.c.layer_04, p.muted())
    };
    let heart = if own { ink } else { k.c.danger.fg };
    // The tail points out, away from the bubble: left of yours.
    let mid = match side {
        Side::Them => [TAP_MID, TAP_MID],
        Side::You => [TAP_BOX - TAP_MID, TAP_MID],
    };
    let out = if side == Side::You { -1.0 } else { 1.0 };
    let r = TAP / 2.0;
    let mut discs = vec![(mid, r)];
    if tail {
        discs.push(([mid[0] + out * 0.67 * r, mid[1] + 0.67 * r], 0.2 * r));
        discs.push(([mid[0] + out * 1.06 * r, mid[1] + 1.06 * r], 0.12 * r));
    }
    let mut shape = Shape::new();
    for &(at, radius) in &discs {
        shape = shape.fill(disc(at, radius + TAP_RING), card);
    }
    for &(at, radius) in &discs {
        shape = shape.fill(disc(at, radius), fill);
    }
    glyph_on(shape, glyph, mid, ink, heart, fill)
}

/// A circle of `radius` around `at`, as a polygon.
fn disc(at: [f32; 2], radius: f32) -> Vec<[f32; 2]> {
    (0..40)
        .map(|i| {
            let a = i as f32 / 40.0 * std::f32::consts::TAU;
            [at[0] + radius * a.cos(), at[1] + radius * a.sin()]
        })
        .collect()
}

/// `glyph` drawn over `shape` on a 16-point grid centred on `mid` and 14
/// points across, in `ink` (a heart in `heart`); `cut` is the badge's own
/// colour, for the gaps between a hand's fingers.
fn glyph_on(
    shape: Shape,
    glyph: Glyph,
    mid: [f32; 2],
    ink: Color,
    heart: Color,
    cut: Color,
) -> Shape {
    const SCALE: f32 = 14.0 / 16.0;
    let at = |[x, y]: [f32; 2]| [mid[0] + (x - 8.0) * SCALE, mid[1] + (y - 8.0) * SCALE];
    let path = |points: &[[f32; 2]]| {
        let mut path = Path::new().move_to(at(points[0]));
        for chunk in points[1..].chunks(3) {
            path = path.cubic_to(at(chunk[0]), at(chunk[1]), at(chunk[2]));
        }
        path
    };
    match glyph {
        Glyph::Heart => shape.fill_path(
            &path(&[
                [8.0, 14.4],
                [0.6, 9.7],
                [1.0, 2.0],
                [5.0, 2.0],
                [6.6, 2.0],
                [7.5, 3.0],
                [8.0, 4.3],
                [8.5, 3.0],
                [9.4, 2.0],
                [11.0, 2.0],
                [15.0, 2.0],
                [15.4, 9.7],
                [8.0, 14.4],
            ])
            .close(),
            heart,
        ),
        Glyph::ThumbsUp | Glyph::ThumbsDown => {
            let down = glyph == Glyph::ThumbsDown;
            let f = |[x, y]: [f32; 2]| {
                if down {
                    at([16.0 - x, 16.0 - y])
                } else {
                    at([x, y])
                }
            };
            let cuff = vec![f([1.4, 7.2]), f([4.4, 7.2]), f([4.4, 14.8]), f([1.4, 14.8])];
            let hand = Path::new()
                .move_to(f([5.4, 7.4]))
                .line_to(f([7.8, 2.4]))
                .quad_to(f([8.5, 0.9]), f([9.9, 1.5]))
                .quad_to(f([11.2, 2.2]), f([10.7, 4.1]))
                .line_to(f([10.2, 6.2]))
                .line_to(f([13.4, 6.2]))
                .quad_to(f([15.3, 6.3]), f([14.9, 8.3]))
                .line_to(f([14.0, 13.5]))
                .quad_to(f([13.6, 15.0]), f([12.0, 15.0]))
                .line_to(f([5.4, 15.0]))
                .close();
            // Fingers: three short cuts in from the hand's far edge.
            let knuckles = [8.9, 10.9, 12.8].map(|y| (f([12.0, y]), f([15.6, y])));
            let mut shape = shape.fill(cuff, ink).fill_path(&hand, ink);
            for (a, b) in knuckles {
                shape = shape.line(a, b, 0.8, LineCap::Round, cut);
            }
            shape
        }
        Glyph::Haha => {
            // Two lines of bold, rounded capitals, the second a little
            // smaller and stepped right, as Messages draws it.
            let mut shape = shape;
            for (x0, top, size) in [(2.9, 2.3, 1.0), (4.5, 9.2, 0.84)] {
                let p = |x: f32, y: f32| at([x0 + x * size, top + y * 5.0 * size]);
                let strokes = [
                    // H
                    [p(0.0, 0.0), p(0.0, 1.0)],
                    [p(3.8, 0.0), p(3.8, 1.0)],
                    [p(0.0, 0.5), p(3.8, 0.5)],
                    // A
                    [p(6.1, 1.0), p(8.25, 0.0)],
                    [p(8.25, 0.0), p(10.4, 1.0)],
                    [p(6.9, 0.66), p(9.6, 0.66)],
                ];
                for [a, b] in strokes {
                    shape = shape.line(a, b, 2.0 * size, LineCap::Round, ink);
                }
            }
            shape
        }
        Glyph::Bang => {
            // Two leaning teardrops over their dots.
            let mut shape = shape;
            for x in [5.6, 10.9] {
                let (r, top) = (1.95, 3.6);
                let tip = [x - 0.9, 10.0];
                let drop = Path::new()
                    .move_to(at(tip))
                    .cubic_to(at([tip[0] - 0.6, 8.2]), at([x - r, 6.0]), at([x - r, top]))
                    .cubic_to(
                        at([x - r, top - 1.1]),
                        at([x - 1.1, top - r]),
                        at([x, top - r]),
                    )
                    .cubic_to(
                        at([x + 1.1, top - r]),
                        at([x + r, top - 1.1]),
                        at([x + r, top]),
                    )
                    .cubic_to(at([x + r, 6.2]), at([tip[0] + 0.7, 8.2]), at(tip))
                    .close();
                shape = shape
                    .fill_path(&drop, ink)
                    .fill(disc(at([x - 1.3, 13.2]), 1.55 * SCALE), ink);
            }
            shape
        }
        Glyph::Question => shape
            .stroke_path(
                &path(&[
                    [4.6, 5.4],
                    [4.6, 1.6],
                    [11.6, 1.4],
                    [11.4, 5.2],
                    [11.3, 7.5],
                    [8.2, 7.6],
                    [8.1, 10.0],
                ]),
                2.3,
                LineCap::Round,
                ink,
            )
            .fill(disc(at([8.1, 13.4]), 1.6 * SCALE), ink),
    }
}

/// What a [`system_line`] is: someone cutting you off, a neutral notice,
/// or a celebration.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Notify {
    Danger,
    Status,
    Cheer,
}

/// A centred system notice, such as someone removing you, in a pill
/// tinted for its [`Notify`], with the weight of a message.
pub(crate) fn system_line(
    p: Paint,
    key: String,
    copy: &str,
    notify: Notify,
    opacity: f32,
    reduced: bool,
) -> El {
    let c = &p.k.c;
    let danger = match notify {
        Notify::Danger => &c.danger,
        Notify::Status => &c.info,
        Notify::Cheer => &c.success,
    };
    line(key, Side::Them, opacity, reduced)
        .justify_center()
        .child(
            row()
                .key("pill")
                .px(12.0)
                .py(5.0)
                .radius(13.0)
                .continuous_corners()
                .fill(danger.bg)
                .border(p.hair, danger.border)
                .child(
                    text(copy.to_string())
                        .key("system")
                        .font(p.k.ty.small_medium)
                        .color(danger.fg),
                ),
        )
}

pub(crate) fn material(p: Paint) -> (Color, Color) {
    let k = p.k;
    if k.dark {
        (k.c.surface_raised, k.c.border_weak)
    } else {
        (k.c.bg_base, k.c.border_weak)
    }
}

pub(crate) fn field_text_origin(card_bottom_left: [f32; 2], lines: usize) -> [f32; 2] {
    let [x, bottom] = card_bottom_left;
    [
        x + CARD_PAD + FIELD_PX,
        bottom - CARD_PAD - FIELD_PY - lines as f32 * FIELD_LINE,
    ]
}

fn app_icon(p: Paint, app: App) -> El {
    let dark = p.k.dark;
    let white = p.k.c.white;
    let squircle = el()
        .key("icon")
        .size(ICON, ICON)
        .radius(ICON * 0.225)
        .continuous_corners()
        .center();
    match app {
        App::Messages => {
            let top = rune_theme_opencode::scale("apple", dark, 9);
            let bot = rune_theme_opencode::scale("apple", dark, 10);
            squircle
                .fill(bot)
                .gradient(top, bot)
                .child(canvas(bubble_mark(white)).key("bubble").size(ICON, ICON))
        }
        App::Slack => squircle
            .fill(rune_theme_opencode::scale("lilac", true, 3))
            .child(
                text("#")
                    .key("mark")
                    .font(TextStyle::new(19.0, 750.0).lh(22.0))
                    .color(white),
            ),
    }
}

fn bubble_mark(color: Color) -> Shape {
    const K: f32 = 0.5523;
    let (cx, cy, rx, ry) = (16.0, 15.0, 10.2, 8.4);
    let oval = Path::new()
        .move_to([cx + rx, cy])
        .cubic_to(
            [cx + rx, cy + ry * K],
            [cx + rx * K, cy + ry],
            [cx, cy + ry],
        )
        .cubic_to(
            [cx - rx * K, cy + ry],
            [cx - rx, cy + ry * K],
            [cx - rx, cy],
        )
        .cubic_to(
            [cx - rx, cy - ry * K],
            [cx - rx * K, cy - ry],
            [cx, cy - ry],
        )
        .cubic_to(
            [cx + rx * K, cy - ry],
            [cx + rx, cy - ry * K],
            [cx + rx, cy],
        )
        .close();
    let tail = Path::new()
        .move_to([9.2, 19.3])
        .quad_to([9.4, 23.4], [6.4, 25.3])
        .quad_to([10.8, 25.1], [13.5, 22.2])
        .close();
    Shape::new()
        .view_box(ICON, ICON)
        .fill_path(&oval, color)
        .fill_path(&tail, color)
}

pub(crate) fn floating(card: El, now: Option<f64>, phase: f64) -> El {
    let Some(now) = now else {
        return card;
    };
    let turn = std::f64::consts::TAU * (now / DRIFT_SECS + phase);
    let y = -DRIFT * (0.5 - 0.5 * turn.cos()) as f32;
    card.offset(0.0, y).offset_motion(Spring::new(0.08, 0.0))
}

/// `mark` underlines autocorrect's byte range of a [`Reply::Lines`] draft.
fn field(
    p: Paint,
    n: &Notice,
    reply: Reply<'_>,
    shape: field_text::Field,
    mark: Option<(usize, usize)>,
) -> El {
    let k = p.k;
    let focus = match reply {
        Reply::Hearing { .. } => true,
        Reply::Lines { focus, .. } => focus,
        _ => false,
    };
    let body = match reply {
        Reply::Empty => placeholder(p, n.field).ml(PLACEHOLDER_X),
        Reply::Ask { keys, hint } => row()
            .key("ask")
            .gap(8.0)
            .center_y()
            .transition(Transition::opacity())
            .child(text("Hold").key("hold").font(k.ty.small).color(p.muted()))
            .child(key_tray(
                p,
                "keys",
                KeySize::Small,
                keys.iter()
                    .enumerate()
                    .map(|(i, cap)| keycap(p, i, cap, KeySize::Small, false)),
            ))
            .child(text(hint).key("hint").font(k.ty.small).color(p.muted())),
        Reply::Hearing { partial: "", .. } => placeholder(p, "Listening…"),
        Reply::Hearing { partial, .. } | Reply::Typing(partial) => {
            live_words(p, "words", partial, false, FIELD_FONT, FIELD_LINE).grow(1.0)
        }
        Reply::Sent(words) => live_words(p, "words", words, true, FIELD_FONT, FIELD_LINE).grow(1.0),
        Reply::Lines {
            text: typed,
            caret,
            lines,
            placeholder: empty,
            ..
        } => {
            let body = shape.typed(p, "typed", typed, caret, lines, mark);
            if empty {
                body.child(placeholder(p, n.field).absolute(PLACEHOLDER_X, 0.0))
            } else {
                body
            }
        }
        Reply::Locked(why) => text(why).key("locked").font(k.ty.small).color(p.faint()),
    };
    let trailing = match reply {
        Reply::Hearing { meter, .. } => Some(mic_level(p, "mic", 16.0, meter.0, meter.1, true)),
        Reply::Sent(_) => Some(allowed(p, "sent", "Sent").transition(Transition::opacity())),
        Reply::Lines { sent: true, .. } => Some(allowed(p, "sent", "Sent")),
        _ => None,
    };
    let drawn = matches!(reply, Reply::Lines { .. });
    if matches!(reply, Reply::Locked(_)) {
        // No field at all, only why, in the field's place.
        return row()
            .key("field")
            .h(2.0 * FIELD_PY + FIELD_LINE)
            .radius(k.radius.lg)
            .fill(Color::TRANSPARENT)
            .border(p.hair, Color::TRANSPARENT)
            .center()
            .instant_changes()
            .child(body);
    }
    let field = row()
        .key("field")
        .px(FIELD_PX)
        .py(FIELD_PY)
        .gap(10.0)
        .radius(k.radius.lg)
        .fill(if k.dark { k.c.bg_deep } else { k.c.layer_01 })
        .border(
            p.hair,
            if focus {
                k.c.border_focus
            } else {
                k.c.border_base
            },
        )
        .child(body)
        .children(trailing);
    if drawn {
        // A draft is wiped or locked on the spot, never cross-faded.
        field.items_end().instant_changes()
    } else {
        field.min_h(FIELD_H).center_y()
    }
}

/// The placeholder sits clear of the caret before it.
const PLACEHOLDER_X: f32 = 2.5;

fn placeholder(p: Paint, copy: &str) -> El {
    text(copy)
        .key("placeholder")
        .font(FIELD_FONT.lh(FIELD_LINE))
        .color(p.faint())
        .grow(1.0)
}
