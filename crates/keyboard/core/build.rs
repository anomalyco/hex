//! Compiles the language manifests (`../languages/<tag>.json`) into static data
//! (`$OUT_DIR/languages.rs`, included by `autocorrect::language`), so the keyboard parses no
//! JSON while it runs. Fails the build when a manifest doesn't parse, its `id` isn't its file
//! name, or its `words_from` names no manifest or one that has a `words_from` itself.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Map, Value};

const FIELDS: [&str; 16] = [
    "id",
    "words_from",
    "model",
    "system_dictionaries",
    "fold_diacritics",
    "fold_pairs",
    "extra_sentence_enders",
    "english_rules",
    "noun_capitals",
    "two_letter_words",
    "accent_restorations",
    "accent_suggestions",
    "elision_heads",
    "sentence_starters",
    "clause_starters",
    "common_words",
];

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../languages");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut manifests: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("crates/keyboard/languages") {
        let path = entry.expect("a manifest").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let tag = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        let value: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{}: not JSON: {e}", path.display()));
        let Value::Object(object) = value else {
            panic!("{}: not an object", path.display());
        };
        for key in object.keys() {
            assert!(
                FIELDS.contains(&key.as_str()),
                "{}: unknown field {key:?}",
                path.display()
            );
        }
        assert_eq!(
            object.get("id").and_then(Value::as_str),
            Some(tag.as_str()),
            "{}: id must be the file's name",
            path.display()
        );
        manifests.insert(tag, object);
    }
    for (tag, m) in &manifests {
        if let Some(from) = m.get("words_from") {
            let from = from.as_str().unwrap_or_else(|| panic!("{tag}: words_from"));
            let target = manifests
                .get(from)
                .unwrap_or_else(|| panic!("{tag}: words_from {from:?} has no manifest"));
            assert!(
                !target.contains_key("words_from"),
                "{tag}: words_from {from:?} has a words_from of its own"
            );
        }
    }
    let mut out = String::from("&[\n");
    for (tag, m) in &manifests {
        let from = m
            .get("words_from")
            .and_then(Value::as_str)
            .map(|f| &manifests[f]);
        // A field this manifest leaves out is read from `words_from`'s.
        let field = |key: &str| m.get(key).or_else(|| from.and_then(|f| f.get(key)));
        let strings = |key: &str| -> String {
            let list: Vec<String> = match field(key) {
                None => Vec::new(),
                Some(v) => v
                    .as_array()
                    .unwrap_or_else(|| panic!("{tag}: {key} must be a list"))
                    .iter()
                    .map(|s| format!("{:?}", s.as_str().unwrap_or_else(|| panic!("{tag}: {key}"))))
                    .collect(),
            };
            format!("&[{}]", list.join(", "))
        };
        let text = |key: &str| -> String {
            match field(key) {
                None | Some(Value::Null) => "None".into(),
                Some(v) => format!(
                    "Some({:?})",
                    v.as_str()
                        .unwrap_or_else(|| panic!("{tag}: {key} must be text"))
                ),
            }
        };
        let flag = |key: &str| -> bool {
            field(key).is_some_and(|v| v.as_bool().unwrap_or_else(|| panic!("{tag}: {key}")))
        };
        let pairs = |key: &str| -> String {
            let list: Vec<String> = match field(key) {
                None => Vec::new(),
                Some(Value::Array(a)) => a
                    .iter()
                    .map(|p| {
                        let p = p.as_array().filter(|p| p.len() == 2);
                        let p = p.unwrap_or_else(|| panic!("{tag}: {key} holds pairs"));
                        format!(
                            "({:?}, {:?})",
                            p[0].as_str().unwrap(),
                            p[1].as_str().unwrap()
                        )
                    })
                    .collect(),
                Some(Value::Object(o)) => o
                    .iter()
                    .map(|(k, v)| format!("({k:?}, {:?})", v.as_str().unwrap()))
                    .collect(),
                Some(_) => panic!("{tag}: {key}"),
            };
            format!("&[{}]", list.join(", "))
        };
        writeln!(
            out,
            "    LanguageConfig {{ id: {tag:?}, model: {}, system_dictionaries: {}, \
             fold_diacritics: {}, fold_pairs: {}, extra_sentence_enders: {}, english_rules: {}, \
             noun_capitals: {}, two_letter_words: {}, accent_restorations: {}, \
             accent_suggestions: {}, elision_heads: {}, sentence_starters: {}, \
             clause_starters: {}, common_words: {} }},",
            text("model"),
            strings("system_dictionaries"),
            flag("fold_diacritics"),
            pairs("fold_pairs"),
            strings("extra_sentence_enders"),
            flag("english_rules"),
            text("noun_capitals"),
            strings("two_letter_words"),
            pairs("accent_restorations"),
            pairs("accent_suggestions"),
            strings("elision_heads"),
            strings("sentence_starters"),
            strings("clause_starters"),
            strings("common_words"),
        )
        .unwrap();
    }
    out.push(']');
    let target = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("languages.rs");
    std::fs::write(target, out).unwrap();
}
