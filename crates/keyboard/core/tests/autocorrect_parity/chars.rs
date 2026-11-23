//! Swift's answers on characters and strings (the fixture's `chars` line) against the port's
//! (`autocorrect::text`).
//!
//! Case-insensitive comparisons are checked against both strings case-folded (Foundation's
//! `folding(options: .caseInsensitive)`), which the port does by Unicode case folding. Where
//! Foundation's `caseInsensitiveCompare` answers otherwise (it isn't symmetric for ß and
//! Greek's iota subscript: "wußte" is "wüste", "wüste" isn't "wußte"), the difference is the
//! owner's decided change and is listed, not failed.

use hex_keyboard_core::autocorrect::text;
use serde_json::{Value, json};

/// The answers that differ, and where only Foundation's `caseInsensitiveCompare` does.
pub fn check(v: &Value) -> (Vec<String>, Vec<String>) {
    let mut failures = Vec::new();
    let mut foundation = Vec::new();
    for row in v["rows"].as_array().unwrap() {
        let code = u32::from_str_radix(row[0].as_str().unwrap(), 16).unwrap();
        let Some(c) = char::from_u32(code) else {
            continue;
        };
        let s = c.to_string();
        let mut flags = 0;
        for (bit, on) in [
            (1, text::is_letter(&s)),
            (2, text::is_number(&s)),
            (4, text::is_whitespace(&s)),
            (8, text::is_newline(&s)),
            (16, text::is_uppercase(&s)),
            (32, text::is_lowercase(&s)),
        ] {
            if on {
                flags |= bit;
            }
        }
        let (lower, upper, fold) = (
            text::lowercased(&s),
            text::uppercased(&s),
            text::without_diacritics(&s),
        );
        let mut ci = 0;
        for (bit, other) in [(1, &lower), (2, &upper), (4, &fold)] {
            if text::case_insensitive_eq(&s, other) {
                ci |= bit;
            }
        }
        let swift_ci = row[5].as_u64().unwrap();
        let got = json!([
            row[0],
            flags,
            lower,
            upper,
            fold,
            ci,
            text::trimmed(&s).is_empty()
        ]);
        let same = got[1] == row[1]
            && got[2] == row[2]
            && got[3] == row[3]
            && text::eq(got[4].as_str().unwrap(), row[4].as_str().unwrap())
            && ci == (swift_ci >> 3) & 7
            && got[6] == row[6];
        if !same {
            failures.push(format!("U+{code:04X} rust {got} swift {row}"));
        } else if ci != swift_ci & 7 {
            foundation.push(format!(
                "U+{code:04X} {s:?}: rust {ci:03b} foundation {:03b}",
                swift_ci & 7
            ));
        }
    }
    let strings = v["strings"].as_array().unwrap();
    let all: Vec<&str> = strings.iter().map(|r| r[0].as_str().unwrap()).collect();
    for row in strings {
        let s = row[0].as_str().unwrap();
        let ci: String = all
            .iter()
            .map(|o| {
                if text::case_insensitive_eq(s, o) {
                    '1'
                } else {
                    '0'
                }
            })
            .collect();
        let got = json!([
            s,
            text::count(s),
            text::lowercased(s),
            text::uppercased(s),
            text::without_diacritics(s),
            row[5],
            ci,
            text::trimmed(s),
            text::without_apostrophes(s)
        ]);
        let same = (0..9).all(|i| match i {
            4 => text::eq(got[4].as_str().unwrap(), row[4].as_str().unwrap()),
            _ => got[i] == row[i],
        });
        if !same {
            failures.push(format!("{s:?} rust {got} swift {row}"));
        } else if row[5] != row[6] {
            foundation.push(format!(
                "{s:?} against the list: rust {ci} foundation {}",
                row[5]
            ));
        }
    }
    for pair in v["pairs"].as_array().into_iter().flatten() {
        let (a, b) = (pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
        let rust = text::case_insensitive_eq(a, b);
        if json!(rust) != pair[3] {
            failures.push(format!("pair {a:?} {b:?}: rust {rust} folded {}", pair[3]));
        } else if json!(rust) != pair[2] {
            foundation.push(format!(
                "{a:?}.caseInsensitiveCompare({b:?}): rust {rust} foundation {}",
                pair[2]
            ));
        }
    }
    (failures, foundation)
}
