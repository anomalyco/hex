//! The real v2 model through the app's own path: the model folder's
//! package compiled once, loaded on the CPU, its tokenizer read, and each
//! recorded sentence run through Core ML, decoded, and rendered. It needs
//! the model folder, so it runs by hand:
//!
//! ```sh
//! HEX_CLEANUP_V2_DIR=<folder> cargo test --locked --bin hex the_real_v2_model -- --ignored --nocapture
//! ```
//!
//! Without `HEX_CLEANUP_V2_DIR` it reads `<models>/cleanup-v2`.

use std::time::{Duration, Instant};

use super::super::tagger::{compile, load};
use super::decode::{self, WordLogits};
use super::parity_tests::model_cases;
use super::runtime::{self, Warm};
use super::{bpe::Bpe, render};

fn argmax(values: &[f32]) -> usize {
    let mut best = 0;
    for (index, &value) in values.iter().enumerate() {
        if value > values[best] {
            best = index;
        }
    }
    best
}

fn heads(word: &WordLogits) -> [usize; 4] {
    [
        argmax(&word.action),
        argmax(&word.punct),
        argmax(&word.case),
        argmax(&word.span),
    ]
}

/// The recorded sentences, on the reference's tags and lines, each inside
/// 15 ms once the model is warm.
#[test]
#[ignore = "needs a v2 model folder and Core ML"]
fn the_real_v2_model_answers_as_the_reference_recorded() {
    let folder = runtime::folder().unwrap();
    let started = Instant::now();
    let compiled = runtime::compiled(&folder, compile).unwrap();
    let model = load(&compiled).unwrap();
    let tokenizer =
        Bpe::from_json(&std::fs::read_to_string(folder.join(runtime::TOKENIZER)).unwrap()).unwrap();
    let settings = runtime::settings(
        std::fs::read_to_string(folder.join(runtime::RUNTIME))
            .ok()
            .as_deref(),
    )
    .unwrap();
    println!(
        "compiled, loaded, and read in {} ms; keep_bias {} min_edit_prob {}",
        started.elapsed().as_millis(),
        settings.decode.keep_bias,
        settings.decode.min_edit_prob
    );
    let warm = Warm {
        model,
        tokenizer,
        settings,
    };
    let (cases, decode) = model_cases();
    // The first run warms Core ML's own caches.
    runtime::score(&warm, &cases.cases[0].words, runtime::run).unwrap();

    let mut elapsed: Vec<Duration> = Vec::new();
    let (mut words, mut agreeing, mut lines, mut same_lines) = (0, 0, 0, 0);
    let mut max_diff = 0.0_f32;
    for case in &cases.cases {
        let started = Instant::now();
        let scored = runtime::score(&warm, &case.words, runtime::run).unwrap();
        elapsed.push(started.elapsed());
        let recorded = case.word_logits();
        assert_eq!(scored.words.len(), recorded.len());
        for (live, recorded) in scored.words.iter().zip(&recorded) {
            words += 1;
            agreeing += usize::from(heads(live) == heads(recorded));
            for (a, b) in live.action.iter().zip(&recorded.action) {
                max_diff = max_diff.max((a - b).abs());
            }
        }
        let tags = decode::tags(&scored.words, &decode);
        let heard: Vec<&str> = case.words.iter().map(String::as_str).collect();
        let written = render::render(&heard, &tags, &decode);
        lines += 1;
        if written == case.cleaned {
            same_lines += 1;
        } else {
            println!(
                "differs: {:?}\n  want {:?}\n  got  {written:?}",
                case.words, case.cleaned
            );
        }
    }
    elapsed.sort();
    let median = elapsed[elapsed.len() / 2];
    let p95 = elapsed[elapsed.len() * 95 / 100];
    println!(
        "{agreeing} of {words} words agree on all four heads; {same_lines} of {lines} lines the same; \
         action logits within {max_diff:.4}; median {median:?}, p95 {p95:?}, slowest {:?}",
        elapsed[elapsed.len() - 1]
    );
    assert_eq!(agreeing, words);
    assert_eq!(same_lines, lines);
    assert!(median < Duration::from_millis(15), "median {median:?}");

    // A 400-word line, read in one window.
    let long: Vec<String> = vec!["so we, um, ship the build on Thursday"; 50]
        .join(" ")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let started = Instant::now();
    let scored = runtime::score(&warm, &long, runtime::run).unwrap();
    println!("400 words in {:?}", started.elapsed());
    assert_eq!(scored.words.len(), 400);
}
