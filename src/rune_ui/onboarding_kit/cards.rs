//! Example notifications to answer by voice: who wrote, what they said, and
//! a reply field that shows a hint, the words as they're heard, or the sent
//! reply. Only the field changes between states, so a card never jumps.

use rune::{El, Spring, TextStyle, Transition, col, el, row, text};

use super::super::controls::{KeySize, Paint, key_tray, keycap};
use super::super::lists::allowed;
use super::mic::mic_level;
use super::words::live_words;

pub(crate) const CARD_W: f32 = 340.0;
const FIELD_H: f32 = 36.0;
const FIELD_FONT: TextStyle = TextStyle::new(13.0, 440.0);
const FIELD_LINE: f32 = 18.0;
/// A floating card drifts this far and back, slowly enough to read as
/// resting, not as moving.
const DRIFT: f32 = 3.0;
const DRIFT_SECS: f64 = 5.0;

/// Someone's message, as the system notification shows it.
#[derive(Clone, Copy)]
pub(crate) struct Notice {
    pub(crate) key: &'static str,
    pub(crate) app: &'static str,
    pub(crate) from: &'static str,
    pub(crate) body: &'static str,
    /// The reply field's placeholder.
    pub(crate) field: &'static str,
}

pub(crate) const FRANK: Notice = Notice {
    key: "frank",
    app: "Messages",
    from: "Frank",
    body: "your phone is on silent right",
    field: "iMessage",
};
pub(crate) const JAY: Notice = Notice {
    key: "jay",
    app: "Messages",
    from: "Jay",
    body: "can we move our sync?",
    field: "iMessage",
};
pub(crate) const DAX: Notice = Notice {
    key: "dax",
    app: "#opencode",
    from: "Dax",
    body: "can you look at the effect PR before 5",
    field: "Reply in thread",
};
pub(crate) const ADAM: Notice = Notice {
    key: "adam",
    app: "#hex",
    from: "Adam",
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
}

/// A notification card. `example` marks one the app plays by itself.
pub(crate) fn notice(p: Paint, n: &Notice, reply: Reply<'_>, example: bool) -> El {
    let k = p.k;
    let avatar = el()
        .key("avatar")
        .size(32.0, 32.0)
        .radius(16.0)
        .fill(k.c.layer_03)
        .center()
        .child(
            text(&n.from[..1])
                .key("initial")
                .font(k.ty.small_medium)
                .color(p.muted()),
        );
    let head = row()
        .key("head")
        .gap(6.0)
        .center_y()
        .child(
            text(n.from)
                .key("from")
                .font(k.ty.ui_medium)
                .color(p.text()),
        )
        .child(el().key("space").grow(1.0))
        .child(
            text(if example {
                format!("{} · example", n.app)
            } else {
                format!("{} · now", n.app)
            })
            .key("app")
            .font(k.ty.small)
            .color(p.faint()),
        );
    let message = col().key("message").grow(1.0).gap(2.0).child(head).child(
        text(n.body)
            .key("body")
            .font(k.ty.ui)
            .color(p.muted())
            .wrap(),
    );
    let card = col()
        .key(n.key)
        .w(CARD_W)
        .padding(14.0)
        .gap(12.0)
        .radius(k.radius.xl)
        .continuous_corners()
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .child(row().key("top").gap(10.0).child(avatar).child(message))
        .child(field(p, n, reply));
    k.elevate(card, &k.elevation.floating)
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
    let hearing = matches!(reply, Reply::Hearing { .. });
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
    };
    let trailing = match reply {
        Reply::Hearing { meter, .. } => Some(mic_level(p, "mic", 16.0, meter.0, meter.1, true)),
        Reply::Sent(_) => Some(allowed(p, "sent", "Sent")),
        _ => None,
    };
    row()
        .key("field")
        .min_h(FIELD_H)
        .px(12.0)
        .py(6.0)
        .gap(10.0)
        .center_y()
        .radius(k.radius.lg)
        .fill(k.c.bg_base)
        .border(
            p.hair,
            if hearing {
                k.c.border_focus
            } else {
                k.c.border_base
            },
        )
        .child(body)
        .children(trailing.map(|t| t.transition(Transition::scale(0.9))))
}

fn placeholder(p: Paint, copy: &str) -> El {
    text(copy)
        .key("placeholder")
        .font(FIELD_FONT.lh(FIELD_LINE))
        .color(p.faint())
        .grow(1.0)
        .transition(Transition::opacity())
}
