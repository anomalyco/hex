//! The real tagger through the app's own path: the pinned archive fetched
//! over `net`, checked, unpacked, compiled, loaded on the CPU, and run
//! through the stage. It needs the network and Core ML, so it runs by hand:
//!
//! ```sh
//! cargo test --locked --bin hex the_real_tagger -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::{Duration, Instant};

use color_eyre::eyre::eyre;
use serde::Deserialize;

use super::archive::tests::Scratch;
use super::guard::ProtectedTerms;
use super::stage::{Skip, TIME_BUDGET, clean_with, tokenizer};
use super::tagger::{Loaded, Runtime, Tagger, compile, load, run};
use super::{Words, model};

const HANDWRITTEN: &str =
    include_str!("../../../ios/HEXMobileTests/Fixtures/cleanup-tagger-handwritten.json");

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    input: String,
    /// The hand-written ideal cleanup.
    target: String,
}

/// A compiled model wherever a test put it.
#[derive(Clone)]
struct CompiledAt(PathBuf);

impl Runtime for CompiledAt {
    type Model = Loaded;

    fn load(&self) -> Result<Loaded, Skip> {
        load(&self.0).map_err(Skip::Failed)
    }

    fn logits(model: &Loaded, input_ids: &[i32], length: usize) -> Result<Vec<f32>, String> {
        run(model, input_ids, length)
    }

    fn idle(&self) -> Option<Duration> {
        None
    }
}

/// The iPhone app's own hand-written sentences, on target as often as the
/// model's recorded probabilities are (`parity_tests`), and every answer
/// inside the time budget once the model is loaded.
#[test]
#[ignore = "downloads the 26 MB model and runs Core ML"]
fn the_real_tagger_cleans_the_handwritten_sentences_as_the_iphone_app_does() {
    let scratch = Scratch::new("real-model");
    let folder = scratch.0.join("cleanup");
    let started = Instant::now();
    let compiled = model::install(
        &model::TAGGER,
        &folder,
        |partial| model::fetch(&model::TAGGER, partial),
        |package, compiled| compile(package, compiled).map_err(|error| eyre!(error)),
    )
    .unwrap();
    println!("installed in {} ms", started.elapsed().as_millis());
    assert_eq!(
        model::installed(&model::TAGGER, &folder),
        Some(compiled.clone())
    );

    let tagger = Tagger::new(CompiledAt(compiled));
    let protected = ProtectedTerms::default();
    // The first answer waits for the load, as a capture's start does.
    let started = Instant::now();
    let (warmed, observation) = clean_with(
        "Um, so we go",
        &protected,
        tokenizer(),
        Duration::from_secs(60),
        |chunks, deadline| tagger.score(chunks, deadline),
    );
    println!(
        "loaded and first run in {} ms",
        started.elapsed().as_millis()
    );
    assert_eq!(observation.unwrap().fallback, None);
    assert_eq!(warmed, "So we go");

    let cases: Cases = serde_json::from_str(HANDWRITTEN).unwrap();
    assert_eq!(cases.cases.len(), 137);
    let mut on_target = 0;
    let mut elapsed = Vec::new();
    for case in &cases.cases {
        let started = Instant::now();
        let (text, observation) = clean_with(
            &case.input,
            &protected,
            tokenizer(),
            TIME_BUDGET,
            |chunks, deadline| tagger.score(chunks, deadline),
        );
        elapsed.push(started.elapsed());
        assert_eq!(observation.unwrap().fallback, None, "{}", case.input);
        // Nothing but words that were heard, in their order.
        let mut heard = case.input.split_whitespace().map(bare);
        for word in text.split_whitespace().map(bare) {
            assert!(
                heard.any(|said| said == word),
                "{text:?} from {:?}",
                case.input
            );
        }
        on_target += usize::from(text == case.target);
    }
    elapsed.sort();
    println!(
        "{on_target} of {} on target; median {:?}, slowest {:?}",
        cases.cases.len(),
        elapsed[elapsed.len() / 2],
        elapsed[elapsed.len() - 1]
    );
    assert_eq!(on_target, 129);

    // A 400-word dictation, four model runs, well inside the budget.
    let long = vec!["so we, um, ship the build on Thursday"; 50].join(" ");
    assert_eq!(Words::new(long.as_str()).len(), 400);
    let started = Instant::now();
    let (text, observation) = clean_with(
        &long,
        &protected,
        tokenizer(),
        TIME_BUDGET,
        |chunks, deadline| tagger.score(chunks, deadline),
    );
    println!("400 words in {:?}", started.elapsed());
    assert_eq!(observation.unwrap().fallback, None);
    assert!(!text.contains("um"), "{text}");
    tagger.release();
}

/// A word without the punctuation and capital the renderer may change.
fn bare(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}
