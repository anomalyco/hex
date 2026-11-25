//! Writing the desktop's lists as a words file (the spec's "Writing"): the
//! shared vocabulary with its spellings and the Global mode's replacements,
//! cleaned, with every entry a reader would skip left out and named. The
//! reader's own rules decide what is left out, so this writer never writes a
//! file its reader would skip anything in.

use serde::Serialize;
use serde_json::{Value, json};

use super::read::{read_replacements, read_vocabulary};
use super::{FORMAT, Lists, MAX_APP_VERSION_CHARS, Skip, VERSION, clean};

/// A file ready to save, and what it leaves out.
#[derive(Clone, Debug)]
pub(crate) struct Export {
    /// UTF-8 JSON without a byte order mark.
    pub(crate) bytes: Vec<u8>,
    pub(crate) terms: usize,
    pub(crate) replacements: usize,
    /// Entries left out, with why, as places in the lists given: the
    /// `n`th term is `vocabulary[n]`, the `n`th Global replacement
    /// `replacements[n]`.
    pub(crate) left_out: Vec<Skip>,
}

/// The file for `lists`, signed as this Hex.
pub(crate) fn export(lists: &Lists) -> Export {
    export_as(lists, app_name(), env!("HEX_BUILD_VERSION"))
}

/// Which Hex the file says wrote it.
fn app_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Hex for Mac"
    } else if cfg!(windows) {
        "Hex for Windows"
    } else {
        "Hex for Linux"
    }
}

#[derive(Serialize)]
struct File<'a> {
    format: &'static str,
    version: u64,
    #[serde(rename = "madeBy")]
    made_by: FileMadeBy<'a>,
    vocabulary: Vec<FileTerm<'a>>,
    replacements: Vec<FileReplacement<'a>>,
}

#[derive(Serialize)]
struct FileMadeBy<'a> {
    app: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[derive(Serialize)]
struct FileTerm<'a> {
    term: &'a str,
    #[serde(rename = "soundsLike", skip_serializing_if = "<[String]>::is_empty")]
    sounds_like: &'a [String],
}

#[derive(Serialize)]
struct FileReplacement<'a> {
    heard: &'a str,
    write: &'a str,
}

fn export_as(lists: &Lists, app: &str, app_version: &str) -> Export {
    let vocabulary: Vec<Value> = lists
        .vocabulary
        .iter()
        .map(|entry| json!({"term": entry.term, "soundsLike": entry.sounds_like}))
        .collect();
    let replacements: Vec<Value> = lists
        .replacements
        .iter()
        .map(|rule| json!({"heard": rule.matched_phrase, "write": rule.output}))
        .collect();
    let (mut terms, mut rules, mut left_out) = (Vec::new(), Vec::new(), Vec::new());
    read_vocabulary(&vocabulary, &mut terms, &mut left_out);
    read_replacements(&replacements, &mut rules, &mut left_out);
    // A build version a reader would skip is left out rather than written.
    let version = Some(clean::words(app_version)).filter(|version| {
        !version.is_empty()
            && clean::chars(version) <= MAX_APP_VERSION_CHARS
            && !clean::hidden(version, false)
    });
    let file = File {
        format: FORMAT,
        version: VERSION,
        made_by: FileMadeBy { app, version },
        vocabulary: terms
            .iter()
            .map(|term| FileTerm {
                term: &term.term,
                sounds_like: &term.sounds_like,
            })
            .collect(),
        replacements: rules
            .iter()
            .map(|rule| FileReplacement {
                heard: &rule.heard,
                write: &rule.write,
            })
            .collect(),
    };
    let mut bytes = serde_json::to_vec_pretty(&file).expect("a words file always serializes");
    bytes.push(b'\n');
    Export {
        bytes,
        terms: terms.len(),
        replacements: rules.len(),
        left_out,
    }
}
