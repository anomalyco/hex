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
/// with it. The saved signature is trimmed, so its own edges never add blank
/// lines.
pub fn append(text: &str, signature: &str, enabled: bool) -> Option<String> {
    if !enabled || text.trim().is_empty() {
        return None;
    }
    let signature = signature.trim();
    if signature.is_empty() || text.trim_end().ends_with(signature) {
        return None;
    }
    Some(format!("{text}\n{signature}"))
}

/// Why a saved signature is refused, if it is: it is one line of at most
/// MAX_CHARS characters. Empty is allowed and appends nothing.
pub fn text_problem(text: &str) -> Option<&'static str> {
    let text = text.trim();
    if text.contains(['\n', '\r']) {
        return Some("Use one line");
    }
    if text.chars().count() > MAX_CHARS {
        return Some("Use 200 characters or fewer");
    }
    None
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
    fn a_text_that_is_only_the_signature_is_not_signed_again() {
        assert_eq!(append(DEFAULT_TEXT, DEFAULT_TEXT, true), None);
    }
}
