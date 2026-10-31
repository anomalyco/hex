//! The agent card's copy: its title over a prompt to an agent, cycling
//! through examples in a shuffled order. A case is written the way it
//! asks, title included: "all lowercase" in lowercase, "ALL CAPS" in caps.
//! Each change is a letter-by-letter edit: the prompt changes first, then
//! the title, a wave running left to right. Only the letters that change
//! move, each popping in in the accent colour and cooling to ink, while
//! its neighbours slide to make room. The window's poll steps the wave
//! from the clock, and Rune's springs carry each letter's entrance, exit
//! and slide. Reduce Motion holds the first example.

use rune::{Color, El, Spring, SpringPair, TextStyle, Transition, col, row, text};

use super::super::controls::{DESCRIPTION, Paint};
use super::super::spark;
use crate::desktop_i18n::tr;
use crate::personal_commands::apply_builtin_transformation;

/// Seconds each example shows: one flip of the spark.
const SLOT: f64 = spark::PERIOD as f64;
/// When in its slot an example starts changing into the next, so the
/// title's wave crosses its middle as the spark turns edge-on.
const CHANGE: f64 = spark::EDGE_ON as f64 - 0.62;
/// Seconds between one request letter changing and the next.
const REQUEST_STEP: f64 = 0.022;
/// When the title's wave starts, after the request's.
const TITLE_DELAY: f64 = 0.28;
/// Seconds between one title letter changing and the next.
const TITLE_STEP: f64 = 0.018;
/// How long a changed letter keeps the accent colour.
const TINT: f64 = 0.32;
/// How often the poll steps the wave while it runs.
const FRAME: f64 = 1.0 / 60.0;

/// How a changing letter arrives and leaves: it pops in from small with a
/// little overshoot, and the old one swells and blurs away.
fn pop() -> Transition {
    Transition::asymmetric(
        Transition::scale(0.55).allow_small_scale(),
        Transition::scale(1.1).combined(Transition::blur(2.0)),
    )
    .springs(SpringPair::new(
        Spring::new(0.32, 0.28),
        Spring::new(0.2, 0.0),
    ))
}

/// The order the examples show in: shuffled for each window, in order for
/// fixtures.
#[derive(Clone, Debug, PartialEq)]
pub(in super::super) struct Order(Vec<usize>);

impl Default for Order {
    fn default() -> Self {
        Self((0..EXAMPLES).collect())
    }
}

impl Order {
    pub(in super::super) fn shuffled() -> Self {
        let mut seed = [0; 8];
        if getrandom::fill(&mut seed).is_err() {
            return Self::default();
        }
        let mut state = u64::from_le_bytes(seed) | 1;
        let mut order: Vec<usize> = (0..EXAMPLES).collect();
        for i in (1..EXAMPLES).rev() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            order.swap(i, (state % (i as u64 + 1)) as usize);
        }
        Self(order)
    }

    /// The example shown in `slot`.
    fn example(&self, slot: usize) -> usize {
        self.0[slot % self.0.len()]
    }
}

/// Where the cycle stands: which slot shows, and how far its change into
/// the next has run, counted in letters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in super::super) struct Moment {
    slot: usize,
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
        (
            index < self.changed,
            index < self.changed && index >= self.cooled,
        )
    }
}

impl Moment {
    /// The moment `clock` seconds into the window's life.
    fn at(clock: f64) -> Self {
        let slot = (clock / SLOT).floor().max(0.0);
        let elapsed = clock - slot * SLOT - CHANGE;
        let slot = slot as usize;
        if elapsed < 0.0 {
            return Self {
                slot,
                ..Self::default()
            };
        }
        Self {
            slot,
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

/// What to ask for: a case written the way it asks, title included, or a
/// request written plainly under the plain title.
enum Ask {
    Written(&'static str, fn(&str) -> String),
    Plain(&'static str),
}

const EXAMPLES: usize = 10;

fn asks() -> [Ask; EXAMPLES] {
    [
        Ask::Written(
            tr("Use all lowercase in my messaging apps"),
            str::to_lowercase,
        ),
        Ask::Written(tr("Use all caps when I sound excited"), str::to_uppercase),
        Ask::Written(tr("Use SpongeBob case when I’m being sarcastic"), |text| {
            apply_builtin_transformation("spongebob-case", text).unwrap_or_else(|| text.to_string())
        }),
        Ask::Written(tr("Use Title Case for my email subjects"), title_case),
        Ask::Plain(tr("Spell it “Kubernetes”, not “Cooper Netties”")),
        Ask::Plain(tr("Delete every “um” and “like”")),
        Ask::Plain(tr("Turn “let’s circle back” into “no”")),
        Ask::Plain(tr("Talk like a pirate on Fridays")),
        Ask::Plain(tr("Write my commit messages like Shakespeare")),
        Ask::Plain(tr("Make “per my last email” sound friendly")),
    ]
}

/// The card's title.
fn title() -> &'static str {
    tr("Tell your agent how Hex should write")
}

fn example(index: usize) -> Example {
    let title = title();
    match &asks()[index] {
        Ask::Written(request, style) => Example {
            title: style(title),
            request: style(request),
        },
        Ask::Plain(request) => Example {
            title: title.to_string(),
            request: (*request).to_string(),
        },
    }
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
pub(super) fn copy(p: Paint, moment: Moment, order: &Order, reduced: bool) -> El {
    let moment = if reduced { Moment::default() } else { moment };
    let from = example(order.example(moment.slot));
    let to = example(order.example(moment.slot + 1));
    let title = line(
        "example-title",
        LineStyle {
            font: p.k.ty.ui_medium,
            ink: p.text(),
            accent: p.accent(),
            space: p.k.ty.ui_medium.size * 0.27,
        },
        &from.title,
        &to.title,
        moment.title,
    );
    col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .gap(6.0)
        .child(title)
        .child(prompt(p, &from.request, &to.request, moment.request))
}

/// The request as a prompt typed to an agent: a mono line behind a `›`, in
/// a sunken field.
fn prompt(p: Paint, from: &str, to: &str, wave: Wave) -> El {
    let font = TextStyle {
        size: DESCRIPTION.size,
        line_height: DESCRIPTION.line_height,
        ..p.k.ty.mono
    };
    row()
        .key("example-request")
        .self_start()
        .fill(p.k.c.bg_deep)
        .border(p.hair, p.k.c.border_weak)
        .radius(p.k.radius.md)
        .px(8.0)
        .py(4.0)
        .gap(font.size * 0.6)
        .child(text("›").key("example-prompt").font(font).color(p.accent()))
        .child(line(
            "example-request-line",
            LineStyle {
                font,
                ink: p.text(),
                accent: p.accent(),
                space: font.size * 0.6,
            },
            from,
            to,
            wave,
        ))
}

#[derive(Clone, Copy)]
struct LineStyle {
    font: TextStyle,
    ink: Color,
    accent: Color,
    /// The gap between words.
    space: f32,
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
                    .transition(pop()),
            );
        }
        words.push(word);
    }
    row()
        .key(key.to_string())
        .shrink()
        .gaps(0.0, s.space)
        .flex_wrap()
        .children(words)
}

/// The copy after Edit with agent: no examples, just what to do next.
pub(super) fn copied(p: Paint) -> El {
    col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .gap(3.0)
        .child(
            text(title())
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
        let late = Moment::at(CHANGE + 1.3);
        assert!(late.title.changed >= 31 && late.title.cooled >= 31);
        assert!(late.request.changed >= 45 && late.request.cooled >= 45);
        assert_eq!(Moment::at(SLOT + 0.1).slot, 1);
    }

    /// A shuffled order still shows every example once a round.
    #[test]
    fn a_shuffled_order_shows_every_example() {
        let order = Order::shuffled();
        let mut seen: Vec<usize> = (0..EXAMPLES).map(|slot| order.example(slot)).collect();
        seen.sort_unstable();
        assert_eq!(seen, Order::default().0);
    }

    #[test]
    fn title_case_capitalizes_each_word() {
        assert_eq!(title_case("ask an AGENT"), "Ask An Agent");
    }
}
