//! Typing under pressure in `"Without Hex"`: Jay asks whether we're ready to
//! ship with the investors in the room. Typing by hand fumbles `"Yse, we're dead"`
//! and accidentally hits Return, freezes in horror, then fires off a panicked
//! `"oh god *reayd"` follow-up that has a typo too (~7.2 s total vs 1.5 s by voice).

use std::sync::LazyLock;

enum Op {
    Type(&'static str),
    Back(usize),
    Send,
    Pause(f64),
}

const SCRIPT: &[Op] = &[
    // Let the 3D keyboard rise at an angle and rotate cleanly into typing view before typing starts.
    Op::Pause(0.76),
    Op::Type("Yse, were"),
    Op::Pause(0.32),
    Op::Back(2),
    Op::Type("'re deady"),
    Op::Pause(0.26),
    Op::Back(1),
    // Accidental premature Return right on "Yse, we're dead"!
    Op::Send,
    // Frozen horror beat staring at the sent bubble so the viewer reads it clearly.
    Op::Pause(0.88),
    // Frantic panic follow-up — submitted with another typo!
    Op::Type("oh god *reayd"),
    Op::Pause(0.24),
    Op::Send,
];

const PACE: f64 = 0.118;
const PANIC_PACE: f64 = 0.094;
const BACK_PACE: f64 = 0.105;
const HOLD: f64 = 0.072;
const MODIFIER_LEAD: f64 = 0.052;
const SINK: f64 = 0.022;
const RISE: f64 = 0.055;

pub(crate) struct Stroke {
    pub(crate) at: f64,
    pub(crate) key: &'static str,
    pub(crate) modifier: Option<&'static str>,
    hold: f64,
    /// Current draft in the input box (cleared on each `Send`).
    pub(crate) text: String,
    pub(crate) caret: usize,
    /// Messages already accidentally sent so far (`1` then `2`).
    pub(crate) sent: Vec<String>,
}

fn key_for(c: char) -> (&'static str, bool) {
    let shifted = c.is_ascii_uppercase() || matches!(c, '?' | '*' | '!');
    let key = match c.to_ascii_lowercase() {
        ' ' => "space",
        ',' => ",",
        '.' => ".",
        '\'' => "'",
        '*' => "8",
        '!' => "1",
        '?' | '/' => "/",
        letter => super::keyboard::letter(letter).unwrap_or("space"),
    };
    (key, shifted)
}

fn wobble(n: usize) -> f64 {
    let x = ((n as f64 * 12.9898).sin() * 43_758.545).fract().abs();
    0.74 + 0.54 * x
}

fn strokes() -> Vec<Stroke> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut sent: Vec<String> = Vec::new();
    let mut caret = 0;
    let mut at = 0.0;
    let mut push = |at: &mut f64, gap: f64, key, modifier, text: &str, caret, sent: &[String]| {
        *at += gap;
        out.push(Stroke {
            at: *at,
            key,
            modifier,
            hold: HOLD.min(gap.max(0.05) * 0.8),
            text: text.to_string(),
            caret,
            sent: sent.to_vec(),
        });
    };
    let mut n = 0;
    for op in SCRIPT {
        match op {
            Op::Type(chars) => {
                let base_pace = if sent.is_empty() { PACE } else { PANIC_PACE };
                for c in chars.chars() {
                    n += 1;
                    let (key, shift) = key_for(c);
                    text.insert(caret, c);
                    caret += c.len_utf8();
                    let extra = if c == ' ' {
                        0.045
                    } else if shift {
                        0.035
                    } else {
                        0.0
                    };
                    let gap = base_pace * wobble(n) + extra;
                    push(
                        &mut at,
                        gap,
                        key,
                        shift.then_some("shift"),
                        &text,
                        caret,
                        &sent,
                    );
                }
            }
            Op::Back(count) => {
                for _ in 0..*count {
                    n += 1;
                    caret -= 1;
                    text.remove(caret);
                    push(
                        &mut at,
                        BACK_PACE * wobble(n),
                        "delete",
                        None,
                        &text,
                        caret,
                        &sent,
                    );
                }
            }
            Op::Send => {
                sent.push(std::mem::take(&mut text));
                caret = 0;
                push(&mut at, 0.14, "return", None, &text, caret, &sent);
            }
            Op::Pause(secs) => at += secs,
        }
    }
    out
}

static STROKES: LazyLock<Vec<Stroke>> = LazyLock::new(strokes);

pub(crate) fn all_strokes() -> &'static [Stroke] {
    STROKES.as_slice()
}

pub(crate) fn at(elapsed: f64) -> Option<&'static Stroke> {
    STROKES.iter().take_while(|s| s.at <= elapsed).last()
}

pub(crate) fn duration() -> f64 {
    STROKES.last().map_or(0.0, |s| s.at + s.hold)
}

pub(crate) fn final_sent() -> &'static [String] {
    STROKES.last().map_or(&[], |s| s.sent.as_slice())
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
    fn typing_accidentally_sends_two_typo_messages() {
        let last = STROKES.last().unwrap();
        assert_eq!(last.key, "return");
        assert_eq!(last.sent, vec!["Yse, we're dead", "oh god *reayd"]);
        assert!(last.text.is_empty());
    }

    #[test]
    fn typing_takes_over_three_times_longer_than_the_1_5s_voice_reply() {
        let secs = duration();
        assert!(secs > ANSWER_JAY.release * 3.0, "{secs}");
        assert!(secs < 8.5, "{secs}");
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
