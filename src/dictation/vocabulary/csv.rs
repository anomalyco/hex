//! CSV import and export for the shared vocabulary, in the columns
//! Microsoft Teams' custom dictionary uses: "Term (required)",
//! "Sounds like", "Long form of the term (for acronyms)", and "Definition
//! and context". Only the first two are read; the rest are kept for other
//! tools.

use std::io::Read as _;
use std::path::Path;

use super::{MAX_ENTRIES, MAX_SOUNDS_LIKE, MAX_TERM_CHARS, VocabularyEntry};

/// The most a file may hold to be imported.
pub(crate) const MAX_IMPORT_BYTES: u64 = 1024 * 1024;

/// The header row Hex writes, and the one Teams' template carries.
pub(crate) const HEADER: &str =
    "Term (required),Sounds like,Long form of the term (for acronyms),Definition and context";

/// One term a file asks to add, with its "sounds like" spellings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImportedTerm {
    pub(crate) term: String,
    pub(crate) sounds_like: Vec<String>,
}

/// What merging a file into the list changed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct MergeSummary {
    /// New terms appended, in file order.
    pub(crate) added: usize,
    /// Existing terms that gained at least one spelling.
    pub(crate) updated: usize,
    /// Rows the limits refused: a term over the character cap, or one past
    /// the entry cap.
    pub(crate) skipped: usize,
}

/// Reads a file's terms, refusing anything larger than MAX_IMPORT_BYTES or
/// not UTF-8. A leading BOM is dropped.
pub(crate) fn read(path: &Path) -> Result<Vec<ImportedTerm>, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("couldn't open it: {error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_IMPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("couldn't read it: {error}"))?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err(format!(
            "the file is larger than {} MB",
            MAX_IMPORT_BYTES / (1024 * 1024)
        ));
    }
    let text = decode(&bytes)?;
    Ok(parse(&text))
}

/// UTF-8 text without its BOM, or why the bytes aren't text.
fn decode(bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the file isn't UTF-8 text".to_string())?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(text).to_owned())
}

/// The rows of the text that name a term, after a header row naming the
/// first column "Term" or "Term (required)" (any case) is left out, and
/// blank rows and rows without a term. Column 2's spellings split as the
/// sheet's "Sounds like" field does.
pub(crate) fn parse(text: &str) -> Vec<ImportedTerm> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut imported = Vec::new();
    let mut header = true;
    for fields in records(text) {
        if fields.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        let first = fields.first().map_or("", String::as_str).trim();
        if header {
            header = false;
            if first.eq_ignore_ascii_case("term") || first.eq_ignore_ascii_case("term (required)") {
                continue;
            }
        }
        if first.is_empty() {
            continue;
        }
        imported.push(ImportedTerm {
            term: first.to_owned(),
            sounds_like: fields
                .get(1)
                .map_or_else(Vec::new, |field| super::parse_sounds_like(field)),
        });
    }
    imported
}

/// Splits the text into RFC 4180 records: double-quoted fields keep commas
/// and line breaks, and a doubled quote inside one is a quote. CRLF and LF
/// both end a record.
fn records(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if quoted {
            match character {
                '"' if characters.peek() == Some(&'"') => {
                    characters.next();
                    field.push('"');
                }
                '"' => quoted = false,
                other => field.push(other),
            }
            continue;
        }
        match character {
            '"' if field.is_empty() => quoted = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                finish(&mut records, &mut record, &mut field);
            }
            '\n' => finish(&mut records, &mut record, &mut field),
            other => field.push(other),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

/// Ends one record, with its last field.
fn finish(records: &mut Vec<Vec<String>>, record: &mut Vec<String>, field: &mut String) {
    record.push(std::mem::take(field));
    records.push(std::mem::take(record));
}

/// Merges the imported rows into the list: a term already listed keeps its
/// entry and gains the spellings it lacks, and a new term appends in file
/// order. Terms and spellings past their character caps are skipped, as is a
/// new term once the list holds MAX_ENTRIES.
pub(crate) fn merge(list: &mut Vec<VocabularyEntry>, imported: &[ImportedTerm]) -> MergeSummary {
    let mut summary = MergeSummary::default();
    for row in imported {
        let term = super::clean(&row.term);
        if term.is_empty() {
            continue;
        }
        if term.chars().count() > MAX_TERM_CHARS {
            summary.skipped += 1;
            continue;
        }
        match list
            .iter()
            .position(|entry| super::same(&entry.term, &term))
        {
            Some(index) => {
                let mut kept = list[index].sounds_like.clone();
                let before = kept.len();
                add_spellings(&mut kept, &term, &row.sounds_like);
                if kept.len() != before {
                    summary.updated += 1;
                }
                list[index].sounds_like = kept;
            }
            None => {
                if list.len() >= MAX_ENTRIES {
                    summary.skipped += 1;
                    continue;
                }
                let mut sounds_like = Vec::new();
                add_spellings(&mut sounds_like, &term, &row.sounds_like);
                list.push(VocabularyEntry { term, sounds_like });
                summary.added += 1;
            }
        }
    }
    summary
}

/// Adds each spelling that fits: at most MAX_SOUNDS_LIKE of them, none over
/// MAX_TERM_CHARS, none that repeats the term or another spelling in any
/// case.
fn add_spellings(kept: &mut Vec<String>, term: &str, sounds_like: &[String]) {
    for spelling in sounds_like {
        if kept.len() >= MAX_SOUNDS_LIKE {
            break;
        }
        if spelling.chars().count() > MAX_TERM_CHARS
            || super::same(spelling, term)
            || kept.iter().any(|one| super::same(one, spelling))
        {
            continue;
        }
        kept.push(spelling.clone());
    }
}

/// The list as a CSV file: HEADER, then one row per entry with its spellings
/// joined by ", " and two empty cells. A field holding a comma, a quote, a
/// CR, or an LF is quoted, its quotes doubled.
pub(crate) fn render(entries: &[VocabularyEntry]) -> String {
    let mut csv = String::from(HEADER);
    csv.push('\n');
    for entry in entries {
        field(&mut csv, &entry.term);
        csv.push(',');
        field(&mut csv, &entry.sounds_like.join(", "));
        csv.push(',');
        csv.push(',');
        csv.push('\n');
    }
    csv
}

fn field(csv: &mut String, value: &str) {
    if value.contains([',', '"', '\r', '\n']) {
        csv.push('"');
        csv.push_str(&value.replace('"', "\"\""));
        csv.push('"');
    } else {
        csv.push_str(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: &str, sounds_like: &[&str]) -> VocabularyEntry {
        VocabularyEntry {
            term: term.into(),
            sounds_like: sounds_like.iter().map(|&text| text.into()).collect(),
        }
    }

    fn imported(term: &str, sounds_like: &[&str]) -> ImportedTerm {
        ImportedTerm {
            term: term.into(),
            sounds_like: sounds_like.iter().map(|&text| text.into()).collect(),
        }
    }

    #[test]
    fn the_header_blank_rows_and_rows_without_a_term_are_left_out() {
        let text = "Term (required),Sounds like\n\
                    cappuccino,\"capuchin, cap a chino\"\n\
                    \n\
                    ,orphan spelling\n\
                    Rune,roon\n\
                    terms,,too,ignored\n";
        assert_eq!(
            parse(text),
            [
                imported("cappuccino", &["capuchin", "cap a chino"]),
                imported("Rune", &["roon"]),
                imported("terms", &[]),
            ]
        );
        // A header row is recognized in any case and with either name, and
        // only as the first row with content.
        assert_eq!(
            parse("term,sounds like\nRune,roon\n"),
            [imported("Rune", &["roon"])]
        );
        assert_eq!(
            parse("\u{feff}TERM,SOUNDS LIKE\r\nRune,roon\r\n"),
            [imported("Rune", &["roon"])]
        );
        assert_eq!(
            parse("Rune,roon\nTerm,not a header\n"),
            [
                imported("Rune", &["roon"]),
                imported("Term", &["not a header"]),
            ]
        );
        assert!(parse("\n\r\n , , \n").is_empty());
    }

    #[test]
    fn quoted_fields_keep_commas_quotes_and_line_breaks() {
        let text = "Term (required),Sounds like,Long form,Definition\r\n\
                    \"Dax, Inc.\",\"\"\"the\"\" boss\",,\"Says \"\"hi\"\" and bye\"\r\n\
                    \"multi\nline\",one,,two\r\n";
        assert_eq!(
            parse(text),
            [
                imported("Dax, Inc.", &["\"the\" boss"]),
                imported("multi\nline", &["one"]),
            ]
        );
    }

    #[test]
    fn terms_merge_into_the_existing_list_and_gain_spellings() {
        let mut list = vec![entry("Rune", &["roon"]), entry("OpenCode", &[])];
        let summary = merge(
            &mut list,
            &[
                imported("  Rune ", &["roon", "RUNE", "ruin"]),
                imported("Cappuccino", &["capuchin", "cap a chino"]),
                imported("opencode", &["open code"]),
            ],
        );
        assert_eq!(
            summary,
            MergeSummary {
                added: 1,
                updated: 2,
                skipped: 0
            }
        );
        assert_eq!(
            list,
            [
                entry("Rune", &["roon", "ruin"]),
                entry("OpenCode", &["open code"]),
                entry("Cappuccino", &["capuchin", "cap a chino"]),
            ]
        );
    }

    #[test]
    fn limits_skip_what_does_not_fit() {
        let long = "x".repeat(MAX_TERM_CHARS + 1);
        let spellings: Vec<String> = (0..20).map(|index| format!("spelling {index}")).collect();
        let mut list = vec![entry("Rune", &[])];
        let names: Vec<String> = spellings.iter().map(|_| long.clone()).collect();
        let with_spellings = ImportedTerm {
            term: "Rune".into(),
            sounds_like: spellings.clone(),
        };
        let with_long = ImportedTerm {
            term: "Rune".into(),
            sounds_like: names,
        };
        assert_eq!(
            merge(
                &mut list,
                &[imported(&long, &[]), with_spellings, with_long],
            ),
            MergeSummary {
                added: 0,
                updated: 1,
                skipped: 1
            }
        );
        assert_eq!(list[0].sounds_like.len(), MAX_SOUNDS_LIKE);
        assert_eq!(list[0].sounds_like[0], "spelling 0");

        // A full list takes no new term, but still merges into one it has.
        let mut full: Vec<_> = (0..MAX_ENTRIES)
            .map(|index| entry(&format!("term {index}"), &[]))
            .collect();
        assert_eq!(
            merge(
                &mut full,
                &[imported("one more", &[]), imported("term 0", &["zero"])],
            ),
            MergeSummary {
                added: 0,
                updated: 1,
                skipped: 1
            }
        );
        assert_eq!(full.len(), MAX_ENTRIES);
        assert_eq!(full[0].sounds_like, ["zero"]);
    }

    #[test]
    fn export_and_import_round_trip() {
        let entries = [
            entry("Rune", &["roon", "ruin"]),
            entry("Dax, Inc.", &[]),
            entry("say \"hi\"", &["says hi"]),
            entry("multi\nline", &[]),
        ];
        let csv = render(&entries);
        assert!(csv.starts_with(HEADER));
        assert!(csv.contains("\"Dax, Inc.\",,"));
        assert!(csv.contains("\"say \"\"hi\"\"\",says hi,,"));
        assert!(csv.contains("\"multi\nline\",,"));
        assert_eq!(
            parse(&csv),
            [
                imported("Rune", &["roon", "ruin"]),
                imported("Dax, Inc.", &[]),
                imported("say \"hi\"", &["says hi"]),
                imported("multi\nline", &[]),
            ]
        );
    }

    #[test]
    fn export_quotes_only_the_fields_that_need_it() {
        assert_eq!(
            render(&[entry("Rune", &[])]),
            format!("{HEADER}\nRune,,,\n")
        );
        assert_eq!(
            render(&[entry("a\tb", &[])]),
            format!("{HEADER}\na\tb,,,\n")
        );
    }

    #[test]
    fn a_file_over_the_limit_or_not_utf8_is_refused() {
        let directory =
            std::env::temp_dir().join(format!("hex-vocabulary-csv-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("terms.csv");
        std::fs::write(&path, "Term (required)\nRune\n").unwrap();
        assert_eq!(read(&path).unwrap(), [imported("Rune", &[])]);

        std::fs::write(&path, vec![b'a'; MAX_IMPORT_BYTES as usize + 1]).unwrap();
        assert!(read(&path).unwrap_err().contains("larger than 1 MB"));

        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(read(&path).unwrap_err().contains("UTF-8"));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
