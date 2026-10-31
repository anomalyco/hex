//! The agent card's copy, cycling through examples of what to tell the
//! agent, each written the way it asks: "all lowercase" in lowercase, "all
//! caps" in caps. The old words fade out left to right, then the new ones fade
//! in the same way, starting as the spark turns edge-on.
//! The clock drives every word's opacity, so nothing rebuilds the view.
//! Reduce Motion holds the first example.

use rune::{Cycle, El, TextStyle, col, row, text};

use super::super::controls::{DESCRIPTION, Paint};
use super::super::spark;
use crate::desktop_i18n::{tr, tr_fill};
use crate::personal_commands::apply_builtin_transformation;

/// Seconds each example shows: one flip of the spark.
const SLOT: f32 = spark::PERIOD;
const EXAMPLES: usize = 4;
const PERIOD: f32 = SLOT * EXAMPLES as f32;
/// Seconds from the first word's fade to the last's.
const STAGGER: f32 = 0.3;
/// Seconds a word takes to fade.
const FADE: f32 = 0.18;
/// Seconds a new word waits after its old one starts fading: long enough
/// for the whole old line to go first, so a wider word never runs into one
/// still showing.
const WAIT: f32 = STAGGER + FADE;
/// A word fades in, shows for its slot less the wait, and fades out.
const WORD: [(f32, f32); 5] = [
    (0.0, 0.0),
    (FADE / PERIOD, 1.0),
    ((SLOT - WAIT) / PERIOD, 1.0),
    ((SLOT - WAIT + FADE) / PERIOD, 0.0),
    (1.0, 0.0),
];

/// The card's title and detail, one layer per example stacked in one cell.
pub(super) fn copy(p: Paint) -> El {
    let title = tr("Your agent writes these steps");
    let layers = (0..EXAMPLES).map(|example| {
        let (title, detail) = match example {
            0 => (
                title.to_string(),
                tr("Tell it what you want, like “no final period in Slack”").to_string(),
            ),
            1 => written(title, tr("all lowercase"), |text| text.to_lowercase()),
            2 => written(title, tr("all caps"), |text| text.to_uppercase()),
            _ => written(title, tr("SpongeBob case"), |text| {
                apply_builtin_transformation("spongebob-case", text)
                    .unwrap_or_else(|| text.to_string())
            }),
        };
        let title_words = title.split_whitespace().count();
        let words = (title_words + detail.split_whitespace().count()).max(2);
        let line = |text: &str, first: usize, font: TextStyle, color, key: &str| {
            row()
                .key(format!("example{example}-{key}"))
                .flex_wrap()
                .gaps(0.0, font.size * 0.27)
                .children(text.split_whitespace().enumerate().map(|(index, word)| {
                    let delay = STAGGER * (first + index) as f32 / (words - 1) as f32;
                    let start = (example as f32 - 1.0) * SLOT + spark::EDGE_ON + delay;
                    rune::text(word.to_string())
                        .key(format!("example{example}-{key}{index}"))
                        .font(font)
                        .color(color)
                        .cycle(Cycle {
                            period: PERIOD,
                            phase: (start / PERIOD).rem_euclid(1.0),
                            stops: &WORD,
                            still: if example == 0 { 1.0 } else { 0.0 },
                            fps: 60.0,
                        })
                }))
        };
        col()
            .key(format!("example{example}"))
            .grid_cell(0)
            .gap(2.0)
            .child(line(&title, 0, p.k.ty.ui_medium, p.text(), "title"))
            .child(line(&detail, title_words, DESCRIPTION, p.muted(), "detail"))
    });
    col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .equal_columns(1)
        .children(layers)
}

/// The title and "Tell it what you want, like “…”" with `request`, both
/// written the way `request` asks.
fn written(title: &str, request: &str, style: impl Fn(&str) -> String) -> (String, String) {
    (
        style(title),
        style(&tr_fill("Tell it what you want, like “{}”", request)),
    )
}

/// The copy after Edit with agent: no examples, just what to do next.
pub(super) fn copied(p: Paint) -> El {
    col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .gap(2.0)
        .child(
            text(tr("Your agent writes these steps"))
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
