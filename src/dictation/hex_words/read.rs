//! Reading a file's bytes into a [`WordsFile`] (the spec's "Reading, step
//! by step"): the refusals first, then each part read entry by entry, where
//! the first rule an entry breaks decides why it is skipped.

use std::collections::HashSet;
use std::io::Read as _;
use std::path::Path;

use serde_json::{Map, Value};

use super::clean;
use super::{
    FORMAT, MAX_APP_CHARS, MAX_APP_VERSION_CHARS, MAX_FILE_BYTES, MAX_REPLACEMENTS, MAX_SPELLINGS,
    MAX_TERM_CHARS, MAX_TERMS, MadeBy, Place, Reason, Refusal, Replacement, Skip, Term, WordsFile,
};

/// Why a chosen file couldn't be read at all.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OpenError {
    Refused(Refusal),
    /// The path isn't a regular file: a folder, a device, a FIFO, ...
    NotFile,
    /// Opening or reading failed; the OS's own text.
    Io(String),
}

/// Reads the file at `path` into memory, at most 8 MiB and one byte of it,
/// and nothing of it is written anywhere: the person's file stays where
/// they keep it, and the read result lives only as long as its review.
pub(crate) fn open(path: &Path) -> Result<WordsFile, OpenError> {
    // The type comes before the open: opening a FIFO waits for a writer.
    let metadata = std::fs::metadata(path).map_err(|error| OpenError::Io(error.to_string()))?;
    if !metadata.is_file() {
        return Err(OpenError::NotFile);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(OpenError::Refused(Refusal::TooLarge));
    }
    let file = std::fs::File::open(path).map_err(|error| OpenError::Io(error.to_string()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| OpenError::Io(error.to_string()))?;
    read(&bytes).map_err(OpenError::Refused)
}

/// Reads a file's bytes: refused whole, or read with every skipped part
/// named.
pub(crate) fn read(bytes: &[u8]) -> Result<WordsFile, Refusal> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(Refusal::TooLarge);
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| Refusal::NotUtf8)?;
    if text
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    {
        return Err(Refusal::Empty);
    }
    super::check::check(text)?;
    // The check pass already refused everything serde_json would.
    let parsed = serde_json::from_str::<Value>(text).map_err(|_| Refusal::NotJson)?;
    let Value::Object(top) = parsed else {
        return Err(Refusal::NotAnObject);
    };
    if top.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(Refusal::NotHexWords);
    }
    let version = top
        .get("version")
        .and_then(whole_version)
        .ok_or(Refusal::BadVersion)?;
    let mut file = WordsFile {
        version,
        ..WordsFile::default()
    };
    for key in top.keys() {
        if !matches!(
            key.as_str(),
            "format" | "version" | "madeBy" | "vocabulary" | "replacements"
        ) {
            skip(
                &mut file.skipped,
                Place::Field(key.clone()),
                Reason::UnknownField,
            );
        }
    }
    if let Some(made_by) = top.get("madeBy") {
        read_made_by(made_by, &mut file);
    }
    if let Some(part) = top.get("vocabulary") {
        match part.as_array() {
            Some(list) => read_vocabulary(list, &mut file.vocabulary, &mut file.skipped),
            None => skip(
                &mut file.skipped,
                Place::Field("vocabulary".into()),
                Reason::BadPart,
            ),
        }
    }
    if let Some(part) = top.get("replacements") {
        match part.as_array() {
            Some(list) => read_replacements(list, &mut file.replacements, &mut file.skipped),
            None => skip(
                &mut file.skipped,
                Place::Field("replacements".into()),
                Reason::BadPart,
            ),
        }
    }
    Ok(file)
}

/// A whole number of at least 1; `1.0` is 1.
fn whole_version(value: &Value) -> Option<u64> {
    let number = value.as_number()?;
    if let Some(whole) = number.as_u64() {
        return (whole >= 1).then_some(whole);
    }
    let float = number.as_f64()?;
    // A whole float past u64 saturates, still newer than any version.
    (float >= 1.0 && float.fract() == 0.0).then_some(float as u64)
}

fn skip(skipped: &mut Vec<Skip>, place: Place, reason: Reason) {
    skipped.push(Skip { place, reason });
}

fn read_made_by(value: &Value, file: &mut WordsFile) {
    let Some(object) = value.as_object() else {
        skip(
            &mut file.skipped,
            Place::Field("madeBy".into()),
            Reason::BadField,
        );
        return;
    };
    let mut made_by = MadeBy::default();
    for (key, value) in object {
        let (slot, max) = match key.as_str() {
            "app" => (&mut made_by.app, MAX_APP_CHARS),
            "version" => (&mut made_by.version, MAX_APP_VERSION_CHARS),
            _ => {
                skip(
                    &mut file.skipped,
                    Place::MadeBy(key.clone()),
                    Reason::UnknownField,
                );
                continue;
            }
        };
        match value.as_str().map(clean::words) {
            Some(text)
                if !text.is_empty()
                    && clean::chars(&text) <= max
                    && !clean::hidden(&text, false) =>
            {
                *slot = Some(text);
            }
            _ => skip(
                &mut file.skipped,
                Place::MadeBy(key.clone()),
                Reason::BadField,
            ),
        }
    }
    file.made_by = made_by;
}

/// Reads the vocabulary part's entries into `terms`, in order.
pub(super) fn read_vocabulary(list: &[Value], terms: &mut Vec<Term>, skipped: &mut Vec<Skip>) {
    let mut keys = HashSet::new();
    for (index, entry) in list.iter().enumerate() {
        let place = Place::Term(index);
        if terms.len() >= MAX_TERMS {
            skip(skipped, place, Reason::OverLimit);
            continue;
        }
        let Some((object, raw)) = entry
            .as_object()
            .and_then(|object| Some((object, object.get("term")?.as_str()?)))
        else {
            skip(skipped, place, Reason::BadEntry);
            continue;
        };
        let term = clean::words(raw);
        if let Some(reason) = clean::phrase_fault(&term, MAX_TERM_CHARS, false) {
            skip(skipped, place, reason);
            continue;
        }
        let key = clean::key(&term);
        if keys.contains(&key) {
            skip(skipped, place, Reason::Duplicate);
            continue;
        }
        let sounds_like = read_term_fields(index, &key, object, skipped);
        keys.insert(key);
        terms.push(Term { term, sounds_like });
    }
}

/// An accepted term's other keys: its spellings, and anything unknown.
fn read_term_fields(
    index: usize,
    term_key: &str,
    object: &Map<String, Value>,
    skipped: &mut Vec<Skip>,
) -> Vec<String> {
    let mut sounds_like = Vec::new();
    for (field, value) in object {
        match field.as_str() {
            "term" => {}
            "soundsLike" => match value.as_array() {
                Some(spellings) => {
                    sounds_like = read_spellings(index, term_key, spellings, skipped);
                }
                None => skip(
                    skipped,
                    Place::TermField(index, field.clone()),
                    Reason::BadField,
                ),
            },
            _ => skip(
                skipped,
                Place::TermField(index, field.clone()),
                Reason::UnknownField,
            ),
        }
    }
    sounds_like
}

fn read_spellings(
    term: usize,
    term_key: &str,
    spellings: &[Value],
    skipped: &mut Vec<Skip>,
) -> Vec<String> {
    let mut kept = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for (index, value) in spellings.iter().enumerate() {
        let place = Place::Spelling(term, index);
        if kept.len() >= MAX_SPELLINGS {
            skip(skipped, place, Reason::OverLimit);
            continue;
        }
        let Some(raw) = value.as_str() else {
            skip(skipped, place, Reason::BadEntry);
            continue;
        };
        let spelling = clean::words(raw);
        if let Some(reason) = clean::phrase_fault(&spelling, MAX_TERM_CHARS, true) {
            skip(skipped, place, reason);
            continue;
        }
        let key = clean::key(&spelling);
        if key == term_key {
            skip(skipped, place, Reason::SameAsTerm);
        } else if keys.contains(&key) {
            skip(skipped, place, Reason::Duplicate);
        } else {
            keys.push(key);
            kept.push(spelling);
        }
    }
    kept
}

/// Reads the replacements part's entries into `replacements`, in order.
pub(super) fn read_replacements(
    list: &[Value],
    replacements: &mut Vec<Replacement>,
    skipped: &mut Vec<Skip>,
) {
    let mut keys = HashSet::new();
    for (index, entry) in list.iter().enumerate() {
        let place = Place::Replacement(index);
        if replacements.len() >= MAX_REPLACEMENTS {
            skip(skipped, place, Reason::OverLimit);
            continue;
        }
        let Some((object, heard, write)) = entry.as_object().and_then(|object| {
            Some((
                object,
                object.get("heard")?.as_str()?,
                object.get("write")?.as_str()?,
            ))
        }) else {
            skip(skipped, place, Reason::BadEntry);
            continue;
        };
        let (heard, write) = (clean::words(heard), clean::output(write));
        if let Some(reason) = clean::replacement_fault(&heard, &write) {
            skip(skipped, place, reason);
            continue;
        }
        let key = clean::key(&heard);
        if keys.contains(&key) {
            skip(skipped, place, Reason::Duplicate);
            continue;
        }
        keys.insert(key);
        let mut learned = false;
        for (field, value) in object {
            match (field.as_str(), value) {
                ("heard" | "write", _) => {}
                ("learned", Value::Bool(flag)) => learned = *flag,
                ("learned", _) => skip(
                    skipped,
                    Place::ReplacementField(index, field.clone()),
                    Reason::BadField,
                ),
                _ => skip(
                    skipped,
                    Place::ReplacementField(index, field.clone()),
                    Reason::UnknownField,
                ),
            }
        }
        replacements.push(Replacement {
            heard,
            write,
            learned,
        });
    }
}
