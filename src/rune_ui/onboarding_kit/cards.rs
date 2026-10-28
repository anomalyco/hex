//! Example notifications to answer by voice: who wrote, what they said, and
//! a reply field that shows a hint, the words as they're heard, or the sent
//! reply. Only the field changes between states, so a card never jumps.

use rune::{Color, El, Path, Shape, Spring, TextStyle, Transition, canvas, col, el, row, text};

use super::super::controls::{KeySize, Paint, key_tray, keycap};
use super::super::lists::allowed;
use super::field_text;
use super::flight::srgb;
use super::mic::mic_level;
use super::words::live_words;

/// macOS notifications are about this wide.
pub(crate) const CARD_W: f32 = 360.0;
const CARD_PAD: f32 = 14.0;
/// The notification material's corners.
const RADIUS: f32 = 18.0;
const ICON: f32 = 32.0;
/// How far each card behind the top one peeks out.
pub(crate) const PEEK: f32 = 10.0;
const FIELD_H: f32 = 36.0;
const FIELD_PX: f32 = 12.0;
const FIELD_PY: f32 = 6.0;
pub(crate) const FIELD_FONT: TextStyle = TextStyle::new(13.0, 440.0);
pub(crate) const FIELD_LINE: f32 = 18.0;
/// Room beside the reply's last line for Sent.
const SENT_W: f32 = 56.0;
/// How wide a reply laid out by hand may run.
pub(crate) const REPLY_W: f32 = CARD_W - 2.0 * CARD_PAD - 2.0 * FIELD_PX - SENT_W;
/// A floating card drifts this far and back, slowly enough to read as
/// resting, not as moving.
const DRIFT: f32 = 3.0;
const DRIFT_SECS: f64 = 5.0;

/// The app a notification comes from, drawn as its icon.
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

/// Someone's message, as the system notification shows it.
#[derive(Clone, Copy)]
pub(crate) struct Notice {
    pub(crate) key: &'static str,
    pub(crate) app: App,
    pub(crate) from: &'static str,
    /// The channel it was posted in, for a team chat.
    pub(crate) place: Option<&'static str>,
    pub(crate) body: &'static str,
    /// The reply field's placeholder.
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
    from: "Jay",
    place: None,
    body: "can we move our sync?",
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

/// What a card's reply field shows.
#[derive(Clone, Copy)]
pub(crate) enum Reply<'a> {
    Empty,
    /// The shortcut's keys and what holding them does.
    Ask {
        keys: &'a [String],
        hint: &'a str,
    },
    /// Words as they're heard, with the microphone's meter beside them.
    Hearing {
        partial: &'a str,
        meter: (f32, f32),
    },
    /// An example typing itself, with no microphone.
    Typing(&'a str),
    Sent(&'a str),
    /// Text laid out by hand on `lines` lines (see [`super::field_text`]):
    /// typed, with a caret while `caret` is set, or empty under words the
    /// caller draws over the field (see [`field_text_origin`]). The
    /// placeholder shows while `placeholder`.
    Lines {
        text: &'a str,
        caret: Option<usize>,
        lines: usize,
        placeholder: bool,
        focus: bool,
        sent: bool,
    },
}

/// A notification card as macOS draws one: the app's icon, its name and
/// the time, who wrote and what they said, on the notification material,
/// with a reply field. `example` marks one the app plays by itself.
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
                    weight: 600.0,
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
        .child(
            row()
                .key("top")
                .gap(10.0)
                .items_start()
                .child(app_icon(n.app))
                .child(message),
        )
        .child(field(p, n, reply));
    k.elevate(card, &k.elevation.floating)
}

/// The notification material where Rune can't draw the system's: a
/// translucent raised layer with a light rim, over a backdrop blur.
fn material(p: Paint) -> (Color, Color) {
    let k = p.k;
    if k.dark {
        (k.alpha(k.c.layer_02, 0.86), k.light(0.1))
    } else {
        (k.alpha(k.c.white, 0.82), k.dark_a(0.08))
    }
}

/// Notification Center's stack: `card` on top, `behind` more peeking out
/// under it, each a little narrower and dimmer.
pub(crate) fn stacked(p: Paint, key: &str, card: El, behind: usize) -> El {
    let k = p.k;
    let (fill, rim) = material(p);
    let mut stack = col().key(key).pb(PEEK * behind as f32);
    for depth in (1..=behind).rev() {
        let inset = 10.0 * depth as f32;
        let layer = el()
            .key(format!("behind{depth}"))
            .pin(
                Some(inset),
                Some(PEEK * depth as f32),
                Some(inset),
                Some(PEEK * (behind - depth) as f32),
            )
            .radius(RADIUS)
            .continuous_corners()
            .fill(fill)
            .border(p.hair, rim)
            .opacity(1.0 - 0.25 * depth as f32);
        stack = stack.child(k.elevate(layer, &k.elevation.raised));
    }
    stack.child(card)
}

/// Where a [`Reply::Lines`] field's text starts, from the bottom left of
/// the card it's in.
pub(crate) fn field_text_origin(card_bottom_left: [f32; 2], lines: usize) -> [f32; 2] {
    let [x, bottom] = card_bottom_left;
    [
        x + CARD_PAD + FIELD_PX,
        bottom - CARD_PAD - FIELD_PY - lines as f32 * FIELD_LINE,
    ]
}

/// The app's icon, drawn: Messages' white bubble on green, Slack's mark on
/// aubergine.
fn app_icon(app: App) -> El {
    let squircle = el()
        .key("icon")
        .size(ICON, ICON)
        .radius(ICON * 0.225)
        .continuous_corners()
        .center();
    match app {
        App::Messages => squircle
            .fill(srgb(0.2, 0.85, 0.36, 1.0))
            .gradient(srgb(0.4, 0.94, 0.49, 1.0), srgb(0.05, 0.76, 0.23, 1.0))
            .child(canvas(bubble()).key("bubble").size(ICON, ICON)),
        App::Slack => squircle.fill(srgb(0.29, 0.08, 0.29, 1.0)).child(
            text("#")
                .key("mark")
                .font(TextStyle::new(20.0, 800.0).lh(22.0))
                .color(Color::WHITE),
        ),
    }
}

/// Messages' speech bubble in a 32 × 32 box.
fn bubble() -> Shape {
    // An ellipse from four quarter arcs, with a tail at its lower left.
    const K: f32 = 0.5523;
    let (cx, cy, rx, ry) = (16.0, 15.0, 10.5, 8.6);
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
        .move_to([9.0, 19.5])
        .quad_to([9.2, 23.6], [6.2, 25.6])
        .quad_to([10.8, 25.4], [13.6, 22.4])
        .close();
    Shape::new()
        .view_box(ICON, ICON)
        .fill_path(&oval, Color::WHITE)
        .fill_path(&tail, Color::WHITE)
}

/// [`notice`] resting on the page, drifting a few points up and back on
/// the clock `now` (`None` under Reduce Motion, holding it still). `phase`
/// is a fraction of the drift's period, so cards beside each other don't
/// move together.
pub(crate) fn floating(card: El, now: Option<f64>, phase: f64) -> El {
    let Some(now) = now else {
        return card;
    };
    let turn = std::f64::consts::TAU * (now / DRIFT_SECS + phase);
    let y = -DRIFT * (0.5 - 0.5 * turn.cos()) as f32;
    card.offset(0.0, y).offset_motion(Spring::new(0.08, 0.0))
}

fn field(p: Paint, n: &Notice, reply: Reply<'_>) -> El {
    let k = p.k;
    let focus = match reply {
        Reply::Hearing { .. } => true,
        Reply::Lines { focus, .. } => focus,
        _ => false,
    };
    let body = match reply {
        Reply::Empty => placeholder(p, n.field),
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
            let body = field_text::typed(p, "typed", typed, caret, lines);
            if empty {
                // Just past the caret, so it doesn't cover the first letter.
                body.child(placeholder(p, n.field).absolute(3.0, 0.0))
            } else {
                body
            }
        }
    };
    let trailing = match reply {
        Reply::Hearing { meter, .. } => Some(mic_level(p, "mic", 16.0, meter.0, meter.1, true)),
        Reply::Sent(_) | Reply::Lines { sent: true, .. } => Some(allowed(p, "sent", "Sent")),
        _ => None,
    };
    let drawn = matches!(reply, Reply::Lines { .. });
    let field = row()
        .key("field")
        .px(FIELD_PX)
        .py(FIELD_PY)
        .gap(10.0)
        .radius(k.radius.lg)
        .fill(k.c.bg_base)
        .border(
            p.hair,
            if focus {
                k.c.border_focus
            } else {
                k.c.border_base
            },
        )
        .child(body)
        .children(trailing.map(|t| t.transition(Transition::scale(0.9))));
    // Text laid out by hand leaves room for Sent beside its last line.
    if drawn {
        field.items_end()
    } else {
        field.min_h(FIELD_H).center_y()
    }
}

fn placeholder(p: Paint, copy: &str) -> El {
    text(copy)
        .key("placeholder")
        .font(FIELD_FONT.lh(FIELD_LINE))
        .color(p.faint())
        .grow(1.0)
        .transition(Transition::opacity())
}
