//! Replays recorded or synthetic taps through the Rust tap decoder alone and prints its word
//! error rate and per-word latency, in the TSV lines `android/benchmark/typing/Replay.java`
//! prints.
//!
//! It measures the decoder (`TypingDecoder::decide` on each space) and nothing around it: no
//! engine contraction or typo rules, no dictionary (`literal_is_valid` is always false), no
//! capitals, no likelier-next-letter touch areas, no strip. Its error rate is not the
//! keyboard's; compare it only with itself (a change to the decoder, or the Swift decoder on
//! the same taps).
//!
//! ```sh
//! cd crates/keyboard
//! # One of the Android benchmark's synthetic sets (export_synthetic.py), or iOS's tap JSON
//! # (ios/README.md, "Replaying real typing"):
//! cargo run --release -p hex-keyboard-core --example tap_replay -- \
//!   en-test.jsonl ../../ios/HEXKeyboard/TypingModel/en.hextm
//! # A small set of its own, for a smoke test:
//! cargo run --release -p hex-keyboard-core --example tap_replay -- \
//!   --make-sample 200 /tmp/sample.jsonl ../../ios/HEXKeyboard/TypingModel/en.hextm
//! ```
//!
//! Two input shapes: one JSON object with `sentences`, or else JSON Lines.
//!
//! - JSON Lines, the Android benchmark's engine-neutral rows: `id`,
//!   `intended`, `typed` (one character per tap, spaces included) and `taps` (`[x, y]` per
//!   character) in the 402 pt iOS geometry `Replay.originalGeometry` gives, optionally
//!   `ios_shared` (iOS's output, for an `agreement` line). As `Replay.java`, a word is read
//!   when a space follows it; the last one stays as typed unless the row ends with a space.
//! - iOS `TypingReplayTests` tap JSON (`{"sentences": [...]}`): each touch's last sample, on
//!   the same 402 pt geometry, is typed as the nearest letter key's (a space when y > 1, at
//!   its fraction along the space bar), and a space ends the sentence, as that test taps one.
//!
//! The decoder's taps are the rows' coordinates as given, the previous word is the word the
//! replay last wrote, and the key geometry is the 402 pt layout's (English and Spanish rows
//! of ten, nine (ten with ñ) and seven keys at a 39.5 pt pitch; German and Bokmål Android's
//! eleven-key rows at 36 pt; French AZERTY), with no Learn how I type offsets.

use std::sync::Arc;
use std::time::Instant;

use hex_keyboard_core::decoder::{Keys, Rect, TypingDecoder};
use hex_keyboard_core::model::Model;
use serde_json::Value;

/// `Replay.originalGeometry`: the portrait 402 pt KeyboardLayout's letter centres, row pitch
/// 56 and space bar.
fn geometry(language: &str) -> Keys {
    let wide = language == "de" || language == "nb";
    let pitch = if wide { 36.0 } else { 39.5 };
    let rows: [&str; 3] = match language {
        "de" => ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"],
        "nb" => ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"],
        "fr" => ["azertyuiop", "qsdfghjklm", "wxcvbn'"],
        "es" => ["qwertyuiop", "asdfghjklñ", "zxcvbnm"],
        _ => ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
    };
    let mut centres = Vec::new();
    for (row, letters) in rows.iter().enumerate() {
        let count = letters.chars().count() as f64;
        for (i, c) in letters.chars().enumerate() {
            if c != '\'' {
                let x = 201.0 + (i as f64 - (count - 1.0) / 2.0) * pitch;
                centres.push((c, x, 66.5 + 56.0 * row as f64));
            }
        }
    }
    Keys {
        centres,
        pitch: (pitch, 56.0),
        bottom_row_y: 66.5 + 56.0 * 2.0,
        space: Some(Rect {
            x: 64.76,
            y: 212.0,
            width: 303.1 - 64.76,
            height: 257.0 - 212.0,
        }),
    }
}

/// `Replay.words`: lowercase, split on whitespace, terminal punctuation dropped.
fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split_whitespace()
        .map(|w| {
            w.trim_end_matches(['.', ',', '!', '?', ';', ':'])
                .to_owned()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// `Replay.edits`: word-level Levenshtein distance.
fn edits(reference: &[String], hypothesis: &[String]) -> usize {
    let mut d: Vec<usize> = (0..=hypothesis.len()).collect();
    for i in 1..=reference.len() {
        let mut prev = d[0];
        d[0] = i;
        for j in 1..=hypothesis.len() {
            let old = d[j];
            let substitute = prev + usize::from(reference[i - 1] != hypothesis[j - 1]);
            d[j] = (d[j] + 1).min(d[j - 1] + 1).min(substitute);
            prev = old;
        }
    }
    d[hypothesis.len()]
}

/// A row to replay: what was meant, and each typed character with its tap.
struct Row {
    intended: String,
    typed: Vec<(char, (f64, f64))>,
    /// Whether a space ends the row after its last character (iOS's replay taps one).
    finish: bool,
    ios: Option<String>,
}

fn jsonl_rows(content: &str) -> Vec<Row> {
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let r: Value = serde_json::from_str(line).expect("a JSON row");
            let typed: Vec<char> = r["typed"].as_str().expect("typed").chars().collect();
            let taps = r["taps"].as_array().expect("taps");
            assert_eq!(typed.len(), taps.len(), "unaligned taps in {}", r["id"]);
            Row {
                intended: r["intended"].as_str().expect("intended").to_owned(),
                typed: typed
                    .into_iter()
                    .zip(taps)
                    .map(|(c, t)| (c, (t[0].as_f64().expect("x"), t[1].as_f64().expect("y"))))
                    .collect(),
                finish: false,
                ios: r["ios_shared"].as_str().map(str::to_owned),
            }
        })
        .collect()
}

fn ios_rows(content: &str, keys: &Keys) -> Vec<Row> {
    let file: Value = serde_json::from_str(content).expect("iOS tap JSON");
    // TypingReplayGrid: x across the letter area from q's left edge to p's right edge, y down
    // it from the first row's top to the third's bottom.
    let [q, p, z] = ['q', 'p', 'z'].map(|c| {
        let &(_, x, y) = keys.centres.iter().find(|k| k.0 == c).expect("q, p and z");
        (x, y)
    });
    let column = (p.0 - q.0) / 9.0;
    let row = (z.1 - q.1) / 2.0;
    let space = keys.space.expect("a space bar");
    file["sentences"]
        .as_array()
        .expect("sentences")
        .iter()
        .map(|s| {
            let typed = s["taps"]
                .as_array()
                .expect("taps")
                .iter()
                .filter_map(|tap| {
                    let x = tap["x"].as_array()?.last()?.as_f64()?;
                    let y = tap["y"].as_array()?.last()?.as_f64()?;
                    if y > 1.0 {
                        let fraction = ((x - 0.25) / 0.5).clamp(0.0, 1.0);
                        let centre = space.x + fraction * space.width;
                        let x = centre.clamp(space.x + 0.5, space.max_x() - 0.5);
                        return Some((' ', (x, space.mid_y())));
                    }
                    let point = (q.0 + (x * 10.0 - 0.5) * column, q.1 + (y * 3.0 - 0.5) * row);
                    Some((nearest(keys, point), point))
                })
                .collect();
            let words: Vec<&str> = s["words"]
                .as_array()
                .expect("words")
                .iter()
                .filter_map(Value::as_str)
                .collect();
            Row {
                intended: words.join(" "),
                typed,
                finish: true,
                ios: None,
            }
        })
        .collect()
}

/// The letter whose key centre is nearest, in pitches.
fn nearest(keys: &Keys, (x, y): (f64, f64)) -> char {
    let distance = |&&(_, cx, cy): &&(char, f64, f64)| {
        let (dx, dy) = ((x - cx) / keys.pitch.0, (y - cy) / keys.pitch.1);
        dx * dx + dy * dy
    };
    keys.centres
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .expect("keys")
        .0
}

/// Types a row: letters gather into a word, a space reads it with the decoder.
fn replay_row(
    decoder: &TypingDecoder,
    keys: &Keys,
    row: &Row,
    latencies: &mut Vec<u128>,
) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let mut taps: Vec<(f64, f64)> = Vec::new();
    let mut previous: Option<String> = None;
    let mut finish = |out: &mut String, word: &mut String, taps: &mut Vec<(f64, f64)>| {
        if !word.is_empty() {
            let start = Instant::now();
            let decision = decoder.decide(taps, word, previous.as_deref(), false, keys);
            latencies.push(start.elapsed().as_nanos());
            let written = decision
                .and_then(|d| d.replacement)
                .unwrap_or_else(|| word.clone());
            out.push_str(&written);
            previous = written.split(' ').next_back().map(str::to_lowercase);
        }
        word.clear();
        taps.clear();
    };
    for &(c, tap) in &row.typed {
        if c == ' ' {
            finish(&mut out, &mut word, &mut taps);
            out.push(' ');
        } else {
            word.push(c);
            taps.push(tap);
        }
    }
    if row.finish {
        finish(&mut out, &mut word, &mut taps);
    } else {
        out.push_str(&word);
    }
    out
}

fn latency(name: &str, nanos: &mut [u128]) {
    if nanos.is_empty() {
        return;
    }
    nanos.sort_unstable();
    let ms = |n: u128| n as f64 / 1e6;
    println!(
        "latency\t{name}\t{}\t{:.4}\t{:.4}\t{:.4}",
        nanos.len(),
        ms(nanos[nanos.len() / 2]),
        ms(nanos[nanos.len() * 95 / 100]),
        ms(nanos[nanos.len() - 1]),
    );
}

/// SplitMix64, for the sample.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    fn gaussian(&mut self) -> f64 {
        let u = self.uniform().max(1e-12);
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * self.uniform()).cos()
    }
}

/// Writes `count` rows of common words typed as key centres plus noise (a quarter of the
/// pitch), in the JSON Lines shape: a smoke test, not a benchmark.
fn make_sample(count: usize, path: &str, model: &Model, keys: &Keys) {
    let mut by_frequency: Vec<usize> =
        (0..hex_keyboard_core::model::SortedWords::word_count(model)).collect();
    by_frequency.sort_by(|&a, &b| model.unigram_cost(a).total_cmp(&model.unigram_cost(b)));
    let mut rng = Rng(0x7A9_5EED);
    let mut lines = Vec::new();
    for id in 0..count {
        let mut intended = Vec::new();
        let mut typed = String::new();
        let mut taps = Vec::new();
        for _ in 0..(3 + rng.next() % 6) {
            let word = model.word(by_frequency[(rng.next() % 3000) as usize]);
            let letters: Vec<(char, f64, f64)> = word
                .chars()
                .filter(|&c| c != '\'')
                .filter_map(|c| keys.centres.iter().find(|k| k.0 == c).copied())
                .collect();
            if letters.len() != word.chars().filter(|&c| c != '\'').count() {
                continue;
            }
            if !intended.is_empty() {
                let space = keys.space.expect("a space bar");
                typed.push(' ');
                taps.push(serde_json::json!([
                    space.x + rng.uniform() * space.width,
                    space.mid_y()
                ]));
            }
            for (_, x, y) in letters {
                let tap = (
                    x + rng.gaussian() * 0.25 * keys.pitch.0,
                    y + rng.gaussian() * 0.25 * keys.pitch.1,
                );
                typed.push(nearest(keys, tap));
                taps.push(serde_json::json!([tap.0, tap.1]));
            }
            intended.push(word);
        }
        typed.push(' ');
        let space = keys.space.expect("a space bar");
        taps.push(serde_json::json!([space.mid_x(), space.mid_y()]));
        lines.push(
            serde_json::json!({
                "id": format!("sample-{id}"),
                "intended": intended.join(" "),
                "typed": typed,
                "taps": taps,
            })
            .to_string(),
        );
    }
    std::fs::write(path, lines.join("\n") + "\n").unwrap_or_else(|e| panic!("{path}: {e}"));
    println!("wrote {count} rows to {path}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "tap_replay <set.jsonl | taps.json> <model.hextm>\n       tap_replay --make-sample <rows> <out.jsonl> <model.hextm>";
    let (sample, set, model_path) = match args.as_slice() {
        [flag, rows, out, model] if flag == "--make-sample" => (
            Some(rows.parse::<usize>().expect("a row count")),
            out,
            model,
        ),
        [set, model] => (None, set, model),
        _ => {
            eprintln!("usage: {usage}");
            std::process::exit(2);
        }
    };
    let bytes = std::fs::read(model_path).unwrap_or_else(|e| panic!("{model_path}: {e}"));
    let model = Arc::new(Model::new(Box::new(bytes)).expect("an HXTM v3 model"));
    let language = model.language().to_owned();
    let keys = geometry(&language);
    if let Some(rows) = sample {
        make_sample(rows, set, &model, &keys);
        return;
    }
    let content = std::fs::read_to_string(set).unwrap_or_else(|e| panic!("{set}: {e}"));
    // One JSON object with sentences is iOS's tap file; anything else, JSON Lines.
    let ios = serde_json::from_str::<Value>(&content).is_ok_and(|v| v.get("sentences").is_some());
    let rows = if ios {
        ios_rows(&content, &keys)
    } else {
        jsonl_rows(&content)
    };
    let decoder = TypingDecoder::new(model, None);
    let (mut raw_errors, mut errors, mut ios_errors, mut total) = (0, 0, 0, 0);
    let (mut raw_exact, mut exact, mut ios_exact, mut agreement, mut with_ios) = (0, 0, 0, 0, 0);
    let mut latencies = Vec::new();
    for row in &rows {
        let reference = words(&row.intended);
        total += reference.len();
        let raw: String = row.typed.iter().map(|&(c, _)| c).collect();
        let out = replay_row(&decoder, &keys, row, &mut latencies);
        let (raw_err, err) = (
            edits(&reference, &words(&raw)),
            edits(&reference, &words(&out)),
        );
        raw_errors += raw_err;
        errors += err;
        raw_exact += usize::from(raw_err == 0);
        exact += usize::from(err == 0);
        if let Some(ios) = &row.ios {
            with_ios += 1;
            let ios_err = edits(&reference, &words(ios));
            ios_errors += ios_err;
            ios_exact += usize::from(ios_err == 0);
            agreement += usize::from(words(ios) == words(&out));
        }
    }
    let count = rows.len();
    let wer = |name: &str, e: usize, x: usize| {
        println!(
            "wer\t{language}_{name}\t{e}\t{total}\t{:.4}\t{x}/{count}",
            100.0 * e as f64 / total.max(1) as f64
        );
    };
    wer("raw", raw_errors, raw_exact);
    if with_ios > 0 {
        wer("ios_shared", ios_errors, ios_exact);
    }
    wer("rust_decoder", errors, exact);
    if with_ios > 0 {
        println!("agreement\t{language}\t{agreement}/{with_ios}");
    }
    latency(&format!("{language}_decide"), &mut latencies);
}
