//! Labs › Clipboard phrase: a spoken phrase inside a normal dictation, "my
//! clipboard" by default, becomes the clipboard's text in double quotes
//! before the dictation is pasted, kept in History, and remembered for Paste
//! Last.
//!
//! Matching reuses [`super::voice_prompt::words`], so it is whole-word and
//! case- and punctuation-insensitive the same way a voice prompt's is: "My
//! clipboard." matches, while "my clipboards" and "my-clipboard" don't. The
//! phrase is replaced after the Output pane's processing, so corrections,
//! rewrites, and transformations never change what was copied. The
//! clipboard is read once, only when the phrase is there, before Hex's own
//! paste puts the dictation on it.

use super::voice_prompt::words;

/// The phrase Labs › Clipboard phrase starts with.
pub const DEFAULT_PHRASE: &str = "my clipboard";

/// Where each whole-word `phrase` sits in `text`, as byte ranges, left to
/// right and never overlapping.
fn spans(text: &str, phrase: &str) -> Vec<(usize, usize)> {
    let phrase: Vec<String> = words(phrase).into_iter().map(|word| word.key).collect();
    if phrase.is_empty() {
        return Vec::new();
    }
    let text = words(text);
    let mut spans = Vec::new();
    let mut start = 0;
    while start + phrase.len() <= text.len() {
        let window = &text[start..start + phrase.len()];
        if window
            .iter()
            .zip(&phrase)
            .all(|(word, key)| word.key == *key)
        {
            spans.push((window[0].start, window[phrase.len() - 1].end));
            start += phrase.len();
        } else {
            start += 1;
        }
    }
    spans
}

/// `text` with every whole-word `phrase` replaced by the clipboard's text in
/// double quotes, or `None` when there is nothing to replace: the phrase
/// isn't there, or `clipboard` has no text once trimmed. `clipboard` is
/// called only when the phrase is there.
pub fn replace(
    text: &str,
    phrase: &str,
    clipboard: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let spans = spans(text, phrase);
    if spans.is_empty() {
        return None;
    }
    let copied = clipboard()?;
    let copied = copied.trim();
    if copied.is_empty() {
        return None;
    }
    let quoted = format!("\"{copied}\"");
    let mut replaced = String::with_capacity(text.len() + spans.len() * quoted.len());
    let mut rest = 0;
    for (start, end) in spans {
        replaced.push_str(&text[rest..start]);
        replaced.push_str(&quoted);
        rest = end;
    }
    replaced.push_str(&text[rest..]);
    Some(replaced)
}

/// The system clipboard's text on macOS and Windows, or `None` when it holds
/// none or can't be read. Linux reads its session's selection instead
/// (`linux::clipboard::Selection::text`).
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn system_text() -> Option<String> {
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
        Ok(text) => Some(text),
        Err(error) => {
            tracing::info!(%error, "Clipboard phrase found no clipboard text");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(text: &str, copied: &str) -> Option<String> {
        replace(text, DEFAULT_PHRASE, || Some(copied.to_owned()))
    }

    #[test]
    fn the_phrase_becomes_the_clipboard_in_quotes() {
        assert_eq!(
            with("fix the error in my clipboard please", "index out of range"),
            Some("fix the error in \"index out of range\" please".to_owned())
        );
    }

    #[test]
    fn matching_ignores_case_and_keeps_the_punctuation_around_it() {
        assert_eq!(
            with("My clipboard. Is that right?", "42"),
            Some("\"42\". Is that right?".to_owned())
        );
        assert_eq!(
            with("Here it is: my clipboard!", "ok"),
            Some("Here it is: \"ok\"!".to_owned())
        );
    }

    #[test]
    fn every_occurrence_is_replaced() {
        assert_eq!(
            with("my clipboard and again my clipboard", "x"),
            Some("\"x\" and again \"x\"".to_owned())
        );
    }

    #[test]
    fn only_whole_words_match() {
        for text in [
            "my clipboards are full",
            "my-clipboard",
            "myclipboard",
            "clipboard my",
            "",
        ] {
            assert_eq!(with(text, "x"), None, "{text:?}");
        }
    }

    #[test]
    fn the_clipboard_is_read_only_when_the_phrase_is_there() {
        let mut read = false;
        assert_eq!(
            replace("we ship friday", DEFAULT_PHRASE, || {
                read = true;
                Some("x".to_owned())
            }),
            None
        );
        assert!(!read);
    }

    #[test]
    fn copied_text_is_trimmed_and_empty_leaves_the_phrase() {
        assert_eq!(
            with("see my clipboard", "  line one\nline two\n"),
            Some("see \"line one\nline two\"".to_owned())
        );
        assert_eq!(with("see my clipboard", " \n\t"), None);
        assert_eq!(replace("see my clipboard", DEFAULT_PHRASE, || None), None);
    }

    #[test]
    fn a_custom_phrase_matches_its_own_words() {
        assert_eq!(
            replace("paste the clip here", "the clip", || Some("ab".to_owned())),
            Some("paste \"ab\" here".to_owned())
        );
        assert_eq!(
            replace("see my clipboard", "", || Some("ab".to_owned())),
            None
        );
    }

    #[test]
    fn the_default_phrase_is_a_valid_phrase() {
        assert_eq!(
            crate::dictation::voice_prompt::phrase_problem(DEFAULT_PHRASE),
            None
        );
    }
}
