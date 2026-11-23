//! The sets the replay reads, the 402 pt geometry it types them on, and typing a row.

use std::io::Read;
use std::time::Instant;

use hex_keyboard_core::decoder::{Keys, Rect, TypingDecoder};
use hex_keyboard_core::model::Model;
use serde_json::Value;

/// `Replay.originalGeometry`: the portrait 402 pt KeyboardLayout's letter centres, row pitch
/// 56 and space bar.
pub fn geometry(language: &str) -> Keys {
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
pub fn words(s: &str) -> Vec<String> {
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
pub fn edits(reference: &[String], hypothesis: &[String]) -> usize {
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
pub struct Row {
    /// The row's own name in its set, which decides its split.
    pub key: String,
    pub intended: String,
    pub typed: Vec<(char, (f64, f64))>,
    /// Whether a space ends the row after its last character (iOS's replay taps one).
    pub finish: bool,
    pub ios: Option<String>,
}

/// Reads a set: one JSON object with `sentences` is iOS's tap file, a gzip file
/// `android/benchmark/typing/held-out`'s TSV, anything else JSON Lines.
pub fn rows(path: &str, keys: &Keys) -> Vec<Row> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut text = String::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_string(&mut text)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        return held_out_rows(&text);
    }
    let content = String::from_utf8(bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
    let ios = serde_json::from_str::<Value>(&content).is_ok_and(|v| v.get("sentences").is_some());
    if ios {
        ios_rows(&content, keys)
    } else {
        jsonl_rows(&content)
    }
}

/// The held-out TSV: Tatoeba ID, contributor, reference, the hit keys (one per tap, the
/// final space included, Shift's capitals kept) and the `x,y` taps. The keys are read
/// lowercase, as the benchmark's JSON Lines hold them.
fn held_out_rows(text: &str) -> Vec<Row> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let columns: Vec<&str> = line.split('\t').collect();
            let [id, _, reference, typed, taps] = columns[..] else {
                panic!("a held-out row has five columns: {line}");
            };
            let taps: Vec<(f64, f64)> = taps
                .split(' ')
                .map(|t| {
                    let (x, y) = t.split_once(',').expect("x,y");
                    (x.parse().expect("x"), y.parse().expect("y"))
                })
                .collect();
            let typed: Vec<char> = typed.to_lowercase().chars().collect();
            assert_eq!(typed.len(), taps.len(), "unaligned taps in {id}");
            Row {
                key: format!("tatoeba:{id}"),
                intended: reference.to_owned(),
                typed: typed.into_iter().zip(taps).collect(),
                finish: false,
                ios: None,
            }
        })
        .collect()
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
                key: format!("jsonl:{}", r["id"].as_str().expect("id")),
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
    // A sentence's name: its source, session and place among that session's sentences.
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    file["sentences"]
        .as_array()
        .expect("sentences")
        .iter()
        .map(|s| {
            let session = format!(
                "{}/{}",
                s["source"].as_str().unwrap_or(""),
                s["session"].as_str().unwrap_or("")
            );
            let place = seen.entry(session.clone()).or_default();
            let key = format!("ios:{session}/{place}");
            *place += 1;
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
                key,
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
pub fn replay_row(
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
pub fn make_sample(count: usize, path: &str, model: &Model, keys: &Keys) {
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
