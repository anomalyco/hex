//! Words as a recognizer hears them: a live line with a caret, and shout
//! words that rise in big, land with a little bounce, and settle into the
//! line. Both take a partial transcript as it streams, so a revised word
//! ("sink" to "sync") cross-fades in place rather than retyping the line.

use rune::{El, Spring, SpringPair, TextStyle, Transition, col, el, row, text};

use super::super::controls::Paint;

/// The newest word, big.
const SHOUT: TextStyle = TextStyle::new(44.0, 600.0).lh(52.0).tracking(-0.6);
const SHOUT_STAGE: f32 = 72.0;
/// Words land on Kit's 0.3 s spring with a little bounce; a shout word
/// bounces more, as it travels farther.
const LAND: Spring = Spring::new(0.3, 0.15);
const SHOUT_LAND: Spring = Spring::new(0.38, 0.3);
const LEAVE: Spring = Spring::new(0.22, 0.0);
const SHOUT_LEAVE: Spring = Spring::new(0.14, 0.0);

/// The words of `partial`, keyed by place and text, so an unchanged word
/// keeps its node and a revised one replaces it.
fn words(partial: &str) -> impl Iterator<Item = (String, &str)> {
    partial
        .split_whitespace()
        .enumerate()
        .map(|(i, word)| (format!("w{i}-{word}"), word))
}

fn word_in() -> Transition {
    Transition::asymmetric(
        Transition::offset(0.0, 4.0).combined(Transition::opacity()),
        Transition::opacity(),
    )
    .springs(SpringPair::new(LAND, LEAVE))
}

/// A live line: settled words in full ink, the word still being heard
/// muted, and a caret after it until `finished`.
pub(crate) fn live_words(
    p: Paint,
    key: &str,
    partial: &str,
    finished: bool,
    font: TextStyle,
    line: f32,
) -> El {
    let count = partial.split_whitespace().count();
    let mut out = row()
        .key(key)
        .flex_wrap()
        .gaps(0.0, font.size * 0.28)
        .items_center()
        .min_h(line);
    for (i, (word_key, word)) in words(partial).enumerate() {
        let hearing = !finished && i + 1 == count;
        out = out.child(
            text(word)
                .key(word_key)
                .font(font.lh(line))
                .color(if hearing { p.muted() } else { p.text() })
                .transition(word_in()),
        );
    }
    out.children((!finished).then(|| caret(p, line)))
}

/// A text caret `line` tall that glides after the words as they arrive.
pub(crate) fn caret(p: Paint, line: f32) -> El {
    el().key("caret")
        .w(2.0)
        .h((line * 0.8).round())
        .radius(1.0)
        .fill(p.k.c.text_accent)
        .animate_layout()
        .transition(Transition::opacity())
}

fn shout_in() -> Transition {
    let rise = Transition::offset(0.0, 36.0)
        .combined(Transition::scale(0.6))
        .allow_small_scale();
    // Drops toward the line as it fades, out of the next word's way.
    let settle = Transition::offset(0.0, 18.0)
        .combined(Transition::scale(0.9))
        .combined(Transition::opacity());
    Transition::asymmetric(rise, settle).springs(SpringPair::new(SHOUT_LAND, SHOUT_LEAVE))
}

/// The newest word of `partial` big, over the live line: each word rises
/// in, lands, then drops into the line as the next arrives. Nothing shouts
/// once `finished`; the line holds the sentence.
pub(crate) fn shout(
    p: Paint,
    key: &str,
    partial: &str,
    finished: bool,
    font: TextStyle,
    line: f32,
) -> El {
    let newest = words(partial).last();
    let stage = row().key("shout-stage").h(SHOUT_STAGE).center().children(
        newest.filter(|_| !finished).map(|(word_key, word)| {
            text(word)
                .key(format!("shout-{word_key}"))
                .font(SHOUT)
                .color(p.text())
                .transition(shout_in())
        }),
    );
    col().key(key).gap(12.0).items_center().child(stage).child(
        live_words(p, "shout-line", partial, finished, font, line)
            .w_full()
            .justify_center(),
    )
}
