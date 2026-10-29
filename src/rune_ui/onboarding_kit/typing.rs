//! Someone typing Jay's reply by hand: noticeably slower and more awkward
//! than speaking (~8.5 s vs 3.0 s), with hesitations, five typos, arrowing
//! back mid-sentence to repair `"snyc"`, and jumping back to the end.

use std::sync::LazyLock;

enum Op {
    Type(&'static str),
    Back(usize),
    Left(usize),
    End,
    Send,
    Pause(f64),
}

/// Produces `"Hey Jay, can we move our sync to Thursday afternoon?"` after
/// five relatable typing stumbles.
const SCRIPT: &[Op] = &[
    Op::Type("Hey Jya"),
    Op::Pause(0.32),
    Op::Back(2),
    Op::Type("ay, cna"),
    Op::Pause(0.28),
    Op::Back(2),
    Op::Type("an we mvoe"),
    Op::Pause(0.26),
    Op::Back(3),
    Op::Type("ove our snyc to thurs"),
    Op::Pause(0.30),
    Op::Back(5),
    Op::Type("Thursday"),
    Op::Pause(0.34),
    Op::Left(12),
    Op::Pause(0.14),
    Op::Back(3),
    Op::Type("ync"),
    Op::Pause(0.16),
    Op::End,
    Op::Type(" afternono"),
    Op::Pause(0.30),
    Op::Back(3),
    Op::Type("oon?"),
    Op::Pause(0.28),
    Op::Send,
];

/// Hesitant human typing cadence (~9.5 s total — three times slower than speaking).
const PACE: f64 = 0.066;
const BACK_PACE: f64 = 0.082;
const REPEAT: f64 = 0.036;
const HOLD: f64 = 0.075;
const MODIFIER_LEAD: f64 = 0.055;
const SINK: f64 = 0.022;
const RISE: f64 = 0.055;

pub(crate) struct Stroke {
    pub(crate) at: f64,
    pub(crate) key: &'static str,
    pub(crate) modifier: Option<&'static str>,
    hold: f64,
    pub(crate) text: String,
    pub(crate) caret: usize,
}

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

fn wobble(n: usize) -> f64 {
    let x = ((n as f64 * 12.9898).sin() * 43_758.545).fract().abs();
    0.72 + 0.58 * x
}

fn strokes() -> Vec<Stroke> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut caret = 0;
    let mut at = 0.22;
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
                    let extra = if c == ' ' {
                        0.04
                    } else if shift {
                        0.035
                    } else {
                        0.0
                    };
                    let gap = PACE * wobble(n) + extra;
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
                push(&mut at, 0.2, "right", Some("command"), &text, caret);
            }
            Op::Send => push(&mut at, 0.12, "return", None, &text, caret),
            Op::Pause(secs) => at += secs,
        }
    }
    out
}

static STROKES: LazyLock<Vec<Stroke>> = LazyLock::new(strokes);

pub(crate) fn at(elapsed: f64) -> Option<&'static Stroke> {
    STROKES.iter().take_while(|s| s.at <= elapsed).last()
}

pub(crate) fn duration() -> f64 {
    STROKES.last().map_or(0.0, |s| s.at + s.hold)
}

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
    fn typing_takes_over_twice_as_long_as_speaking() {
        let secs = duration();
        assert!(secs > ANSWER_JAY.release * 2.4, "{secs}");
        assert!(secs < 10.5, "{secs}");
    }

    #[test]
    fn multiple_typos_go_in_and_come_back_out() {
        assert!(STROKES.iter().any(|s| s.text.ends_with("Jya")));
        assert!(STROKES.iter().any(|s| s.text.ends_with("cna")));
        assert!(STROKES.iter().any(|s| s.text.ends_with("mvoe")));
        assert!(STROKES.iter().any(|s| s.text.contains("snyc")));
        assert!(STROKES.iter().any(|s| s.text.ends_with("afternono")));
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
