//! Importing vocabulary from a file: CSV, TSV, or plain text, with the
//! quoting and optional header the lists people already have use. The
//! header mapping also reads Microsoft Teams' custom dictionary CSV.

use std::io::Read as _;
use std::path::Path;

use super::VocabularyEntry;

/// Most bytes an import file may hold.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// The first cells a header row may hold; such a row is skipped.
const HEADER_WORDS: &[&str] = &[
    "term",
    "term (required)",
    "word",
    "phrase",
    "display form",
    "vocabulary",
];

/// A term and the spellings its row lists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedTerm {
    pub term: String,
    pub sounds_like: Vec<String>,
}

/// What merging imported terms into the list did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Merged {
    /// New terms the list took; a row that only adds spellings to an
    /// existing term counts none.
    pub added: usize,
    /// Whether the list differs from before.
    pub changed: bool,
    /// Whether a term was cut, a spelling left out, or a term refused
    /// because the list was full.
    pub clamped: bool,
}

/// Reads and parses `path`, refusing a file over [`MAX_BYTES`] or one that
/// isn't UTF-8. A byte-order mark is skipped.
pub fn read(path: &Path) -> Result<Vec<ImportedTerm>, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("the file is larger than {MAX_BYTES} bytes"));
    }
    let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    Ok(parse(&text))
}

/// Parses a vocabulary file. The first column is the term and further
/// columns are "sounds like" spellings; a header row names a "Sounds like"
/// column, and then only that column supplies spellings, as Microsoft
/// Teams' custom dictionary does. Blank lines and lines starting with `#`
/// are skipped.
pub fn parse(text: &str) -> Vec<ImportedTerm> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let delimiter = delimiter(text);
    let mut terms = Vec::new();
    let mut header: Option<Vec<String>> = None;
    for record in records(text, delimiter) {
        if record.fields.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        let first = record.fields[0].trim();
        if !record.first_quoted && first.starts_with('#') {
            continue;
        }
        if terms.is_empty() && header.is_none() && header_word(first) {
            header = Some(record.fields.iter().map(|field| normalize(field)).collect());
            continue;
        }
        if first.is_empty() {
            continue;
        }
        terms.push(ImportedTerm {
            term: first.to_owned(),
            sounds_like: spellings(&record.fields, header.as_deref()),
        });
    }
    terms
}

/// Merges `terms` into `list` through `add_clamped`, so repeated terms
/// merge in any case and the list's limits hold.
pub fn merge(list: &mut Vec<VocabularyEntry>, terms: &[ImportedTerm]) -> Merged {
    let before = list.clone();
    let mut added = 0;
    let mut clamped = false;
    for term in terms {
        let length = list.len();
        clamped |= super::add_clamped(list, &term.term, &term.sounds_like);
        added += usize::from(list.len() > length);
    }
    Merged {
        added,
        changed: *list != before,
        clamped,
    }
}

/// One parsed record: its fields and whether its first field was quoted (a
/// quoted `#` is a term, not a comment).
struct Record {
    fields: Vec<String>,
    first_quoted: bool,
}

/// Reads RFC 4180 style records: double quotes around a field, `""` for a
/// quote inside it, and delimiters or newlines allowed inside the quotes.
fn records(text: &str, delimiter: char) -> Vec<Record> {
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut first_quoted = false;
    let mut in_quotes = false;
    let mut at_field_start = true;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if in_quotes {
            match character {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => in_quotes = false,
                other => field.push(other),
            }
            continue;
        }
        match character {
            // Whitespace before an opening quote is dropped, as spreadsheet
            // exports often leave it after a delimiter.
            '"' if at_field_start || field.trim().is_empty() => {
                field.clear();
                in_quotes = true;
                at_field_start = false;
                if fields.is_empty() {
                    first_quoted = true;
                }
            }
            other if other == delimiter => {
                fields.push(std::mem::take(&mut field));
                at_field_start = true;
            }
            '\n' | '\r' => {
                if character == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                fields.push(std::mem::take(&mut field));
                records.push(Record {
                    fields: std::mem::take(&mut fields),
                    first_quoted,
                });
                first_quoted = false;
                at_field_start = true;
            }
            other => {
                field.push(other);
                at_field_start = false;
            }
        }
    }
    if !field.is_empty() || !fields.is_empty() {
        fields.push(field);
        records.push(Record {
            fields,
            first_quoted,
        });
    }
    records
}

/// The delimiter the first non-empty line uses most outside quotes; a line
/// with none is a one-column list.
fn delimiter(text: &str) -> char {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or_default();
    let mut chosen = (',', 0);
    for candidate in [',', ';', '\t'] {
        let count = unquoted_count(line, candidate);
        if count > chosen.1 {
            chosen = (candidate, count);
        }
    }
    chosen.0
}

fn unquoted_count(line: &str, delimiter: char) -> usize {
    let mut quoted = false;
    let mut count = 0;
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
            }
            '"' => quoted = !quoted,
            other if other == delimiter && !quoted => count += 1,
            _ => {}
        }
    }
    count
}

/// The spellings a record lists: every further non-empty cell, or only the
/// header's "Sounds like" columns when it names any.
fn spellings(fields: &[String], header: Option<&[String]>) -> Vec<String> {
    let columns: Vec<usize> = match header {
        Some(header) => {
            let named: Vec<usize> = header
                .iter()
                .enumerate()
                .filter(|(_, name)| name.as_str() == "sounds like")
                .map(|(index, _)| index)
                .collect();
            if named.is_empty() {
                (1..fields.len()).collect()
            } else {
                named
            }
        }
        None => (1..fields.len()).collect(),
    };
    columns
        .into_iter()
        .filter_map(|index| fields.get(index))
        .map(|field| field.trim())
        .filter(|field| !field.is_empty())
        .map(str::to_owned)
        .collect()
}

fn header_word(cell: &str) -> bool {
    let name = normalize(cell);
    HEADER_WORDS.contains(&name.as_str())
}

/// A header cell as it is compared: lowercased, with runs of whitespace and
/// `-` or `_` as one space.
fn normalize(cell: &str) -> String {
    cell.trim()
        .to_lowercase()
        .replace(['-', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imported(term: &str, sounds_like: &[&str]) -> ImportedTerm {
        ImportedTerm {
            term: term.into(),
            sounds_like: sounds_like.iter().map(|&text| text.into()).collect(),
        }
    }

    #[test]
    fn commas_semicolons_and_tabs_pick_themselves_from_the_first_line() {
        assert_eq!(
            parse("cappuccino,capuchin\nRune,roon"),
            [
                imported("cappuccino", &["capuchin"]),
                imported("Rune", &["roon"])
            ]
        );
        assert_eq!(
            parse("cappuccino;capuchin\nRune;roon"),
            [
                imported("cappuccino", &["capuchin"]),
                imported("Rune", &["roon"])
            ]
        );
        assert_eq!(
            parse("cappuccino\tcapuchin\nRune\troon"),
            [
                imported("cappuccino", &["capuchin"]),
                imported("Rune", &["roon"])
            ]
        );
    }

    #[test]
    fn quoted_fields_keep_delimiters_quotes_and_line_breaks() {
        let text = "\"Rune, the framework\", \"roon\"\n\"say \"\"hi\"\"\", \"say hi\"\n\"two\nlines\", \"too\"";
        assert_eq!(
            parse(text),
            [
                imported("Rune, the framework", &["roon"]),
                imported("say \"hi\"", &["say hi"]),
                imported("two\nlines", &["too"]),
            ]
        );
    }

    #[test]
    fn a_byte_order_mark_crlf_comments_and_blank_lines_read_in() {
        let text = "\u{feff}# my names\r\n\r\ncappuccino,capuchin\r\n# skip me\r\nRune,roon\r\n";
        assert_eq!(
            parse(text),
            [
                imported("cappuccino", &["capuchin"]),
                imported("Rune", &["roon"])
            ]
        );
    }

    #[test]
    fn plain_text_is_one_term_a_line() {
        assert_eq!(
            parse("cappuccino\n\nRune\n  Dax  \n"),
            [
                imported("cappuccino", &[]),
                imported("Rune", &[]),
                imported("Dax", &[])
            ]
        );
    }

    #[test]
    fn a_header_row_is_skipped_and_its_sounds_like_column_wins() {
        assert_eq!(
            parse(
                "Term,Sounds like,Long form of the term (for acronyms),Definition and context\nTAC,Tee-Ay-Cee,Teams Admin Center,Admin portal managing Teams\nViva Glint,Vee-Vah-Glint,Part of Microsoft Viva,\n"
            ),
            [
                imported("TAC", &["Tee-Ay-Cee"]),
                imported("Viva Glint", &["Vee-Vah-Glint"])
            ]
        );
        // A header without a Sounds like column leaves every further cell a
        // spelling, as an unheadered row's do.
        assert_eq!(
            parse("word,spelling\nRune,roon"),
            [imported("Rune", &["roon"])]
        );
        assert_eq!(parse("Display Form\nOpenCode"), [imported("OpenCode", &[])]);
    }

    #[test]
    fn a_quoted_hash_stays_a_term() {
        assert_eq!(
            parse("\"#hashtag\", hash tag\n# comment\nRune"),
            [imported("#hashtag", &["hash tag"]), imported("Rune", &[])]
        );
    }

    #[test]
    fn merging_counts_new_terms_and_merges_repeats_in_any_case() {
        let mut list = vec![VocabularyEntry {
            term: "Rune".into(),
            sounds_like: vec!["roon".into()],
        }];
        let merged = merge(
            &mut list,
            &[
                imported("rune", &["rooney"]),
                imported("Cappuccino", &["capuchin", "cap a chino"]),
                imported("cappuccino", &["capuchino"]),
            ],
        );
        assert_eq!(
            merged,
            Merged {
                added: 1,
                changed: true,
                clamped: false
            }
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].sounds_like, ["roon", "rooney"]);
        assert_eq!(list[1].term, "Cappuccino");
        assert_eq!(
            list[1].sounds_like,
            ["capuchin", "cap a chino", "capuchino"]
        );
    }

    #[test]
    fn merging_keeps_the_limits_and_says_what_did_not_fit() {
        let mut list: Vec<_> = (0..super::super::MAX_ENTRIES)
            .map(|index| VocabularyEntry {
                term: format!("Term {index}"),
                sounds_like: Vec::new(),
            })
            .collect();
        let spellings: Vec<String> = (0..17).map(|n| format!("sound {n}")).collect();
        let merged = merge(
            &mut list,
            &[
                imported("Extra", &[]),
                imported(
                    "Term 0",
                    &spellings.iter().map(String::as_str).collect::<Vec<_>>(),
                ),
            ],
        );
        assert_eq!(merged.added, 0);
        assert!(merged.changed);
        assert!(merged.clamped);
        assert_eq!(list.len(), super::super::MAX_ENTRIES);
        assert_eq!(list[0].sounds_like.len(), super::super::MAX_SOUNDS_LIKE);
    }
}
