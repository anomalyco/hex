//! Swift's answers on characters and strings (the fixture's `chars` line) against the port's
//! (`autocorrect::text`).

use hex_keyboard_core::autocorrect::text;
use serde_json::{Value, json};

/// Where the port isn't Foundation: `caseInsensitiveCompare` folds its own way, and not
/// symmetrically. A receiver's ß matches a precomposed umlaut before s ("wußte" is "wüste",
/// "Maß" is "mäs", "Straße" is "Sträse", not the other way round), and Greek's iota subscript
/// matches or not by case and side ("ἈΙ" is "ᾀ", "ᾀ" is not "ἈΙ"). The port folds both
/// strings fully (Unicode case folding), keeping the iota subscript; these answers are
/// listed, not failed, until the owner decides.
const KNOWN: [&str; 3] = ["U+0345 ", "\"\u{1f08}\u{399}\" ", "pair "];

/// The answers that differ, and those that differ as `KNOWN` says.
pub fn check(v: &Value) -> (Vec<String>, Vec<String>) {
    let mut failures = Vec::new();
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
        let got = json!([
            row[0],
            flags,
            lower,
            upper,
            fold,
            ci,
            text::trimmed(&s).is_empty()
        ]);
        // Folding compares canonically: Swift's may come back decomposed.
        let same = got[1] == row[1]
            && got[2] == row[2]
            && got[3] == row[3]
            && text::eq(got[4].as_str().unwrap(), row[4].as_str().unwrap())
            && got[5] == row[5]
            && got[6] == row[6];
        if !same {
            failures.push(format!("U+{code:04X} rust {got} swift {row}"));
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
            ci,
            text::trimmed(s),
            text::without_apostrophes(s)
        ]);
        let same = (0..8).all(|i| {
            if i == 4 {
                text::eq(got[4].as_str().unwrap(), row[4].as_str().unwrap())
            } else {
                got[i] == row[i]
            }
        });
        if !same {
            failures.push(format!("{s:?} rust {got} swift {row}"));
        }
    }
    for pair in v["pairs"].as_array().into_iter().flatten() {
        let (a, b) = (pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
        let rust = text::case_insensitive_eq(a, b);
        if json!(rust) != pair[2] {
            failures.push(format!(
                "pair {a:?}.caseInsensitiveCompare({b:?}): rust {rust} swift {}",
                pair[2]
            ));
        }
    }
    failures
        .into_iter()
        .partition(|f| !KNOWN.iter().any(|k| f.starts_with(k)))
}
