//! The Swift semantics the rules stand on, against what the iOS exporter wrote: each
//! Character's properties and `folding` (the parity fixtures' "characters" and "fold" cases),
//! and, with `HEX_DICTATION_UNICODE=/path/dictation-unicode.tsv`, every scalar's properties,
//! case mappings and folds as iOS has them.

use serde_json::Value;

use super::{fold::fold, text as t, unicode};

const FIXTURE: &str = include_str!("../../tests/fixtures/dictation-parity.jsonl");

/// What `ParityUnicode.characterFlags` writes for a Character.
fn character_flags(c: &str) -> String {
    let mut flags = String::new();
    for (set, flag) in [
        (t::is_letter(c), 'L'),
        (t::is_number(c), 'N'),
        (t::is_whitespace(c), 'W'),
        (t::is_uppercase(c), 'U'),
        (t::is_lowercase(c), 'l'),
        (t::is_cased(c), 'C'),
        (t::is_newline(c), 'n'),
        (t::is_ascii(c), 'A'),
    ] {
        if set {
            flags.push(flag);
        }
    }
    flags
}

fn check_cases(name: &str, lines: &str) -> usize {
    let mut checked = 0;
    let mut failures = Vec::new();
    for line in lines.lines() {
        let case: Value = serde_json::from_str(line).expect("a JSON case");
        let input = case["text"].as_str().unwrap_or_default();
        let got = match case["op"].as_str() {
            Some("characters") => Value::from(
                t::chars(input)
                    .iter()
                    .map(|c| format!("{}:{}", character_flags(c), c.chars().count()))
                    .collect::<Vec<_>>(),
            ),
            Some("fold") => Value::from(fold(input)),
            _ => continue,
        };
        checked += 1;
        if got != case["out"] {
            failures.push(format!("{case}\n  port gives {got}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{name}: {} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n")
    );
    checked
}

#[test]
fn characters_and_folds_match_swift() {
    assert!(check_cases("dictation-parity.jsonl", FIXTURE) > 0);
    if let Some(path) = std::env::var_os("HEX_DICTATION_PARITY") {
        let name = path.to_string_lossy().into_owned();
        let lines = std::fs::read_to_string(&path).expect("the large export");
        println!(
            "{name}: {} Character and fold cases, all as Swift",
            check_cases(&name, &lines)
        );
    }
}

fn scalars(hex: &str, original: &str) -> String {
    if hex == "=" {
        return original.to_owned();
    }
    hex.split(' ')
        .map(|h| char::from_u32(u32::from_str_radix(h, 16).expect("hex")).expect("a scalar"))
        .collect()
}

#[test]
fn every_scalar_matches_ios() {
    let Some(path) = std::env::var_os("HEX_DICTATION_UNICODE") else {
        eprintln!("HEX_DICTATION_UNICODE unset: the per-scalar check is skipped");
        return;
    };
    let dump = std::fs::read_to_string(&path).expect("the Unicode dump");
    let path = path.to_string_lossy();
    let mut failures = Vec::new();
    let mut count = 0;
    for line in dump.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let c = char::from_u32(u32::from_str_radix(fields[0], 16).expect("hex")).expect("a scalar");
        let s = c.to_string();
        let mut flags = character_flags(&s);
        for (set, flag) in [
            (unicode::is_horizontal_whitespace(c), 's'),
            (unicode::is_whitespace_or_newline(c), 't'),
            (unicode::icu_letter(c), 'p'),
            (unicode::icu_number(c), 'd'),
            (unicode::is_white_space(c), 'S'),
            (unicode::icu_word(c), 'w'),
            (unicode::icu_extend(c), 'e'),
        ] {
            if set {
                flags.push(flag);
            }
        }
        let mut expected: Vec<char> = fields[1].chars().collect();
        let mut got: Vec<char> = flags.chars().collect();
        expected.sort_unstable();
        got.sort_unstable();
        let a = format!("a{s}");
        let checks = [
            ("flags", expected == got),
            ("lowercased", t::lowercased(&s) == scalars(fields[2], &s)),
            ("uppercased", t::uppercased(&s) == scalars(fields[3], &s)),
            ("fold", fold(&s) == scalars(fields[4], &s)),
            ("fold after a", fold(&a) == scalars(fields[5], &a)),
        ];
        for (what, ok) in checks {
            if !ok {
                failures.push(format!(
                    "U+{:04X} {what}: iOS {line:?}, port flags {flags}",
                    c as u32
                ));
            }
        }
        count += 1;
    }
    assert!(count > 1_100_000, "{path} has {count} scalars");
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(30)].join("\n")
    );
    println!("{path}: {count} scalars, all as iOS");
}
