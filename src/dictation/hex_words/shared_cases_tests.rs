//! The shared cases every Hex runs (`tests/fixtures/hex-words`, read from
//! the repository, never copied): every case in `read.json`, the desktop
//! receiver's cases in `merge.json` with Undo after each, and the desktop
//! writer's cases in `write.json` with the written file checked against
//! `hex-words.schema.json`. The phones run the phone cases.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Map, Value, json};

use super::read::read;
use super::{Limits, Lists, Parts, Replacement, Skip, Term, Undo, export, merge};
use crate::app_settings::{AppSettings, DictationMode};
use crate::dictation::text_replacements::TextReplacement;
use crate::dictation::vocabulary::VocabularyEntry;

const READ: &str = include_str!("../../../tests/fixtures/hex-words/read.json");
const MERGE: &str = include_str!("../../../tests/fixtures/hex-words/merge.json");
const WRITE: &str = include_str!("../../../tests/fixtures/hex-words/write.json");
const SCHEMA: &str = include_str!("../../../tests/fixtures/hex-words/hex-words.schema.json");

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hex-words")
}

fn cases(json: &str) -> Vec<Value> {
    let cases: Value = serde_json::from_str(json).unwrap();
    cases["cases"].as_array().unwrap().clone()
}

fn text(value: &Value) -> String {
    value.as_str().unwrap().to_owned()
}

fn texts(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|list| list.iter().map(text).collect())
        .unwrap_or_default()
}

/// A list compared as a set: its entries, each as JSON text, sorted.
fn as_set(list: &Value) -> Vec<String> {
    let mut entries: Vec<String> = list
        .as_array()
        .unwrap()
        .iter()
        .map(Value::to_string)
        .collect();
    entries.sort();
    entries
}

fn skips(skipped: &[Skip]) -> Value {
    skipped
        .iter()
        .map(|skip| json!({"path": skip.place.to_string(), "reason": skip.reason.name()}))
        .collect()
}

/// Fails with every case that disagreed, by name.
fn report(file: &str, failures: Vec<String>, checked: usize) {
    assert!(checked > 0, "{file}: no cases ran");
    assert!(
        failures.is_empty(),
        "{file}: {} of {checked} cases failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

fn mismatch(name: &Value, expected: &Value, actual: &Value) -> String {
    format!("{name}\n  expected {expected}\n  actual   {actual}")
}

// ---------------------------------------------------------------------------
// read.json

/// The case's bytes: its file, or the base file padded with spaces.
fn bytes(case: &Value) -> Vec<u8> {
    if let Some(file) = case["file"].as_str() {
        return std::fs::read(folder().join(file)).unwrap();
    }
    let generate = &case["generate"];
    let mut bytes = std::fs::read(folder().join(text(&generate["base"]))).unwrap();
    bytes.resize(generate["padWithSpacesTo"].as_u64().unwrap() as usize, b' ');
    bytes
}

fn without(value: &Value, key: &str) -> Value {
    let mut copy = value.clone();
    if let Some(object) = copy.as_object_mut() {
        object.remove(key);
    }
    copy
}

/// Skips counted by reason, as `skippedCounts` holds them.
fn counted(skipped: &Value) -> Value {
    let mut counts = Map::new();
    for skip in skipped.as_array().unwrap() {
        let reason = text(&skip["reason"]);
        let count = counts.get(&reason).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(reason, (count + 1).into());
    }
    Value::Object(counts)
}

/// The reader's result in the cases' shape.
fn read_result(bytes: &[u8]) -> Value {
    let file = match read(bytes) {
        Ok(file) => file,
        Err(refusal) => return json!({"result": "refused", "reason": refusal.name()}),
    };
    let mut made_by = Map::new();
    if let Some(app) = &file.made_by.app {
        made_by.insert("app".into(), app.clone().into());
    }
    if let Some(version) = &file.made_by.version {
        made_by.insert("version".into(), version.clone().into());
    }
    json!({
        "result": "read",
        "version": file.version,
        "newerVersion": file.newer(),
        "madeBy": made_by,
        "vocabulary": file.vocabulary.iter()
            .map(|term| json!({"term": term.term, "soundsLike": term.sounds_like}))
            .collect::<Vec<_>>(),
        "replacements": file.replacements.iter()
            .map(|rule| json!({"heard": rule.heard, "write": rule.write, "learned": rule.learned}))
            .collect::<Vec<_>>(),
        "skipped": skips(&file.skipped),
    })
}

#[test]
fn every_read_case() {
    let cases = cases(READ);
    let mut failures = Vec::new();
    for case in &cases {
        let expected = &case["expect"];
        let actual = read_result(&bytes(case));
        let agrees = match (expected.get("skippedCounts"), expected.get("skipped")) {
            (Some(counts), _) => {
                without(&actual, "skipped") == without(expected, "skippedCounts")
                    && counted(&actual["skipped"]) == *counts
            }
            (None, Some(skipped)) => {
                without(&actual, "skipped") == without(expected, "skipped")
                    && as_set(&actual["skipped"]) == as_set(skipped)
            }
            (None, None) => actual == *expected,
        };
        if !agrees {
            failures.push(mismatch(&case["name"], &case["expect"], &actual));
        }
    }
    report("read.json", failures, cases.len());
}

// ---------------------------------------------------------------------------
// merge.json

fn lists_from(value: &Value) -> Lists {
    Lists {
        vocabulary: value["vocabulary"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| VocabularyEntry {
                term: text(&entry["term"]),
                sounds_like: texts(&entry["soundsLike"]),
            })
            .collect(),
        replacements: value["replacements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| TextReplacement {
                matched_phrase: text(&rule["heard"]),
                output: text(&rule["write"]),
            })
            .collect(),
    }
}

fn lists_json(lists: &Lists) -> Value {
    json!({
        "vocabulary": lists.vocabulary.iter()
            .map(|entry| json!({"term": entry.term, "soundsLike": entry.sounds_like}))
            .collect::<Vec<_>>(),
        "replacements": lists.replacements.iter()
            .map(|rule| json!({"heard": rule.matched_phrase, "write": rule.output}))
            .collect::<Vec<_>>(),
    })
}

fn incoming(value: &Value) -> (Vec<Term>, Vec<Replacement>) {
    let terms = value["vocabulary"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| Term {
            term: text(&entry["term"]),
            sounds_like: texts(&entry["soundsLike"]),
        })
        .collect();
    let replacements = value["replacements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rule| Replacement {
            heard: text(&rule["heard"]),
            write: text(&rule["write"]),
            learned: rule["learned"].as_bool().unwrap(),
        })
        .collect();
    (terms, replacements)
}

fn limits(value: &Value) -> Limits {
    let mut limits = Limits::default();
    for (name, limit) in value.as_object().into_iter().flatten() {
        let limit = limit.as_u64().unwrap() as usize;
        match name.as_str() {
            "terms" => limits.terms = limit,
            "soundsLike" => limits.spellings = limit,
            "replacements" => limits.replacements = limit,
            other => panic!("unknown limit {other}"),
        }
    }
    limits
}

/// The file's replacements the person switched to theirs. On the desktop
/// only a replacement can clash.
fn choices(value: &Value) -> BTreeSet<usize> {
    let pattern = Regex::new(r"^replacements\[(\d+)\]$").unwrap();
    value
        .as_object()
        .into_iter()
        .flatten()
        .map(|(path, choice)| {
            assert_eq!(choice, "useTheirs", "{path}");
            pattern.captures(path).unwrap()[1].parse().unwrap()
        })
        .collect()
}

#[test]
fn every_desktop_merge_case_and_its_undo() {
    let cases = cases(MERGE);
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in &cases {
        match case["receiver"].as_str() {
            Some("desktop") => {}
            Some("phone") => continue,
            other => panic!("{}: unknown receiver {other:?}", case["name"]),
        }
        checked += 1;
        let before = lists_from(&case["before"]);
        let (terms, replacements) = incoming(&case["incoming"]);
        let parts = Parts {
            vocabulary: case["parts"]["vocabulary"].as_bool().unwrap_or(true),
            replacements: case["parts"]["replacements"].as_bool().unwrap_or(true),
        };
        let merged = merge(
            &before,
            &terms,
            &replacements,
            limits(&case["limits"]),
            parts,
            &choices(&case["choices"]),
        );
        let summary = merged.summary;
        let actual = json!({
            "after": lists_json(&merged.lists),
            "summary": {
                "addedTerms": summary.added_terms,
                "updatedTerms": summary.updated_terms,
                "alreadyHaveTerms": summary.already_have_terms,
                "addedReplacements": summary.added_replacements,
                "changedReplacements": summary.changed_replacements,
                "keptYours": summary.kept_yours,
                "alreadyHaveReplacements": summary.already_have_replacements,
            },
            "notApplied": merged.over_limit.iter()
                .map(|place| json!({"path": place.to_string(), "reason": "overDeviceLimit"}))
                .collect::<Vec<_>>(),
            "clashes": merged.clashes.iter()
                .map(|clash| json!({
                    "path": format!("replacements[{}]", clash.index),
                    "heard": clash.heard,
                    "yours": clash.yours,
                    "theirs": clash.theirs,
                }))
                .collect::<Vec<_>>(),
        });
        if actual != case["expect"] {
            failures.push(mismatch(&case["name"], &case["expect"], &actual));
            continue;
        }
        // What the review lists agrees with what it counts.
        assert_eq!(
            merged.new_terms.len(),
            summary.added_terms,
            "{}",
            case["name"]
        );
        assert_eq!(
            merged.new_spellings.len(),
            summary.updated_terms,
            "{}",
            case["name"]
        );
        assert_eq!(
            merged.new_replacements.len(),
            summary.added_replacements,
            "{}",
            case["name"]
        );
        // Undo, through the copy as the data folder keeps it, and back into
        // settings, gives back `before` exactly.
        let kept = Undo::new(before, merged.lists.clone()).to_bytes();
        let undo = Undo::from_bytes(&kept).unwrap();
        let mut settings = AppSettings::default();
        merged.lists.put(&mut settings);
        assert!(undo.applies_to(&Lists::of(&settings)), "{}", case["name"]);
        undo.before().put(&mut settings);
        let restored = lists_json(&Lists::of(&settings));
        if restored != case["before"] {
            failures.push(format!("{}: Undo gave back {restored}", case["name"]));
        }
    }
    report("merge.json (desktop)", failures, checked);
}

// ---------------------------------------------------------------------------
// write.json

/// Settings holding the case's lists: the vocabulary, the Global mode's
/// replacements, and other modes with their own.
fn settings_from(input: &Value) -> AppSettings {
    let rules = |list: &Value| -> Vec<TextReplacement> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|rule| TextReplacement {
                matched_phrase: text(&rule["matchedPhrase"]),
                output: text(&rule["output"]),
            })
            .collect()
    };
    let lists = Lists {
        vocabulary: input["vocabulary"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| VocabularyEntry {
                term: text(&entry["term"]),
                sounds_like: texts(&entry["soundsLike"]),
            })
            .collect(),
        replacements: rules(&input["replacements"]),
    };
    let mut settings = AppSettings::default();
    lists.put(&mut settings);
    settings.dictation_processing.modes = input["modes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mode| DictationMode {
            name: text(&mode["name"]),
            replacements: rules(&mode["replacements"]),
            ..DictationMode::default()
        })
        .collect();
    settings
}

#[test]
fn every_desktop_write_case_passes_the_schema_and_reads_back_whole() {
    let cases = cases(WRITE);
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in &cases {
        match case["writer"].as_str() {
            Some("desktop") => {}
            Some("phone") => continue,
            other => panic!("{}: unknown writer {other:?}", case["name"]),
        }
        checked += 1;
        let name = &case["name"];
        let export = export(&Lists::of(&settings_from(&case["input"])));
        assert!(
            !export.bytes.starts_with(b"\xef\xbb\xbf"),
            "{name}: a byte order mark"
        );
        let written: Value = serde_json::from_slice(&export.bytes).unwrap();
        if let Err(error) = validate(&schema, &schema, &written, "") {
            failures.push(format!("{name}: fails the schema at {error}"));
        }
        let read_back = read(&export.bytes).unwrap();
        if !read_back.skipped.is_empty() {
            failures.push(format!("{name}: reads back with {:?}", read_back.skipped));
        }
        let mut file = written.clone();
        let made_by = file.as_object_mut().unwrap().remove("madeBy");
        if made_by
            .as_ref()
            .and_then(|made_by| made_by["app"].as_str())
            .is_none()
        {
            failures.push(format!("{name}: no madeBy.app"));
        }
        if file != case["expect"]["file"] {
            failures.push(mismatch(name, &case["expect"]["file"], &file));
        }
        let left_out = skips(&export.left_out);
        if as_set(&left_out) != as_set(&case["expect"]["notExported"]) {
            failures.push(mismatch(name, &case["expect"]["notExported"], &left_out));
        }
    }
    report("write.json (desktop)", failures, checked);
}

/// Checks `value` against `schema` for exactly the JSON Schema keywords
/// `hex-words.schema.json` uses; any other keyword fails, so a schema that
/// grows one fails here rather than passing unchecked.
fn validate(root: &Value, schema: &Value, value: &Value, at: &str) -> Result<(), String> {
    let fail = |what: &str| Err(format!("{at}: {what}"));
    for (keyword, rule) in schema.as_object().unwrap() {
        match keyword.as_str() {
            "$schema" | "title" | "description" | "$defs" => {}
            "$ref" => {
                let name = rule.as_str().unwrap().strip_prefix("#/$defs/").unwrap();
                validate(root, &root["$defs"][name], value, at)?;
            }
            "allOf" => {
                for part in rule.as_array().unwrap() {
                    validate(root, part, value, at)?;
                }
            }
            "type" => {
                let agrees = match rule.as_str().unwrap() {
                    "object" => value.is_object(),
                    "array" => value.is_array(),
                    "string" => value.is_string(),
                    other => return fail(&format!("unknown type {other}")),
                };
                if !agrees {
                    return fail(&format!("not {rule}"));
                }
            }
            "const" if value != rule => return fail(&format!("not {rule}")),
            "const" => {}
            "required" => {
                for key in rule.as_array().unwrap() {
                    if value.get(key.as_str().unwrap()).is_none() {
                        return fail(&format!("no {key}"));
                    }
                }
            }
            "properties" => {
                for (key, property) in rule.as_object().unwrap() {
                    if let Some(field) = value.get(key) {
                        validate(root, property, field, &format!("{at}.{key}"))?;
                    }
                }
            }
            "additionalProperties" => {
                assert_eq!(rule, false);
                let known = schema["properties"].as_object().unwrap();
                if let Some(extra) = value
                    .as_object()
                    .and_then(|object| object.keys().find(|key| !known.contains_key(*key)))
                {
                    return fail(&format!("{extra} isn't allowed"));
                }
            }
            "items" => {
                for (index, item) in value.as_array().into_iter().flatten().enumerate() {
                    validate(root, rule, item, &format!("{at}[{index}]"))?;
                }
            }
            "minItems" | "maxItems" => {
                let count = value.as_array().map_or(0, Vec::len) as u64;
                let limit = rule.as_u64().unwrap();
                if (keyword == "minItems" && count < limit)
                    || (keyword == "maxItems" && count > limit)
                {
                    return fail(&format!("{keyword} {limit}"));
                }
            }
            "uniqueItems" => {
                let items = value.as_array().into_iter().flatten().map(Value::to_string);
                let unique: BTreeSet<String> = items.clone().collect();
                if unique.len() != items.count() {
                    return fail("repeats an item");
                }
            }
            "minLength" | "maxLength" => {
                let count = value.as_str().map_or(0, |text| text.chars().count()) as u64;
                let limit = rule.as_u64().unwrap();
                if (keyword == "minLength" && count < limit)
                    || (keyword == "maxLength" && count > limit)
                {
                    return fail(&format!("{keyword} {limit}"));
                }
            }
            "pattern" => {
                let pattern = Regex::new(rule.as_str().unwrap()).unwrap();
                if value.as_str().is_some_and(|text| !pattern.is_match(text)) {
                    return fail(&format!("doesn't match {rule}"));
                }
            }
            other => return fail(&format!("unknown keyword {other}")),
        }
    }
    Ok(())
}
