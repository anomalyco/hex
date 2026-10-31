//! Example macOS notifications to answer by voice, and the Messages
//! group conversation `"Without Hex"` plays out in, drawn strictly with OpenCode
//! theme tokens.

use rune::{
    Color, El, Path, Shape, Spring, SpringPair, TextStyle, Transition, canvas, col, el, row, text,
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
        text: &'a str,
        caret: Option<usize>,
        lines: usize,
        placeholder: bool,
        focus: bool,
        sent: bool,
        /// Autocorrect's underline under this byte range of `text`.
        mark: Option<(usize, usize)>,
    },
    /// No field any more, only why.
    Locked(&'a str),
}

/// Height of [`notice`] when its reply field uses [`Reply::Lines`] with
/// `lines` wrapped lines.
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

    let (fill, rim) = material(p);
    let card = col()
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
        )
        .child(field(p, n, reply, field_text::CARD));
    k.elevate(card, &k.elevation.floating)
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

/// The banner's row over a thread, and the space under it.
const BANNER_H: f32 = 20.0;
const BANNER_GAP: f32 = 6.0;

/// A Messages conversation with `n.from` (a group when `n.place` names its
/// members): header, the newest of `thread`'s lines (older ones scroll up
/// under a soft edge), and the reply field.
/// `banner`, such as [`presenting`], keeps its own row, centred at the
/// thread's top.
pub(crate) fn chat(
    p: Paint,
    n: &Notice,
    banner: Option<El>,
    thread: Vec<El>,
    reply: Reply<'_>,
    field_opacity: f32,
) -> El {
    let k = p.k;
    let head = row()
        .key("head")
        .h(HEADER_H)
        .gap(10.0)
        .center_y()
        .child(app_icon(p, n.app))
        .child(
            col()
                .key("who")
                .grow(1.0)
                .child(
                    text(n.from)
                        .key("title")
                        .font(TextStyle {
                            weight: if k.dark { 560.0 } else { 600.0 },
                            ..k.ty.ui_medium
                        })
                        .color(p.text()),
                )
                .child(
                    text(n.place.unwrap_or(n.field))
                        .key("app")
                        .font(k.ty.small)
                        .color(p.faint()),
                ),
        );
    // A banner keeps its own row at the top, so lines fade out before
    // they'd pass under it.
    let lines = |h: f32| {
        col()
            .key("thread")
            .w_full()
            .h(h)
            .justify_end()
            .gap(6.0)
            .clip_fade(0.0, THREAD_FADE, 0.0, 0.0)
            .children(thread)
    };
    let thread = match banner {
        Some(banner) => col()
            .key("thread-wrap")
            .w_full()
            .h(THREAD_H)
            .gap(BANNER_GAP)
            .child(
                row()
                    .key("banner")
                    .h(BANNER_H)
                    .justify_center()
                    .center_y()
                    .child(banner),
            )
            .child(lines(THREAD_H - BANNER_H - BANNER_GAP)),
        None => lines(THREAD_H),
    };
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
            field(p, n, reply, field_text::CHAT)
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
    /// The sender's name over the first of theirs in a group, in their
    /// colour: an OpenCode theme scale such as `"cobalt"`.
    From(&'a str, &'a str),
    /// `"Read"` under one of yours, moving with it.
    Read,
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
        Caption::From(from, colour) => line.child(
            col()
                .key("named")
                .gap(2.0)
                .shrink()
                .child(
                    text(from.to_string())
                        .key("from")
                        .font(k.ty.small_medium)
                        .color(rune_theme_opencode::scale(colour, k.dark, 11))
                        .ml(11.0),
                )
                .child(bubble.shrink()),
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

/// Their three typing dots, pulsing in turn at `now`.
pub(crate) fn typing_dots(p: Paint, key: String, now: f64, opacity: f32, reduced: bool) -> El {
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
    line(key, Side::Them, opacity, reduced).child(
        row()
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
            .child(dot(2)),
    )
}

/// A small red pill saying who is sharing their screen, its dot pulsing
/// like a recording light at `now`; `flare` (`0..=1`) swells it for a beat.
pub(crate) fn presenting(p: Paint, copy: &str, now: f64, flare: f32) -> El {
    let danger = &p.k.c.danger;
    let pulse = 0.5 + 0.5 * (now * std::f64::consts::TAU / 1.6).cos();
    row()
        .key("presenting")
        .scale(1.0 + 0.12 * flare)
        .shadow(0.0, 14.0 * flare, danger.fg.alpha(0.45 * flare))
        .h(20.0)
        .px(8.0)
        .gap(5.0)
        .center_y()
        .radius(10.0)
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

/// Tapback badges: how far one sits out past the bubble's corner, its size,
/// and how far each newer one steps out over the last.
const TAP: f32 = 24.0;
const TAP_OUT: f32 = 13.0;
const TAP_STEP: f32 = 14.0;
/// The room a line keeps above its bubble once a tapback lands on it.
pub(crate) const TAP_ROOM: f32 = 11.0;

/// iMessage tapbacks on a bubble's top corner (left on yours, right on
/// theirs), each a round badge cut out of the card that pops in on a spring,
/// newer ones stacked over and past the last.
fn tapbacks(p: Paint, side: Side, taps: &[Glyph], reduced: bool) -> Vec<El> {
    let k = p.k;
    let (card, _) = material(p);
    taps.iter()
        .enumerate()
        .map(|(i, &glyph)| {
            let out = -TAP_OUT - TAP_STEP * i as f32;
            let badge = row()
                .key(format!("tap{i}"))
                .size(TAP, TAP)
                .center()
                .radius(TAP * 0.5)
                .fill(if k.dark { k.c.layer_04 } else { k.c.layer_03 })
                .border(2.0, card)
                .child(tap_glyph(p, glyph));
            let badge = match side {
                Side::You => badge.pin(Some(out), Some(-TAP_OUT), None, None),
                Side::Them => badge.pin(None, Some(-TAP_OUT), Some(out), None),
            };
            let pop = if reduced {
                Transition::opacity()
            } else {
                Transition::opacity()
                    .combined(Transition::scale(0.2))
                    .springs(SpringPair::new(
                        Spring::new(0.36, 0.42),
                        Spring::new(0.12, 0.0),
                    ))
            };
            badge.transition(pop)
        })
        .collect()
}

/// A tapback's mark: plain text or a vector, never an emoji font.
fn tap_glyph(p: Paint, glyph: Glyph) -> El {
    let k = p.k;
    let red = k.c.danger.fg;
    let word = |copy: &str, size: f32, color: Color| {
        text(copy.to_string())
            .key("glyph")
            .font(TextStyle::new(size, 800.0).lh(size + 1.0))
            .color(color)
    };
    let drawn = |shape: Shape| canvas(shape).key("glyph").size(13.0, 13.0);
    match glyph {
        Glyph::Haha => word("Haha", 7.5, p.text()),
        Glyph::Bang => word("!!", 12.0, red),
        Glyph::Question => word("?", 13.0, p.text()),
        Glyph::ThumbsUp => drawn(thumb(p.text(), false)),
        Glyph::ThumbsDown => drawn(thumb(p.text(), true)),
        Glyph::Heart => drawn(heart(red)),
    }
}

/// A thumbs-up on a 16-point grid, or turned over for a thumbs-down.
fn thumb(color: Color, down: bool) -> Shape {
    let f = |[x, y]: [f32; 2]| if down { [16.0 - x, 16.0 - y] } else { [x, y] };
    let cuff = Path::new()
        .move_to(f([1.8, 7.4]))
        .line_to(f([4.4, 7.4]))
        .line_to(f([4.4, 15.0]))
        .line_to(f([1.8, 15.0]))
        .close();
    let hand = Path::new()
        .move_to(f([5.5, 7.6]))
        .line_to(f([8.0, 2.5]))
        .quad_to(f([8.6, 1.3]), f([9.7, 1.8]))
        .quad_to(f([10.8, 2.4]), f([10.4, 4.1]))
        .line_to(f([9.9, 6.5]))
        .line_to(f([13.1, 6.5]))
        .quad_to(f([14.7, 6.6]), f([14.4, 8.3]))
        .line_to(f([13.5, 13.6]))
        .quad_to(f([13.2, 15.0]), f([11.8, 15.0]))
        .line_to(f([5.5, 15.0]))
        .close();
    Shape::new()
        .view_box(16.0, 16.0)
        .fill_path(&cuff, color)
        .fill_path(&hand, color)
}

fn heart(color: Color) -> Shape {
    let path = Path::new()
        .move_to([8.0, 14.2])
        .cubic_to([1.0, 9.6], [1.4, 2.6], [5.2, 2.6])
        .cubic_to([6.8, 2.6], [7.6, 3.7], [8.0, 4.8])
        .cubic_to([8.4, 3.7], [9.2, 2.6], [10.8, 2.6])
        .cubic_to([14.6, 2.6], [15.0, 9.6], [8.0, 14.2])
        .close();
    Shape::new().view_box(16.0, 16.0).fill_path(&path, color)
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

fn material(p: Paint) -> (Color, Color) {
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

fn field(p: Paint, n: &Notice, reply: Reply<'_>, shape: field_text::Field) -> El {
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
            mark,
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
