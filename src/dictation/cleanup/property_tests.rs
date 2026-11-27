//! What holds for any words and any proposals, checked on seeded random
//! ones drawn from the fixtures' sentences and from words picked to be
//! awkward.

use std::collections::BTreeSet;

use super::parity_tests::guard_cases;
use super::{CleanupGuard, EditOp, EditPlan, Policy, Proposal, ProtectedTerms, Words, render};

/// SplitMix64: the same cases on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap()
    }

    fn pick<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        &from[self.below(from.len())]
    }
}

/// Words that sit on the renderer's edges: bare marks, ellipses, letters
/// whose capital is two letters, emoji, quotes.
const AWKWARD: &[&str] = &[
    "...",
    "\u{2026}",
    "um,",
    "Um.",
    "uh",
    "No.",
    "no,",
    "I",
    "iPhone",
    "ßtraße",
    "ǆungla",
    "ŉot",
    "İstanbul",
    "😀ok",
    "Çava?",
    "at",
    "At",
    "The",
    "the,",
    "3pm,",
    "$5.",
    "wait\u{2026}",
    "left...",
    "so;",
    "then:",
    "\u{2014}",
    "\"Um\"",
    "follow-up",
    "Mike?",
    "yes!",
    "-",
];
/// One value on each side of every threshold of the guard.
const CONFIDENCES: &[f64] = &[0.2, 0.98, 0.985, 0.995, 0.997, 0.999, 0.9995, 1.0];
const SEPARATORS: &[&str] = &[" ", "  ", "\n", "\t", "\u{a0}", " \r\n "];

/// Every sentence of the guard reference, once.
fn sentences() -> Vec<Vec<String>> {
    let unique: BTreeSet<Vec<String>> = guard_cases().into_iter().map(|case| case.words).collect();
    unique.into_iter().collect()
}

/// A word as it counts for "the same word": without the marks the renderer
/// may take or add at its end, and in capitals, since the renderer may
/// change a first letter's case (and ß capitalizes to SS).
fn key(word: &str) -> String {
    word.trim_end_matches([',', '.', '?', '!', ';', ':', '\u{2026}'])
        .to_uppercase()
}

fn keys<'a>(words: impl Iterator<Item = &'a str>) -> Vec<String> {
    words.map(key).filter(|key| !key.is_empty()).collect()
}

/// Some of the words, proposed alone or in pairs, at any confidence.
fn random_proposals(words: &Words, random: &mut Random) -> Vec<Proposal> {
    let mut proposals = Vec::new();
    let mut marked = random.below(2) == 0;
    for index in 0..words.len() {
        if random.below(3) == 0 {
            marked = !marked;
        }
        let end = (index + 1 + random.below(2)).min(words.len());
        if marked && let Some(at) = words.span(index..end) {
            proposals.push(Proposal::delete(at, *random.pick(CONFIDENCES)));
        }
    }
    proposals
}

/// Which words the proposals cover above the delete threshold.
fn marked(words: &Words, proposals: &[Proposal]) -> Vec<bool> {
    let mut marked = vec![false; words.len()];
    for proposal in proposals
        .iter()
        .filter(|proposal| proposal.confidence() > 0.98)
    {
        marked[proposal.at().range()].fill(true);
    }
    marked
}

#[test]
fn rendering_deletions_never_adds_a_word() {
    let mut random = Random(1);
    let mut pool: Vec<String> = sentences().into_iter().flatten().collect();
    pool.extend(AWKWARD.iter().map(|word| (*word).to_owned()));
    for _ in 0..4000 {
        let heard: Vec<&str> = (0..1 + random.below(12))
            .map(|_| random.pick(&pool).as_str())
            .collect();
        let deletes: Vec<bool> = heard.iter().map(|_| random.below(5) < 2).collect();
        let words = Words::new(heard.join(" "));
        let output = render(&words, &EditPlan::deleting(&words, &deletes));

        // What is left is the kept words, each one still itself, in order.
        let kept = heard
            .iter()
            .zip(&deletes)
            .filter(|(_, delete)| !**delete)
            .map(|(word, _)| *word);
        assert_eq!(
            keys(output.split_whitespace()),
            keys(kept),
            "{heard:?} {deletes:?} {output:?}"
        );
    }
}

#[test]
fn nothing_to_delete_gives_the_transcript_back_exactly() {
    let mut random = Random(2);
    let sentences = sentences();
    let none = ProtectedTerms::default();
    for policy in [Policy::IOS, Policy::DESKTOP] {
        let guard = CleanupGuard {
            policy,
            protected: &none,
        };
        for sentence in &sentences {
            let mut text = String::from(*random.pick(&["", " ", "\n\n"]));
            for word in sentence {
                text.push_str(word);
                text.push_str(random.pick(SEPARATORS));
            }
            let words = Words::new(text.as_str());
            assert!(words.iter().eq(sentence.iter().map(String::as_str)));
            assert_eq!(render(&words, &EditPlan::default()), text);

            let unproposed = guard.admit(&words, &[]);
            assert!(unproposed.plan.is_empty() && unproposed.refused.is_empty());

            // Proposals at or below the delete threshold are all refused.
            let unsure: Vec<Proposal> = (0..words.len())
                .map(|index| {
                    let at = words.span(index..index + 1).unwrap();
                    Proposal::delete(at, *random.pick(&[0.0, 0.5, 0.98, f64::NAN]))
                })
                .collect();
            let admitted = guard.admit(&words, &unsure);
            assert!(admitted.plan.is_empty());
            assert_eq!(render(&words, &admitted.plan), text);
        }
    }
}

/// The guard removes a word only while something proposes it above the
/// delete threshold, so taking proposals away never removes a word they
/// did not name. It can still change what becomes of the words left
/// proposed: "Tuesday, no wait," is no correction once "no" is unsure, and
/// "Tuesday," alone may then go as a false start.
#[test]
fn the_guard_removes_only_what_is_proposed() {
    let mut random = Random(3);
    for sentence in sentences() {
        let words = Words::new(sentence.join(" "));
        for round in 0..24 {
            let terms = [
                random.pick(&sentence).as_str(),
                random.pick(&sentence).as_str(),
            ];
            let protected = ProtectedTerms::new(terms.into_iter().take(round % 3));
            let mut proposals = random_proposals(&words, &mut random);
            let mut removed = Vec::new();
            for policy in [Policy::IOS, Policy::DESKTOP] {
                let guard = CleanupGuard {
                    policy,
                    protected: &protected,
                };
                let admitted = guard.admit(&words, &proposals);
                let deletes = admitted.plan.deletes(words.len());
                let marked = marked(&words, &proposals);
                assert!(
                    deletes
                        .iter()
                        .zip(&marked)
                        .all(|(delete, marked)| *marked || !delete),
                    "{sentence:?} {proposals:?}"
                );

                // Every proposed word has exactly one outcome, and no other word has one.
                let mut outcomes = vec![0; words.len()];
                let spans = admitted.plan.edits().iter().map(|edit| edit.at);
                for span in spans.chain(admitted.refused.iter().map(|refused| refused.at)) {
                    for index in span.range() {
                        outcomes[index] += 1;
                    }
                }
                let mut proposed = vec![0; words.len()];
                for proposal in &proposals {
                    proposed[proposal.at().range()].fill(1);
                }
                assert_eq!(outcomes, proposed, "{sentence:?} {proposals:?}");
                assert!(
                    admitted
                        .plan
                        .edits()
                        .is_sorted_by(|a, b| a.at.range().end <= b.at.range().start)
                );
                removed.push(deletes);
            }
            // The desktop policy only keeps more.
            assert!(
                removed[1]
                    .iter()
                    .zip(&removed[0])
                    .all(|(desktop, ios)| *ios || !desktop)
            );

            // With some proposals taken away, the same holds for the rest.
            proposals.retain(|_| random.below(2) == 0);
            let guard = CleanupGuard {
                policy: Policy::DESKTOP,
                protected: &protected,
            };
            let deletes = guard.admit(&words, &proposals).plan.deletes(words.len());
            let marked = marked(&words, &proposals);
            assert!(
                deletes
                    .iter()
                    .zip(&marked)
                    .all(|(delete, marked)| *marked || !delete)
            );
        }
    }
}

#[test]
fn undoing_an_edit_puts_back_its_words_and_no_others() {
    let mut random = Random(4);
    let none = ProtectedTerms::default();
    let guard = CleanupGuard {
        policy: Policy::IOS,
        protected: &none,
    };
    let mut undone = 0;
    for sentence in sentences() {
        let words = Words::new(sentence.join(" "));
        for _ in 0..8 {
            let plan = guard
                .admit(&words, &random_proposals(&words, &mut random))
                .plan;
            let deletes = plan.deletes(words.len());
            for (index, edit) in plan.edits().iter().enumerate() {
                let EditOp::Delete(_) = edit.op;
                let mut expected = deletes.clone();
                expected[edit.at.range()].fill(false);
                assert_eq!(plan.without(index).deletes(words.len()), expected);
                undone += 1;
            }
            let mut plan = plan;
            while !plan.is_empty() {
                plan = plan.without(0);
            }
            assert_eq!(render(&words, &plan), words.text());
        }
    }
    assert!(undone > 100, "{undone}");
}
