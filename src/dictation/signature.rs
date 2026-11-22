//! Labs › Signature: the one line appended to a normal dictation's text
//! before it is pasted, kept in History, and remembered for Paste Last.
//!
//! The text is signed once, at the point the final text feeds all three, so
//! history shows exactly what was inserted and Paste Last reinserts it
//! unchanged. Raw, printed, and API output are not signed.

/// The signature a new install starts with.
pub const DEFAULT_TEXT: &str = "Sent from Hex";

/// The longest signature a person may save, in characters.
pub const MAX_CHARS: usize = 200;

/// The text with the signature on its own line at the end, or None when
/// there is nothing to append: the lab is off, the text is empty after
/// trimming, the signature is empty after trimming, or the text already ends
/// with it on a line of its own. The saved signature is trimmed, so its own
/// edges never add blank lines.
pub fn append(text: &str, signature: &str, enabled: bool) -> Option<String> {
    if !enabled || text.trim().is_empty() {
        return None;
    }
    let signature = signature.trim();
    if signature.is_empty() || already_signed(text.trim(), signature) {
        return None;
    }
    Some(format!("{text}\n{signature}"))
}

/// Whether the text already carries the signature as a line of its own: it is
/// only the signature, or the signature follows a line break with nothing but
/// spaces or tabs in between. A signature that merely ends a line of prose,
/// such as "Hex" in "I love Hex", does not make the line signed.
fn already_signed(text: &str, signature: &str) -> bool {
    text == signature
        || text
            .strip_suffix(signature)
            .is_some_and(|rest| rest.trim_end_matches([' ', '\t']).ends_with(['\n', '\r']))
}

/// Why a saved signature is refused, if it is: it is one line of at most
/// MAX_CHARS characters. Empty is allowed and appends nothing.
pub fn text_problem(text: &str) -> Option<&'static str> {
    let text = text.trim();
    if text.chars().any(splits_a_line) {
        return Some("Use one line");
    }
    if text.chars().count() > MAX_CHARS {
        return Some("Use 200 characters or fewer");
    }
    None
}

/// A character that breaks or controls a line: every control character (a
/// line break, a tab, NEL, and the rest) plus the Unicode line and paragraph
/// separators.
fn splits_a_line(character: char) -> bool {
    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signature_is_appended_once_on_its_own_line() {
        assert_eq!(
            append("we ship friday", DEFAULT_TEXT, true),
            Some("we ship friday\nSent from Hex".to_owned())
        );
        assert_eq!(
            append("we ship friday\n", DEFAULT_TEXT, true),
            Some("we ship friday\n\nSent from Hex".to_owned())
        );
    }

    #[test]
    fn a_saved_signature_is_trimmed_before_it_is_appended() {
        assert_eq!(
            append("hi", "  Sent from Hex  ", true),
            Some("hi\nSent from Hex".to_owned())
        );
    }

    #[test]
    fn off_appends_nothing() {
        assert_eq!(append("we ship friday", DEFAULT_TEXT, false), None);
    }

    #[test]
    fn empty_text_appends_nothing() {
        assert_eq!(append("", DEFAULT_TEXT, true), None);
        assert_eq!(append("   \n\t", DEFAULT_TEXT, true), None);
    }

    #[test]
    fn an_empty_or_whitespace_signature_appends_nothing() {
        assert_eq!(append("we ship friday", "", true), None);
        assert_eq!(append("we ship friday", "  \n\t ", true), None);
    }

    #[test]
    fn text_that_already_ends_with_the_signature_is_left_alone() {
        assert_eq!(
            append("we ship friday\nSent from Hex", DEFAULT_TEXT, true),
            None
        );
        assert_eq!(append("Sent from Hex", DEFAULT_TEXT, true), None);
        // A trailing space or newline after it still reads as signed.
        assert_eq!(
            append("we ship friday\nSent from Hex\n", DEFAULT_TEXT, true),
            None
        );
    }

    #[test]
    fn a_signature_that_ends_a_line_of_prose_is_still_appended() {
        let signature = "Hex";
        assert_eq!(
            append("I love Hex", signature, true),
            Some("I love Hex\nHex".to_owned())
        );
        assert_eq!(
            append("we ship friday, Hex", signature, true),
            Some("we ship friday, Hex\nHex".to_owned())
        );
        assert_eq!(
            append("Hex is the best", signature, true),
            Some("Hex is the best\nHex".to_owned())
        );
    }

    #[test]
    fn a_signature_after_a_line_break_is_left_alone() {
        for text in [
            "we ship friday\nSent from Hex",
            "we ship friday\n  Sent from Hex",
            "we ship friday\r\nSent from Hex",
            "we ship friday\rSent from Hex",
            "\nSent from Hex",
        ] {
            assert_eq!(append(text, DEFAULT_TEXT, true), None, "{text:?}");
        }
    }

    #[test]
    fn a_signature_must_be_one_line_of_at_most_the_limit() {
        assert_eq!(text_problem(""), None);
        assert_eq!(text_problem("Sent from Hex"), None);
        assert_eq!(text_problem("two\nlines"), Some("Use one line"));
        assert_eq!(text_problem("two\r\nlines"), Some("Use one line"));
        assert_eq!(text_problem(&"x".repeat(MAX_CHARS)), None);
        assert_eq!(
            text_problem(&"x".repeat(MAX_CHARS + 1)),
            Some("Use 200 characters or fewer")
        );
    }

    #[test]
    fn a_signature_refuses_line_and_control_characters() {
        for text in [
            "a\u{2028}b",
            "a\u{2029}b",
            "a\u{0085}b",
            "a\tb",
            "a\u{0007}b",
            "a\u{001b}b",
        ] {
            assert_eq!(text_problem(text), Some("Use one line"), "{text:?}");
        }
    }

    #[test]
    fn a_text_that_is_only_the_signature_is_not_signed_again() {
        assert_eq!(append(DEFAULT_TEXT, DEFAULT_TEXT, true), None);
    }
}
