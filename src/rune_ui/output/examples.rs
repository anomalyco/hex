//! The agent card's copy, cycling through examples of what to ask an agent
//! for, each written the way it asks: "all lowercase" in lowercase, "all
//! caps" in caps. Each change runs in two stages. First the request shakes
//! and scrambles, then locks into the next one letter by letter. Then the
//! title glitches out word by word and back in, written the new way.
//! Every frame of it is laid out ahead of time and shown through a
//! clock-driven opacity cycle, so nothing rebuilds the view. Reduce Motion
//! holds the first example.

use rune::{Color, Cycle, El, TextStyle, col, row, text};

use super::super::controls::{DESCRIPTION, Paint};
use super::super::spark;
use crate::desktop_i18n::tr;
use crate::personal_commands::apply_builtin_transformation;

/// Seconds each example shows: one flip of the spark.
const SLOT: f32 = spark::PERIOD;
const EXAMPLES: usize = 4;
const PERIOD: f32 = SLOT * EXAMPLES as f32;
/// When in its slot an example starts changing into the next, so the
/// request has locked in just as the spark turns edge-on.
const CHANGE: f32 = spark::EDGE_ON - 0.4;
/// Frames of the request's scramble, and how long each shows.
const SCRAMBLE: usize = 9;
const SCRAMBLE_FRAME: f32 = 0.05;
/// How many of the first scramble frames shake.
const SHAKES: usize = 4;
/// When the title starts glitching out, after the change starts.
const TITLE_OUT: f32 = 0.38;
/// Glitch frames per word on the way out and in, and how long each shows.
const GLITCHES: usize = 2;
const GLITCH_FRAME: f32 = 0.06;
/// Seconds from the first word's glitch to the last's.
const STAGGER: f32 = 0.24;
/// When the title starts glitching back in: once the last word is out.
const TITLE_IN: f32 = TITLE_OUT + STAGGER + GLITCHES as f32 * GLITCH_FRAME;
/// A hair of overlap so rounding at a frame boundary never shows nothing.
const OVERLAP: f32 = 0.003;

/// Shown from the cycle's phase for `seconds`, hidden for the rest.
const fn window(seconds: f32) -> [(f32, f32); 4] {
    let end = (seconds + OVERLAP) / PERIOD;
    [(0.0, 1.0), (end, 1.0), (end, 0.0), (1.0, 0.0)]
}
const SCRAMBLE_SHOWS: [(f32, f32); 4] = window(SCRAMBLE_FRAME);
const GLITCH_SHOWS: [(f32, f32); 4] = window(GLITCH_FRAME);
/// A request holds from when it locks in until the next one starts.
const REQUEST_SHOWS: [(f32, f32); 4] = window(SLOT - SCRAMBLE as f32 * SCRAMBLE_FRAME);
/// A title word holds from its last glitch in to its first glitch out.
const WORD_SHOWS: [(f32, f32); 4] =
    window(SLOT - (TITLE_IN - TITLE_OUT) - GLITCHES as f32 * GLITCH_FRAME);

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
        written(tr("no final period in Slack"), str::to_string),
        written(tr("all lowercase"), str::to_lowercase),
        written(tr("all caps"), str::to_uppercase),
        written(tr("SpongeBob case"), |text| {
            apply_builtin_transformation("spongebob-case", text).unwrap_or_else(|| text.to_string())
        }),
    ]
}

/// The card's title and request.
pub(super) fn copy(p: Paint) -> El {
    let examples = examples();
    col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .gap(3.0)
        .child(title(p, &examples))
        .child(request(p, &examples))
}

/// Every example's title stacked in one cell, word by word.
fn title(p: Paint, examples: &[Example; EXAMPLES]) -> El {
    let font = p.k.ty.ui_medium;
    let layers = examples.iter().enumerate().map(|(example, shown)| {
        let words: Vec<&str> = shown.title.split_whitespace().collect();
        let last = words.len().saturating_sub(1).max(1) as f32;
        row()
            .key(format!("example{example}-title"))
            .grid_cell(0)
            .flex_wrap()
            .gaps(0.0, font.size * 0.27)
            .children(words.iter().enumerate().map(|(index, word)| {
                let delay = STAGGER * index as f32 / last;
                let arrives = slot_start(example) + CHANGE + TITLE_IN + delay;
                let leaves = slot_start(example + 1) + CHANGE + TITLE_OUT + delay;
                let key = format!("example{example}-word{index}");
                let mut cell = col().key(key.clone()).child(
                    text(word.to_string())
                        .key(format!("{key}-shown"))
                        .font(font)
                        .color(p.text())
                        .cycle(cycle(
                            arrives + GLITCHES as f32 * GLITCH_FRAME,
                            &WORD_SHOWS,
                            example == 0,
                        )),
                );
                for (stage, start) in [("in", arrives), ("out", leaves)] {
                    for frame in 0..GLITCHES {
                        let seed = [example, index, frame, usize::from(stage == "out")];
                        cell = cell.child(
                            text(scramble(word, seed))
                                .key(format!("{key}-{stage}{frame}"))
                                .font(font)
                                .color(p.accent())
                                .absolute(jitter(seed, 1.2), jitter(seed.map(|s| s + 7), 0.8))
                                .cycle(cycle(
                                    start + frame as f32 * GLITCH_FRAME,
                                    &GLITCH_SHOWS,
                                    false,
                                )),
                        );
                    }
                }
                cell
            }))
    });
    col().key("title").equal_columns(1).children(layers)
}

/// Every example's request as a prompt line, and the scramble from each to
/// the next, stacked in one cell.
fn request(p: Paint, examples: &[Example; EXAMPLES]) -> El {
    let font = TextStyle {
        size: DESCRIPTION.size,
        line_height: DESCRIPTION.line_height,
        ..p.k.ty.mono
    };
    let line = |content: String, color: Color, key: String| {
        text(format!("› {content}"))
            .key(key)
            .font(font)
            .color(color)
    };
    let mut cell = col().key("request").equal_columns(1);
    for (example, shown) in examples.iter().enumerate() {
        let locked = slot_start(example) + CHANGE + SCRAMBLE as f32 * SCRAMBLE_FRAME;
        cell = cell.child(
            line(
                shown.request.clone(),
                p.muted(),
                format!("example{example}-request"),
            )
            .grid_cell(0)
            .cycle(cycle(locked, &REQUEST_SHOWS, example == 0)),
        );
        let next = &examples[(example + 1) % EXAMPLES].request;
        for frame in 0..SCRAMBLE {
            let seed = [example, frame, 0, 2];
            let shake = frame < SHAKES;
            cell = cell.child(
                line(
                    morph(&shown.request, next, frame, seed),
                    p.accent(),
                    format!("example{example}-scramble{frame}"),
                )
                .absolute(
                    if shake { jitter(seed, 1.6) } else { 0.0 },
                    if shake {
                        jitter(seed.map(|s| s + 3), 0.9)
                    } else {
                        0.0
                    },
                )
                .cycle(cycle(
                    slot_start(example + 1) + CHANGE + frame as f32 * SCRAMBLE_FRAME,
                    &SCRAMBLE_SHOWS,
                    false,
                )),
            );
        }
    }
    cell
}

/// When `example`'s slot starts, in seconds of the loop; the first starts a
/// whole loop late, so its change into the second wraps to the loop's start.
fn slot_start(example: usize) -> f32 {
    (example as f32 - 1.0) * SLOT
}

/// Shown from `start` seconds into the loop through `stops`.
fn cycle(start: f32, stops: &'static [(f32, f32)], still: bool) -> Cycle {
    Cycle {
        period: PERIOD,
        phase: (start / PERIOD).rem_euclid(1.0),
        stops,
        still: if still { 1.0 } else { 0.0 },
        fps: 60.0,
    }
}

/// Frame `frame` of the scramble from `from` to `to`: first every letter
/// scrambles while the length eases from one to the other, then `to`'s
/// letters lock in from the left.
fn morph(from: &str, to: &str, frame: usize, seed: [usize; 4]) -> String {
    let (from, to): (Vec<char>, Vec<char>) = (from.chars().collect(), to.chars().collect());
    let t = (frame + 1) as f32 / (SCRAMBLE + 1) as f32;
    let length = (from.len() as f32 + (to.len() as f32 - from.len() as f32) * (t * 1.6).min(1.0))
        .round() as usize;
    let locked = (to.len() as f32 * ((t - 0.35) / 0.65).clamp(0.0, 1.0)).floor() as usize;
    (0..length)
        .map(|i| match to.get(i) {
            Some(c) if i < locked => *c,
            target => {
                let like = target.or(from.get(i)).copied().unwrap_or('a');
                glyph(like, [seed[0], seed[1], i, seed[3]])
            }
        })
        .collect()
}

/// `word` with every letter swapped for a random one of the same case.
fn scramble(word: &str, seed: [usize; 4]) -> String {
    word.chars()
        .enumerate()
        .map(|(i, c)| {
            if c.is_alphanumeric() {
                glyph(c, [seed[0], seed[1] * 31 + seed[2], i, seed[3]])
            } else {
                c
            }
        })
        .collect()
}

/// A random letter or symbol in `like`'s case; spaces stay spaces. No wide
/// glyphs, so a scrambled word stays about its own width, and no `=` or `>`,
/// which the font joins into arrows.
fn glyph(like: char, seed: [usize; 4]) -> char {
    const LETTERS: &[u8] = b"abcdefghijklnopqrstuvxyz#*+/<?";
    if like.is_whitespace() {
        return like;
    }
    let c = char::from(LETTERS[hash(seed) as usize % LETTERS.len()]);
    if like.is_uppercase() {
        c.to_ascii_uppercase()
    } else {
        c
    }
}

/// A random offset from -`reach` to `reach` points.
fn jitter(seed: [usize; 4], reach: f32) -> f32 {
    (hash(seed) % 1000) as f32 / 999.0 * 2.0 * reach - reach
}

/// SplitMix64 over the seed, so the scramble looks the same every build.
fn hash(seed: [usize; 4]) -> u64 {
    seed.iter().fold(0x9e37_79b9_7f4a_7c15_u64, |state, part| {
        let mut z = state ^ (*part as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    })
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

    /// The scramble starts as scrambled letters and ends as the next
    /// request, so the last frame hands over to the request itself.
    #[test]
    fn the_scramble_lands_on_the_next_request() {
        let last = morph("no final period", "all caps", SCRAMBLE - 1, [0; 4]);
        assert_eq!(last.chars().count(), "all caps".len());
        assert!(last.starts_with("all c"), "{last}");
        let first = morph("no final period", "all caps", 0, [0; 4]);
        assert!(!first.starts_with('a') || !first.contains("all"), "{first}");
    }
}
