//! The last completed dictation Paste Last reinserts.
//!
//! Its text and whether it carries the signature line are remembered
//! together: a signed dictation pastes standalone, so a later dictation never
//! joins its signature line, and that decision never depends on how the
//! signature is set when Paste Last runs. A saved signature that changed or
//! a lab switched off still reinserts exactly what was kept.

/// The last completed dictation Paste Last can reinsert.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LastDictation {
    text: String,
    signed: bool,
}

impl LastDictation {
    pub fn new(text: String, signed: bool) -> Self {
        Self { text, signed }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether a reinsertion stands alone. A signed dictation must: it ends in
    /// the signature line, which a following (unsigned) dictation would
    /// otherwise continue.
    pub fn standalone(&self) -> bool {
        self.signed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_dictation_reinserts_standalone() {
        let last = LastDictation::new("we ship friday\nSent from Hex".to_owned(), true);
        assert!(last.standalone());
        assert_eq!(last.text(), "we ship friday\nSent from Hex");
    }

    #[test]
    fn an_unsigned_dictation_reinserts_as_a_continuation() {
        let last = LastDictation::new("we ship friday".to_owned(), false);
        assert!(!last.standalone());
        assert_eq!(last.text(), "we ship friday");
    }
}
