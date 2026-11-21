//! Importing vocabulary from a file: CSV, TSV, or plain text, with the
//! quoting and optional header the lists people already have use. The
//! header mapping also reads Microsoft Teams' custom dictionary CSV.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

use super::{MAX_ENTRIES, MAX_SOUNDS_LIKE, MAX_TERM_CHARS, VocabularyEntry};

/// Most bytes an import file may hold.
pub const MAX_BYTES: u64 = 1024 * 1024;
/// Most terms one file may add; later rows are dropped and marked clamped.
pub const MAX_ROWS: usize = 5_000;

/// The first cells a header row may hold.
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

/// A parsed file: its rows, and whether more were dropped at MAX_ROWS.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Parsed {
    pub terms: Vec<ImportedTerm>,
    pub capped: bool,
}

/// A file that isn't a valid list, such as one with an unterminated quote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseError;

/// What merging imported terms into the list did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Merged {
    /// New terms the list took; a row that only adds spellings to an
    /// existing term counts none.
    pub added: usize,
    /// Whether the list differs from before.
    pub changed: bool,
    /// Whether a term was cut, a spelling left out, a row dropped at
    /// MAX_ROWS, or a term refused because the list was full.
    pub clamped: bool,
}

/// Reads and parses path, refusing a file over MAX_BYTES or one that isn't
/// UTF-8. A byte-order mark is skipped.
pub fn read(path: &Path) -> Result<Parsed, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("the file is larger than {MAX_BYTES} bytes"));
    }
    let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    parse(&text).map_err(|_| "the file has an unterminated quote".to_owned())
}

/// Parses a vocabulary file. The first column is the term and further
/// columns are "sounds like" spellings; a header row names a "Sounds like"
/// column, and then only that column supplies spellings, as Microsoft
/// Teams' custom dictionary does. Blank lines and lines starting with # are
/// skipped. At most MAX_ROWS terms are read; more set Parsed::capped.
pub fn parse(text: &str) -> Result<Parsed, ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let delimiter = delimiter(text);
    // One more record than terms, so a header can be skipped without
    // mistaking its file for a capped one.
    let (records, truncated) = records(text, delimiter, MAX_ROWS + 1)?;
    let mut terms = Vec::new();
    let mut capped = truncated;
    let mut header: Option<Vec<String>> = None;
    for record in records {
        if header.is_none() && terms.is_empty() && is_header(&record) {
            header = Some(record.fields.iter().map(|field| normalize(field)).collect());
            continue;
        }
        let first = record.fields[0].trim();
        if first.is_empty() {
            continue;
        }
        if terms.len() >= MAX_ROWS {
            capped = true;
            break;
        }
        terms.push(ImportedTerm {
            term: first.to_owned(),
            sounds_like: spellings(&record.fields, header.as_deref()),
        });
    }
    Ok(Parsed { terms, capped })
}

/// Merges terms into list with add_clamped's rules: repeated terms merge in
/// any case, terms are cut to 64 characters, spellings stop at 16, and a full
/// list takes no new term. Existing terms are found through one lowercase
/// index rather than a scan per row.
pub fn merge(list: &mut Vec<VocabularyEntry>, terms: &[ImportedTerm]) -> Merged {
    let mut index: HashMap<String, usize> = list
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.term.to_lowercase(), index))
        .collect();
    let mut merged = Merged::default();
    for term in terms {
        let mut name = super::clean(&term.term);
        if name.chars().count() > MAX_TERM_CHARS {
            name = super::clean(&name.chars().take(MAX_TERM_CHARS).collect::<String>());
            merged.clamped = true;
        }
        if name.is_empty() {
            continue;
        }
        let key = name.to_lowercase();
        let position = match index.get(&key) {
            Some(&position) => position,
            None if list.len() >= MAX_ENTRIES => {
                merged.clamped = true;
                continue;
            }
            None => {
                list.push(VocabularyEntry {
                    term: name,
                    sounds_like: Vec::new(),
                });
                merged.added += 1;
                merged.changed = true;
                index.insert(key.clone(), list.len() - 1);
                list.len() - 1
            }
        };
        let entry = &mut list[position];
        // The sheet keeps spellings comma-separated, so one imported
        // spelling with a comma is two there and must count as two here.
        for spelling in term
            .sounds_like
            .iter()
            .flat_map(|spelling| spelling.split(','))
        {
            let spelling = super::clean(spelling);
            if spelling.is_empty() {
                continue;
            }
            if spelling.chars().count() > MAX_TERM_CHARS {
                merged.clamped = true;
                continue;
            }
            if spelling.to_lowercase() == key {
                continue;
            }
            if entry
                .sounds_like
                .iter()
                .any(|kept| kept.to_lowercase() == spelling.to_lowercase())
            {
                continue;
            }
            if entry.sounds_like.len() >= MAX_SOUNDS_LIKE {
                merged.clamped = true;
                continue;
            }
            entry.sounds_like.push(spelling);
            merged.changed = true;
        }
    }
    merged
}

/// One parsed record: its fields and whether its first field was quoted (a
/// quoted # is a term, not a comment).
struct Record {
    fields: Vec<String>,
    first_quoted: bool,
}

/// Reads RFC 4180 style records: double quotes around a field, two quotes
/// for a quote inside it, and delimiters or newlines allowed inside the
/// quotes. A line starting with # is skipped before it is tokenized, so its
/// quotes and delimiters never count. An unterminated quote is an error.
fn records(
    text: &str,
    delimiter: Option<char>,
    limit: usize,
) -> Result<(Vec<Record>, bool), ParseError> {
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut first_quoted = false;
    let mut in_quotes = false;
    let mut at_field_start = true;
    let mut at_record_start = true;
    let mut truncated = false;
    // Records with a term count toward the cap; one with a blank first cell
    // is skipped later, so it must not use the room for a header plus terms.
    let mut kept = 0;
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
        if at_record_start && fields.is_empty() && field.trim().is_empty() && character == '#' {
            field.clear();
            while let Some(next) = chars.next() {
                match next {
                    '\n' => break,
                    '\r' => {
                        if chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                        break;
                    }
                    _ => {}
                }
            }
            continue;
        }
        if !character.is_whitespace() || delimiter == Some(character) {
            at_record_start = false;
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
            other if delimiter == Some(other) => {
                fields.push(std::mem::take(&mut field));
                at_field_start = true;
            }
            '\n' | '\r' => {
                if character == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                fields.push(std::mem::take(&mut field));
                let first_kept = fields.first().is_some_and(|field| !field.trim().is_empty());
                if fields.iter().any(|field| !field.trim().is_empty()) {
                    if first_kept {
                        if kept >= limit {
                            truncated = true;
                            break;
                        }
                        kept += 1;
                    }
                    records.push(Record {
                        fields: std::mem::take(&mut fields),
                        first_quoted,
                    });
                } else {
                    fields.clear();
                }
                first_quoted = false;
                at_field_start = true;
                at_record_start = true;
            }
            other => {
                field.push(other);
                at_field_start = false;
            }
        }
    }
    if in_quotes {
        return Err(ParseError);
    }
    if !field.is_empty() || !fields.is_empty() {
        fields.push(field);
        let first_kept = fields.first().is_some_and(|field| !field.trim().is_empty());
        if fields.iter().any(|field| !field.trim().is_empty()) {
            if first_kept && kept >= limit {
                truncated = true;
            } else {
                records.push(Record {
                    fields,
                    first_quoted,
                });
            }
        }
    }
    Ok((records, truncated))
}

/// The delimiter to use, or None for one column. The first line that isn't
/// blank or a # comment decides; with no delimiter there, a one-column
/// header word lets the next line with one decide, since a semicolon export
/// may write Term above Rune;roon.
fn delimiter(text: &str) -> Option<char> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let first = lines.next()?;
    if let Some(delimiter) = line_delimiter(first) {
        return Some(delimiter);
    }
    if !first.starts_with('"') && one_column_header(first) {
        return lines.find_map(line_delimiter);
    }
    None
}

/// The delimiter a line uses most outside quotes; none when it uses none.
fn line_delimiter(line: &str) -> Option<char> {
    let mut chosen: Option<(char, usize)> = None;
    for candidate in [',', ';', '\t'] {
        let count = unquoted_count(line, candidate);
        if count > 0 && chosen.is_none_or(|(_, best)| count > best) {
            chosen = Some((candidate, count));
        }
    }
    chosen.map(|(delimiter, _)| delimiter)
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
                .filter(|(_, name)| name.starts_with("sounds like"))
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

/// Whether the first record is a header rather than a term. A quoted first
/// cell is a term, and so is a lone ordinary word ("Term", "Word"): a
/// one-column file is a plain list. Another column makes it a header, and
/// so does the unambiguous one-word "Term (required)" or "Display Form".
fn is_header(record: &Record) -> bool {
    if record.first_quoted {
        return false;
    }
    let Some(first) = record.fields.first().map(|field| field.trim()) else {
        return false;
    };
    if !header_word(first) {
        return false;
    }
    record.fields.len() >= 2 || one_column_header(first)
}

/// Whether one cell is a one-column header: the words that are unambiguous
/// alone. A lone "Term" or "Word" is a term, because a one-column file is a
/// plain list.
fn one_column_header(cell: &str) -> bool {
    matches!(normalize(cell).as_str(), "term (required)" | "display form")
}

fn header_word(cell: &str) -> bool {
    let name = normalize(cell);
    HEADER_WORDS.contains(&name.as_str())
}

/// A header cell as it is compared: lowercased, with runs of whitespace and
/// - or _ as one space.
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

    fn parsed(text: &str) -> Vec<ImportedTerm> {
        parse(text).unwrap().terms
    }

    /// What the sheet's rows keep after saving and reloading: each entry's
    /// spellings joined with commas, split again, and added back.
    fn project_like_the_sheet(entries: &[VocabularyEntry]) -> Vec<VocabularyEntry> {
        let mut kept = Vec::new();
        for entry in entries {
            let _ = super::super::add_clamped(
                &mut kept,
                &entry.term,
                &super::super::parse_sounds_like(&entry.sounds_like.join(", ")),
            );
        }
        kept
    }

    #[test]
    fn commas_semicolons_and_tabs_pick_themselves_from_the_first_line() {
        assert_eq!(
            parsed("cappuccino,capuchin\nRune,roon"),
            [
                imported("cappuccino", &["capuchin"]),
                imported("Rune", &["roon"])
            ]
        );
        assert_eq!(
            parsed("cappuccino;capuchin\nRune;roon"),
            [
                imported("cappuccino", &["capuchin"]),
                imported("Rune", &["roon"])
            ]
        );
        assert_eq!(
            parsed("cappuccino\tcapuchin\nRune\troon"),
            [
                imported("cappuccino", &["capuchin"]),
                imported("Rune", &["roon"])
            ]
        );
    }

    #[test]
    fn a_plain_line_with_a_comma_stays_one_term() {
        assert_eq!(
            parsed("cappuccino\nRune\nAcme, Inc.\n"),
            [
                imported("cappuccino", &[]),
                imported("Rune", &[]),
                imported("Acme, Inc.", &[])
            ]
        );
    }

    #[test]
    fn a_one_column_header_is_skipped_and_sets_the_next_line_s_delimiter() {
        assert_eq!(
            parsed("Term (required)\nRune;roon\n"),
            [imported("Rune", &["roon"])]
        );
        assert_eq!(
            parsed("Display Form\nRune;roon\n"),
            [imported("Rune", &["roon"])]
        );
    }

    #[test]
    fn a_plain_list_starting_with_a_header_word_and_a_comma_stays_one_column() {
        assert_eq!(
            parsed("Word\nAcme, Inc.\n"),
            [imported("Word", &[]), imported("Acme, Inc.", &[])]
        );
    }

    #[test]
    fn a_leading_space_still_starts_a_comment() {
        assert_eq!(
            parsed("  # note with a \"quote\"\nRune\n"),
            [imported("Rune", &[])]
        );
    }

    #[test]
    fn a_one_column_plain_list_keeps_a_header_word_as_its_term() {
        assert_eq!(
            parsed("Word\nRune\nDax\n"),
            [
                imported("Word", &[]),
                imported("Rune", &[]),
                imported("Dax", &[])
            ]
        );
    }

    #[test]
    fn quoted_fields_keep_delimiters_quotes_and_line_breaks() {
        let text = "\"Rune, the framework\", \"roon\"\n\"say \"\"hi\"\"\", \"say hi\"\n\"two\nlines\", \"too\"";
        assert_eq!(
            parsed(text),
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
            parsed(text),
            [
                imported("cappuccino", &["capuchin"]),
                imported("Rune", &["roon"])
            ]
        );
    }

    #[test]
    fn a_comment_line_with_a_quote_is_skipped_before_tokenizing() {
        assert_eq!(parsed("# don't panic\nRune\n"), [imported("Rune", &[])]);
        assert!(parse("\"unterminated").is_err());
    }

    #[test]
    fn plain_text_is_one_term_a_line() {
        assert_eq!(
            parsed("cappuccino\n\nRune\n  Dax  \n"),
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
            parsed(
                "Term,Sounds like,Long form of the term (for acronyms),Definition and context\nTAC,Tee-Ay-Cee,Teams Admin Center,Admin portal managing Teams\nViva Glint,Vee-Vah-Glint,Part of Microsoft Viva,\n"
            ),
            [
                imported("TAC", &["Tee-Ay-Cee"]),
                imported("Viva Glint", &["Vee-Vah-Glint"])
            ]
        );
        // A Sounds like column may name what it holds.
        assert_eq!(
            parsed("Term,Sounds like (optional),Long form\nTAC,Tee-Ay-Cee,Teams Admin Center\n"),
            [imported("TAC", &["Tee-Ay-Cee"])]
        );
        // A header without a Sounds like column leaves every further cell a
        // spelling, as an unheadered row's do.
        assert_eq!(
            parsed("word,spelling\nRune,roon"),
            [imported("Rune", &["roon"])]
        );
        assert_eq!(
            parsed("Display Form\nOpenCode"),
            [imported("OpenCode", &[])]
        );
        // A quoted first cell is a term, even when it reads like a header.
        assert_eq!(parsed("\"Term\",roon"), [imported("Term", &["roon"])]);
    }

    #[test]
    fn a_quoted_hash_stays_a_term() {
        assert_eq!(
            parsed("\"#hashtag\", hash tag\n# comment\nRune"),
            [imported("#hashtag", &["hash tag"]), imported("Rune", &[])]
        );
    }

    #[test]
    fn records_with_a_blank_first_cell_do_not_use_the_row_cap() {
        let text = (0..=MAX_ROWS).map(|_| ",x\n").collect::<String>() + "Rune\n";
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed.terms, [imported("Rune", &[])]);
        assert!(!parsed.capped);
    }

    #[test]
    fn rows_past_the_cap_are_dropped_and_marked() {
        let text = (0..=MAX_ROWS)
            .map(|index| format!("Term {index}\n"))
            .collect::<String>();
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed.terms.len(), MAX_ROWS);
        assert!(parsed.capped);
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
    fn a_spelling_with_a_comma_is_split_the_way_the_sheet_saves_it() {
        let mut list = Vec::new();
        merge(
            &mut list,
            &[imported("cappuccino", &["capuchin, cap a chino"])],
        );
        assert_eq!(
            list[0].sounds_like,
            ["capuchin".to_string(), "cap a chino".to_string()]
        );
    }

    #[test]
    fn merging_keeps_the_limits_and_says_what_did_not_fit() {
        let mut list: Vec<_> = (0..MAX_ENTRIES)
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
        assert_eq!(list.len(), MAX_ENTRIES);
        assert_eq!(list[0].sounds_like.len(), MAX_SOUNDS_LIKE);
    }

    #[test]
    fn merging_matches_add_clamped() {
        let long = "s".repeat(65);
        let many: Vec<String> = (0..17).map(|n| format!("many {n}")).collect();
        let many_refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let terms = [
            imported(" Rune  ", &["roon", "rooney"]),
            imported("cappuccino", &["capuchin", "cap a chino"]),
            imported("Rune", &["ROON", "rune"]),
            imported(&"x".repeat(70), &[]),
            imported("Rune", &[long.as_str()]),
            imported("Rune", &many_refs),
        ];
        let original = vec![VocabularyEntry {
            term: "Rune".into(),
            sounds_like: Vec::new(),
        }];
        let mut fast = original.clone();
        let fast_merged = merge(&mut fast, &terms);
        let mut slow = original.clone();
        let mut slow_merged = Merged::default();
        for term in &terms {
            let length = slow.len();
            slow_merged.clamped |=
                super::super::add_clamped(&mut slow, &term.term, &term.sounds_like);
            slow_merged.added += usize::from(slow.len() > length);
        }
        slow_merged.changed = slow != original;
        assert_eq!(fast, slow);
        assert_eq!(fast_merged, slow_merged);
        assert!(fast_merged.clamped);
        assert!(
            fast.iter()
                .flat_map(|entry| &entry.sounds_like)
                .all(|spelling| spelling.chars().count() <= MAX_TERM_CHARS)
        );
        assert_eq!(project_like_the_sheet(&fast), fast);
    }
}
