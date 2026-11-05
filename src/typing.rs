//! Labs › Type dictations instead of pasting: the strokes every platform's
//! typer sends, and the routing that picks typing over the clipboard.
//!
//! The typers are `keyboard::type_text` (macOS, also its Chromium 155/156
//! path), `windows::paste`'s SendInput path (`KEYEVENTF_UNICODE`), and
//! `linux`'s XTest and `wtype` paths. They all type [`strokes`], so a line
//! break is one Shift-Return everywhere: a chat box inserts a newline
//! instead of sending, as the macOS typer already did.

/// The most UTF-16 units one macOS key event carries.
pub(crate) const MAX_UNICODE_UNITS: usize = 20;

/// One key event's worth of typing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Stroke {
    /// Text of at most [`MAX_UNICODE_UNITS`] units, never splitting a
    /// surrogate pair.
    Text(Vec<u16>),
    /// One `\r\n`, `\n`, or `\r`.
    LineBreak,
}

/// Splits `text` into key events of at most [`MAX_UNICODE_UNITS`] UTF-16
/// units, without splitting a surrogate pair. `\r\n`, `\n`, and `\r` each
/// become one line break.
pub(crate) fn strokes(text: &str) -> Vec<Stroke> {
    let mut strokes = Vec::new();
    let mut units = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if matches!(character, '\r' | '\n') {
            if character == '\r' {
                characters.next_if_eq(&'\n');
            }
            if !units.is_empty() {
                strokes.push(Stroke::Text(std::mem::take(&mut units)));
            }
            strokes.push(Stroke::LineBreak);
            continue;
        }
        let mut encoded = [0; 2];
        let encoded = character.encode_utf16(&mut encoded);
        if units.len() + encoded.len() > MAX_UNICODE_UNITS {
            strokes.push(Stroke::Text(std::mem::take(&mut units)));
        }
        units.extend_from_slice(encoded);
    }
    if !units.is_empty() {
        strokes.push(Stroke::Text(units));
    }
    strokes
}

/// How a finished dictation reaches the focused app.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Output {
    /// The clipboard and the target's paste shortcut.
    Paste,
    /// Synthesized keystrokes; the clipboard is never written.
    Type,
}

/// Labs › Type dictations instead of pasting, read as each dictation's
/// output commits. Typing wins over Copy dictation only, so the promise
/// that the clipboard is never touched holds even with both on.
pub(crate) fn dictation_output(type_dictations: bool) -> Output {
    if type_dictations {
        Output::Type
    } else {
        Output::Paste
    }
}

/// Whether this OS and desktop can type a dictation at all, so Labs lists
/// the row only where it works: macOS and Windows everywhere, Linux on X11
/// or Wayland with `wtype`'s virtual keyboard.
pub(crate) fn available() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::linux::typing::available()
    }
    #[cfg(not(target_os = "linux"))]
    {
        cfg!(any(target_os = "macos", target_os = "windows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Stroke {
        Stroke::Text(text.encode_utf16().collect())
    }

    #[test]
    fn typing_splits_text_into_twenty_unit_events_without_splitting_surrogate_pairs() {
        assert_eq!(strokes(""), []);
        assert_eq!(strokes("hello"), [text("hello")]);
        assert_eq!(
            strokes("abcdefghijklmnopqrstuvwxyz"),
            [text("abcdefghijklmnopqrst"), text("uvwxyz")]
        );
        // 19 units, then a surrogate pair that would make 21, moves whole.
        assert_eq!(
            strokes("abcdefghijklmnopqrs😀z"),
            [text("abcdefghijklmnopqrs"), text("😀z")]
        );
        assert_eq!(
            strokes("😀😀😀😀😀😀😀😀😀😀😀"),
            [text("😀😀😀😀😀😀😀😀😀😀"), text("😀")]
        );
        for stroke in strokes(&"é漢😀a".repeat(40)) {
            let Stroke::Text(units) = stroke else {
                panic!("unexpected line break");
            };
            assert!(units.len() <= MAX_UNICODE_UNITS);
            assert!(String::from_utf16(&units).is_ok());
        }
    }

    #[test]
    fn typing_sends_each_line_break_as_one_shift_return() {
        assert_eq!(
            strokes("one\ntwo\r\nthree\rfour\n\n"),
            [
                text("one"),
                Stroke::LineBreak,
                text("two"),
                Stroke::LineBreak,
                text("three"),
                Stroke::LineBreak,
                text("four"),
                Stroke::LineBreak,
                Stroke::LineBreak,
            ]
        );
    }

    #[test]
    fn a_dictation_is_typed_only_with_its_lab_on() {
        assert_eq!(dictation_output(false), Output::Paste);
        assert_eq!(dictation_output(true), Output::Type);
    }
}
