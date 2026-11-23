//! The dictation rules give exactly what today's Swift gives: every case the iOS exporter
//! wrote (ios/HEXMobileTests/DictationParity, from oracle copies of the Swift files) is run
//! through the port and compared. The committed fixture always runs; a larger export runs
//! with `HEX_DICTATION_PARITY=/path/dictation-parity.jsonl`. A case marked `"decided"` is the
//! owner's decided change (back-to-back addresses, where the Swift rules trapped): its `out` is
//! the Swift rules with that fix, so it's compared too and counted on its own.

use std::collections::HashSet;

use hex_keyboard_core::dictation::{self, FormatOptions, Language, Style};
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/dictation-parity.jsonl");

fn language(case: &Value) -> Option<Language> {
    let identifier = case.get("lang")?.as_str()?.to_owned();
    let field = |name: &str| case.get(name).and_then(Value::as_str).map(str::to_owned);
    Some(Language {
        identifier,
        code: field("code"),
        region: field("region"),
    })
}

fn text<'a>(case: &'a Value, name: &str) -> &'a str {
    case[name]
        .as_str()
        .unwrap_or_else(|| panic!("{name} missing in {case}"))
}

fn flag(case: &Value, name: &str) -> bool {
    case[name]
        .as_bool()
        .unwrap_or_else(|| panic!("{name} missing in {case}"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v.as_str().expect("a string").to_owned())
        .collect()
}

/// What the port gives for one case, or none for an operation this test doesn't cover (the
/// crate's own tests check the Unicode ones).
fn run(case: &Value) -> Option<Value> {
    let language = language(case);
    let language = language.as_ref();
    let out = match case["op"].as_str()? {
        "format" => {
            let style = Style::from_name(text(case, "style")).expect("a style");
            let options = FormatOptions {
                style,
                language,
                lists: flag(case, "lists"),
                punctuation: flag(case, "punct"),
                ends_with_period: flag(case, "period"),
            };
            Value::from(dictation::format(text(case, "text"), &options))
        }
        "spoken_punctuation" => {
            Value::from(dictation::spoken_punctuation(text(case, "text"), language))
        }
        "spoken_lists" => Value::from(dictation::spoken_lists(text(case, "text"), language)),
        "cleanup_rules" => Value::from(dictation::cleanup_rules(text(case, "text"), language)),
        "spoken_symbols" => Value::from(dictation::spoken_symbols(text(case, "text"), language)),
        "spoken_counts" => Value::from(dictation::spoken_counts(text(case, "text"), language)),
        "cleanup_chain" => {
            let symbols = dictation::spoken_symbols(text(case, "text"), language);
            let counted = dictation::spoken_counts(&symbols, language);
            Value::from(dictation::cleanup_rules(&counted, language))
        }
        "french_spacing" => Value::from(dictation::french_spacing(text(case, "text"), language)),
        "reconstruct" => {
            let words = strings(&case["words"]);
            let deletes: Vec<bool> = case["deletes"]
                .as_array()?
                .iter()
                .map(|d| d.as_bool().unwrap())
                .collect();
            Value::from(dictation::reconstruct(&words, &deletes))
        }
        "insertion" => {
            let dictionary: Option<HashSet<String>> =
                case.get("words").map(|w| strings(w).into_iter().collect());
            let is_word = |word: &str| dictionary.as_ref().is_some_and(|d| d.contains(word));
            let check: Option<&dyn Fn(&str) -> bool> = dictionary
                .as_ref()
                .map(|_| &is_word as &dyn Fn(&str) -> bool);
            let before = case.get("before").and_then(Value::as_str);
            let after = case.get("after").and_then(Value::as_str);
            Value::from(dictation::insertion_text(
                text(case, "text"),
                before,
                after,
                check,
            ))
        }
        "starts_sentence" => Value::from(dictation::starts_sentence(Some(text(case, "text")))),
        "dropping_final_full_stop" => {
            Value::from(dictation::dropping_final_full_stop(text(case, "text")))
        }
        "continuing_sentence" => {
            let dictionary: HashSet<String> = strings(&case["words"]).into_iter().collect();
            Value::from(dictation::continuing_sentence(
                text(case, "text"),
                &|word| dictionary.contains(word),
            ))
        }
        "words" => Value::from(dictation::words(text(case, "text"))),
        "lowercases_continuations" => match language {
            Some(language) => Value::from(dictation::lowercases_continuations(language)),
            None => Value::Null,
        },
        "supports" => Value::from(dictation::cleanup_supports(language)),
        "filler_pattern" => dictation::filler_pattern(language).map_or(Value::Null, Value::from),
        _ => return None,
    };
    Some(out)
}

fn check(name: &str, lines: &str) {
    let mut cases = 0;
    let mut decided = 0;
    let mut failures = Vec::new();
    for (number, line) in lines.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let case: Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("{name}:{}: {error}", number + 1));
        let Some(got) = run(&case) else { continue };
        cases += 1;
        decided += usize::from(case.get("decided").is_some());
        if got != case["out"] {
            failures.push(format!("{name}:{}: {case}\n  port gives {got}", number + 1));
        }
    }
    assert!(cases > 0, "{name} has no cases");
    if !failures.is_empty() {
        let shown: Vec<&str> = failures.iter().take(20).map(String::as_str).collect();
        panic!(
            "{} of {cases} cases differ from Swift:\n{}",
            failures.len(),
            shown.join("\n")
        );
    }
    println!("{name}: {cases} cases ({decided} of them the decided change), all as Swift");
}

#[test]
fn committed_fixture_matches_swift() {
    check("dictation-parity.jsonl", FIXTURE);
}

#[test]
fn large_export_matches_swift() {
    let Some(path) = std::env::var_os("HEX_DICTATION_PARITY") else {
        eprintln!("HEX_DICTATION_PARITY unset: the large export is skipped");
        return;
    };
    let name = path.to_string_lossy().into_owned();
    let lines = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
    check(&name, &lines);
}
