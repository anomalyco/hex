//! The tap decoder against the Swift it replaces: every case in the fixture
//! `tests/fixtures/tap-parity.jsonl` (written by the iOS test `TapParityExport` with the oracle
//! `ios/HEXMobileTests/TapParity/TypingDecoderOracle.swift`, a copy of `TypingDecoder.swift`),
//! read here with the same inputs, must give the oracle's outputs bit for bit: key geometry
//! from the layout, Learn how I type offsets, `decide`, `merge`, `replaces` and the model's
//! predictions.
//!
//! Where Swift walks a dictionary, exact cost ties come out in an order that changes from run
//! to run. The oracle ran each case in the port's fixed order (`fixed`, which must match), the
//! reverse of it and Swift's own (`swift`, kept where it differed); a case whose result
//! depends on that order is `tie`. A larger set (see the exporter) runs with
//! `HEX_TAP_PARITY=/path/to/set.jsonl`.
//!
//! Regenerate the fixture (from `ios/`, after `xcodegen generate`):
//!
//! ```sh
//! TEST_RUNNER_HEX_TAP_PARITY_OUT=$PWD/../crates/keyboard/core/tests/fixtures/tap-parity.jsonl \
//! TEST_RUNNER_HEX_TAP_PARITY_COUNT=300 xcodebuild test -project HEXMobile.xcodeproj \
//!   -scheme HEXMobile -destination "id=<SIM_UDID>" SWIFT_OPTIMIZATION_LEVEL=-O \
//!   -only-testing:HEXMobileTests/TapParityExport
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use hex_keyboard_core::decoder::{
    Decision, KeyStats, Keys, LayoutKey, Rect, Split, TypingDecoder, UserWords, offsets,
};
use hex_keyboard_core::model::Model;
use hex_keyboard_core::never_suggest::NeverSuggest;
use serde_json::Value;

fn f(v: &Value) -> f64 {
    let s = v.as_str().expect("a bit pattern");
    f64::from_bits(u64::from_str_radix(s, 16).expect("hex bits"))
}

fn point(v: &Value) -> (f64, f64) {
    (f(&v[0]), f(&v[1]))
}

fn points(v: &Value) -> Vec<(f64, f64)> {
    v.as_array().expect("points").iter().map(point).collect()
}

fn text(v: &Value) -> Option<&str> {
    v.as_str()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("strings")
        .iter()
        .map(|s| s.as_str().expect("a string").to_owned())
        .collect()
}

fn rect(v: &Value) -> Option<Rect> {
    let a = v.as_array()?;
    Some(Rect {
        x: f(&a[0]),
        y: f(&a[1]),
        width: f(&a[2]),
        height: f(&a[3]),
    })
}

fn keys(v: &Value) -> Keys {
    Keys {
        centres: v["centres"]
            .as_array()
            .expect("centres")
            .iter()
            .map(|c| {
                let letter = c[0]
                    .as_str()
                    .expect("letter")
                    .chars()
                    .next()
                    .expect("a letter");
                (letter, f(&c[1]), f(&c[2]))
            })
            .collect(),
        pitch: point(&v["pitch"]),
        bottom_row_y: f(&v["bottom"]),
        space: rect(&v["space"]),
    }
}

/// Keys compared bit for bit, centres by letter.
fn same_keys(a: &Keys, b: &Keys) -> bool {
    let by_letter = |k: &Keys| -> BTreeMap<char, (u64, u64)> {
        k.centres
            .iter()
            .map(|&(c, x, y)| (c, (x.to_bits(), y.to_bits())))
            .collect()
    };
    let rect_bits = |r: Option<Rect>| {
        r.map(|r| {
            [
                r.x.to_bits(),
                r.y.to_bits(),
                r.width.to_bits(),
                r.height.to_bits(),
            ]
        })
    };
    by_letter(a) == by_letter(b)
        && a.pitch.0.to_bits() == b.pitch.0.to_bits()
        && a.pitch.1.to_bits() == b.pitch.1.to_bits()
        && a.bottom_row_y.to_bits() == b.bottom_row_y.to_bits()
        && rect_bits(a.space) == rect_bits(b.space)
}

/// A decision as the fixture writes it, costs as bits.
fn decision_json(d: &Option<Decision>) -> Value {
    match d {
        None => Value::Null,
        Some(d) => serde_json::json!({
            "replacement": d.replacement,
            "readings": d.readings.iter()
                .map(|r| serde_json::json!([r.text, format!("{:x}", r.cost.to_bits())]))
                .collect::<Vec<_>>(),
            "literal": format!("{:x}", d.literal_cost.to_bits()),
        }),
    }
}

fn bundled(language: &str) -> Arc<Model> {
    let path = format!(
        "{}/../../../ios/HEXKeyboard/TypingModel/{language}.hextm",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    Arc::new(Model::new(Box::new(bytes)).expect("a valid model"))
}

struct Language {
    model: Arc<Model>,
    /// Never-suggest lists by Suggest swear words.
    blocked: [Vec<String>; 2],
    clean: HashSet<String>,
    users: Vec<Vec<String>>,
    /// (decoder's keys, the layout's own)
    keysets: HashMap<u64, (Keys, Keys)>,
    decoders: HashMap<(u64, bool), TypingDecoder>,
}

impl Language {
    fn decoder(&mut self, user: u64, swearing: bool) -> &TypingDecoder {
        let (model, blocked, users) = (&self.model, &self.blocked, &self.users);
        self.decoders.entry((user, swearing)).or_insert_with(|| {
            let mut d = TypingDecoder::new(model.clone(), None);
            d.user_words = UserWords::new(&users[user as usize], model);
            d.never_suggest = NeverSuggest::new(
                blocked[usize::from(swearing)].iter().map(String::as_str),
                model,
            );
            d
        })
    }
}

#[derive(Default)]
struct Tally {
    cases: usize,
    exact: usize,
    /// Cases whose result depends on the order of exact ties.
    tie: usize,
    /// Of those, cases where Swift's own order gave another result than the fixed rule.
    swift_differs: usize,
    by_kind: BTreeMap<String, usize>,
    decide_nanos: Vec<u128>,
    failures: Vec<String>,
}

fn check_keys(line: &Value) -> Result<(Keys, Keys), String> {
    let layout = &line["layout"];
    let layout_keys: Vec<LayoutKey> = layout["keys"]
        .as_array()
        .expect("keys")
        .iter()
        .map(|k| LayoutKey {
            character: k["c"].as_str().map(str::to_owned),
            space: k["space"].as_bool().expect("space"),
            frame: rect(&k["frame"]).expect("frame"),
        })
        .collect();
    let split = layout["split"].as_array().map(|s| Split {
        middle: f(&s[0]),
        shift: f(&s[1]),
    });
    let stats: HashMap<String, KeyStats> = line["stats"]
        .as_object()
        .expect("stats")
        .iter()
        .map(|(c, s)| {
            let stats = KeyStats {
                count: s[0].as_u64().expect("count"),
                dx: f(&s[1]),
                dy: f(&s[2]),
            };
            (c.clone(), stats)
        })
        .collect();
    let ours = offsets(&layout_keys, &stats);
    let theirs: HashMap<char, (u64, u64)> = line["offsets"]
        .as_object()
        .expect("offsets")
        .iter()
        .map(|(c, v)| {
            let c = c.chars().next().expect("a letter");
            (c, (f(&v[0]).to_bits(), f(&v[1]).to_bits()))
        })
        .collect();
    let ours_bits: HashMap<char, (u64, u64)> = ours
        .iter()
        .map(|(&c, &(x, y))| (c, (x.to_bits(), y.to_bits())))
        .collect();
    if ours_bits != theirs {
        return Err(format!("offsets differ: {ours:?}"));
    }
    let made = Keys::from_layout(&layout_keys, split, point(&layout["pitch"]), &ours);
    let expected = keys(&line["out"]);
    if !same_keys(&made, &expected) {
        return Err(format!("keys differ: {made:?} vs {expected:?}"));
    }
    let truth = Keys::from_layout(
        &layout_keys,
        split,
        point(&layout["pitch"]),
        &HashMap::new(),
    );
    let expected_truth = keys(&line["truth"]);
    if !same_keys(&truth, &expected_truth) {
        return Err(format!(
            "unshifted keys differ: {truth:?} vs {expected_truth:?}"
        ));
    }
    Ok((expected, expected_truth))
}

/// The case's result as the fixture writes it.
fn run_case(language: &mut Language, line: &Value, tally: &mut Tally) -> Value {
    let kind = line["kind"].as_str().expect("kind");
    if kind == "model" {
        return model_case(language, line);
    }
    let set = line["keys"].as_u64().expect("keys");
    let user = line["user"].as_u64().expect("user");
    let swearing = line["swear"].as_bool().expect("swear");
    let (keys, truth) = language.keysets[&set].clone();
    let decoder = language.decoder(user, swearing);
    let previous = text(&line["previous"]);
    match kind {
        "decide" => {
            let taps = points(&line["taps"]);
            let typed = line["typed"].as_str().expect("typed");
            let valid = line["valid"].as_bool().expect("valid");
            let start = Instant::now();
            let d = decoder.decide(&taps, typed, previous, valid, &keys);
            tally.decide_nanos.push(start.elapsed().as_nanos());
            decision_json(&d)
        }
        "merge" => {
            let merged = decoder.merge(
                line["first"].as_str().expect("first"),
                &points(&line["firstTaps"]),
                point(&line["space"]),
                line["second"].as_str().expect("second"),
                &points(&line["secondTaps"]),
                previous,
                line["firstValid"].as_bool().expect("firstValid"),
                line["secondValid"].as_bool().expect("secondValid"),
                &keys,
            );
            serde_json::json!(merged)
        }
        "replaces" => {
            let keys = if line["truth"].as_bool().expect("truth") {
                &truth
            } else {
                &keys
            };
            let replaces = decoder.replaces(
                line["reading"].as_str().expect("reading"),
                &points(&line["taps"]),
                line["typed"].as_str().expect("typed"),
                previous,
                line["valid"].as_bool().expect("valid"),
                keys,
            );
            serde_json::json!(replaces)
        }
        other => panic!("unknown case {other}"),
    }
}

fn model_case(language: &Language, line: &Value) -> Value {
    let model = &language.model;
    let word = line["word"].as_str().expect("word");
    let clean = &language.clean;
    let allow_clean = line["clean"].as_bool().unwrap_or(false);
    let allowing = |w: &str| !allow_clean || !clean.contains(w);
    match line["fn"].as_str().expect("fn") {
        "cost" => match model.cost(word, text(&line["previous"])) {
            Some(c) => serde_json::json!(format!("{:x}", c.to_bits())),
            None => Value::Null,
        },
        "hasPair" => {
            serde_json::json!(model.has_pair(word, line["second"].as_str().expect("second")))
        }
        "nextWords" => serde_json::json!(model.next_words(word, allowing)),
        "completions" => {
            serde_json::json!(model.completions(word, text(&line["previous"]), allowing))
        }
        "continuations" => match model
            .encode(word)
            .and_then(|codes| model.continuations(&codes))
        {
            Some(mass) => serde_json::json!(
                mass.iter()
                    .enumerate()
                    .filter(|(_, m)| **m != 0.0)
                    .map(|(code, m)| serde_json::json!([code, format!("{:x}", m.to_bits())]))
                    .collect::<Vec<_>>()
            ),
            None => Value::Null,
        },
        other => panic!("unknown model function {other}"),
    }
}

fn replay(path: &str) -> BTreeMap<String, Tally> {
    let content = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut languages: HashMap<String, Language> = HashMap::new();
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    for (n, raw) in content.lines().enumerate() {
        let line: Value =
            serde_json::from_str(raw).unwrap_or_else(|e| panic!("line {}: {e}", n + 1));
        let lang = line["lang"].as_str().expect("lang").to_owned();
        let tally = tallies.entry(lang.clone()).or_default();
        match line["kind"].as_str().expect("kind") {
            "setup" => {
                let model = bundled(&lang);
                let clean = strings(&line["blocked"]["clean"]);
                languages.insert(
                    lang,
                    Language {
                        model,
                        clean: clean.iter().cloned().collect(),
                        blocked: [clean, strings(&line["blocked"]["swearing"])],
                        users: line["users"]
                            .as_array()
                            .expect("users")
                            .iter()
                            .map(strings)
                            .collect(),
                        keysets: HashMap::new(),
                        decoders: HashMap::new(),
                    },
                );
            }
            "keys" => {
                tally.cases += 1;
                *tally.by_kind.entry("keys".into()).or_default() += 1;
                match check_keys(&line) {
                    Ok(sets) => {
                        tally.exact += 1;
                        let language = languages.get_mut(&lang).expect("setup first");
                        language
                            .keysets
                            .insert(line["id"].as_u64().expect("id"), sets);
                    }
                    Err(e) => tally.failures.push(format!("line {}: {e}", n + 1)),
                }
            }
            kind => {
                let language = languages.get_mut(&lang).expect("setup first");
                tally.cases += 1;
                *tally.by_kind.entry(kind.into()).or_default() += 1;
                let ours = run_case(language, &line, tally);
                let expected = if kind == "model" {
                    &line["out"]
                } else {
                    &line["fixed"]
                };
                let tie = line["tie"].as_bool().unwrap_or(false);
                if tie {
                    tally.tie += 1;
                }
                if line.get("swift").is_some() {
                    tally.swift_differs += 1;
                }
                if &ours == expected {
                    tally.exact += 1;
                } else if tally.failures.len() < 20 {
                    tally.failures.push(format!(
                        "line {} ({kind}{}): ours {ours}, oracle {expected}, input {raw}",
                        n + 1,
                        if tie { ", tie" } else { "" }
                    ));
                } else {
                    tally.failures.push(String::new());
                }
            }
        }
    }
    tallies
}

fn report(path: &str, tallies: &BTreeMap<String, Tally>) {
    let mut failures = 0;
    for (lang, t) in tallies {
        let mut nanos = t.decide_nanos.clone();
        nanos.sort_unstable();
        let at = |q: f64| {
            nanos
                .get(((q * nanos.len() as f64) as usize).min(nanos.len().saturating_sub(1)))
                .map_or(0.0, |&n| n as f64 / 1000.0)
        };
        let mean = nanos.iter().sum::<u128>() as f64 / nanos.len().max(1) as f64 / 1000.0;
        println!(
            "tap parity {path} {lang}: {} cases {:?}, {} exact, {} tie-affected ({:.2}%), {} where Swift's order differed, {} other differences; decide µs median {:.1} p95 {:.1} mean {:.1}",
            t.cases,
            t.by_kind,
            t.exact,
            t.tie,
            100.0 * t.tie as f64 / t.cases.max(1) as f64,
            t.swift_differs,
            t.cases - t.exact,
            at(0.5),
            at(0.95),
            mean,
        );
        for f in t.failures.iter().filter(|f| !f.is_empty()) {
            println!("  {f}");
        }
        failures += t.cases - t.exact;
    }
    assert_eq!(
        failures, 0,
        "the Rust decoder differs from the Swift oracle"
    );
}

#[test]
fn matches_the_swift_oracle() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tap-parity.jsonl"
    );
    let tallies = replay(path);
    for lang in ["en", "es", "de"] {
        assert!(
            tallies.get(lang).is_some_and(|t| t.cases > 100),
            "{lang} has cases"
        );
    }
    report(path, &tallies);
}

#[test]
fn matches_the_swift_oracle_on_a_large_set() {
    let Some(path) = std::env::var_os("HEX_TAP_PARITY") else {
        return;
    };
    let path = path.to_string_lossy();
    let tallies = replay(&path);
    report(&path, &tallies);
}
