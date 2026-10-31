//! The practice field's dictated words, flying in from the `"Your turn"`
//! plate on `"With Hex"`'s flight. While the shortcut is held, a model that
//! streams sends its live words ([`crate::live_text`]), which fly out as they
//! are heard. The final paste then flies whatever they did not bring: all of
//! it for a model that doesn't stream, after the shortcut comes up.
//!
//! The field's text stays the truth. Live words only draw over the field,
//! and a pasted word is in the field at once, hidden until its flight lands.
//! With Reduce Motion, words fade in where they land.

use rune::{El, Span, el, text};

use super::super::controls::Paint;
use super::field_text::{Field, Slot};
use super::flight::{Flight, HANDOFF, flicker, flying, ink, typeset};
use super::sparks::impacts;
use crate::transcription_engine::Partial;

/// `"With Hex"`'s calm flight.
const CALM: f32 = 0.20;
/// Seconds between words leaving the plate.
const STAGGER: f64 = 0.04;
/// Reduce Motion's fade.
const FADE: f64 = 0.15;
/// How long live words wait for the paste once their dictation ends.
const WAIT: f64 = 8.0;
/// How long a landed word keeps its span after it cools.
const SETTLE: f64 = 1.2;
/// The longest a flight takes ([`Flight::new`]).
const LONGEST_FLIGHT: f64 = 0.85 * 1.2;
/// The flying sprite's weight.
const WEIGHT: f32 = 520.0;

/// A live word, drawn over the field after its text.
#[derive(Clone, Debug)]
struct Heard {
    word: String,
    launch: f64,
}

/// A pasted word: bytes `start..end` of the field's text, flying in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Landing {
    start: usize,
    end: usize,
    launch: f64,
}

/// Where the field's text sits on the stage, and how it lays out.
#[derive(Clone, Copy)]
pub(crate) struct Stage {
    /// The plate's centre, where words leave from.
    pub(crate) from: [f32; 2],
    /// The field text's top left.
    pub(crate) origin: [f32; 2],
    pub(crate) field: Field,
    /// Lines the field shows before it scrolls.
    pub(crate) rows: usize,
    pub(crate) size: [f32; 2],
}

#[derive(Default)]
pub(crate) struct Dictated {
    heard: Vec<Heard>,
    tentative: String,
    /// When the dictation that sent the live words ended.
    ended: Option<f64>,
    landing: Vec<Landing>,
    /// The field's text when it last changed.
    seen: String,
    /// The soonest the next word may leave the plate.
    next: f64,
}

impl Dictated {
    /// The next word's launch time.
    fn launch(&mut self, now: f64) -> f64 {
        let at = self.next.max(now);
        self.next = at + STAGGER;
        at
    }

    /// Takes the running dictation's live text, or `None` once it ended.
    pub(crate) fn hear(&mut self, partial: Option<&Partial>, now: f64) {
        let Some(partial) = partial else {
            self.tentative.clear();
            if !self.heard.is_empty() && self.ended.is_none() {
                self.ended = Some(now);
            }
            if self.ended.is_some_and(|ended| now - ended > WAIT) {
                self.forget();
            }
            return;
        };
        self.ended = None;
        let words: Vec<&str> = partial.stable.split_whitespace().collect();
        let kept = self
            .heard
            .iter()
            .zip(&words)
            .take_while(|(heard, word)| same_word(&heard.word, word))
            .count();
        self.heard.truncate(kept);
        // A word that only gained its case or punctuation stays where it is.
        for (heard, word) in self.heard.iter_mut().zip(&words) {
            if heard.word != *word {
                heard.word = (*word).to_owned();
            }
        }
        for word in &words[kept..] {
            let launch = self.launch(now);
            self.heard.push(Heard {
                word: (*word).to_owned(),
                launch,
            });
        }
        self.tentative = partial.tentative.trim().to_owned();
    }

    /// Drops live words the paste never came for (nothing heard, too short).
    pub(crate) fn forget(&mut self) {
        self.heard.clear();
        self.tentative.clear();
        self.ended = None;
    }

    /// Starts over on an empty field.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// The field's text changed to `text`. A paste of words flies them in,
    /// starting from where the live words already are; anything else only
    /// moves the words still landing.
    pub(crate) fn changed(&mut self, text: &str, now: f64) {
        let old = std::mem::replace(&mut self.seen, text.to_owned());
        let (start, removed_end, end) = edit(&old, text);
        let shift = text.len() as isize - old.len() as isize;
        self.landing.retain_mut(|landing| {
            if landing.end <= start {
                true
            } else if landing.start >= removed_end {
                landing.start = landing.start.saturating_add_signed(shift);
                landing.end = landing.end.saturating_add_signed(shift);
                true
            } else {
                false
            }
        });
        let pasted = &text[start..end];
        if pasted.trim().chars().count() < 2 {
            return;
        }
        let heard = std::mem::take(&mut self.heard);
        self.tentative.clear();
        self.ended = None;
        // Live words keep their flights up to the first the paste rewrote;
        // the rest come from the plate.
        let mut live = heard.iter();
        let mut matching = true;
        for (offset, word) in words(pasted) {
            let launch = match live.next() {
                Some(heard) if matching && same_word(&heard.word, word) => heard.launch,
                _ => {
                    matching = false;
                    self.launch(now)
                }
            };
            self.landing.push(Landing {
                start: start + offset,
                end: start + offset + word.len(),
                launch,
            });
        }
    }

    /// Whether live words show over the field.
    pub(crate) fn hearing(&self) -> bool {
        !self.heard.is_empty() || !self.tentative.is_empty()
    }

    /// Whether any word still flies, lands, or waits for its paste.
    pub(crate) fn moving(&self) -> bool {
        !self.heard.is_empty() || !self.landing.is_empty()
    }

    /// Lets go of words that landed and cooled.
    pub(crate) fn settle(&mut self, now: f64) {
        self.landing
            .retain(|landing| now - landing.launch < LONGEST_FLIGHT + HANDOFF + SETTLE);
    }

    fn flies(stage: &Stage, text: &str) -> bool {
        !text.contains(['\n', '\t']) && stage.field.wrap(text).len() <= stage.rows
    }

    /// The field's colours for its landing words: hidden in flight, then hot
    /// as they land and cooling to the field's ink.
    pub(crate) fn spans(&self, p: Paint, stage: &Stage, now: f64, reduced: bool) -> Vec<Span> {
        let text = &self.seen;
        let fly = !reduced && Self::flies(stage, text);
        let slots = stage.field.slots(text);
        self.landing
            .iter()
            .map(|landing| {
                let flight = slot_of(text, &slots, landing)
                    .filter(|_| fly)
                    .map(|i| flight(stage, &slots[i], landing.launch, i));
                let color = match flight {
                    Some(flight) => match flight.landed(now, false) {
                        Some(landed) => ink(p, landed.heat).alpha(landed.fade),
                        None => p.k.c.text_base.alpha(0.0),
                    },
                    None => p.k.c.text_base.alpha(fade(now - landing.launch)),
                };
                Span::new(landing.start, landing.end).color(color)
            })
            .collect()
    }

    /// The words in flight and the live words, over the stage.
    pub(crate) fn overlay(
        &self,
        p: Paint,
        key: &str,
        stage: &Stage,
        now: f64,
        reduced: bool,
    ) -> El {
        let font = stage.field.font.lh(stage.field.line);
        let mut out = el()
            .key(key.to_string())
            .absolute(0.0, 0.0)
            .size(stage.size[0], stage.size[1])
            .instant_changes();
        let field = &self.seen;
        let mut live = field.trim_end().to_owned();
        let field_words = stage.field.slots(&live).len();
        for word in self
            .heard
            .iter()
            .map(|heard| heard.word.as_str())
            .chain(self.tentative.split_whitespace())
        {
            if !live.is_empty() {
                live.push(' ');
            }
            live.push_str(word);
        }
        let fly = !reduced && Self::flies(stage, &live) && Self::flies(stage, field);
        let at = |slot: &Slot<'_>| {
            [
                stage.origin[0] + slot.centre[0],
                stage.origin[1] + slot.centre[1],
            ]
        };
        let mut flights = Vec::new();
        if fly {
            let slots = stage.field.slots(field);
            for landing in &self.landing {
                let Some(i) = slot_of(field, &slots, landing) else {
                    continue;
                };
                let flight = flight(stage, &slots[i], landing.launch, i);
                if let Some(pose) = flight.pose(now, false) {
                    out = out.child(flying(p, &format!("p{i}"), slots[i].word, WEIGHT, pose));
                }
                flights.push(flight);
            }
        }
        let slots = stage.field.slots(&live);
        for (n, slot) in slots.iter().enumerate().skip(field_words) {
            let i = n - field_words;
            let [x, y] = at(slot);
            let Some(heard) = self.heard.get(i) else {
                let (w, h) = rune::text::measure(slot.word, &[], font, None);
                out = out.child(
                    text(slot.word.to_string())
                        .key(format!("t{i}"))
                        .font(font)
                        .color(p.faint())
                        .absolute(x - w / 2.0, y - h / 2.0),
                );
                continue;
            };
            if !fly {
                let (w, h) = rune::text::measure(slot.word, &[], font, None);
                out = out.child(
                    text(slot.word.to_string())
                        .key(format!("h{i}"))
                        .font(font)
                        .color(p.k.c.text_base)
                        .absolute(x - w / 2.0, y - h / 2.0)
                        .opacity(fade(now - heard.launch)),
                );
                continue;
            }
            let flight = flight(stage, slot, heard.launch, n);
            if let Some(landed) = flight.landed(now, false) {
                out = out.children(typeset(
                    p,
                    &format!("h{i}"),
                    slot.word,
                    font,
                    [x, y],
                    landed,
                    flicker(now, n),
                ));
            }
            if let Some(pose) = flight.pose(now, false) {
                out = out.child(flying(p, &format!("f{i}"), slot.word, WEIGHT, pose));
            }
            flights.push(flight);
        }
        out.child(impacts(p, "impacts", stage.size, &flights, now, reduced).absolute(0.0, 0.0))
    }
}

fn fade(since: f64) -> f32 {
    (since / FADE).clamp(0.0, 1.0) as f32
}

/// `word`'s byte offset in `text`, which it is a slice of.
fn offset(text: &str, word: &str) -> usize {
    word.as_ptr() as usize - text.as_ptr() as usize
}

/// The slot of the word `landing` flies to.
fn slot_of(text: &str, slots: &[Slot<'_>], landing: &Landing) -> Option<usize> {
    slots
        .iter()
        .position(|slot| offset(text, slot.word) == landing.start)
}

/// A word's flight from the plate to `slot`.
fn flight(stage: &Stage, slot: &Slot<'_>, launch: f64, index: usize) -> Flight {
    let to = [
        stage.origin[0] + slot.centre[0],
        stage.origin[1] + slot.centre[1],
    ];
    Flight::new(stage.from, to, launch, CALM, stage.field.font.size, index)
}

/// `text`'s words with their byte offsets.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split_whitespace()
        .map(move |word| (offset(text, word), word))
}

/// Whether a live word and its pasted self are the same word, whatever
/// the paste's case and punctuation.
fn same_word(live: &str, pasted: &str) -> bool {
    let bare = |word: &str| -> String {
        word.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    bare(live) == bare(pasted)
}

/// One edit from `old` to `new`: where it starts, where the replaced bytes
/// ended in `old`, and where the inserted bytes end in `new`.
fn edit(old: &str, new: &str) -> (usize, usize, usize) {
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .find(|((_, a), b)| a != b)
        .map_or(old.len().min(new.len()), |((i, _), _)| i);
    let room = old.len().min(new.len()) - prefix;
    let suffix = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .scan(0, |sum, len| {
            *sum += len;
            Some(*sum)
        })
        .take_while(|&sum| sum <= room)
        .last()
        .unwrap_or(0);
    (prefix, old.len() - suffix, new.len() - suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(stable: &str) -> Partial {
        Partial {
            stable: stable.into(),
            tentative: String::new(),
        }
    }

    #[test]
    fn an_edit_is_found_between_what_stayed() {
        assert_eq!(edit("", "hello there"), (0, 0, 11));
        assert_eq!(edit("ab", "a hello b"), (1, 1, 8));
        assert_eq!(edit("abc", "ac"), (1, 2, 1));
        assert_eq!(edit("aa", "aaa"), (2, 2, 3));
        assert_eq!(edit("same", "same"), (4, 4, 4));
    }

    #[test]
    fn live_words_fly_one_after_another_and_keep_their_flight_into_the_paste() {
        let mut d = Dictated::default();
        d.hear(Some(&said("so the")), 1.0);
        d.hear(Some(&said("so the plan")), 1.5);
        let launches: Vec<f64> = d.heard.iter().map(|h| h.launch).collect();
        assert_eq!(launches, [1.0, 1.0 + STAGGER, 1.5]);
        d.hear(None, 2.0);
        assert_eq!(d.ended, Some(2.0));
        d.changed("So the plan, today.", 2.4);
        assert!(d.heard.is_empty());
        let landing: Vec<(usize, f64)> = d.landing.iter().map(|l| (l.start, l.launch)).collect();
        assert_eq!(landing, [(0, 1.0), (3, 1.0 + STAGGER), (7, 1.5), (13, 2.4)]);
    }

    #[test]
    fn without_live_words_the_whole_paste_queues_from_the_plate() {
        let mut d = Dictated::default();
        d.changed("one two three", 3.0);
        let launches: Vec<f64> = d.landing.iter().map(|l| l.launch).collect();
        assert_eq!(launches, [3.0, 3.0 + STAGGER, 3.0 + 2.0 * STAGGER]);
    }

    #[test]
    fn a_rewritten_live_word_flies_again_with_the_rest() {
        let mut d = Dictated::default();
        d.hear(Some(&said("so the plant")), 1.0);
        d.changed("So the plan is.", 5.0);
        let launches: Vec<f64> = d.landing.iter().map(|l| l.launch).collect();
        assert_eq!(launches, [1.0, 1.0 + STAGGER, 5.0, 5.0 + STAGGER]);
    }

    #[test]
    fn typing_flies_nothing_and_moves_landing_words_with_the_text() {
        let mut d = Dictated::default();
        d.changed("hello world", 1.0);
        d.changed("Ahello world", 1.1);
        assert_eq!((d.landing[0].start, d.landing[1].start), (1, 7));
        assert_eq!(d.landing.len(), 2);
        d.changed("Ahello world!", 1.2);
        assert_eq!(d.landing.len(), 2);
    }

    #[test]
    fn punctuation_arriving_late_does_not_fly_a_word_again() {
        let mut d = Dictated::default();
        d.hear(Some(&said("okay this")), 1.0);
        d.hear(Some(&said("Okay, this is.")), 2.0);
        let heard: Vec<(&str, f64)> = d
            .heard
            .iter()
            .map(|h| (h.word.as_str(), h.launch))
            .collect();
        assert_eq!(
            heard,
            [("Okay,", 1.0), ("this", 1.0 + STAGGER), ("is.", 2.0)]
        );
    }

    #[test]
    fn live_words_nobody_pasted_go_away() {
        let mut d = Dictated::default();
        d.hear(Some(&said("um")), 1.0);
        d.hear(None, 1.5);
        d.hear(None, 1.5 + WAIT + 0.1);
        assert!(!d.moving());
    }
}
