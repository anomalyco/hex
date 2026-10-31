//! The agent card's copy, cycling through examples of what to ask an agent
//! for, each written the way it asks: "all lowercase" in lowercase, "ALL
//! CAPS" in caps. Each change is a letter-by-letter edit: the request
//! changes first, then the title, a wave running left to right. Only the
//! letters that change move, each arriving in the accent colour and
//! cooling to ink, while its neighbours slide to make room. The window's
//! poll steps the wave from the clock, and Rune's springs carry each
//! letter's entrance, exit and slide. Reduce Motion holds the first example.

use rune::{Color, Edge, El, Spring, SpringPair, TextStyle, Transition, col, row, text};

use super::super::controls::{DESCRIPTION, Paint};
use super::super::spark;
use crate::desktop_i18n::tr;
use crate::personal_commands::apply_builtin_transformation;

/// Seconds each example shows: one flip of the spark.
const SLOT: f64 = spark::PERIOD as f64;
const EXAMPLES: usize = 4;
/// When in its slot an example starts changing into the next, so the
/// title's wave crosses its middle as the spark turns edge-on.
const CHANGE: f64 = spark::EDGE_ON as f64 - 0.62;
/// Seconds between one request letter changing and the next.
const REQUEST_STEP: f64 = 0.03;
/// When the title's wave starts, after the request's.
const TITLE_DELAY: f64 = 0.28;
/// Seconds between one title letter changing and the next.
const TITLE_STEP: f64 = 0.018;
/// How long a changed letter keeps the accent colour.
const TINT: f64 = 0.32;
/// How often the poll steps the wave while it runs.
const FRAME: f64 = 1.0 / 60.0;

/// How a changing letter leaves and arrives. One ships; the others are
/// here to compare.
#[derive(Clone, Copy, PartialEq)]
enum Style {
    /// An odometer: the old letter rolls up and out, the new one rolls in
    /// from below, inside a soft-edged window.
    Roll,
    /// The old letter blurs out in place as the new one sharpens in.
    Blur,
    /// The new letter pops in from small with a little overshoot.
    Pop,
}

impl Style {
    fn current() -> Self {
        match std::env::var("HEX_OUTPUT_MORPH").as_deref() {
            Ok("blur") => Self::Blur,
            Ok("pop") => Self::Pop,
            _ => Self::Roll,
        }
    }

    fn transition(self) -> Transition {
        match self {
            Self::Roll => Transition::push(Edge::Bottom)
                .springs(SpringPair::new(Spring::new(0.34, 0.12), Spring::new(0.26, 0.0))),
            Self::Blur => Transition::blur(3.0)
                .combined(Transition::scale(0.92))
                .springs(SpringPair::new(Spring::new(0.36, 0.0), Spring::new(0.28, 0.0))),
            Self::Pop => Transition::asymmetric(
                Transition::scale(0.55).allow_small_scale(),
                Transition::scale(1.1).combined(Transition::blur(2.0)),
            )
            .springs(SpringPair::new(Spring::new(0.32, 0.28), Spring::new(0.2, 0.0))),
        }
    }
}

/// Where the cycle stands: which example shows, and how far its change
/// into the next has run, counted in letters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in super::super) struct Moment {
    example: usize,
    request: Wave,
    title: Wave,
}

/// How many letters of a line have changed, and how many have cooled from
/// the accent colour since.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Wave {
    changed: usize,
    cooled: usize,
}

impl Wave {
    fn at(elapsed: f64, start: f64, step: f64) -> Self {
        let count = |at: f64| {
            if at < 0.0 {
                0
            } else {
                (at / step).floor() as usize + 1
            }
        };
        Self {
            changed: count(elapsed - start),
            cooled: count(elapsed - start - TINT),
        }
    }

    /// A letter's state: whether it has changed yet, and whether it still
    /// shows the accent colour.
    fn letter(self, index: usize) -> (bool, bool) {
        (index < self.changed, index < self.changed && index >= self.cooled)
    }
}

impl Moment {
    /// The moment `clock` seconds into the window's life.
    fn at(clock: f64) -> Self {
        let slot = (clock / SLOT).floor().max(0.0);
        let elapsed = clock - slot * SLOT - CHANGE;
        let example = slot as usize % EXAMPLES;
        if elapsed < 0.0 {
            return Self {
                example,
                ..Self::default()
            };
        }
        Self {
            example,
            request: Wave::at(elapsed, 0.0, REQUEST_STEP),
            title: Wave::at(elapsed, TITLE_DELAY, TITLE_STEP),
        }
    }
}

/// Steps the cycle to `clock`, asking for the next frame while a change
/// runs and for the next change otherwise. True when the copy changed.
pub(in super::super) fn poll(moment: &mut Moment, clock: f64) -> (bool, f64) {
    let next = Moment::at(clock);
    let changed = next != *moment;
    *moment = next;
    let into = clock.rem_euclid(SLOT) - CHANGE;
    let wait = if (0.0..1.6).contains(&into) {
        FRAME
    } else if into < 0.0 {
        -into
    } else {
        SLOT - into
    };
    (changed, wait)
}

/// One example: the title and the request, both written the way it asks.
struct Example {
    title: String,
    request: String,
}

fn examples() -> [Example; EXAMPLES] {
    let title = tr("Ask an agent to change these steps");
    let written = |request: &str, style: fn(&str) -> String| Example {
        title: style(title),
        request: style(request),
    };
    [
        written(tr("Title Case"), title_case),
        written(tr("all lowercase"), str::to_lowercase),
        written(tr("all caps"), str::to_uppercase),
        written(tr("SpongeBob case"), |text| {
            apply_builtin_transformation("spongebob-case", text).unwrap_or_else(|| text.to_string())
        }),
    ]
}

/// Every word starting with a capital, the rest as written.
fn title_case(text: &str) -> String {
    let mut start = true;
    text.chars()
        .flat_map(|c| {
            let upper = start && c.is_alphabetic();
            start = c.is_whitespace();
            if upper {
                c.to_uppercase().collect::<Vec<_>>()
            } else {
                c.to_lowercase().collect()
            }
        })
        .collect()
}

/// The card's title and request at `moment`; Reduce Motion holds the first
/// example.
pub(super) fn copy(p: Paint, moment: Moment, reduced: bool) -> El {
    let moment = if reduced { Moment::default() } else { moment };
    let examples = examples();
    let from = &examples[moment.example];
    let to = &examples[(moment.example + 1) % EXAMPLES];
    let style = Style::current();
    let request_font = TextStyle {
        size: DESCRIPTION.size,
        line_height: DESCRIPTION.line_height,
        ..p.k.ty.mono
    };
    let title = line(
        "example-title",
        LineStyle {
            font: p.k.ty.ui_medium,
            ink: p.text(),
            accent: p.accent(),
            space: p.k.ty.ui_medium.size * 0.27,
            style,
        },
        &from.title,
        &to.title,
        moment.title,
    )
    .flex_wrap();
    let request = row()
        .key("example-request")
        .child(
            text("› ")
                .key("example-prompt")
                .font(request_font)
                .color(p.muted()),
        )
        .child(line(
            "example-request-line",
            LineStyle {
                font: request_font,
                ink: p.muted(),
                accent: p.accent(),
                space: request_font.size * 0.6,
                style,
            },
            &from.request,
            &to.request,
            moment.request,
        ));
    col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .gap(3.0)
        .child(title)
        .child(request)
}

#[derive(Clone, Copy)]
struct LineStyle {
    font: TextStyle,
    ink: Color,
    accent: Color,
    /// The gap between words.
    space: f32,
    style: Style,
}

/// `from` part way to `to`: the first `wave.changed` letters written as in
/// `to`, the rest as in `from`, word by word. A letter that changes gets a
/// new key, so the old one leaves and the new one enters.
fn line(key: &str, s: LineStyle, from: &str, to: &str, wave: Wave) -> El {
    let (from, to): (Vec<&str>, Vec<&str>) = (
        from.split_whitespace().collect(),
        to.split_whitespace().collect(),
    );
    let mut index = 0;
    let mut words = Vec::new();
    for w in 0..from.len().max(to.len()) {
        let old: Vec<char> = from.get(w).map_or_else(Vec::new, |w| w.chars().collect());
        let new: Vec<char> = to.get(w).map_or_else(Vec::new, |w| w.chars().collect());
        let mut word = row().key(format!("{key}-{w}"));
        for c in 0..old.len().max(new.len()) {
            let (changed, tinted) = wave.letter(index);
            index += 1;
            let Some(shown) = (if changed { new.get(c) } else { old.get(c) }) else {
                continue;
            };
            let moved = changed && old.get(c) != new.get(c);
            word = word.child(
                text(shown.to_string())
                    .key(format!("{key}-{w}-{c}-{shown}"))
                    .font(s.font)
                    .color(if tinted && moved { s.accent } else { s.ink })
                    .transition(s.style.transition()),
            );
        }
        words.push(if s.style == Style::Roll {
            word.clip_fade(0.0, 3.0, 0.0, 3.0)
        } else {
            word
        });
    }
    row().key(key.to_string()).gaps(0.0, s.space).children(words)
}

/// The copy after Edit with agent: no examples, just what to do next.
pub(super) fn copied(p: Paint) -> El {
    col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .gap(3.0)
        .child(
            text(tr("Ask an agent to change these steps"))
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
        .child(
            text(tr("Copied. Paste it into your agent and say what you want"))
                .key("detail")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A change runs the request's wave, then the title's, and ends
    /// holding the next example exactly as the next slot starts it.
    #[test]
    fn a_change_runs_the_request_then_the_title() {
        let rest = Moment::at(1.0);
        assert_eq!(rest, Moment::default());
        let early = Moment::at(CHANGE + 0.1);
        assert!(early.request.changed > 0 && early.title.changed == 0);
        let late = Moment::at(CHANGE + 1.5);
        assert!(late.title.changed >= 34 && late.title.cooled >= 34);
        assert_eq!(Moment::at(SLOT + 0.1).example, 1);
    }

    #[test]
    fn title_case_capitalizes_each_word() {
        assert_eq!(title_case("ask an AGENT"), "Ask An Agent");
    }
}
