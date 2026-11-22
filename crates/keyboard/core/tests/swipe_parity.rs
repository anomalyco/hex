//! The Rust swipe decoder reads every swipe as the Swift one it was ported from does: the same
//! candidates in the same order and the same `scored`. Costs are the core's own since it
//! computes exp, log, atan2 and cos with the `libm` crate rather than Apple's libm: they must
//! equal a file's bit for bit when its header says they are the core's (`"costs"`), as the
//! committed fixture's are; against the Swift decoder's costs, the changed ones are counted.
//!
//! The cases come from `SwipeParityExportTests` (`ios/HEXMobileTests/SwipeParity`), which
//! documents the format: the committed `tests/fixtures/swipe-parity.jsonl`, with the Swift
//! decoder's readings, and with `HEX_SWIPE_PARITY=/path/to.jsonl` a larger export too (for
//! timings, run it with `--release`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use hex_keyboard_core::model::Model;
use hex_keyboard_core::never_suggest::NeverSuggest;
use hex_keyboard_core::swipe::{ExtraWord, Keys, Parameters, SwipeDecoder};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn reads_every_swipe_as_the_swift_decoder() {
    let committed = format!(
        "{}/tests/fixtures/swipe-parity.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    check(&committed);
    if let Some(large) = std::env::var_os("HEX_SWIPE_PARITY") {
        check(&large.to_string_lossy());
    }
}

struct Layout {
    keys: Keys,
    /// A split layout's left edge, right edge and gap (`KeyboardLayout.joinedForSwipe`).
    join: Option<(f64, f64, f64)>,
}

fn check(path: &str) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut models: HashMap<String, Arc<Model>> = HashMap::new();
    let mut decoders: HashMap<u64, (String, SwipeDecoder)> = HashMap::new();
    let mut layouts: HashMap<u64, Layout> = HashMap::new();
    let mut cases: HashMap<String, usize> = HashMap::new();
    let mut differences: Vec<String> = Vec::new();
    // Whether the file's costs are the core's, so they must match bit for bit.
    let mut core_costs = false;
    let (mut candidates, mut changed_costs, mut max_ulps) = (0usize, 0usize, 0u64);
    // Changed costs by how far: 1 ulp, 2 ulps, 3 to 1,000, more.
    let mut by_size = [0usize; 4];
    let mut rust_ns: Vec<u128> = Vec::new();
    let mut swift_ns: Vec<u128> = Vec::new();
    for line in text.lines() {
        let v: Value = serde_json::from_str(line).expect("a JSON line");
        match v["kind"].as_str().expect("kind") {
            "header" => {
                assert_eq!(v["generator"], 1, "{path}: a generator this test reads");
                core_costs = v["costs"].is_string();
                check_parameters(&v["parameters"]);
                for (language, digest) in v["models"].as_object().expect("models") {
                    let bytes = bundled_model(language);
                    assert_eq!(
                        hex(&Sha256::digest(&bytes)),
                        digest.as_str().unwrap(),
                        "{path} was written for another {language} model: export it again"
                    );
                    let model = Model::new(Box::new(bytes)).expect("a valid model");
                    models.insert(language.clone(), Arc::new(model));
                }
            }
            "context" => {
                let language = v["language"].as_str().unwrap().to_owned();
                let model = models[&language].clone();
                let blocked: Vec<&str> = strings(&v["blocked"]);
                let never_suggest = NeverSuggest::new(blocked, &model);
                let extra = v["extra"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|w| ExtraWord {
                        text: w[0].as_str().unwrap().to_owned(),
                        cost: bits(&w[1]),
                        contraction: w[2].as_bool().unwrap(),
                    })
                    .collect();
                let decoder = SwipeDecoder::new(model, Parameters::default(), never_suggest, extra);
                decoders.insert(v["id"].as_u64().unwrap(), (language, decoder));
            }
            "keys" => {
                let centres = v["centres"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| {
                        let mut letter = c[0].as_str().unwrap().chars();
                        let character = letter.next().unwrap();
                        assert!(letter.next().is_none(), "one letter a key");
                        (character, decimal(&c[1]), decimal(&c[2]))
                    })
                    .collect();
                let pitch = (decimal(&v["pitch"][0]), decimal(&v["pitch"][1]));
                let join = (!v["join"].is_null()).then(|| {
                    (
                        decimal(&v["join"][0]),
                        decimal(&v["join"][1]),
                        decimal(&v["join"][2]),
                    )
                });
                let keys = Keys { centres, pitch };
                layouts.insert(v["id"].as_u64().unwrap(), Layout { keys, join });
            }
            "case" => {
                let (language, decoder) = &decoders[&v["context"].as_u64().unwrap()];
                let layout = &layouts[&v["keys"].as_u64().unwrap()];
                let points = joined(&screen(v["path"].as_str().unwrap()), layout.join);
                let times = v["ticks"].as_str().map(|ticks| {
                    let base = decimal(&v["t0"]);
                    let mut tick = 0i64;
                    unpack(ticks)
                        .into_iter()
                        .map(|step| {
                            tick += step;
                            // `SwipeParity.Case.times`.
                            base + tick as f64 * 0.001
                        })
                        .collect::<Vec<f64>>()
                });
                assert_eq!(
                    inputs(&points, times.as_deref()),
                    v["inputs"].as_str().unwrap(),
                    "case {}: the path and times rebuilt from the fixture aren't the fixture's",
                    v["id"]
                );
                let previous = v["previous"].as_str();
                let limit = v["limit"].as_u64().unwrap() as usize;
                let started = Instant::now();
                let reading =
                    decoder.read(&points, times.as_deref(), &layout.keys, previous, limit);
                rust_ns.push(started.elapsed().as_nanos());
                if let Some(ns) = v["ns"].as_u64() {
                    swift_ns.push(u128::from(ns));
                }
                *cases.entry(language.clone()).or_default() += 1;
                let expected: Vec<(String, String)> = v["out"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| {
                        (
                            c[0].as_str().unwrap().to_owned(),
                            c[1].as_str().unwrap().to_owned(),
                        )
                    })
                    .collect();
                let got: Vec<(String, String)> = reading
                    .candidates
                    .iter()
                    .map(|c| (c.text.clone(), format!("{:016x}", c.cost.to_bits())))
                    .collect();
                let scored = v["scored"].as_u64().unwrap() as usize;
                let words =
                    |c: &[(String, String)]| c.iter().map(|c| c.0.clone()).collect::<Vec<_>>();
                let mut costs_differ = false;
                // Costs compared where the same word is read in the same place.
                for (a, b) in expected.iter().zip(&got).filter(|(a, b)| a.0 == b.0) {
                    candidates += 1;
                    let ulps = ulps(&a.1, &b.1);
                    if ulps > 0 {
                        changed_costs += 1;
                        by_size[match ulps {
                            1 => 0,
                            2 => 1,
                            3..=1000 => 2,
                            _ => 3,
                        }] += 1;
                        max_ulps = max_ulps.max(ulps);
                        costs_differ = true;
                    }
                }
                if words(&expected) != words(&got)
                    || reading.scored != scored
                    || (core_costs && costs_differ)
                {
                    differences.push(format!(
                        "case {} ({language} {}, {:?}): swift {expected:?} of {scored}, rust {got:?} of {}",
                        v["id"], v["type"], v["word"], reading.scored
                    ));
                }
            }
            other => panic!("{path}: unknown line kind {other}"),
        }
    }
    let mut counts: Vec<_> = cases.iter().collect();
    counts.sort();
    println!(
        "{path}: cases {counts:?}, differences {}; costs {} ({candidates} candidates): {changed_costs} changed (1 ulp {}, 2 ulps {}, 3-1000 {}, more {}), max {max_ulps} ulp",
        differences.len(),
        if core_costs {
            "the core's"
        } else {
            "the Swift decoder's"
        },
        by_size[0],
        by_size[1],
        by_size[2],
        by_size[3],
    );
    println!("  rust read {}", timing(&mut rust_ns));
    if !swift_ns.is_empty() {
        println!("  swift read {}", timing(&mut swift_ns));
    }
    for d in differences.iter().take(20) {
        println!("  {d}");
    }
    assert!(cases.values().sum::<usize>() > 0, "{path} has no cases");
    assert!(
        differences.is_empty(),
        "{} readings differ from Swift's",
        differences.len()
    );
}

/// How many representable doubles apart two costs (IEEE 754 bits in hex) are.
fn ulps(a: &str, b: &str) -> u64 {
    let (a, b) = (
        u64::from_str_radix(a, 16).unwrap(),
        u64::from_str_radix(b, 16).unwrap(),
    );
    // Costs are finite and positive, so their bits order as their values do.
    a.abs_diff(b)
}

fn check_parameters(swift: &Value) {
    let p = Parameters::default();
    let ours: [(&str, String); 37] = [
        ("samples", p.samples.to_string()),
        ("startRadius", p.start_radius.to_string()),
        ("endRadius", p.end_radius.to_string()),
        ("maxStartKeys", p.max_start_keys.to_string()),
        ("maxEndKeys", p.max_end_keys.to_string()),
        ("nearRadius", p.near_radius.to_string()),
        ("minLengthRatio", p.min_length_ratio.to_string()),
        ("maxLengthRatio", p.max_length_ratio.to_string()),
        ("startSigma", p.start_sigma.to_string()),
        ("endSigma", p.end_sigma.to_string()),
        ("startStraySigma", p.start_stray_sigma.to_string()),
        ("endStraySigma", p.end_stray_sigma.to_string()),
        ("stray", p.stray.to_string()),
        ("locationWeight", p.location_weight.to_string()),
        ("shapeWeight", p.shape_weight.to_string()),
        ("lmWeight", p.lm_weight.to_string()),
        ("shortlist", p.shortlist.to_string()),
        ("prefilter", p.prefilter.to_string()),
        ("band", p.band.to_string()),
        ("minimumLength", p.minimum_length.to_string()),
        (
            "contractionMissingPair",
            p.contraction_missing_pair.to_string(),
        ),
        ("evidenceStep", p.evidence_step.to_string()),
        ("turnWindow", p.turn_window.to_string()),
        ("turnFrom", p.turn_from.to_string()),
        ("turnFull", p.turn_full.to_string()),
        ("dwellWindow", p.dwell_window.to_string()),
        ("dwellFrom", p.dwell_from.to_string()),
        ("dwellFull", p.dwell_full.to_string()),
        ("evidenceFloor", p.evidence_floor.to_string()),
        ("evidenceSpacing", p.evidence_spacing.to_string()),
        ("evidenceGuard", p.evidence_guard.to_string()),
        ("evidenceSigma", p.evidence_sigma.to_string()),
        ("evidenceReach", p.evidence_reach.to_string()),
        ("unexplainedCost", p.unexplained_cost.to_string()),
        ("straightKeyCost", p.straight_key_cost.to_string()),
        ("cornerKeyCost", p.corner_key_cost.to_string()),
        ("straightAngle", p.straight_angle.to_string()),
    ];
    let swift = swift.as_object().expect("parameters");
    assert_eq!(
        swift.len(),
        ours.len(),
        "Swift's Parameters has {} fields",
        swift.len()
    );
    for (name, value) in ours {
        let theirs: f64 = swift[name].as_str().unwrap().parse().unwrap();
        assert_eq!(theirs, value.parse::<f64>().unwrap(), "Parameters.{name}");
    }
}

fn bundled_model(language: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../../ios/HEXKeyboard/TypingModel/{language}.hextm",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn strings(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect()
}

/// Swift's shortest round-trip decimal, which `parse` reads back exactly.
fn decimal(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}

/// An IEEE 754 bit pattern in 16 hex digits.
fn bits(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The touches, from whole quarter points packed as steps.
fn screen(packed: &str) -> Vec<(f64, f64)> {
    let steps = unpack(packed);
    let (mut x, mut y) = (0i64, 0i64);
    steps
        .chunks(2)
        .map(|s| {
            x += s[0];
            y += s[1];
            (x as f64 / 4.0, y as f64 / 4.0)
        })
        .collect()
}

/// `KeyboardLayout.joinedForSwipe`.
fn joined(points: &[(f64, f64)], join: Option<(f64, f64, f64)>) -> Vec<(f64, f64)> {
    let Some((left, right, gap)) = join else {
        return points.to_vec();
    };
    let shift = right - left - gap;
    points
        .iter()
        .map(|&(x, y)| {
            if x <= left {
                (x, y)
            } else if x >= right {
                (x - shift, y)
            } else {
                let width = right - left;
                // Swift's `max(1, width)`.
                let across = (x - left) / if width >= 1.0 { width } else { 1.0 };
                (left + across * gap, y)
            }
        })
        .collect()
}

/// `SwipeParityExportTests.inputs`: FNV-1a of the points' and times' bits.
fn inputs(points: &[(f64, f64)], times: Option<&[f64]>) -> String {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    let mut add = |value: f64| {
        for byte in value.to_bits().to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3);
        }
    };
    for &(x, y) in points {
        add(x);
        add(y);
    }
    for &t in times.unwrap_or(&[]) {
        add(t);
    }
    format!("{hash:016x}")
}

/// `JSONLines.packed`: base64 of zigzag LEB128 numbers.
fn unpack(packed: &str) -> Vec<i64> {
    let mut bytes = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in packed.bytes().filter(|&c| c != b'=') {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {c}"),
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    let mut out = Vec::new();
    let (mut z, mut shift) = (0u64, 0);
    for b in bytes {
        z |= u64::from(b & 0x7F) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            out.push((z >> 1) as i64 ^ -((z & 1) as i64));
            (z, shift) = (0, 0);
        }
    }
    out
}

fn timing(ns: &mut [u128]) -> String {
    ns.sort();
    let at = |q: usize| ns[(ns.len() * q / 100).min(ns.len() - 1)] as f64 / 1000.0;
    format!(
        "median {:.0} µs, p95 {:.0} µs, max {:.0} µs ({} reads)",
        at(50),
        at(95),
        *ns.last().unwrap() as f64 / 1000.0,
        ns.len()
    )
}
