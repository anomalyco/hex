//! Autocorrect against the Swift it replaces: every case in `tests/fixtures/autocorrect-parity.jsonl`
//! (written by the iOS test `AutocorrectParityExport` with the oracle
//! `ios/HEXMobileTests/AutocorrectParity/ACOracle*.swift`, a renamed copy of
//! `AutocorrectEngine*.swift`) replayed here with the same operations and the same deterministic
//! dictionary (`stub.rs`) must give the oracle's answers, the engine's state after each one, and
//! the same dictionary calls in the same order.
//!
//! The fixture's `out` is the oracle with your own words in the port's order (by shortcut and
//! typo) and the decoder's fixed tie order; `today` (where it differs) is Swift's dictionary
//! order, `tie` marks a case Swift's own decoder walk would answer otherwise, and `shared` the
//! answers with the shared never-suggest list read by base language, which the port reads when
//! handed it. A larger set runs with `HEX_AUTOCORRECT_PARITY=/path/to/set.jsonl`.
//!
//! Regenerate the fixture (from `ios/`, after `xcodegen generate`):
//!
//! ```sh
//! TEST_RUNNER_HEX_AUTOCORRECT_PARITY_OUT=$PWD/../crates/keyboard/core/tests/fixtures/autocorrect-parity.jsonl \
//! TEST_RUNNER_HEX_AUTOCORRECT_PARITY_COUNT=200 xcodebuild test -project HEXMobile.xcodeproj \
//!   -scheme HEXMobile -destination "id=<SIM_UDID>" SWIFT_OPTIMIZATION_LEVEL=-O \
//!   -only-testing:HEXMobileTests/AutocorrectParityExport
//! ```

mod chars;
mod ops;
mod stub;

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::rc::Rc;
use std::sync::Arc;

use hex_keyboard_core::autocorrect::GermanCapitals;
use hex_keyboard_core::decoder::{Keys, Rect};
use hex_keyboard_core::model::{Model, SortedWords};
use serde_json::Value;

use ops::World;
use stub::{StubDictionary, fnv};

fn repo(path: &str) -> String {
    format!("{}/../../../{path}", env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn bits(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().expect("bits"), 16).expect("hex"))
}

fn keys(v: &Value) -> Keys {
    let rect = v["space"].as_array().map(|a| Rect {
        x: bits(&a[0]),
        y: bits(&a[1]),
        width: bits(&a[2]),
        height: bits(&a[3]),
    });
    Keys {
        centres: v["centres"]
            .as_array()
            .expect("centres")
            .iter()
            .map(|c| {
                let letter = c[0].as_str().unwrap().chars().next().unwrap();
                (letter, bits(&c[1]), bits(&c[2]))
            })
            .collect(),
        pitch: (bits(&v["pitch"][0]), bits(&v["pitch"][1])),
        bottom_row_y: bits(&v["bottom"]),
        space: rect,
    }
}

fn model(code: &str) -> Arc<Model> {
    let bytes =
        std::fs::read(repo(&format!("ios/HEXKeyboard/TypingModel/{code}.hextm"))).expect("a model");
    Arc::new(Model::new(Box::new(bytes)).expect("a valid model"))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_owned()).collect())
        .unwrap_or_default()
}

/// The stub dictionaries from the setup line: the models' commonest words less one in
/// `dropEvery` by hash, plus extras; the short lists; the names.
fn dictionaries(
    setup: &Value,
    models: &HashMap<String, Arc<Model>>,
) -> HashMap<String, StubDictionary> {
    let count = setup["modelWords"].as_u64().unwrap() as usize;
    let drop = setup["dropEvery"].as_u64().unwrap();
    let names = |code: &str| strings(&setup["names"][code]);
    let mut out = HashMap::new();
    for (code, model) in models {
        let mut order: Vec<usize> = (0..model.word_count()).collect();
        order.sort_by(|&a, &b| {
            model
                .unigram_cost(a)
                .partial_cmp(&model.unigram_cost(b))
                .unwrap()
                .then(a.cmp(&b))
        });
        let mut words: Vec<String> = order
            .into_iter()
            .take(count)
            .map(|i| model.word(i))
            .filter(|w| !fnv(w.as_bytes()).is_multiple_of(drop))
            .collect();
        words.extend(strings(&setup["extras"][code.as_str()]));
        out.insert(code.clone(), StubDictionary::new(&words, &names(code)));
    }
    for (code, list) in setup["lists"].as_object().unwrap() {
        out.insert(
            code.clone(),
            StubDictionary::new(&strings(list), &names(code)),
        );
    }
    out
}

#[derive(Default)]
struct Tally {
    cases: usize,
    ops: usize,
    calls: usize,
    failures: usize,
    by_kind: BTreeMap<String, usize>,
    today: usize,
    tie: usize,
    examples: Vec<String>,
}

fn compare(
    world: &World,
    tsv: &str,
    case: &Value,
    expected: &[Value],
    calls: &str,
    tally: &mut Tally,
) -> bool {
    let ops = case["ops"].as_array().unwrap();
    let learned = strings(&case["learned"]);
    let run = world.run(tsv, ops, expected, &learned, false);
    let mut differs = format!("{:x}", run.hash) != calls;
    let mut first = None;
    let mut want_state = Value::Null;
    for (i, (got, want)) in run.outputs.iter().zip(expected).enumerate() {
        // The fixture writes the state only when it changed.
        if let Some(state) = want.get("s") {
            want_state = state.clone();
        }
        if got["r"] != want["r"] || got["s"] != want_state {
            differs = true;
            first.get_or_insert(i);
        }
    }
    if differs {
        let at = first.map_or("calls".to_owned(), |i| {
            ops[i]["op"].as_str().unwrap().to_owned()
        });
        *tally.by_kind.entry(at.clone()).or_default() += 1;
        if tally.examples.len() < 12 {
            let detail = match first {
                Some(i) => format!(
                    "op {i} {}\n    rust  {}\n    swift {}",
                    ops[i],
                    serde_json::to_string(&run.outputs[i]).unwrap(),
                    serde_json::to_string(&expected[i]).unwrap()
                ),
                None => {
                    let lines = world
                        .run(tsv, ops, expected, &learned, true)
                        .lines
                        .unwrap_or_default();
                    let swift = strings(&case["log"]);
                    let at = lines
                        .iter()
                        .zip(&swift)
                        .position(|(a, b)| a != b)
                        .unwrap_or(lines.len().min(swift.len()));
                    format!(
                        "calls {} vs {} ({} vs {} calls){}",
                        format_args!("{:x}", run.hash),
                        calls,
                        run.calls,
                        case["ncalls"],
                        if swift.is_empty() {
                            String::new()
                        } else {
                            format!(
                                "\n    first differing call {at}: rust {:?} swift {:?}",
                                lines.get(at),
                                swift.get(at)
                            )
                        }
                    )
                }
            };
            tally
                .examples
                .push(format!("case {} ({}): {detail}", case["id"], case["lang"]));
        }
    }
    tally.ops += ops.len();
    tally.calls += run.calls;
    differs
}

fn check(path: &str) {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let ios = read("ios/HEXShared/FullKeyboard/never-suggest.tsv");
    let models: HashMap<String, Arc<Model>> = ["en", "es", "de"]
        .into_iter()
        .map(|c| (c.to_owned(), model(c)))
        .collect();
    let german =
        GermanCapitals::parse(&read("ios/HEXKeyboard/TypingModel/de.capitals")).map(Arc::new);
    let mut world: Option<World> = None;
    let mut keysets = HashMap::new();
    let mut tally = Tally::default();
    let mut char_failures = Vec::new();
    let mut char_known = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        match v["kind"].as_str().unwrap() {
            "setup" => {
                world = Some(World {
                    dictionaries: Rc::new(dictionaries(&v, &models)),
                    models: models.clone(),
                    keysets: HashMap::new(),
                    german: german.clone(),
                    available: strings(&v["available"]),
                });
            }
            "chars" => (char_failures, char_known) = chars::check(&v),
            "keys" => {
                keysets.insert(v["id"].as_u64().unwrap(), keys(&v["keys"]));
            }
            "case" => {
                let world = world.as_mut().expect("a setup line first");
                if world.keysets.len() != keysets.len() {
                    world.keysets = keysets.clone();
                }
                tally.cases += 1;
                let out = v["out"].as_array().unwrap();
                let calls = v["calls"].as_str().unwrap();
                if v.get("tie").is_some() {
                    tally.tie += 1;
                }
                // Your own words reach the decoders in the order the host passes them: a case
                // Swift's dictionary order decides is left out.
                if v.get("today").is_some() {
                    tally.today += 1;
                    continue;
                }
                if compare(world, &ios, &v, out, calls, &mut tally) {
                    tally.failures += 1;
                }
            }
            other => panic!("unknown line {other}"),
        }
    }
    println!(
        "{path}: {} cases, {} operations, {} dictionary calls; {} differ; {} left out (own words \
         in Swift's dictionary order decide them), {} with Swift's decoder tie order",
        tally.cases, tally.ops, tally.calls, tally.failures, tally.today, tally.tie
    );
    for (kind, n) in &tally.by_kind {
        println!("  first difference at {kind}: {n}");
    }
    for e in &tally.examples {
        println!("  {e}");
    }
    for e in char_failures.iter().take(40) {
        println!("  chars: {e}");
    }
    for e in &char_known {
        println!("  chars (known, Foundation): {e}");
    }
    assert!(tally.cases > 0, "no cases in {path}");
    assert!(
        char_failures.is_empty(),
        "{} character answers differ",
        char_failures.len()
    );
    assert_eq!(tally.failures, 0, "cases differ from the Swift oracle");
}

#[test]
fn matches_the_swift_oracle() {
    check(&format!(
        "{}/tests/fixtures/autocorrect-parity.jsonl",
        env!("CARGO_MANIFEST_DIR")
    ));
}

#[test]
fn matches_the_swift_oracle_on_a_large_set() {
    let Some(path) = std::env::var_os("HEX_AUTOCORRECT_PARITY") else {
        eprintln!("set HEX_AUTOCORRECT_PARITY=/path/to/set.jsonl to run the large set");
        return;
    };
    check(&path.to_string_lossy());
}
