//! The v2 model on disk and the warm thread that runs it.
//!
//! The model is not downloaded: it is whatever folder a person puts at
//! `<models>/cleanup-v2/` (or `HEX_CLEANUP_V2_DIR`), holding
//! - `CleanupTaggerV2.mlpackage`, the Core ML package;
//! - `tokenizer.json`, its Hugging Face tokenizer;
//! - optionally `runtime.json`, how its answer is decoded.
//!
//! The package is compiled once, on the CPU, into
//! `CleanupTaggerV2.<manifest>.mlmodelc`, named for the package's manifest so
//! a new export is compiled again. The compile goes to a staging folder that
//! is renamed into place, under the folder's install lock, as v1's install
//! does. Then one thread keeps the model, its tokenizer, and its decode
//! warm (v1's [`Tagger`]), tokenizes each line, and answers by the deadline
//! a dictation gives it.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::super::stage::Skip;
use super::bpe::Bpe;
use super::decode::{ACTIONS, CASES, Decode, PUNCTS, SCHEMA, SPANS, WordLogits};

pub(super) const PACKAGE: &str = "CleanupTaggerV2.mlpackage";
pub(super) const TOKENIZER: &str = "tokenizer.json";
pub(super) const RUNTIME: &str = "runtime.json";

/// `runtime.json`, as the exporter writes it.
#[derive(Deserialize)]
struct RuntimeFile {
    #[serde(default)]
    max_seq_len: Option<usize>,
    #[serde(default)]
    decode: DecodeFile,
    #[serde(default)]
    schema: Option<SchemaFile>,
}

#[derive(Default, Deserialize)]
struct DecodeFile {
    keep_bias: Option<f64>,
    min_edit_prob: Option<f64>,
    delete_openers: Option<bool>,
    use_itn: Option<bool>,
    apply_case_punct: Option<bool>,
}

#[derive(Deserialize)]
struct SchemaFile {
    heads: std::collections::HashMap<String, Vec<String>>,
}

/// How a model folder's answer is read: the decode, and the longest run
/// the model takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Settings {
    pub(super) decode: Decode,
    pub(super) max_tokens: usize,
}

/// Reads `runtime.json`. A missing file is the shipped calibration; a
/// schema other than the one this decoder knows is refused.
pub(super) fn settings(json: Option<&str>) -> Result<Settings, String> {
    let defaults = Decode::default();
    let Some(json) = json else {
        return Ok(Settings {
            decode: defaults,
            max_tokens: 512,
        });
    };
    let file: RuntimeFile = serde_json::from_str(json)
        .map_err(|error| format!("the cleanup v2 runtime.json is unreadable: {error}"))?;
    if let Some(schema) = &file.schema {
        for (head, labels) in SCHEMA {
            let found = schema
                .heads
                .get(head)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if !found.iter().map(String::as_str).eq(labels.iter().copied()) {
                return Err(format!(
                    "the cleanup v2 model's {head} labels are not this decoder's"
                ));
            }
        }
    }
    let decode = file.decode;
    Ok(Settings {
        decode: Decode {
            keep_bias: decode.keep_bias.unwrap_or(defaults.keep_bias),
            min_edit_prob: decode.min_edit_prob.unwrap_or(defaults.min_edit_prob),
            delete_openers: decode.delete_openers.unwrap_or(defaults.delete_openers),
            use_itn: decode.use_itn.unwrap_or(defaults.use_itn),
            apply_case_punct: decode.apply_case_punct.unwrap_or(defaults.apply_case_punct),
        },
        max_tokens: file.max_seq_len.unwrap_or(512).clamp(3, 512),
    })
}

/// `<models>/cleanup-v2`, or `HEX_CLEANUP_V2_DIR` when it is set.
pub(super) fn folder() -> color_eyre::eyre::Result<PathBuf> {
    match std::env::var_os("HEX_CLEANUP_V2_DIR") {
        Some(folder) if !folder.is_empty() => Ok(PathBuf::from(folder)),
        _ => Ok(crate::transcription::models::models_dir()?.join("cleanup-v2")),
    }
}

/// The compiled model for the package in `folder`, named for its manifest.
fn compiled_name(folder: &Path) -> Result<String, Skip> {
    let manifest = folder.join(PACKAGE).join("Manifest.json");
    let digest = crate::transcription::models::sha256_hex(&manifest, None)
        .map_err(|error| Skip::Failed(format!("the cleanup v2 package is unreadable: {error}")))?;
    Ok(format!("CleanupTaggerV2.{}.mlmodelc", &digest[..16]))
}

/// The compiled model in `folder`, compiling the package first when it has
/// not been: `compile` turns a package into a compiled model at a path
/// that does not exist yet. A folder without the package or its tokenizer
/// is not installed.
pub(super) fn compiled(
    folder: &Path,
    compile: impl FnOnce(&Path, &Path) -> Result<(), String>,
) -> Result<PathBuf, Skip> {
    if !folder.join(PACKAGE).is_dir() || !folder.join(TOKENIZER).is_file() {
        return Err(Skip::NotInstalled);
    }
    let name = compiled_name(folder)?;
    let target = folder.join(&name);
    if target.is_dir() {
        return Ok(target);
    }
    let failed = |error: std::io::Error| {
        Skip::Failed(format!("could not compile the cleanup v2 model: {error}"))
    };
    let lock = std::fs::File::create(folder.join(".install.lock")).map_err(failed)?;
    fs2::FileExt::lock_exclusive(&lock).map_err(failed)?;
    if target.is_dir() {
        return Ok(target);
    }
    let staging = folder.join(".CleanupTaggerV2.compiling");
    let _ = std::fs::remove_dir_all(&staging);
    let started = std::time::Instant::now();
    if let Err(error) = compile(&folder.join(PACKAGE), &staging)
        .and_then(|()| std::fs::rename(&staging, &target).map_err(|error| error.to_string()))
    {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(Skip::Failed(format!(
            "could not compile the cleanup v2 model: {error}"
        )));
    }
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        "the cleanup v2 model is compiled"
    );
    // An earlier export's compiled model is never loaded again.
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let other = entry.file_name().to_string_lossy().into_owned();
        if other != name && other.starts_with("CleanupTaggerV2.") && other.ends_with(".mlmodelc") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    Ok(target)
}

/// One line's answer: each word's logits, and how to decode them.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::dictation::cleanup) struct Scored {
    pub(super) words: Vec<WordLogits>,
    pub(super) decode: Decode,
}

/// A loaded model, with what it needs to read a line.
pub(in crate::dictation::cleanup) struct Warm<M> {
    pub(super) model: M,
    pub(super) tokenizer: Bpe,
    pub(super) settings: Settings,
}

/// Reads one line of words with `run`, a model run that answers four heads
/// for each token id: windows of whole words, each word read at its first
/// sub-token.
pub(super) fn score<M>(
    warm: &Warm<M>,
    words: &[String],
    run: impl Fn(&M, &[i32]) -> Result<Vec<WordLogits>, String>,
) -> Result<Scored, String> {
    let mut scored = Vec::with_capacity(words.len());
    for window in warm.tokenizer.windows(words, warm.settings.max_tokens) {
        let heads = run(&warm.model, &window.input_ids)?;
        if heads.len() != window.input_ids.len() {
            return Err("the cleanup v2 model answered fewer tokens than it was given".into());
        }
        scored.extend(window.first.iter().map(|&at| heads[at]));
    }
    if scored.len() != words.len() {
        return Err("the cleanup v2 model answered fewer words than it was given".into());
    }
    Ok(Scored {
        words: scored,
        decode: warm.settings.decode,
    })
}

#[cfg(all(target_os = "macos", not(test)))]
pub(super) use core_ml::CoreMlV2;
#[cfg(target_os = "macos")]
pub(super) use core_ml::run;

/// Core ML on the CPU, through v1's bridge for compiling and loading and
/// `hex_cleanup_tagger_v2_run` for the four heads.
#[cfg(target_os = "macos")]
mod core_ml {
    use std::ffi::c_char;
    use std::ptr;

    use super::super::super::tagger::{Loaded, take_error};
    use super::{ACTIONS, CASES, PUNCTS, SPANS, WordLogits};

    unsafe extern "C" {
        fn hex_cleanup_tagger_v2_run(
            tagger: *mut std::ffi::c_void,
            input_ids: *const i32,
            length: i32,
            action: *mut f32,
            punct: *mut f32,
            casing: *mut f32,
            span: *mut f32,
            error: *mut *mut c_char,
        ) -> bool;
    }

    /// One run over `input_ids`: each token's four heads.
    pub(in crate::dictation::cleanup) fn run(
        model: &Loaded,
        input_ids: &[i32],
    ) -> Result<Vec<WordLogits>, String> {
        let count = input_ids.len();
        let length = i32::try_from(count)
            .ok()
            .filter(|&length| length > 0)
            .ok_or("the cleanup v2 model was given no words")?;
        let mut action = vec![0.0_f32; count * ACTIONS];
        let mut punct = vec![0.0_f32; count * PUNCTS];
        let mut casing = vec![0.0_f32; count * CASES];
        let mut span = vec![0.0_f32; count * SPANS];
        let mut error = ptr::null_mut();
        // SAFETY: `input_ids` holds `length` ids and each head's buffer
        // holds `length` rows of its classes, all for the whole call, and the
        // model stays loaded while it is borrowed.
        let ran = unsafe {
            hex_cleanup_tagger_v2_run(
                model.as_ptr(),
                input_ids.as_ptr(),
                length,
                action.as_mut_ptr(),
                punct.as_mut_ptr(),
                casing.as_mut_ptr(),
                span.as_mut_ptr(),
                &mut error,
            )
        };
        if !ran {
            return Err(take_error(error));
        }
        fn row<const N: usize>(values: &[f32], at: usize) -> [f32; N] {
            values[at * N..(at + 1) * N].try_into().unwrap_or([0.0; N])
        }
        Ok((0..count)
            .map(|at| WordLogits {
                action: row(&action, at),
                punct: row(&punct, at),
                case: row(&casing, at),
                span: row(&span, at),
            })
            .collect())
    }

    /// The model in the v2 folder, loaded with its tokenizer and decode.
    #[cfg(not(test))]
    #[derive(Clone, Copy)]
    pub(in crate::dictation::cleanup) struct CoreMlV2;

    #[cfg(not(test))]
    impl super::super::super::tagger::Runtime for CoreMlV2 {
        type Model = super::Warm<Loaded>;
        type Input = Vec<String>;
        type Output = super::Scored;

        fn load(&self) -> Result<Self::Model, super::Skip> {
            use super::super::super::tagger::{compile, load};
            use super::{RUNTIME, Skip, TOKENIZER};
            let folder = super::folder().map_err(|error| Skip::Failed(error.to_string()))?;
            let started = std::time::Instant::now();
            let compiled = super::compiled(&folder, compile)?;
            let read = |name: &str| std::fs::read_to_string(folder.join(name));
            let tokenizer = read(TOKENIZER)
                .map_err(|error| error.to_string())
                .and_then(|json| super::Bpe::from_json(&json))
                .map_err(Skip::Failed)?;
            let settings = super::settings(read(RUNTIME).ok().as_deref()).map_err(Skip::Failed)?;
            let model = load(&compiled).map_err(|error| {
                tracing::warn!(%error, "could not load the cleanup v2 model");
                // A compiled model Core ML cannot load is compiled again.
                let _ = std::fs::remove_dir_all(&compiled);
                Skip::Failed(error)
            })?;
            tracing::info!(
                elapsed_ms = started.elapsed().as_millis() as u64,
                keep_bias = settings.decode.keep_bias,
                min_edit_prob = settings.decode.min_edit_prob,
                "the cleanup v2 model is loaded"
            );
            Ok(super::Warm {
                model,
                tokenizer,
                settings,
            })
        }

        fn run(warm: &Self::Model, words: &Vec<String>) -> Result<super::Scored, String> {
            super::score(warm, words, run)
        }

        /// Labs › Free memory when idle's time, while that lab is on.
        fn idle(&self) -> Option<std::time::Duration> {
            crate::labs::unload_idle()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_json_sets_the_decode_and_a_missing_one_is_the_calibration() {
        let shipped = settings(None).unwrap();
        assert_eq!(shipped.decode, Decode::default());
        assert_eq!(shipped.max_tokens, 512);
        let json = r#"{"max_seq_len": 256, "decode": {"keep_bias": 0.3, "min_edit_prob": 0.6, "delete_openers": true, "apply_case_punct": true}}"#;
        let tuned = settings(Some(json)).unwrap();
        assert_eq!(tuned.max_tokens, 256);
        assert_eq!(
            tuned.decode,
            Decode {
                keep_bias: 0.3,
                min_edit_prob: 0.6,
                delete_openers: true,
                use_itn: true,
                apply_case_punct: true,
            }
        );
        let exported = include_str!("fixtures/runtime.json");
        assert_eq!(settings(Some(exported)).unwrap().decode.min_edit_prob, 0.5);
        let renamed = exported.replace("DELETE_OPENER", "DELETE_INTRO");
        assert!(
            settings(Some(&renamed))
                .unwrap_err()
                .contains("action labels")
        );
        assert!(settings(Some(r#"{"decode": 3}"#)).is_err());
    }

    #[test]
    fn a_folder_is_compiled_once_and_a_new_export_again() {
        let scratch = super::super::super::archive::tests::Scratch::new("cleanup-v2");
        let folder = scratch.0.join("cleanup-v2");
        std::fs::create_dir_all(&folder).unwrap();
        let never = |_: &Path, _: &Path| -> Result<(), String> { panic!("compiled") };
        assert_eq!(compiled(&folder, never), Err(Skip::NotInstalled));
        let package = folder.join(PACKAGE);
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("Manifest.json"), "{\"one\": 1}").unwrap();
        assert_eq!(
            compiled(&folder, never),
            Err(Skip::NotInstalled),
            "no tokenizer"
        );
        std::fs::write(folder.join(TOKENIZER), "{}").unwrap();

        let write = |package: &Path, staging: &Path| -> Result<(), String> {
            assert!(package.join("Manifest.json").is_file());
            assert!(!staging.exists());
            std::fs::create_dir_all(staging).map_err(|error| error.to_string())
        };
        let first = compiled(&folder, write).unwrap();
        assert!(first.is_dir());
        assert_eq!(compiled(&folder, never).unwrap(), first);

        // A new export has a new manifest: compiled again, the old one gone.
        std::fs::write(package.join("Manifest.json"), "{\"two\": 2}").unwrap();
        let second = compiled(&folder, write).unwrap();
        assert_ne!(second, first);
        assert!(!first.exists());

        // A failed compile leaves nothing behind.
        std::fs::write(package.join("Manifest.json"), "{\"three\": 3}").unwrap();
        let failing = |_: &Path, staging: &Path| -> Result<(), String> {
            std::fs::create_dir_all(staging).unwrap();
            Err("Core ML could not read the model".into())
        };
        assert_eq!(
            compiled(&folder, failing),
            Err(Skip::Failed(
                "could not compile the cleanup v2 model: Core ML could not read the model".into()
            ))
        );
        assert!(!folder.join(".CleanupTaggerV2.compiling").exists());
    }

    #[test]
    fn a_line_is_read_at_each_words_first_sub_token() {
        let tokenizer = super::super::bpe::tests::tiny();
        let warm = Warm {
            model: (),
            tokenizer,
            settings: Settings {
                decode: Decode::default(),
                max_tokens: 5,
            },
        };
        // Each token's action logits name its id.
        let echo = |_: &(), ids: &[i32]| -> Result<Vec<WordLogits>, String> {
            Ok(ids
                .iter()
                .map(|&id| WordLogits {
                    action: [id as f32; ACTIONS],
                    punct: [0.0; PUNCTS],
                    case: [0.0; CASES],
                    span: [0.0; SPANS],
                })
                .collect())
        };
        let words: Vec<String> = ["hello", "helo", "hello", "lo"].map(String::from).into();
        let scored = score(&warm, &words, echo).unwrap();
        let firsts: Vec<f32> = scored.words.iter().map(|word| word.action[0]).collect();
        assert_eq!(firsts, [259.0, 257.0, 259.0, 258.0]);
        let short = |_: &(), _: &[i32]| -> Result<Vec<WordLogits>, String> { Ok(Vec::new()) };
        assert!(score(&warm, &words, short).is_err());
    }
}
