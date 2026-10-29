//! Example macOS notifications to answer by voice, drawn strictly with
//! OpenCode theme tokens. Only the reply field changes between states, so the
//! card never jumps.

use rune::{Color, El, Path, Shape, Spring, TextStyle, Transition, canvas, col, el, row, text};

use super::super::controls::{KeySize, Paint, key_tray, keycap};
use super::super::lists::allowed;
use super::field_text;
use super::mic::mic_level;
use super::words::live_words;

/// Width of a centred macOS notification card on the onboarding stage.
pub(crate) const CARD_W: f32 = 388.0;
const CARD_PAD: f32 = 14.0;
const RADIUS: f32 = 16.0;
const ICON: f32 = 32.0;
const FIELD_H: f32 = 36.0;
const FIELD_PX: f32 = 12.0;
const FIELD_PY: f32 = 7.0;
pub(crate) const FIELD_FONT: TextStyle = TextStyle::new(13.0, 440.0);
pub(crate) const FIELD_LINE: f32 = 18.0;
/// Room beside the reply's last line for `✓ Sent`.
const SENT_W: f32 = 58.0;
/// Maximum line width inside a [`Reply::Lines`] field.
pub(crate) const REPLY_W: f32 = CARD_W - 2.0 * CARD_PAD - 2.0 * FIELD_PX - SENT_W;
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
    },
}

/// Height of [`notice`] when its reply field uses [`Reply::Lines`] with `lines`.
pub(crate) fn lines_card_height(lines: usize) -> f32 {
    // Top row: 12 (head) + 1 + 20 (title) + 1 + 20 (body) = 54, gap 10, field, 2 * CARD_PAD.
    2.0 * CARD_PAD + 54.0 + 10.0 + 2.0 * FIELD_PY + lines as f32 * FIELD_LINE
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
        .child(
            row()
                .key("top")
                .gap(10.0)
                .items_start()
                .child(app_icon(p, n.app))
                .child(message),
        )
        .child(field(p, n, reply));
    k.elevate(card, &k.elevation.floating)
}

fn material(p: Paint) -> (Color, Color) {
    let k = p.k;
    if k.dark {
        (k.c.surface_raised, k.c.border_weak)
    } else {
        (k.c.bg_base, k.c.border_weak)
    }
}

/// Top-left origin of [`Reply::Lines`] text given the card's bottom-left point.
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
                .child(canvas(bubble(white)).key("bubble").size(ICON, ICON))
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

fn bubble(color: Color) -> Shape {
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
                body.child(placeholder(p, n.field).absolute(4.0, 0.0))
            } else {
                body
            }
        }
    };
    let trailing = match reply {
        Reply::Hearing { meter, .. } => Some(mic_level(p, "mic", 16.0, meter.0, meter.1, true)),
        // Instant clearance when resetting between Without Hex and With Hex,
        // so `✓ Sent` never lingers over the fresh `iMessage` placeholder.
        Reply::Sent(_) => Some(allowed(p, "sent", "Sent").transition(Transition::opacity())),
        Reply::Lines { sent: true, .. } => Some(allowed(p, "sent", "Sent")),
        _ => None,
    };
    let drawn = matches!(reply, Reply::Lines { .. });
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
}
