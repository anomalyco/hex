//! Someone typing Jay's reply by hand, as the keys they press: a little
//! slowly, with a typo backed out at once and one they go back for. Every
//! press is timed, so the field and a drawn keyboard play from one clock.

use std::sync::LazyLock;

/// What the person does next.
enum Op {
    Type(&'static str),
    /// Backspace this many times.
    Back(usize),
    /// The left arrow this many times.
    Left(usize),
    /// ⌘→, to the end of the line.
    End,
    Send,
    Pause(f64),
}

/// The same reply as [`super::fixture::ANSWER_JAY`], typed.
const SCRIPT: &[Op] = &[
    Op::Type("Hey Jya"),
    Op::Pause(0.25),
    Op::Back(2),
    Op::Type("ay, can we move our snyc to Thursday"),
    Op::Pause(0.3),
    Op::Left(12),
    Op::Pause(0.1),
    Op::Back(3),
    Op::Type("ync"),
    Op::Pause(0.12),
    Op::End,
    Op::Type(" afternon"),
    Op::Pause(0.2),
    Op::Back(2),
    Op::Type("oon?"),
    Op::Pause(0.25),
    Op::Send,
];

/// A person's pace: a key about every 60 ms, unevenly, about 40 words a
/// minute.
const PACE: f64 = 0.06;
const BACK_PACE: f64 = 0.09;
/// Held arrows repeat faster than anyone types.
const REPEAT: f64 = 0.035;
const HOLD: f64 = 0.07;
/// Shift and ⌘ go down a moment before the key and come up after it.
const MODIFIER_LEAD: f64 = 0.05;
/// How long a key takes to sink, and to come back up.
const SINK: f64 = 0.02;
const RISE: f64 = 0.05;

/// One key press and what the field holds after it.
pub(crate) struct Stroke {
    pub(crate) at: f64,
    /// The keyboard's id for the key (see [`super::keyboard`]).
    pub(crate) key: &'static str,
    pub(crate) modifier: Option<&'static str>,
    hold: f64,
    pub(crate) text: String,
    /// Where the caret is, in bytes.
    pub(crate) caret: usize,
}

/// The key that types `c`, and whether it takes Shift.
fn key_for(c: char) -> (&'static str, bool) {
    let shifted = c.is_ascii_uppercase() || c == '?';
    let key = match c.to_ascii_lowercase() {
        ' ' => "space",
        ',' => ",",
        '.' => ".",
        '?' | '/' => "/",
        letter => super::keyboard::letter(letter).unwrap_or("space"),
    };
    (key, shifted)
}

/// A steady, uneven wobble for press `n`, from 0.7 to 1.3.
fn wobble(n: usize) -> f64 {
    let x = ((n as f64 * 12.9898).sin() * 43_758.545).fract().abs();
    0.7 + 0.6 * x
}

fn strokes() -> Vec<Stroke> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut caret = 0;
    let mut at = 0.2;
    let mut push = |at: &mut f64, gap: f64, key, modifier, text: &str, caret| {
        *at += gap;
        out.push(Stroke {
            at: *at,
            key,
            modifier,
            hold: HOLD.min(gap * 0.8),
            text: text.to_string(),
            caret,
        });
    };
    let mut n = 0;
    for op in SCRIPT {
        match op {
            Op::Type(chars) => {
                for c in chars.chars() {
                    n += 1;
                    let (key, shift) = key_for(c);
                    text.insert(caret, c);
                    caret += c.len_utf8();
                    // A beat longer at the start of each word.
                    let gap = PACE * wobble(n) + if c == ' ' { 0.03 } else { 0.0 };
                    push(&mut at, gap, key, shift.then_some("shift"), &text, caret);
                }
            }
            Op::Back(count) => {
                for _ in 0..*count {
                    n += 1;
                    caret -= 1;
                    text.remove(caret);
                    push(&mut at, BACK_PACE * wobble(n), "delete", None, &text, caret);
                }
            }
            Op::Left(count) => {
                for _ in 0..*count {
                    caret -= 1;
                    push(&mut at, REPEAT, "left", None, &text, caret);
                }
            }
            Op::End => {
                caret = text.len();
                push(&mut at, 0.18, "right", Some("command"), &text, caret);
            }
            Op::Send => push(&mut at, 0.1, "return", None, &text, caret),
            Op::Pause(secs) => at += secs,
        }
    }
    out
}

static STROKES: LazyLock<Vec<Stroke>> = LazyLock::new(strokes);

/// The newest stroke `elapsed` seconds in, if any.
pub(crate) fn at(elapsed: f64) -> Option<&'static Stroke> {
    STROKES.iter().take_while(|s| s.at <= elapsed).last()
}

/// How long typing the reply takes, the send included.
pub(crate) fn duration() -> f64 {
    STROKES.last().map_or(0.0, |s| s.at + s.hold)
}

/// How far `key` is pressed `elapsed` seconds in, 0 up to 1 down.
pub(crate) fn depth(elapsed: f64, key: &str) -> f32 {
    let mut deepest = 0.0_f64;
    for s in STROKES.iter() {
        let span = if s.key == key {
            Some((s.at, s.at + s.hold))
        } else if s.modifier == Some(key) {
            Some((s.at - MODIFIER_LEAD, s.at + s.hold + MODIFIER_LEAD * 0.6))
        } else {
            None
        };
        if let Some((down, up)) = span {
            if elapsed < down - SINK || elapsed > up + RISE {
                continue;
            }
            let d = if elapsed < down {
                1.0 - (down - elapsed) / SINK
            } else if elapsed <= up {
                1.0
            } else {
                1.0 - (elapsed - up) / RISE
            };
            deepest = deepest.max(d);
        }
    }
    deepest.clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::super::fixture::ANSWER_JAY;
    use super::*;

    #[test]
    fn typing_ends_on_the_spoken_reply() {
        let last = STROKES.last().unwrap();
        assert_eq!(last.key, "return");
        assert_eq!(last.text, ANSWER_JAY.updates.last().unwrap().1);
    }

    #[test]
    fn typing_is_slow_but_only_a_few_seconds() {
        let secs = duration();
        assert!(secs > ANSWER_JAY.release * 1.6, "{secs}");
        assert!(secs < 7.0, "{secs}");
    }

    #[test]
    fn a_typo_goes_in_and_comes_back_out() {
        assert!(STROKES.iter().any(|s| s.text.ends_with("Jya")));
        assert!(STROKES.iter().any(|s| s.text.contains("snyc")));
        let fixed = STROKES
            .iter()
            .position(|s| s.text.contains("sync"))
            .unwrap();
        assert!(STROKES[..fixed].iter().any(|s| s.key == "left"));
    }

    #[test]
    fn keys_sink_while_pressed() {
        let first = &STROKES[0];
        assert_eq!(first.modifier, Some("shift"));
        assert_eq!(depth(first.at + 0.01, first.key), 1.0);
        assert_eq!(depth(first.at - 0.03, "shift"), 1.0);
        assert_eq!(depth(0.0, first.key), 0.0);
    }
}
