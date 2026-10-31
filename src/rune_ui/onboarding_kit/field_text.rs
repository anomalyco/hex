//! A reply laid out by hand in the card's field: wrapped at word breaks,
//! so a caret can sit mid-line while someone fixes a typo, and words flying
//! in know the slot they land in.

use rune::{El, TextStyle, el, text};

use super::super::controls::Paint;
use super::cards::{CHAT_REPLY_W, FIELD_FONT, FIELD_LINE, REPLY_W};

/// A field's text: its font, line height, and how wide it may run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Field {
    pub(crate) font: TextStyle,
    pub(crate) line: f32,
    pub(crate) width: f32,
}

/// A notification's reply field.
pub(crate) const CARD: Field = Field {
    font: FIELD_FONT,
    line: FIELD_LINE,
    width: REPLY_W,
};

/// A group chat's reply field, as wide as the chat.
pub(crate) const CHAT: Field = Field {
    font: FIELD_FONT,
    line: FIELD_LINE,
    width: CHAT_REPLY_W,
};

/// [`Field::caret`] in the card's field.
pub(crate) fn caret(p: Paint, key: &str, x: f32, line: usize) -> El {
    CARD.caret(p, key, x, line)
}

impl Field {
    /// How far `run` advances the pen; Rune measures an empty run as a space.
    fn advance(self, run: &str) -> f32 {
        if run.is_empty() {
            0.0
        } else {
            rune::text::advance(run, self.font)
        }
    }

    /// The reply's lines as byte ranges, broken after spaces once a word
    /// would run past the field.
    pub(crate) fn wrap(self, reply: &str) -> Vec<(usize, usize)> {
        let mut lines = Vec::new();
        let mut start = 0;
        let mut space = None;
        let bytes = reply.as_bytes();
        for (i, c) in reply.char_indices() {
            if c == ' ' {
                space = Some(i + 1);
                continue;
            }
            let end = i + c.len_utf8();
            let word_ends = end == reply.len() || bytes[end] == b' ';
            if !word_ends {
                continue;
            }
            if self.advance(&reply[start..end]) > self.width
                && let Some(at) = space.filter(|&at| at > start)
            {
                lines.push((start, at));
                start = at;
            }
        }
        lines.push((start, reply.len()));
        lines
    }

    /// Where a caret after byte `caret` goes: its x, and its line.
    pub(crate) fn caret_point(self, reply: &str, caret: usize) -> (f32, usize) {
        let lines = self.wrap(reply);
        let line = lines
            .iter()
            .rposition(|&(start, _)| start <= caret)
            .unwrap_or(0);
        let start = lines[line].0;
        (self.advance(&reply[start..caret]), line)
    }

    /// Every word of `reply` where it lands.
    pub(crate) fn slots(self, reply: &str) -> Vec<Slot<'_>> {
        let mut out = Vec::new();
        for (line, (start, end)) in self.wrap(reply).into_iter().enumerate() {
            let text = &reply[start..end];
            let mut at = 0;
            for word in text.split(' ') {
                if !word.is_empty() {
                    let left = self.advance(&text[..at]);
                    let right = left + self.advance(word);
                    out.push(Slot {
                        word,
                        line,
                        left,
                        right,
                        centre: [(left + right) / 2.0, (line as f32 + 0.5) * self.line],
                    });
                }
                at += word.len() + 1;
            }
        }
        out
    }

    /// A text caret at `(x, line)`.
    pub(crate) fn caret(self, p: Paint, key: &str, x: f32, line: usize) -> El {
        let inset = (self.line * 0.11).round();
        el().key(key.to_string())
            .absolute(x, line as f32 * self.line + inset)
            .size((self.line / 12.0).max(1.5), self.line - 2.0 * inset)
            .radius(0.75)
            .fill(p.k.c.text_accent)
    }
}

/// One word's slot: its text, its line, and its box's left and centre
/// from the field text's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Slot<'a> {
    pub(crate) word: &'a str,
    pub(crate) line: usize,
    pub(crate) left: f32,
    pub(crate) right: f32,
    pub(crate) centre: [f32; 2],
}

impl Field {
    /// `reply` as typed so far, with the caret after byte `caret` while it's
    /// being typed, on `lines` lines kept however much there is, and
    /// autocorrect's underline under the `mark` byte range. In an empty
    /// field the caret sits just before the placeholder's first glyph.
    pub(crate) fn typed(
        self,
        p: Paint,
        key: &str,
        reply: &str,
        caret_at: Option<usize>,
        lines: usize,
        mark: Option<(usize, usize)>,
    ) -> El {
        let font = self.font.lh(self.line);
        let mut out = el()
            .key(key.to_string())
            .w(self.width)
            .h(lines as f32 * self.line)
            .instant_changes();
        for (i, (start, end)) in self.wrap(reply).into_iter().enumerate() {
            out = out.child(
                text(reply[start..end].to_string())
                    .key(format!("line{i}"))
                    .font(font)
                    .color(p.text())
                    .absolute(0.0, i as f32 * self.line),
            );
        }
        if let Some((start, end)) = mark.filter(|&(s, e)| s < e && e <= reply.len()) {
            let (left, line) = self.caret_point(reply, start);
            let (right, _) = self.caret_point(reply, end);
            out = out.child(
                el().key("autocorrect")
                    .absolute(left - 1.0, line as f32 * self.line + self.line - 2.0)
                    .size((right - left + 2.0).max(4.0), 2.0)
                    .radius(1.0)
                    .fill(p.k.c.text_accent),
            );
        }
        if let Some(at) = caret_at {
            let (x, line) = self.caret_point(reply, at);
            let x = if reply.is_empty() { -1.5 } else { x };
            out = out.child(self.caret(p, "caret", x, line));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Longer than one line of the card's field.
    const REPLY: &str =
        "Yes, we're ready. Everything's tested, the rollback is rehearsed, go ahead.";

    #[test]
    fn the_reply_wraps_at_spaces_within_the_field() {
        let reply = REPLY;
        let lines = CARD.wrap(reply);
        assert!(lines.len() >= 2, "{lines:?}");
        for &(start, end) in &lines {
            assert!(rune::text::advance(reply[start..end].trim_end(), FIELD_FONT) <= REPLY_W);
        }
        assert_eq!(lines.last().unwrap().1, reply.len());
    }

    #[test]
    fn every_word_has_a_slot_in_order() {
        let reply = REPLY;
        let slots = CARD.slots(reply);
        assert_eq!(slots.len(), reply.split_whitespace().count());
        assert_eq!(slots[0].left, 0.0);
        assert!(
            slots
                .windows(2)
                .all(|w| w[0].line < w[1].line || w[0].right < w[1].left)
        );
    }

    #[test]
    fn a_caret_after_a_break_starts_the_next_line() {
        let reply = REPLY;
        let (start, _) = CARD.wrap(reply)[1];
        assert_eq!(CARD.caret_point(reply, start), (0.0, 1));
        assert_eq!(CARD.caret_point(reply, 0), (0.0, 0));
    }
}
