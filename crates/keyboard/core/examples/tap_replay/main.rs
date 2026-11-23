//! The tap decoder's benchmark: replays recorded or synthetic taps through the Rust tap decoder
//! alone and reports its word error rate and per-word latency, as TSV lines in the style of
//! `android/benchmark/typing/Replay.java` and as a markdown table.
//!
//! It measures the decoder (`TypingDecoder::decide` on each space, with the parameters tuned
//! for the model's language) and nothing around it: no engine contraction or typo rules, no
//! dictionary (`literal_is_valid` is always false), no capitals, no likelier-next-letter touch
//! areas, no strip. Its error rate is not the keyboard's; compare it only with itself.
//!
//! ```sh
//! cd crates/keyboard
//! # The dev split of one or more sets, each followed by its language's model:
//! cargo run --release -p hex-keyboard-core --example tap_replay -- \
//!   ../../android/benchmark/typing/held-out/en.tsv.gz ../../ios/HEXKeyboard/TypingModel/en.hextm
//! # The sealed test split: once per locked setting, recorded in bench/sealed.tsv.
//! cargo run --release -p hex-keyboard-core --example tap_replay -- --split test --sealed \
//!   <set> <model> [<set> <model> ...]
//! # A small set of its own, for a smoke test:
//! cargo run --release -p hex-keyboard-core --example tap_replay -- \
//!   --make-sample 200 /tmp/sample.jsonl ../../ios/HEXKeyboard/TypingModel/en.hextm
//! ```
//!
//! Sets (`sets.rs`): `android/benchmark/typing/held-out/<lang>.tsv.gz`, the Android
//! benchmark's JSON Lines (`export_synthetic.py`), or iOS `TypingReplayTests` tap JSON (what
//! `ios/benchmark/typing-replay/convert.py` writes from the public datasets). All are typed on
//! the 402 pt geometry `Replay.originalGeometry` gives, with no Learn how I type offsets.
//!
//! Splits: every row has a name in its set (the held-out rows' Tatoeba ID, a JSON Lines row's
//! `id`, an iOS sentence's source, session and place in it). A row is in the test split when
//! the first byte of SHA-256(`hex-tap-split-v1:` + name) is odd, else in the dev split, so the
//! splits never depend on the rows' order or the file's name. Tune on `dev` (the default).
//! `--split test` needs `--sealed`: the tool hashes the decoder parameters of every language
//! the run reads with its model's SHA-256, refuses the run when `bench/sealed.tsv` already
//! holds that hash, and appends the hash, the date and the results to it.

mod sets;

use std::fmt::Write as _;
use std::sync::Arc;

use hex_keyboard_core::decoder::{Parameters, TypingDecoder};
use hex_keyboard_core::model::Model;
use sha2::{Digest, Sha256};

use sets::{edits, geometry, make_sample, replay_row, rows, words};

/// The ledger of sealed test reads.
const LEDGER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/sealed.tsv");
const LEDGER_HEADER: &str = "hash\tdate\tlanguage\tset\tset_sha256\tsentences\twords\traw_errors\tdecoder_errors\twer\texact";

#[derive(Clone, Copy, PartialEq)]
enum Split {
    Dev,
    Test,
}

fn split_of(key: &str) -> Split {
    let digest = Sha256::digest(format!("hex-tap-split-v1:{key}"));
    if digest[0] & 1 == 1 {
        Split::Test
    } else {
        Split::Dev
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// One set's results.
struct Measured {
    set: String,
    set_sha256: String,
    language: String,
    sentences: usize,
    words: usize,
    raw_errors: usize,
    errors: usize,
    exact: usize,
    /// Rows with iOS's recorded output (`ios_shared`), and how many of those read the same.
    with_ios: usize,
    agreement: usize,
    /// Per-word `decide` times, sorted.
    nanos: Vec<u128>,
}

impl Measured {
    fn wer(&self, errors: usize) -> f64 {
        100.0 * errors as f64 / self.words.max(1) as f64
    }

    fn ms(&self, q: f64) -> f64 {
        let i = ((q * self.nanos.len() as f64) as usize).min(self.nanos.len().saturating_sub(1));
        self.nanos.get(i).map_or(0.0, |&n| n as f64 / 1e6)
    }
}

fn run(set: &str, model_path: &str, split: Split) -> Measured {
    let bytes = std::fs::read(model_path).unwrap_or_else(|e| panic!("{model_path}: {e}"));
    let model = Arc::new(Model::new(Box::new(bytes)).expect("an HXTM v3 model"));
    let language = model.language().to_owned();
    let keys = geometry(&language);
    let set_bytes = std::fs::read(set).unwrap_or_else(|e| panic!("{set}: {e}"));
    let decoder = TypingDecoder::new(model, None);
    let mut result = Measured {
        set: std::path::Path::new(set)
            .file_name()
            .map_or_else(|| set.to_owned(), |n| n.to_string_lossy().into_owned()),
        set_sha256: hex(&Sha256::digest(&set_bytes)),
        language: language.clone(),
        sentences: 0,
        words: 0,
        raw_errors: 0,
        errors: 0,
        exact: 0,
        with_ios: 0,
        agreement: 0,
        nanos: Vec::new(),
    };
    for row in rows(set, &keys)
        .iter()
        .filter(|r| split_of(&r.key) == split)
    {
        let reference = words(&row.intended);
        let raw: String = row.typed.iter().map(|&(c, _)| c).collect();
        let out = replay_row(&decoder, &keys, row, &mut result.nanos);
        let err = edits(&reference, &words(&out));
        result.sentences += 1;
        result.words += reference.len();
        result.raw_errors += edits(&reference, &words(&raw));
        result.errors += err;
        result.exact += usize::from(err == 0);
        if let Some(ios) = &row.ios {
            result.with_ios += 1;
            result.agreement += usize::from(words(ios) == words(&out));
        }
    }
    result.nanos.sort_unstable();
    result
}

/// Today's date, UTC, as YYYY-MM-DD (days since 1970 to a civil date).
#[allow(clippy::disallowed_methods)] // The ledger records when a sealed read ran.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_secs();
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn usage() -> ! {
    eprintln!(
        "usage: tap_replay [--split dev|test [--sealed]] <set> <model.hextm> [<set> <model.hextm> ...]\n       tap_replay --make-sample <rows> <out.jsonl> <model.hextm>"
    );
    std::process::exit(2);
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--make-sample") {
        let [_, rows, out, model] = args.as_slice() else {
            usage()
        };
        let bytes = std::fs::read(model).unwrap_or_else(|e| panic!("{model}: {e}"));
        let model = Model::new(Box::new(bytes)).expect("an HXTM v3 model");
        let keys = geometry(model.language());
        make_sample(rows.parse().expect("a row count"), out, &model, &keys);
        return;
    }
    let (mut split, mut sealed) = (Split::Dev, false);
    while args.first().is_some_and(|a| a.starts_with("--")) {
        match args.remove(0).as_str() {
            "--sealed" => sealed = true,
            "--split" if !args.is_empty() => {
                split = match args.remove(0).as_str() {
                    "dev" => Split::Dev,
                    "test" => Split::Test,
                    _ => usage(),
                }
            }
            _ => usage(),
        }
    }
    if args.is_empty() || !args.len().is_multiple_of(2) || sealed != (split == Split::Test) {
        if split == Split::Test && !sealed {
            eprintln!(
                "tap_replay: the test split is sealed: read it with --sealed, once per locked setting"
            );
        }
        usage();
    }
    // The setting's hash, before any test row is read: every language's parameters and model.
    let mut settings: Vec<String> = Vec::new();
    for pair in args.chunks(2) {
        let bytes = std::fs::read(&pair[1]).unwrap_or_else(|e| panic!("{}: {e}", pair[1]));
        let model = Model::new(Box::new(bytes.clone())).expect("an HXTM v3 model");
        let language = model.language().to_owned();
        settings.push(format!(
            "{language}\t{:?}\t{}\n",
            Parameters::tuned(&language),
            hex(&Sha256::digest(&bytes))
        ));
    }
    settings.sort();
    settings.dedup();
    let hash = hex(&Sha256::digest(format!(
        "hex-tap-bench-v1\n{}",
        settings.concat()
    )))[..16]
        .to_owned();
    let ledger = std::fs::read_to_string(LEDGER).unwrap_or_default();
    if sealed
        && ledger
            .lines()
            .any(|l| l.split('\t').next() == Some(hash.as_str()))
    {
        eprintln!(
            "tap_replay: setting {hash} already had its sealed test read ({LEDGER}); change the parameters or a model to read it again"
        );
        std::process::exit(1);
    }

    let results: Vec<Measured> = args
        .chunks(2)
        .map(|pair| run(&pair[0], &pair[1], split))
        .collect();
    let split_name = if split == Split::Test { "test" } else { "dev" };
    println!("setting\t{hash}\tsplit\t{split_name}");
    for r in &results {
        let name = format!("{}_{}", r.language, r.set);
        println!(
            "wer\t{name}_raw\t{}\t{}\t{:.4}",
            r.raw_errors,
            r.words,
            r.wer(r.raw_errors)
        );
        println!(
            "wer\t{name}_rust_decoder\t{}\t{}\t{:.4}\t{}/{}",
            r.errors,
            r.words,
            r.wer(r.errors),
            r.exact,
            r.sentences
        );
        if r.with_ios > 0 {
            println!("agreement\t{name}\t{}/{}", r.agreement, r.with_ios);
        }
        println!(
            "latency\t{name}_decide\t{}\t{:.4}\t{:.4}\t{:.4}",
            r.nanos.len(),
            r.ms(0.5),
            r.ms(0.95),
            r.ms(1.0)
        );
    }
    println!();
    println!("Tap decoder alone, {split_name} split, setting `{hash}`:");
    println!();
    println!(
        "| Set | Language | Sentences | Words | Raw hit keys | Rust tap decoder | Sentences exact | `decide` median / p95 ms |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|");
    for r in &results {
        println!(
            "| {} | {} | {} | {} | {} ({:.2}%) | {} ({:.2}%) | {} / {} | {:.3} / {:.3} |",
            r.set,
            r.language,
            r.sentences,
            r.words,
            r.raw_errors,
            r.wer(r.raw_errors),
            r.errors,
            r.wer(r.errors),
            r.exact,
            r.sentences,
            r.ms(0.5),
            r.ms(0.95)
        );
    }
    if sealed {
        let date = today();
        let mut text = if ledger.is_empty() {
            format!("{LEDGER_HEADER}\n")
        } else {
            ledger
        };
        for r in &results {
            let _ = writeln!(
                text,
                "{hash}\t{date}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{}",
                r.language,
                r.set,
                r.set_sha256,
                r.sentences,
                r.words,
                r.raw_errors,
                r.errors,
                r.wer(r.errors),
                r.exact
            );
        }
        std::fs::write(LEDGER, text).unwrap_or_else(|e| panic!("{LEDGER}: {e}"));
        eprintln!("tap_replay: recorded setting {hash}'s sealed read in {LEDGER}");
    }
}
