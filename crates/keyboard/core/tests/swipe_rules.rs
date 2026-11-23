//! What a swipe types (`swipe::typing`) is what the Swift rules it was ported from typed: the
//! space before the word, the word cased, the strip's alternatives, the readings' order with
//! rare names, the name test, the space rule and the slid tap, for every case of the committed
//! `tests/fixtures/swipe-rules.jsonl`. The cases and outcomes come from iOS's
//! `SwipeRulesFixtureTests`, proven alike to the Swift rules in one process
//! (`ios/HEXMobileTests/SwipeParity`); the dictionary is each profile's list of words.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use hex_keyboard_core::model::Model;
use hex_keyboard_core::never_suggest::NeverSuggest;
use hex_keyboard_core::swipe::typing::{self, AutoCapitalization, Shift, SwipeContext, SwipeWords};
use hex_keyboard_core::swipe::{Candidate, Parameters, SwipeDecoder};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

#[test]
fn types_every_swipe_as_the_swift_rules() {
    let path = format!(
        "{}/tests/fixtures/swipe-rules.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut profiles: HashMap<u64, (SwipeWords, HashSet<String>)> = HashMap::new();
    let mut decoders: HashMap<String, SwipeDecoder> = HashMap::new();
    let mut kinds: HashMap<String, usize> = HashMap::new();
    let mut differences: Vec<String> = Vec::new();
    for line in text.lines() {
        let o: Value = serde_json::from_str(line).expect("a JSON line");
        let kind = o["kind"].as_str().unwrap();
        // Swift's `Set<String>` finds a word by canonical equivalence.
        let valid = |profile: &(SwipeWords, HashSet<String>), word: &str| {
            profile.1.contains(&word.nfc().collect::<String>())
        };
        let got: String = match kind {
            "header" => {
                assert_eq!(o["generator"], 1, "a generator this test reads");
                continue;
            }
            "profile" => {
                let words = SwipeWords::new(
                    o["language"].as_str().unwrap(),
                    o["hasDictionary"].as_bool().unwrap(),
                    o["replacements"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| (string(&r[0]), string(&r[1])))
                        .collect(),
                    strings(&o["own"]),
                    strings(&o["vocabulary"]),
                );
                let valid = strings(&o["valid"])
                    .iter()
                    .map(|w| w.nfc().collect())
                    .collect();
                profiles.insert(o["id"].as_u64().unwrap(), (words, valid));
                continue;
            }
            "typed" => {
                let profile = &profiles[&o["profile"].as_u64().unwrap()];
                let s = &o["state"];
                let before = s["before"].as_str();
                let context = SwipeContext {
                    before,
                    shift: [Shift::Off, Shift::On, Shift::Locked]
                        [s["shift"].as_u64().unwrap() as usize],
                    shift_held: s["shiftHeld"].as_bool().unwrap(),
                    auto_capitalization: [
                        AutoCapitalization::None,
                        AutoCapitalization::Words,
                        AutoCapitalization::Sentences,
                        AutoCapitalization::AllCharacters,
                    ][s["autoCapitalization"].as_u64().unwrap() as usize],
                    auto_capitalizes: s["autoCapitalizes"].as_bool().unwrap(),
                    autocorrection: s["autocorrection"].as_bool().unwrap(),
                };
                let readings = strings(&o["readings"]);
                let readings: Vec<&str> = readings.iter().map(String::as_str).collect();
                let typed =
                    typing::typed(&readings, &context, &profile.0, &mut |w| valid(profile, w))
                        .expect("a reading");
                serde_json::json!({"space": typed.space, "word": typed.word, "alternatives": typed.alternatives})
                    .to_string()
            }
            "name" => {
                let profile = &profiles[&o["profile"].as_u64().unwrap()];
                let word = o["word"].as_str().unwrap();
                profile
                    .0
                    .reads_as_name(word, &mut |w| valid(profile, w))
                    .to_string()
            }
            "space" => typing::needs_space(o["before"].as_str()).to_string(),
            "rank" => {
                let profile = &profiles[&o["profile"].as_u64().unwrap()];
                let language = o["language"].as_str().unwrap().to_owned();
                let decoder = decoders
                    .entry(language.clone())
                    .or_insert_with(|| decoder(&language));
                let candidates: Vec<Candidate> = o["candidates"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| Candidate {
                        text: string(&c[0]),
                        cost: f64::from_bits(
                            u64::from_str_radix(c[1].as_str().unwrap(), 16).unwrap(),
                        ),
                    })
                    .collect();
                serde_json::json!(
                    decoder.ranked(&candidates, &profile.0, &mut |w| valid(profile, w))
                )
                .to_string()
            }
            "slid" => {
                let points: Vec<(f64, f64)> = strings(&o["points"])
                    .iter()
                    .map(|p| {
                        let (x, y) = p.split_once(',').unwrap();
                        (x.parse().unwrap(), y.parse().unwrap())
                    })
                    .collect();
                let pitch: f64 = o["pitch"].as_str().unwrap().parse().unwrap();
                typing::is_slid_tap(&points, pitch, o["key"].as_str(), o["first"].as_str())
                    .to_string()
            }
            "ends" => typing::starts_and_ends_with(
                o["word"].as_str().unwrap(),
                o["letter"].as_str().unwrap(),
            )
            .to_string(),
            other => panic!("{path}: unknown line kind {other}"),
        };
        *kinds.entry(kind.to_owned()).or_default() += 1;
        // Compared as JSON values, so key order and spacing don't count.
        if serde_json::from_str::<Value>(&got).ok().as_ref() != Some(&o["out"]) {
            differences.push(format!("{}: read {got}", &line[..line.len().min(200)]));
        }
    }
    let mut counts: Vec<_> = kinds.iter().collect();
    counts.sort();
    println!(
        "{path}: cases {counts:?}, differences {}",
        differences.len()
    );
    for d in differences.iter().take(20) {
        println!("  {d}");
    }
    assert!(!kinds.is_empty(), "{path} has no cases");
    assert!(
        differences.is_empty(),
        "{} cases type differently",
        differences.len()
    );
}

fn decoder(language: &str) -> SwipeDecoder {
    let path = format!(
        "{}/../../../ios/HEXKeyboard/TypingModel/{language}.hextm",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let model = Arc::new(Model::new(Box::new(bytes)).expect("a valid model"));
    SwipeDecoder::new(
        model,
        Parameters::default(),
        NeverSuggest::default(),
        Vec::new(),
    )
}

fn string(v: &Value) -> String {
    v.as_str().unwrap().to_owned()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(string).collect()
}
