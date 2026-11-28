//! v2 held to the tagger's Python reference, on every OS.
//!
//! The fixtures are written by the reference itself from the exported
//! model:
//! - `model-parity.json`: sentences with the Core ML model's logits for each
//!   word, the actions and spans the reference decoded from them, and the
//!   line it wrote, with and without the case and punctuation heads;
//! - `render-parity.json`: 600 random lines with random tags and options,
//!   and what the reference's `apply_cleanup` wrote for each, which reaches
//!   every span grammar and boundary repair;
//! - `tokenizer-parity.json`: words and the ids Hugging Face's tokenizer
//!   gives them, checked against the model folder's tokenizer by
//!   `the_tokenizer_matches_hugging_face` when one is installed.

use serde::Deserialize;

use super::decode::{self, Action, Decode, Tag, WordLogits};
use super::render;

#[derive(Deserialize)]
struct RenderCases {
    cases: Vec<RenderCase>,
}

#[derive(Deserialize)]
struct RenderCase {
    words: Vec<String>,
    actions: Vec<String>,
    spans: Vec<String>,
    cases: Vec<String>,
    puncts: Vec<String>,
    use_itn: bool,
    delete_openers: bool,
    apply_case_punct: bool,
    cleaned: String,
}

#[derive(Deserialize)]
pub(super) struct ModelCases {
    keep_bias: f64,
    min_edit_prob: f64,
    pub(super) cases: Vec<ModelCase>,
}

#[derive(Deserialize)]
pub(super) struct ModelCase {
    pub(super) words: Vec<String>,
    pub(super) input_ids: Vec<i32>,
    /// Each word's four heads: action, punctuation, case, span.
    pub(super) logits: Vec<[Vec<f32>; 4]>,
    pub(super) actions: Vec<String>,
    pub(super) spans: Vec<String>,
    pub(super) cleaned: String,
    cleaned_case_punct: String,
}

pub(super) fn model_cases() -> (ModelCases, Decode) {
    let cases: ModelCases =
        serde_json::from_str(include_str!("fixtures/model-parity.json")).unwrap();
    let decode = Decode {
        keep_bias: cases.keep_bias,
        min_edit_prob: cases.min_edit_prob,
        ..Decode::default()
    };
    (cases, decode)
}

impl ModelCase {
    pub(super) fn word_logits(&self) -> Vec<WordLogits> {
        self.logits
            .iter()
            .map(|[action, punct, case, span]| WordLogits {
                action: action.as_slice().try_into().unwrap(),
                punct: punct.as_slice().try_into().unwrap(),
                case: case.as_slice().try_into().unwrap(),
                span: span.as_slice().try_into().unwrap(),
            })
            .collect()
    }

    /// The actions and spans the reference decoded, as labels.
    pub(super) fn labels(tags: &[Tag]) -> (Vec<String>, Vec<String>) {
        tags.iter()
            .map(|tag| {
                let (action, span) = tag.labels();
                (action.to_owned(), span.to_owned())
            })
            .unzip()
    }
}

#[test]
fn the_renderer_writes_what_the_reference_writes_for_every_recorded_line() {
    let cases: RenderCases =
        serde_json::from_str(include_str!("fixtures/render-parity.json")).unwrap();
    assert_eq!(cases.cases.len(), 600);
    let mut wrong = Vec::new();
    for case in &cases.cases {
        let tags: Vec<Tag> = (0..case.words.len())
            .map(|index| {
                Tag::from_labels(
                    &case.actions[index],
                    &case.puncts[index],
                    &case.cases[index],
                    &case.spans[index],
                )
                .unwrap()
            })
            .collect();
        let decode = Decode {
            use_itn: case.use_itn,
            delete_openers: case.delete_openers,
            apply_case_punct: case.apply_case_punct,
            ..Decode::default()
        };
        let words: Vec<&str> = case.words.iter().map(String::as_str).collect();
        let written = render::render(&words, &tags, &decode);
        if written != case.cleaned {
            wrong.push(format!(
                "{words:?}\n  want {:?}\n  got  {written:?}",
                case.cleaned
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of 600 differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_real_models_recorded_logits_decode_and_render_as_the_reference_does() {
    let (cases, decode) = model_cases();
    assert_eq!(cases.cases.len(), 40);
    let with_heads = Decode {
        apply_case_punct: true,
        ..decode
    };
    let mut wrong = Vec::new();
    for case in &cases.cases {
        let tags = decode::tags(&case.word_logits(), &decode);
        let (actions, spans) = ModelCase::labels(&tags);
        let words: Vec<&str> = case.words.iter().map(String::as_str).collect();
        let written = render::render(&words, &tags, &decode);
        let rewritten = render::render(&words, &tags, &with_heads);
        if actions != case.actions
            || spans != case.spans
            || written != case.cleaned
            || rewritten != case.cleaned_case_punct
        {
            wrong.push(format!(
                "{words:?}\n  want {:?}\n  got  {written:?}",
                case.cleaned
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Hex's guard only keeps more than the reference: every word the guarded
/// line has, the reference's line has too.
#[test]
fn the_guard_only_ever_keeps_more_than_the_reference() {
    use super::super::guard::{CleanupGuard, Policy, ProtectedTerms};
    use super::super::words::Words;
    let (cases, decode) = model_cases();
    let terms = ProtectedTerms::default();
    let guard = CleanupGuard {
        policy: Policy::DESKTOP,
        protected: &terms,
    };
    for case in &cases.cases {
        let words = Words::new(case.words.join(" "));
        let mut tags = decode::tags(&case.word_logits(), &decode);
        let before: Vec<bool> = tags.iter().map(|tag| tag.action.is_delete()).collect();
        decode::protect(&words, &mut tags, &guard);
        for (index, tag) in tags.iter().enumerate() {
            assert!(
                before[index] || tag.action == Action::Keep,
                "{:?}",
                case.words
            );
        }
    }
}

/// The model folder's tokenizer against the ids Hugging Face gives the
/// same words, and the ids the model was run on for its fixtures.
#[test]
#[ignore = "needs a v2 model folder (HEX_CLEANUP_V2_DIR or <models>/cleanup-v2)"]
fn the_tokenizer_matches_hugging_face() {
    #[derive(Deserialize)]
    struct Cases {
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        words: Vec<String>,
        input_ids: Vec<i32>,
        first: Vec<usize>,
    }
    let folder = super::runtime::folder().unwrap();
    let json = std::fs::read_to_string(folder.join(super::runtime::TOKENIZER)).unwrap();
    let started = std::time::Instant::now();
    let tokenizer = super::bpe::Bpe::from_json(&json).unwrap();
    println!("tokenizer loaded in {:?}", started.elapsed());
    let tokenized: Cases =
        serde_json::from_str(include_str!("fixtures/tokenizer-parity.json")).unwrap();
    let (modelled, _) = model_cases();
    let cases = tokenized
        .cases
        .iter()
        .map(|case| (&case.words, &case.input_ids, Some(&case.first)));
    let modelled = modelled
        .cases
        .iter()
        .map(|case| (&case.words, &case.input_ids, None));
    let mut checked = 0;
    for (words, ids, first) in cases.chain(modelled) {
        let windows = tokenizer.windows(words, 512);
        assert_eq!(windows.len(), 1, "{words:?}");
        assert_eq!(&windows[0].input_ids, ids, "{words:?}");
        if let Some(first) = first {
            assert_eq!(&windows[0].first, first, "{words:?}");
        }
        checked += 1;
    }
    assert_eq!(checked, 60);
}
