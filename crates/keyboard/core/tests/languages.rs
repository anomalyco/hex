//! The language manifests (`crates/keyboard/languages/<tag>.json`): every file parses, its `id`
//! is its file name, a `words_from` names a manifest that has none of its own, and each one is
//! compiled into the core (`build.rs` checks the same before it compiles them).

use std::collections::BTreeMap;

use hex_keyboard_core::autocorrect::LanguageConfig;
use serde_json::Value;

fn manifests() -> BTreeMap<String, serde_json::Map<String, Value>> {
    let dir = format!("{}/../languages", env!("CARGO_MANIFEST_DIR"));
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("the languages folder") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let tag = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        let value: Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{tag}.json: {e}"));
        let Value::Object(object) = value else {
            panic!("{tag}.json is not an object");
        };
        out.insert(tag, object);
    }
    out
}

#[test]
fn every_manifest_parses_and_names_itself() {
    let all = manifests();
    assert!(all.len() >= 18, "{} manifests", all.len());
    for (tag, m) in &all {
        assert_eq!(
            m.get("id").and_then(Value::as_str),
            Some(tag.as_str()),
            "{tag}"
        );
        if let Some(from) = m.get("words_from") {
            let from = from.as_str().expect("words_from is a tag");
            let target = all
                .get(from)
                .unwrap_or_else(|| panic!("{tag}: no {from}.json"));
            assert!(
                !target.contains_key("words_from"),
                "{tag}: {from} has a words_from"
            );
        }
        assert_eq!(LanguageConfig::for_tag(tag).id, tag, "{tag} is compiled in");
    }
    assert_eq!(LanguageConfig::all().len(), all.len());
}

#[test]
fn a_regional_variant_reads_its_language() {
    let fr = LanguageConfig::for_tag("fr");
    let ca = LanguageConfig::for_tag("fr-CA");
    assert_eq!(ca.system_dictionaries, ["fr_CA", "fr", "fr_FR"]);
    assert_eq!(ca.elision_heads, fr.elision_heads);
    assert_eq!(ca.model, Some("fr"));
    assert_eq!(LanguageConfig::for_tag("ko").system_dictionaries.len(), 0);
}
