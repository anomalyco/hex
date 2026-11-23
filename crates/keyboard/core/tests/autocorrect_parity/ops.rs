//! One case's operations on a fresh `Autocorrect`, as the Swift `ACParityRunner` performs them
//! on the oracle, each answered the way the fixture writes it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use hex_keyboard_core::autocorrect::{
    self as ac, Autocorrect, Bundled, Change, FieldKind, GermanCapitals, KeyCell, KeyGrid,
    LanguageConfig, LanguageId, SuggestionKind, TypingLearning,
};
use hex_keyboard_core::decoder::Keys;
use hex_keyboard_core::model::Model;
use serde_json::{Value, json};

use crate::stub::{CallLog, StubChecker, StubDictionary};

pub struct World {
    pub dictionaries: Rc<HashMap<String, StubDictionary>>,
    pub models: HashMap<String, Arc<Model>>,
    pub keysets: HashMap<u64, Keys>,
    pub german: Option<Arc<GermanCapitals>>,
    /// The exporter's standard list of dictionaries (`"avail": "all"`).
    pub available: Vec<String>,
    /// The letter grids by name ("fr-CA/qwerty"): the layout's language and its cells.
    pub grids: HashMap<String, (String, Vec<KeyCell>)>,
}

pub struct Run {
    pub outputs: Vec<Value>,
    pub hash: u64,
    pub calls: usize,
    pub lines: Option<Vec<String>>,
}

fn bits(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().expect("bits"), 16).expect("hex"))
}

fn point(v: &Value) -> (f64, f64) {
    (bits(&v[0]), bits(&v[1]))
}

fn points(v: &Value) -> Option<Vec<(f64, f64)>> {
    Some(v.as_array()?.iter().map(point).collect())
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|s| s.as_str().expect("a string").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn pairs(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|p| {
                    (
                        p[0].as_str().unwrap().to_owned(),
                        p[1].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn language(v: &Value) -> Option<&str> {
    v.as_str()
}

/// A grid line: the layout's language and (letter, row, column) cells.
pub fn grid(v: &Value) -> (String, Vec<KeyCell>) {
    let cells = v["cells"]
        .as_array()
        .expect("cells")
        .iter()
        .map(|c| KeyCell {
            letter: c[0].as_str().unwrap().chars().next().unwrap(),
            row: bits(&c[1]),
            column: bits(&c[2]),
        })
        .collect();
    (v["l"].as_str().unwrap().to_owned(), cells)
}

fn field(v: &Value) -> FieldKind {
    match v.as_str() {
        Some("url") => FieldKind::Url,
        Some("email") => FieldKind::Email,
        _ => FieldKind::Text,
    }
}

fn opt(s: Option<String>) -> Value {
    s.map_or(Value::Null, Value::String)
}

impl World {
    fn checker(&self, tag: &str, log: &Rc<RefCell<CallLog>>, any: bool) -> Box<StubChecker> {
        Box::new(StubChecker {
            tag: tag.to_owned(),
            log: log.clone(),
            dictionaries: self.dictionaries.clone(),
            accepts_everything: any,
            code: "en_US".to_owned(),
        })
    }

    fn grid(&self, name: &Value) -> (&str, &[KeyCell]) {
        let (layout, cells) = &self.grids[name.as_str().expect("a grid's name")];
        (layout, cells)
    }

    fn available(&self, v: &Value) -> Vec<String> {
        if v.as_str() == Some("all") {
            self.available.clone()
        } else {
            strings(v)
        }
    }

    fn model(&self, v: &Value) -> Option<Arc<Model>> {
        v.as_str().and_then(|m| self.models.get(m).cloned())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &self,
        tsv: &str,
        ops: &[Value],
        expected: &[Value],
        learned: &[String],
        grid0: &Value,
        keep: bool,
    ) -> Run {
        let log = Rc::new(RefCell::new(CallLog::new(keep)));
        let bundled = Arc::new(Bundled {
            never_suggest: Arc::new(ac::NeverSuggestList::new(tsv)),
            german_capitals: self.german.clone(),
        });
        let mut e = Autocorrect::new(bundled, self.checker("p", &log, false));
        e.set_learned_words(learned);
        // The host hands over the keys it shows before anything else, and whenever they change.
        let (layout, cells) = self.grid(grid0);
        e.set_keys(layout, cells);
        let mut outputs = Vec::new();
        for (i, op) in ops.iter().enumerate() {
            let r = self.perform(op, &mut e, &log);
            let s = json!([
                e.last_fix_language(),
                e.last_revision_language(),
                e.own_words_revision(),
                e.partner_language(),
                e.has_dictionary(),
            ]);
            outputs.push(json!({"r": r, "s": s}));
            // The host hands over the kept words whenever they change.
            if let Some(out) = expected.get(i) {
                if let Some(words) = out.get("lw") {
                    e.set_learned_words(&strings(words));
                }
                if let Some(words) = out.get("plw") {
                    e.set_partner_learned_words(&strings(words));
                }
                if let Some(g) = out.get("g") {
                    let (layout, cells) = self.grid(g);
                    e.set_keys(layout, cells);
                }
            }
        }
        let log = log.borrow();
        Run {
            outputs,
            hash: log.hash,
            calls: log.count,
            lines: log.lines.clone(),
        }
    }

    fn perform(&self, op: &Value, e: &mut Autocorrect, log: &Rc<RefCell<CallLog>>) -> Value {
        let s = |k: &str| op[k].as_str().map(str::to_owned);
        let b = |k: &str| op[k].as_bool().unwrap_or(false);
        match op["op"].as_str().expect("an op") {
            "lang" => e.set_language(language(&op["l"]).unwrap(), &self.available(&op["avail"])),
            // The grid it makes comes after it, as the host hands it over.
            "keys" => {}
            "partner" => {
                let checker = self.checker(op["tag"].as_str().unwrap(), log, b("any"));
                e.set_partner(language(&op["l"]), &self.available(&op["avail"]), checker);
            }
            "perlang" => e.set_learns_per_language(b("on")),
            "enabled" => e.set_enabled(b("on")),
            "swear" => e.set_allows_swearing(b("on")),
            "caps" => e.set_adds_capitals(b("on")),
            "lexicon" => e.load_lexicon_entries(&pairs(&op["entries"])),
            "vocab" => e.set_vocabulary(&strings(&op["terms"])),
            "learning" => {
                let learning = |t: &str, r: &str| {
                    let typos = pairs(&op[t]);
                    TypingLearning::new(
                        &typos
                            .iter()
                            .map(|(a, b)| (a.as_str(), b.as_str()))
                            .collect::<Vec<_>>(),
                        &strings(&op[r])
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>(),
                    )
                };
                let mine = learning("typos", "rejected");
                let theirs = (!op["ptypos"].is_null()).then(|| learning("ptypos", "prejected"));
                e.apply_learning(&mine, theirs.as_ref());
            }
            "decoder" => e.set_decoder(self.model(&op["model"])),
            "pdecoder" => e.set_partner_decoder(self.model(&op["model"])),
            "context" => e.set_context_model(self.model(&op["model"])),
            "dkeys" => e.set_decoder_keys(
                op["keys"]
                    .as_u64()
                    .and_then(|k| self.keysets.get(&k).cloned()),
            ),
            "bkeys" => e.set_decoder_baseline_keys(
                op["keys"]
                    .as_u64()
                    .and_then(|k| self.keysets.get(&k).cloned()),
            ),
            "taps" => e.set_decoder_taps(points(&op["taps"])),
            "german" => e.set_german_capitals(if s("set").as_deref() == Some("bundled") {
                self.german.clone()
            } else {
                None
            }),
            "keep" => e.suppress_and_keep(&s("w").unwrap()),
            "erase" => {}
            "forget" => e.forget_correction(),
            "suggest" => {
                let model = self.model(&op["model"]);
                let out = e.suggestions(
                    s("before").as_deref(),
                    field(&op["field"]),
                    b("off"),
                    model.as_deref(),
                    b("cap"),
                );
                return Value::Array(
                    out.into_iter()
                        .map(|x| {
                            let kind = match x.kind {
                                SuggestionKind::KeepLiteral => "literal",
                                SuggestionKind::Primary { auto_replace: true } => "auto",
                                SuggestionKind::Primary {
                                    auto_replace: false,
                                } => "primary",
                                SuggestionKind::Secondary => "secondary",
                                SuggestionKind::NextWord => "next",
                            };
                            json!([x.display_text, x.replacement, kind])
                        })
                        .collect(),
                );
            }
            "space" => {
                let fix =
                    e.replacement_on_space(s("before").as_deref(), field(&op["field"]), b("off"));
                return fix.map_or(Value::Null, |(o, r)| json!([o, r]));
            }
            "revision" => {
                let w = s("w").unwrap();
                return opt(e.revision(
                    &w,
                    &s("before").unwrap(),
                    &s("next").unwrap(),
                    field(&op["field"]),
                    b("off"),
                ));
            }
            "merge" => {
                let taps = points(&op["ftaps"]).unwrap_or_default();
                return opt(e.merge(
                    &s("first").unwrap(),
                    &taps,
                    point(&op["space"]),
                    &s("second").unwrap(),
                    &s("before").unwrap(),
                ));
            }
            "replacement" => {
                return e
                    .text_replacement(s("before").as_deref())
                    .map_or(Value::Null, |(t, r)| json!([t, r]));
            }
            "offer" => return json!(e.may_offer(&s("w").unwrap(), s("after").as_deref())),
            "knows" => return json!(e.knows_as_typed(&s("w").unwrap())),
            "lean" => {
                let from = op["from"].as_u64().unwrap() as usize;
                return e
                    .language_lean(&s("before").unwrap(), from)
                    .map_or(Value::Null, |l| json!(l));
            }
            "choice" => {
                let w = s("w").unwrap();
                return opt(e.context_choice(
                    &w,
                    s("prev").as_deref(),
                    s("next").as_deref(),
                    b("starts"),
                ));
            }
            "stray" => {
                return json!(e.without_stray_capital(&s("w").unwrap(), &s("typed").unwrap()));
            }
            "added" => return opt(e.without_added_capitals(&s("w").unwrap(), &s("typed").unwrap())),
            "addedpool" => {
                return json!(
                    e.without_added_capitals_pool(strings(&op["pool"]), &s("typed").unwrap())
                );
            }
            "ownspelling" => return json!(e.is_own_spelling(&s("w").unwrap())),
            "ownwords" => return json!(e.own_words()),
            "vterm" => return opt(e.vocabulary_term(&s("w").unwrap())),
            "record" => {
                let changes = op["changes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| Change {
                        from: c[0].as_str().unwrap().to_owned(),
                        to: c[1].as_str().unwrap().to_owned(),
                        language: language(&c[2]).map(LanguageId::new),
                    })
                    .collect();
                e.record_applied_correction(
                    &s("original").unwrap(),
                    &s("corrected").unwrap(),
                    &s("anchor").unwrap(),
                    changes,
                );
            }
            "revertible" => {
                return e
                    .revertible_correction(s("before").as_deref())
                    .map_or(Value::Null, |c| {
                        let changes: Vec<Value> = c
                            .changes
                            .iter()
                            .map(|x| {
                                json!([x.from, x.to, x.language.as_ref().map(LanguageId::as_str)])
                            })
                            .collect();
                        json!([c.original, c.corrected, c.anchor, changes])
                    });
            }
            "blocked" => {
                let mut words = e.blocked_words();
                words.sort();
                return json!(words);
            }
            "swipereadings" => {
                let Some(model) = self.model(&op["model"]) else {
                    return Value::Null;
                };
                return json!(e.readings_offering_own_capitals(
                    strings(&op["readings"]),
                    b("caps"),
                    &model
                ));
            }
            "haswords" => return json!(e.has_words()),
            "token" => return opt(ac::current_token(s("before").as_deref())),
            "sentence" => {
                return json!(ac::is_sentence_start(
                    s("before").as_deref(),
                    LanguageConfig::for_tag(language(&op["l"]).unwrap())
                ));
            }
            "prevword" => {
                return opt(ac::previous_word(
                    &s("before").unwrap(),
                    &s("token").unwrap(),
                ));
            }
            "starts" => {
                return json!(ac::starts_sentence(
                    &s("before").unwrap(),
                    &s("token").unwrap()
                ));
            }
            "matchcase" => {
                return json!(ac::match_case(&s("typed").unwrap(), &s("target").unwrap()));
            }
            "cost" => {
                let (_, cells) = self.grid(&op["grid"]);
                let cost = ac::single_edit_keyboard_cost(
                    &s("s").unwrap(),
                    &s("t").unwrap(),
                    LanguageConfig::for_tag(language(&op["l"]).unwrap()),
                    &KeyGrid::new(cells),
                );
                return cost.map_or(Value::Null, |c| json!(format!("{:x}", c.to_bits())));
            }
            "vowel" => return json!(ac::has_vowel(&s("w").unwrap())),
            "vspell" => {
                return opt(ac::vocabulary_spelling(
                    &s("token").unwrap(),
                    &s("term").unwrap(),
                ));
            }
            other => panic!("unknown op {other}"),
        }
        Value::Null
    }
}
