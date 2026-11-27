//! Typed edits over the words as heard.

use super::words::Span;
#[cfg(test)]
use super::words::Words;

/// Why a stretch of speech is not part of what the person meant to say.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Disfluency {
    /// A hesitation sound or a verbal tic: "um", "you know".
    Filler,
    /// Words said again right after: "I I", "the, the office", "w- we".
    Repeat,
    /// A clause begun and abandoned: "Did you, have you".
    FalseStart,
    /// Something said and then taken back: "Tuesday, no wait,".
    Correction,
}

/// What an edit does to its words. No operation carries text, so no edit can
/// write a word that was not heard.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum EditOp {
    Delete(Disfluency),
}

/// One change to the words as heard: what it does, and to which words.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Edit {
    pub(crate) at: Span,
    pub(crate) op: EditOp,
}

/// An edit something suggests, and how sure it is. A proposal changes
/// nothing: the guard decides what becomes of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Proposal {
    at: Span,
    confidence: f64,
}

impl Proposal {
    /// Suggests removing the words at `at`. `confidence` is the proposer's
    /// probability, from 0 to 1, that they are a disfluency. Which kind is
    /// the guard's to say.
    pub(crate) fn delete(at: Span, confidence: f64) -> Self {
        Self { at, confidence }
    }

    pub(super) fn at(&self) -> Span {
        self.at
    }

    pub(super) fn confidence(&self) -> f64 {
        self.confidence
    }
}

/// The edits the guard admitted for one transcript, in the order of its
/// words and never overlapping. The cleaned text is always rendered from
/// the words as heard and a plan, so an undo or a retry is a new plan and
/// nothing is ever edited in place.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EditPlan {
    edits: Vec<Edit>,
}

impl EditPlan {
    /// Only the guard admits edits. They arrive in word order.
    pub(super) fn admitted(edits: Vec<Edit>) -> Self {
        Self { edits }
    }

    pub(crate) fn edits(&self) -> &[Edit] {
        &self.edits
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// The plan with one edit undone. An `index` past the last edit undoes
    /// nothing.
    pub(crate) fn without(&self, index: usize) -> Self {
        let mut edits = self.edits.clone();
        if index < edits.len() {
            edits.remove(index);
        }
        Self { edits }
    }

    /// Which of `count` words the plan removes.
    pub(super) fn deletes(&self, count: usize) -> Vec<bool> {
        let mut deletes = vec![false; count];
        for edit in &self.edits {
            let EditOp::Delete(_) = edit.op;
            for index in edit.at.range() {
                if let Some(delete) = deletes.get_mut(index) {
                    *delete = true;
                }
            }
        }
        deletes
    }

    /// A plan that removes exactly the flagged words, for tests that render
    /// what no guard would admit.
    #[cfg(test)]
    pub(super) fn deleting(words: &Words, deletes: &[bool]) -> Self {
        let mut edits = Vec::new();
        let mut index = 0;
        while index < words.len() {
            let start = index;
            while index < words.len() && deletes.get(index).copied().unwrap_or(false) {
                index += 1;
            }
            match words.span(start..index) {
                Some(at) => edits.push(Edit {
                    at,
                    op: EditOp::Delete(Disfluency::Filler),
                }),
                None => index += 1,
            }
        }
        Self { edits }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_removes_the_words_its_edits_name() {
        let words = Words::new("um so uh uh yes");
        let plan = EditPlan::deleting(&words, &[true, false, true, true, false]);
        assert_eq!(plan.edits().len(), 2);
        assert_eq!(plan.edits()[1].at.range(), 2..4);
        assert_eq!(plan.deletes(words.len()), [true, false, true, true, false]);
        assert!(!plan.is_empty());
        assert!(EditPlan::default().is_empty());
        assert_eq!(EditPlan::default().deletes(2), [false, false]);
    }

    #[test]
    fn undoing_an_edit_leaves_the_others_as_they_were() {
        let words = Words::new("um so uh uh yes");
        let plan = EditPlan::deleting(&words, &[true, false, true, true, false]);
        assert_eq!(
            plan.without(0).deletes(words.len()),
            [false, false, true, true, false]
        );
        assert_eq!(
            plan.without(1).deletes(words.len()),
            [true, false, false, false, false]
        );
        assert_eq!(plan.without(2), plan);
        assert!(plan.without(0).without(0).is_empty());
    }

    #[test]
    fn a_plan_for_longer_words_removes_only_words_that_exist() {
        let long = Words::new("one two three four");
        let plan = EditPlan::deleting(&long, &[false, true, true, true]);
        assert_eq!(plan.deletes(2), [false, true]);
    }
}
