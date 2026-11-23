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
use std::io::{BufRead, BufReader, Write};
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
    today_examples: Vec<String>,
    foundation: usize,
    foundation_examples: Vec<String>,
    shared: usize,
    shared_failures: usize,
    shared_examples: Vec<String>,
}

/// JSON with everything past ASCII escaped, so composed and decomposed letters show.
fn escaped(v: &Value) -> String {
    serde_json::to_string(v)
        .unwrap()
        .chars()
        .map(|c| {
            if c.is_ascii() {
                c.to_string()
            } else {
                c.escape_unicode().to_string()
            }
        })
        .collect()
}

/// The first answer a decided change makes otherwise: Swift's (`today`) against the port's.
fn order_example(case: &Value, today: &Value, out: &Value) -> String {
    let ops = case["ops"].as_array().unwrap();
    let (today, out) = (today.as_array().unwrap(), out.as_array().unwrap());
    let i = (0..out.len())
        .find(|&i| today[i]["r"] != out[i]["r"])
        .unwrap_or(0);
    format!(
        "case {} ({}): op {i} {}\n    before {}\n    now    {}",
        case["id"], case["lang"], ops[i], today[i]["r"], out[i]["r"]
    )
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
    let run = world.run(tsv, ops, expected, &learned, &case["grid0"], false);
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
                Some(i) => {
                    let swift = strings(&case["log"]);
                    let calls = if swift.is_empty() {
                        String::new()
                    } else {
                        let lines = world
                            .run(tsv, ops, expected, &learned, &case["grid0"], true)
                            .lines
                            .unwrap_or_default();
                        let at = lines
                            .iter()
                            .zip(&swift)
                            .position(|(a, b)| a != b)
                            .unwrap_or(0);
                        format!(
                            "\n    first differing call {at}: rust {:?} swift {:?}",
                            lines.get(at),
                            swift.get(at)
                        )
                    };
                    format!(
                        "op {i} {}\n    rust  {}\n    swift {}{calls}",
                        ops[i],
                        escaped(&run.outputs[i]),
                        escaped(&expected[i])
                    )
                }
                None => {
                    let lines = world
                        .run(tsv, ops, expected, &learned, &case["grid0"], true)
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
    let shared = read("android/app/src/main/assets/typing/never-suggest.tsv");
    let models: HashMap<String, Arc<Model>> = ["en", "es", "de"]
        .into_iter()
        .map(|c| (c.to_owned(), model(c)))
        .collect();
    let german =
        GermanCapitals::parse(&read("ios/HEXKeyboard/TypingModel/de.capitals")).map(Arc::new);
    let mut world: Option<World> = None;
    let mut keysets = HashMap::new();
    let mut tally = Tally::default();
    // `HEX_AUTOCORRECT_PARITY_FAILURES=/path`: the failing cases, with the lines they need, as a
    // set of their own.
    let mut failures_out = std::env::var_os("HEX_AUTOCORRECT_PARITY_FAILURES")
        .map(|p| std::io::BufWriter::new(std::fs::File::create(p).expect("a failures file")));
    let mut char_failures = Vec::new();
    let mut char_known = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        let failed_before = tally.failures + tally.shared_failures;
        if let Some(out) = &mut failures_out
            && v["kind"] != "case"
        {
            writeln!(out, "{line}").unwrap();
        }
        match v["kind"].as_str().unwrap() {
            "setup" => {
                world = Some(World {
                    dictionaries: Rc::new(dictionaries(&v, &models)),
                    models: models.clone(),
                    keysets: HashMap::new(),
                    german: german.clone(),
                    available: strings(&v["available"]),
                    grids: HashMap::new(),
                });
            }
            "chars" => (char_failures, char_known) = chars::check(&v),
            "keys" => {
                keysets.insert(v["id"].as_u64().unwrap(), keys(&v["keys"]));
            }
            "grid" => {
                let world = world.as_mut().expect("a setup line first");
                let name = v["ref"].as_str().unwrap().to_owned();
                world.grids.insert(name, ops::grid(&v));
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
                // `foundation`: what Foundation's `caseInsensitiveCompare` gave instead.
                if let Some(theirs) = v.get("foundation") {
                    tally.foundation += 1;
                    if tally.foundation_examples.len() < 3 {
                        tally
                            .foundation_examples
                            .push(order_example(&v, theirs, &v["out"]));
                    }
                }
                // Your own words reach the decoders by shortcut and typo, as `out`'s oracle run
                // had them; `today` is what Swift's dictionary order gave instead.
                if let Some(today) = v.get("today") {
                    tally.today += 1;
                    if tally.today_examples.len() < 3 {
                        tally
                            .today_examples
                            .push(order_example(&v, today, &v["out"]));
                    }
                }
                if compare(world, &ios, &v, out, calls, &mut tally) {
                    tally.failures += 1;
                }
                // The list both apps read: Android's file, by base language.
                let shared_out = v.get("shared").and_then(Value::as_array).unwrap_or(out);
                if v.get("shared").is_some() {
                    tally.shared += 1;
                    if tally.shared_examples.len() < 3 {
                        tally
                            .shared_examples
                            .push(order_example(&v, &v["out"], &v["shared"]));
                    }
                }
                let shared_calls = v
                    .get("sharedCalls")
                    .and_then(Value::as_str)
                    .unwrap_or(calls);
                let mut scratch = Tally::default();
                if compare(world, &shared, &v, shared_out, shared_calls, &mut scratch) {
                    tally.shared_failures += 1;
                    if tally.examples.len() < 6 {
                        tally.examples.extend(
                            scratch
                                .examples
                                .into_iter()
                                .map(|e| format!("[shared list] {e}"))
                                .take(1),
                        );
                    }
                }
            }
            other => panic!("unknown line {other}"),
        }
        if let Some(out) = &mut failures_out
            && tally.failures + tally.shared_failures > failed_before
        {
            writeln!(out, "{line}").unwrap();
        }
    }
    println!(
        "{path}: {} cases, {} operations, {} dictionary calls; {} differ ({} with the shared \
         never-suggest list). Counted apart: {} answer otherwise with own words in Swift's \
         dictionary order, {} with Swift's decoder tie order, {} with Foundation's \
         caseInsensitiveCompare, {} change with the shared list",
        tally.cases,
        tally.ops,
        tally.calls,
        tally.failures,
        tally.shared_failures,
        tally.today,
        tally.tie,
        tally.foundation,
        tally.shared
    );
    for e in &tally.foundation_examples {
        println!("  case folding: {e}");
    }
    for e in &tally.today_examples {
        println!("  own words' order: {e}");
    }
    for e in &tally.shared_examples {
        println!("  shared list: {e}");
    }
    for (kind, n) in &tally.by_kind {
        println!("  first difference at {kind}: {n}");
    }
    for e in &tally.examples {
        println!("  {e}");
    }
    for e in char_failures.iter().take(40) {
        println!("  chars: {e}");
    }
    println!(
        "  chars: {} answers where Foundation's caseInsensitiveCompare isn't case folding",
        char_known.len()
    );
    for e in char_known.iter().take(12) {
        println!("    {e}");
    }
    assert!(tally.cases > 0, "no cases in {path}");
    assert!(
        char_failures.is_empty(),
        "{} character answers differ",
        char_failures.len()
    );
    assert_eq!(tally.failures, 0, "cases differ from the Swift oracle");
    assert_eq!(
        tally.shared_failures, 0,
        "cases differ with the shared list"
    );
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
