use super::bare;
use crate::dictation::cleanup::{
    Admitted, CleanupGuard, Disfluency, Edit, EditOp, Policy, Proposal, ProtectedTerms, Protection,
    Reason, Refused, Span, Words, render,
};

/// Above the delete threshold only.
const MARKED: f64 = 0.985;
/// Above the repair floor.
const REPAIR: f64 = 0.997;
/// Above the sure floor.
const SURE: f64 = 0.9995;

/// A transcript, what a proposer marked in it, and the person's vocabulary.
struct Dictation {
    words: Words,
    proposals: Vec<Proposal>,
    protected: ProtectedTerms,
}

fn heard(text: &str) -> Dictation {
    Dictation {
        words: Words::new(text),
        proposals: Vec::new(),
        protected: ProtectedTerms::default(),
    }
}

impl Dictation {
    /// Proposes each word of `phrase`, where the transcript first has it.
    fn mark(mut self, phrase: &str, confidence: f64) -> Self {
        let phrase: Vec<&str> = phrase.split_whitespace().collect();
        let heard: Vec<&str> = self.words.iter().collect();
        let start = heard
            .windows(phrase.len())
            .position(|window| window == phrase)
            .expect("the transcript has the phrase");
        for index in start..start + phrase.len() {
            let at = self.words.span(index..index + 1).unwrap();
            self.proposals.push(Proposal::delete(at, confidence));
        }
        self
    }

    fn knowing(mut self, terms: &[&str]) -> Self {
        self.protected = ProtectedTerms::new(terms.iter().copied());
        self
    }

    fn admit(&self, policy: Policy) -> Admitted {
        let guard = CleanupGuard {
            policy,
            protected: &self.protected,
        };
        guard.admit(&self.words, &self.proposals)
    }

    fn clean(&self, policy: Policy) -> String {
        render(&self.words, &self.admit(policy).plan)
    }

    fn said(&self, at: Span) -> &str {
        &self.words.text()[self.words.bytes(at)]
    }

    fn edits(&self, policy: Policy) -> Vec<(&str, Disfluency)> {
        let admitted = self.admit(policy);
        let edits = admitted.plan.edits().iter().map(|edit: &Edit| {
            let EditOp::Delete(kind) = edit.op;
            (self.said(edit.at), kind)
        });
        edits.collect()
    }

    fn refused(&self, policy: Policy) -> Vec<(&str, Reason)> {
        let admitted = self.admit(policy);
        let refused = admitted
            .refused
            .iter()
            .map(|refused: &Refused| (self.said(refused.at), refused.reason));
        refused.collect()
    }
}

const BOTH: [Policy; 2] = [Policy::IOS, Policy::DESKTOP];
const SHIP: &str = "Let's ship it Tuesday, no wait, Thursday.";

#[test]
fn a_correction_goes_whole_when_every_word_is_above_the_repair_floor() {
    for policy in BOTH {
        let cue = heard(SHIP).mark("Tuesday, no wait,", REPAIR);
        assert_eq!(
            cue.edits(policy),
            [("Tuesday, no wait,", Disfluency::Correction)]
        );
        assert_eq!(cue.refused(policy), []);
        assert_eq!(cue.clean(policy), "Let's ship it Thursday.");

        // A hesitation before the same thing said again differently.
        let again = heard("It costs 15 euros, er, 50 euros.").mark("15 euros, er,", REPAIR);
        assert_eq!(again.clean(policy), "It costs 50 euros.");
        let number = heard("Give me 3, uh, 4 of them.").mark("3, uh,", REPAIR);
        assert_eq!(number.edits(policy), [("3, uh,", Disfluency::Correction)]);
        assert_eq!(number.clean(policy), "Give me 4 of them.");

        // A correction is the one thing that removes a name.
        let name = heard("Send it to John, I mean Mike.").mark("John, I mean", REPAIR);
        assert_eq!(name.clean(policy), "Send it to Mike.");
    }

    let unsure = heard(SHIP).mark("Tuesday, no wait,", MARKED);
    assert_eq!(unsure.clean(Policy::IOS), SHIP);
    assert_eq!(
        unsure.refused(Policy::IOS),
        [
            ("Tuesday,", Reason::Protected(Protection::Name)),
            ("no wait,", Reason::NotADisfluency)
        ]
    );
}

#[test]
fn a_repeat_goes_when_it_is_said_again_right_after() {
    for policy in BOTH {
        let twice = heard("I I think the, the office is closed.")
            .mark("I", MARKED)
            .mark("the,", MARKED);
        assert_eq!(
            twice.edits(policy),
            [("I", Disfluency::Repeat), ("the,", Disfluency::Repeat)]
        );
        assert_eq!(twice.clean(policy), "I think the office is closed.");

        let stutter = heard("So w- we should go.").mark("w-", MARKED);
        assert_eq!(stutter.edits(policy), [("w-", Disfluency::Repeat)]);
        assert_eq!(stutter.clean(policy), "So we should go.");

        // A content adverb said twice is emphasis.
        let adverb = heard("It was really, really good.").mark("really,", SURE);
        assert_eq!(adverb.refused(policy), [("really,", Reason::Emphasis)]);
        assert_eq!(adverb.clean(policy), "It was really, really good.");
    }
}

#[test]
fn a_false_start_goes_when_it_is_function_words_or_the_proposer_is_sure() {
    for policy in BOTH {
        let function = heard("Did you, have you seen it?").mark("Did you,", MARKED);
        assert_eq!(
            function.edits(policy),
            [("Did you,", Disfluency::FalseStart)]
        );
        assert_eq!(function.clean(policy), "Have you seen it?");

        let content = "The red, I want the blue one.";
        let unsure = heard(content).mark("The red,", REPAIR);
        assert_eq!(
            unsure.refused(policy),
            [("The red,", Reason::NotADisfluency)]
        );
        assert_eq!(
            heard(content).mark("The red,", SURE).clean(policy),
            "I want the blue one."
        );
    }
}

#[test]
fn fillers_go_and_the_same_words_stay_where_they_mean_something() {
    for policy in BOTH {
        let fillers = heard("Um, so I was, like, thinking, you know, we could go.")
            .mark("Um,", MARKED)
            .mark("like,", MARKED)
            .mark("you know,", MARKED);
        assert_eq!(
            fillers.edits(policy),
            [
                ("Um,", Disfluency::Filler),
                ("like,", Disfluency::Filler),
                ("you know,", Disfluency::Filler)
            ]
        );
        assert_eq!(fillers.clean(policy), "So I was thinking we could go.");

        // Neighbouring fillers are one edit.
        let two = heard("Um, uh, so yes.").mark("Um, uh,", MARKED);
        assert_eq!(two.edits(policy), [("Um, uh,", Disfluency::Filler)]);
        assert_eq!(two.clean(policy), "So yes.");

        let after_be = heard("It was like super expensive.").mark("like", MARKED);
        assert_eq!(after_be.clean(policy), "It was super expensive.");
        for (verb, confidence) in [("I like it.", SURE), ("It was like a dream.", MARKED)] {
            let like = heard(verb).mark("like", confidence);
            assert_eq!(
                like.refused(policy),
                [("like", Reason::NotADisfluency)],
                "{verb}"
            );
        }
        let meant = heard("You know I mean what I say.").mark("You know I mean", MARKED);
        assert_eq!(
            meant.refused(policy),
            [("You know I mean", Reason::NotADisfluency)]
        );
    }
}

#[test]
fn a_protected_word_stays_and_says_why() {
    for policy in BOTH {
        let hyphen = heard("Tell Hye-jin I said thanks.").mark("Hye-jin I said", SURE);
        assert_eq!(
            hyphen.refused(policy),
            [
                ("Hye-jin", Reason::Protected(Protection::Hyphenated)),
                ("I said", Reason::NotADisfluency)
            ]
        );
        assert!(hyphen.admit(policy).plan.is_empty());

        let adverb = heard("Let's do it later, maybe after dinner.").mark("maybe", SURE);
        assert_eq!(
            adverb.refused(policy),
            [("maybe", Reason::Protected(Protection::Adverb))]
        );

        let name = heard("Send it to Mike, at 3pm, okay?")
            .mark("Mike,", SURE)
            .mark("3pm,", SURE);
        assert_eq!(
            name.refused(policy),
            [
                ("Mike,", Reason::Protected(Protection::Name)),
                ("3pm,", Reason::Protected(Protection::Digit))
            ]
        );

        let tool = heard("We use, kubectl, for that.").mark("kubectl,", SURE);
        assert_eq!(tool.edits(policy), [("kubectl,", Disfluency::FalseStart)]);
        let known = tool.knowing(&["kubectl"]);
        assert_eq!(
            known.refused(policy),
            [("kubectl,", Reason::Protected(Protection::Vocabulary))]
        );
    }
}

#[test]
fn a_proposal_at_or_below_the_delete_threshold_is_refused_as_unsure() {
    for policy in BOTH {
        for confidence in [0.0, 0.5, 0.98, f64::NAN] {
            let unsure = heard("Um, hello.").mark("Um,", confidence);
            assert_eq!(unsure.refused(policy), [("Um,", Reason::Unsure)]);
            assert!(unsure.admit(policy).plan.is_empty());
        }
        assert_eq!(
            heard("Um, hello.").mark("Um,", MARKED).clean(policy),
            "Hello."
        );
    }
}

#[test]
fn proposals_may_span_words_and_overlap() {
    let none = ProtectedTerms::default();
    let guard = CleanupGuard {
        policy: Policy::DESKTOP,
        protected: &none,
    };
    let words = Words::new(SHIP);
    let correction = words.span(3..6).unwrap();

    let whole = guard.admit(&words, &[Proposal::delete(correction, REPAIR)]);
    assert_eq!(render(&words, &whole.plan), "Let's ship it Thursday.");

    // A word is as sure as the surest proposal that covers it.
    let overlapping = [
        Proposal::delete(correction, 0.5),
        Proposal::delete(words.span(3..4).unwrap(), REPAIR),
        Proposal::delete(words.span(4..6).unwrap(), REPAIR),
    ];
    assert_eq!(guard.admit(&words, &overlapping), whole);

    // A span made by longer words proposes only the words that exist here.
    let longer = Words::new("a b c d e f g h i j");
    let foreign = guard.admit(
        &words,
        &[Proposal::delete(longer.span(6..10).unwrap(), SURE)],
    );
    assert!(foreign.plan.is_empty());
    assert_eq!(foreign.refused.len(), 1);
    assert_eq!(foreign.refused[0].at.range(), 6..7);
}

/// Emphatic repeats, marked as the tagger marks them. The iOS guard cuts
/// each to one.
#[test]
fn on_desktop_a_word_said_three_times_is_emphasis() {
    for (said, marked, ios) in [
        (
            "No, no, no, that's not what I meant.",
            "no,",
            "No, no, that's not what I meant.",
        ),
        (
            "I said stop, stop, stop the deploy.",
            "stop, stop,",
            "I said stop the deploy.",
        ),
        (
            "Wait wait wait, go back one slide.",
            "Wait wait",
            "Wait, go back one slide.",
        ),
    ] {
        let emphatic = heard(said).mark(marked, SURE);
        assert_eq!(emphatic.clean(Policy::IOS), ios);
        assert_eq!(emphatic.clean(Policy::DESKTOP), said);
        assert_eq!(
            emphatic.refused(Policy::DESKTOP),
            [(marked, Reason::Emphasis)]
        );
    }

    // Three of a function word is still a stutter.
    let stutter = heard("I I I think so.").mark("I I", MARKED);
    assert_eq!(stutter.clean(Policy::DESKTOP), "I think so.");

    // Not fixed yet: a word said twice still reads as a repeat.
    let twice = heard("Never, ever, ever share that link.").mark("ever,", SURE);
    assert_eq!(twice.clean(Policy::DESKTOP), "Never, ever share that link.");
}

/// The iOS guard removes both fillers of the first sentence.
#[test]
fn on_desktop_a_filler_that_is_talked_about_stays() {
    let said = "Um is a filler word, and so is uh.";
    let named = heard(said).mark("Um", SURE).mark("uh.", SURE);
    assert_eq!(named.clean(Policy::IOS), "Is a filler word, and so is.");
    assert_eq!(named.clean(Policy::DESKTOP), said);
    assert_eq!(
        named.refused(Policy::DESKTOP),
        [("Um", Reason::Mentioned), ("uh.", Reason::Mentioned)]
    );

    let quoted = heard("She said \"um\" a lot.").mark("\"um\"", SURE);
    assert_eq!(quoted.clean(Policy::IOS), "She said a lot.");
    assert_eq!(
        quoted.refused(Policy::DESKTOP),
        [("\"um\"", Reason::Mentioned)]
    );

    // A pause on either side makes it a hesitation again.
    for (said, marked, cleaned) in [
        ("It is, uh, complicated.", "uh,", "It is complicated."),
        ("Um, is that right?", "Um,", "Is that right?"),
        ("So is, uh.", "uh.", "So is."),
    ] {
        assert_eq!(
            heard(said).mark(marked, SURE).clean(Policy::DESKTOP),
            cleaned
        );
    }
}

#[test]
fn on_desktop_a_false_start_never_takes_a_negation() {
    let said = "We can't, the build is red.";
    let negation = heard(said).mark("We can't,", SURE);
    assert_eq!(negation.clean(Policy::IOS), "The build is red.");
    assert_eq!(negation.clean(Policy::DESKTOP), said);
    assert_eq!(
        negation.refused(Policy::DESKTOP),
        [
            ("We", Reason::NotADisfluency),
            ("can't,", Reason::Protected(Protection::Negation))
        ]
    );
    let curly = heard("We can\u{2019}t, the build is red.").mark("We can\u{2019}t,", SURE);
    assert!(curly.admit(Policy::DESKTOP).plan.is_empty());

    // A correction still takes its cue, and a repeat its copy.
    let cue = heard(SHIP).mark("Tuesday, no wait,", REPAIR);
    assert_eq!(cue.clean(Policy::DESKTOP), "Let's ship it Thursday.");
    let twice = heard("I did not not see it.").mark("not", MARKED);
    assert_eq!(twice.clean(Policy::DESKTOP), "I did not see it.");
}

#[test]
fn on_desktop_edits_that_would_leave_no_word_are_all_refused() {
    for (said, um) in [("um uh", "um"), ("\u{2014} um, uh \u{2014}", "um,")] {
        let fillers = heard(said).mark(um, SURE).mark("uh", SURE);
        assert_eq!(fillers.edits(Policy::IOS).len(), 1);
        assert!(fillers.admit(Policy::DESKTOP).plan.is_empty());
        assert_eq!(fillers.clean(Policy::DESKTOP), said);
        let refused = fillers.refused(Policy::DESKTOP);
        assert!(
            refused
                .iter()
                .all(|(_, reason)| *reason == Reason::LeavesNothing)
        );
        assert_eq!(refused.len(), 1);
    }
    assert_eq!(
        heard("um yes uh")
            .mark("um", SURE)
            .mark("uh", SURE)
            .clean(Policy::DESKTOP),
        "Yes"
    );
}

#[test]
fn vocabulary_protects_each_word_of_a_term_but_not_a_single_letter() {
    let terms = ProtectedTerms::new(["Open Code,", "x", "$5", "  "]);
    let mut words: Vec<&str> = terms.0.iter().map(String::as_str).collect();
    words.sort_unstable();
    assert_eq!(words, ["$5", "code", "open"]);
}

#[test]
fn words_are_compared_bare() {
    assert_eq!(bare("\"Hello,\""), "hello");
    assert_eq!(bare("$5."), "$5");
    assert_eq!(bare("100%."), "100%");
    assert_eq!(bare("#1!"), "#1");
    assert_eq!(bare("I'm"), "i'm");
    assert_eq!(bare("\u{2014}"), "");
    assert_eq!(bare("Cafe\u{301},"), "caf\u{e9}");
}

// Synthetic regression cases: none are copied from evaluation transcripts.
#[test]
fn comparative_like_keeps_the_clause_relationship() {
    for said in [
        "Arrange the folders like we did yesterday.",
        "It reads like I said before.",
        "Keep it like it was yesterday.",
        "Arrange it like Morgan does.",
        "Arrange it like the team does.",
    ] {
        let case = heard(said).mark("like", SURE);
        assert_ne!(case.clean(Policy::IOS), said);
        assert_eq!(case.clean(Policy::DESKTOP), said);
    }
    assert_eq!(
        heard("It was like unbelievably loud.")
            .mark("like", SURE)
            .clean(Policy::DESKTOP),
        "It was unbelievably loud."
    );
    assert_eq!(
        heard("It was, like, surprisingly bright.")
            .mark("like,", SURE)
            .clean(Policy::DESKTOP),
        "It was surprisingly bright."
    );
}

#[test]
fn parallel_items_keep_their_distinct_content() {
    for (said, marked) in [
        (
            "Pack 2 bags of green tea, 2 bags of black tea, and cups.",
            "2 bags of green tea,",
        ),
        (
            "A box of red pencils, a box of blue pencils.",
            "A box of red pencils,",
        ),
        (
            "Add a box of red pencils, a box of blue pencils.",
            "Add a box of red pencils,",
        ),
    ] {
        let case = heard(said).mark(marked, SURE);
        assert_ne!(case.clean(Policy::IOS), said);
        assert_eq!(case.clean(Policy::DESKTOP), said);
    }
    // Identical repeats and explicit repairs still go.
    assert_eq!(
        heard("Pack a bag of tea, a bag of tea.")
            .mark("a bag of tea,", SURE)
            .clean(Policy::DESKTOP),
        "Pack a bag of tea."
    );
    assert_eq!(
        heard("Pack 2 bags of green tea, sorry, 2 bags of black tea.")
            .mark("2 bags of green tea, sorry,", SURE)
            .clean(Policy::DESKTOP),
        "Pack 2 bags of black tea."
    );
}

#[test]
fn anaphoric_repairs_keep_the_noun_they_refer_to() {
    for (said, marked) in [
        (
            "Use the striped mug, sorry, the plain one beside it.",
            "the striped mug, sorry,",
        ),
        (
            "Bring the metal bowls, no, the wooden ones.",
            "the metal bowls, no,",
        ),
    ] {
        let case = heard(said).mark(marked, REPAIR);
        assert_ne!(case.clean(Policy::IOS), said);
        assert_eq!(case.clean(Policy::DESKTOP), said);
    }
    assert_eq!(
        heard("Bring two boxes, sorry, one box.")
            .mark("two boxes, sorry,", REPAIR)
            .clean(Policy::DESKTOP),
        "Bring one box."
    );
    assert_eq!(
        heard("Use the striped mug, sorry, the plain mug.")
            .mark("the striped mug, sorry,", REPAIR)
            .clean(Policy::DESKTOP),
        "Use the plain mug."
    );
}

#[test]
fn corrections_never_strand_a_contradictory_clause_initial_particle() {
    for particle in ["Yes", "Yeah", "Sure"] {
        let said = format!("{particle}, we should no, we should postpone.");
        let case = heard(&said).mark("we should no,", REPAIR);
        assert_ne!(case.clean(Policy::IOS), said);
        assert_eq!(case.clean(Policy::DESKTOP), said);
        // A fully proposed correction includes the particle already.
        let marked = format!("{particle}, we should no,");
        assert_eq!(
            heard(&said).mark(&marked, REPAIR).clean(Policy::DESKTOP),
            "We should postpone."
        );
    }
    assert_eq!(
        heard("Yes, Tuesday, sorry, Thursday works.")
            .mark("Tuesday, sorry,", REPAIR)
            .clean(Policy::DESKTOP),
        "Yes, Thursday works."
    );
    // The preceding particle belongs to another sentence.
    assert_eq!(
        heard("Yes. We should no, we should postpone.")
            .mark("We should no,", REPAIR)
            .clean(Policy::DESKTOP),
        "Yes. We should postpone."
    );
}

#[test]
fn structural_guards_leave_real_fillers_and_restarts_cleanable() {
    assert_eq!(
        heard("Um, we we should, uh, leave.")
            .mark("Um,", SURE)
            .mark("we", SURE)
            .mark("uh,", SURE)
            .clean(Policy::DESKTOP),
        "We should leave."
    );
    assert_eq!(
        heard("Can you, would you open it?")
            .mark("Can you,", REPAIR)
            .clean(Policy::DESKTOP),
        "Would you open it?"
    );
}
