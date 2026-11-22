//! `SpokenLists` (ios/HEXShared/SpokenLists.swift): dictated lists and line breaks as lines,
//! in English and Spanish. Runs inside a style, after its fillers and before its punctuation.

use super::fold::fold;
use super::text as t;
use super::{Language, lists_tables as tables};

pub(super) use tables::Table;

pub fn format(text: &str, language: Option<&Language>) -> String {
    let Some(table) = Table::for_language(language) else {
        return text.to_owned();
    };
    if text.is_empty() {
        return text.to_owned();
    }
    listing(&breaking(text, table), table)
}

struct Token {
    text: String,
    is_word: bool,
    /// Lowercased, accents and "ñ" folded: "Viñeta" reads "vineta".
    folded: String,
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_is_word: Option<bool> = None;
    let flush = |tokens: &mut Vec<Token>, current: &str, is_word: Option<bool>| {
        if let Some(is_word) = is_word {
            let folded = if is_word {
                fold(current)
            } else {
                current.to_owned()
            };
            tokens.push(Token {
                text: current.to_owned(),
                is_word,
                folded,
            });
        }
    };
    for c in t::chars(text) {
        let is_word = t::is_letter_or_number(c) || t::is(c, '\'') || t::is(c, '’');
        if current_is_word == Some(is_word) {
            current.push_str(c);
        } else {
            flush(&mut tokens, &current, current_is_word);
            current = c.to_owned();
            current_is_word = Some(is_word);
        }
    }
    flush(&mut tokens, &current, current_is_word);
    tokens
}

fn joined(tokens: &[Token]) -> String {
    tokens.iter().map(|token| token.text.as_str()).collect()
}

/// Marks that end a clause, so a marker after them starts one.
const CLAUSE_BOUNDARIES: &str = ".,;:!?…—–\n(¿¡";

struct Words<'a> {
    tokens: Vec<Token>,
    table: &'a Table,
}

impl Words<'_> {
    fn is_word(&self, index: usize) -> bool {
        self.tokens.get(index).is_some_and(|token| token.is_word)
    }

    fn spaces_only(&self, index: usize) -> bool {
        self.tokens.get(index).is_some_and(|token| {
            !token.is_word
                && t::chars(&token.text)
                    .iter()
                    .all(|c| t::is(c, ' ') || t::is(c, '\t'))
        })
    }

    /// Where `words` said one after another (spaces between) starting at `index` end.
    fn phrase(&self, words: &[&str], index: usize) -> Option<usize> {
        let mut position = index;
        for (number, word) in words.iter().enumerate() {
            if number > 0 {
                if !self.spaces_only(position) {
                    return None;
                }
                position += 1;
            }
            if !self.is_word(position) || !t::same(&self.tokens[position].folded, word) {
                return None;
            }
            position += 1;
        }
        Some(position)
    }

    fn next_word(&self, end: usize) -> Option<&str> {
        (self.spaces_only(end) && self.is_word(end + 1))
            .then(|| self.tokens[end + 1].folded.as_str())
    }

    fn previous_word(&self, index: usize) -> Option<&str> {
        (index >= 2 && self.spaces_only(index - 1) && self.is_word(index - 2))
            .then(|| self.tokens[index - 2].folded.as_str())
    }

    /// Whether the word at `index` starts a clause.
    fn starts_clause(&self, index: usize) -> bool {
        let mut position = index;
        while position >= 2 {
            if t::chars(&self.tokens[position - 1].text)
                .iter()
                .any(|c| t::is_one_of(c, CLAUSE_BOUNDARIES))
            {
                return true;
            }
            if !tables::has(self.table.connectors, &self.tokens[position - 2].folded) {
                return false;
            }
            position -= 2;
        }
        true
    }

    fn separator(&self, end: usize) -> &str {
        match self.tokens.get(end) {
            Some(token) if !token.is_word => &token.text,
            _ => "",
        }
    }

    /// A number said as a word or written in digits, 1 to 20.
    fn number(&self, index: usize) -> Option<usize> {
        if !self.is_word(index) {
            return None;
        }
        let word = &self.tokens[index].folded;
        if let Some(value) = tables::lookup(self.table.cardinals, word) {
            return Some(value);
        }
        small_number(word)
    }
}

/// `Int(word)` for a word of at most two ASCII Characters, 1 to 20.
pub(super) fn small_number(word: &str) -> Option<usize> {
    if t::count(word) > 2 || !t::chars(word).iter().all(|c| t::is_ascii(c)) {
        return None;
    }
    let value: usize = word.parse().ok()?;
    (1..=20).contains(&value).then_some(value)
}

fn breaking(text: &str, table: &Table) -> String {
    let words = Words {
        tokens: tokenize(text),
        table,
    };
    let tokens = &words.tokens;
    let mut out = String::new();
    let mut index = 0;
    let mut saw_word = false;
    while index < tokens.len() {
        if tokens[index].is_word
            && saw_word
            && let Some((breaks, end)) = line_break(index, &words)
        {
            while t::last(&out).is_some_and(|c| t::is_one_of(c, " \t,;")) {
                t::remove_last(&mut out);
            }
            out.push_str(breaks);
            index = end;
            if index < tokens.len() && !tokens[index].is_word {
                let chars = t::chars(&tokens[index].text);
                let skip = chars
                    .iter()
                    .take_while(|c| t::is_one_of(c, " \t,.;:"))
                    .count();
                out.push_str(&t::join(&chars[skip..]));
                index += 1;
            }
            continue;
        }
        saw_word = saw_word || tokens[index].is_word;
        out.push_str(&tokens[index].text);
        index += 1;
    }
    out
}

fn line_break(index: usize, words: &Words) -> Option<(&'static str, usize)> {
    for &(phrase, breaks) in words.table.breaks {
        let Some(end) = words.phrase(phrase, index) else {
            continue;
        };
        if words
            .previous_word(index)
            .is_some_and(|w| tables::has(words.table.determiners, w))
        {
            return None;
        }
        if words
            .next_word(end)
            .is_some_and(|w| tables::has(words.table.break_followers, w))
        {
            return None;
        }
        return Some((breaks, end));
    }
    None
}

/// A marker's kind; markers of one kind make a list. A keyword is the table's own word, so
/// two of them compare as Swift's strings do.
#[derive(Clone, Copy, PartialEq)]
enum Family {
    Ordinal,
    Cardinal,
    Keyword(&'static str),
    Bullet,
}

#[derive(Clone, Copy)]
struct Marker {
    family: Family,
    /// The item's number; 0 for "finally".
    value: usize,
    start: usize,
    end: usize,
}

fn markers(words: &Words) -> Vec<Marker> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < words.tokens.len() {
        match words.is_word(index).then(|| marker(index, words)).flatten() {
            Some(marker) => {
                found.push(marker);
                index = marker.end;
            }
            None => index += 1,
        }
    }
    found
}

fn marker(index: usize, words: &Words) -> Option<Marker> {
    let table = words.table;
    let word = words.tokens[index].folded.as_str();
    for &(phrase, needs_mark) in table.finals {
        let Some(end) = words.phrase(phrase, index) else {
            continue;
        };
        if !words.starts_clause(index) {
            continue;
        }
        let after = words.separator(end);
        if needs_mark && !t::has_prefix(after, ",") && !t::has_prefix(after, ":") {
            continue;
        }
        return Some(Marker {
            family: Family::Ordinal,
            value: 0,
            start: index,
            end,
        });
    }
    for &(phrase, value) in table.ordinal_phrases {
        let Some(end) = words.phrase(phrase, index) else {
            continue;
        };
        if !words.starts_clause(index) {
            continue;
        }
        return Some(Marker {
            family: Family::Ordinal,
            value,
            start: index,
            end,
        });
    }
    if let Some(keyword) = tables::find(table.keywords, word)
        && words.spaces_only(index + 1)
        && let Some(value) = words.number(index + 2)
        && words.starts_clause(index)
    {
        return Some(Marker {
            family: Family::Keyword(keyword),
            value,
            start: index,
            end: index + 3,
        });
    }
    for &(phrase, strict) in table.bullets {
        let Some(end) = words.phrase(phrase, index) else {
            continue;
        };
        let previous = words.previous_word(index);
        let starts = words.starts_clause(index)
            || (!strict && !previous.is_some_and(|w| tables::has(table.determiners, w)));
        if !starts {
            return None;
        }
        if words
            .next_word(end)
            .is_some_and(|w| tables::has(table.bullet_followers, w))
        {
            return None;
        }
        return Some(Marker {
            family: Family::Bullet,
            value: 1,
            start: index,
            end,
        });
    }
    if let Some(value) = table.ordinal(word)
        && words.starts_clause(index)
    {
        if words.spaces_only(index + 1) && words.phrase(table.of_all, index + 2).is_some() {
            return Some(Marker {
                family: Family::Ordinal,
                value,
                start: index,
                end: index + 1 + 2 * table.of_all.len(),
            });
        }
        if words
            .next_word(index + 1)
            .is_some_and(|w| tables::has(table.ordinal_followers, w))
        {
            return None;
        }
        return Some(Marker {
            family: Family::Ordinal,
            value,
            start: index,
            end: index + 1,
        });
    }
    if let Some(value) = words.number(index)
        && words.starts_clause(index)
    {
        // A bare number counts only with its mark and a space after: "one, eggs", "1. eggs",
        // never "1:30" or "1.5".
        let separator = t::chars(words.separator(index + 1));
        let mark = separator.first()?;
        let spaced = match separator.get(1) {
            Some(c) => t::is_whitespace(c),
            None => index + 2 >= words.tokens.len(),
        };
        if !t::is_one_of(mark, ",:.)") || !spaced {
            return None;
        }
        return Some(Marker {
            family: Family::Cardinal,
            value,
            start: index,
            end: index + 1,
        });
    }
    None
}

/// The lists among `markers` in order: numbered runs counting up from 1, and two bullets or
/// more.
fn lists(markers: &[Marker]) -> Vec<Vec<Marker>> {
    let mut runs = Vec::new();
    for (position, first) in markers.iter().enumerate() {
        if first.family == Family::Bullet {
            let run: Vec<Marker> = markers[position..]
                .iter()
                .filter(|m| m.family == Family::Bullet)
                .copied()
                .collect();
            if run.len() >= 2 {
                runs.push(run);
            }
            continue;
        }
        if first.value != 1 {
            continue;
        }
        let mut run = vec![*first];
        for marker in markers[position + 1..]
            .iter()
            .filter(|m| m.family == first.family)
        {
            if marker.value == run.len() + 1 {
                run.push(*marker);
            } else if marker.value == 0 && run.len() >= 2 {
                run.push(*marker);
                break;
            }
        }
        if run.len() >= 2 {
            runs.push(run);
        }
    }
    runs
}

fn listing(text: &str, table: &Table) -> String {
    let words = Words {
        tokens: tokenize(text),
        table,
    };
    for run in lists(&markers(&words)) {
        // A run with an empty item ("one, two, three, go") is no list; a later one may be.
        if let Some(formatted) = format_run(&run, &words) {
            return formatted;
        }
    }
    text.to_owned()
}

fn format_run(run: &[Marker], words: &Words) -> Option<String> {
    let tokens = &words.tokens;
    let table = words.table;
    let last = run.last()?;

    // The last item ends at its first sentence (or line); what follows is its own line.
    let mut items_end = tokens.len();
    let mut tail_break = "\n";
    let mut saw_word = false;
    for index in last.end..tokens.len() {
        if tokens[index].is_word {
            saw_word = true;
            continue;
        }
        if !saw_word || index + 1 >= tokens.len() {
            continue;
        }
        let separator = &tokens[index].text;
        if t::contains(separator, "\n") || ends_sentence(separator, &tokens[index - 1].folded) {
            items_end = index;
            tail_break = if t::contains(separator, "\n\n") {
                "\n\n"
            } else {
                "\n"
            };
            break;
        }
    }

    let mut items = Vec::new();
    for (number, marker) in run.iter().enumerate() {
        let end = if number + 1 < run.len() {
            run[number + 1].start
        } else {
            items_end
        };
        let mut raw = joined(&tokens[marker.end..end]);
        // The last item keeps its sentence's mark ("?", "!") but not what follows.
        if number + 1 == run.len() && items_end < tokens.len() {
            let separator = t::chars(&tokens[items_end].text);
            raw.push_str(&t::join(
                &separator
                    .iter()
                    .take_while(|c| !t::is_whitespace(c))
                    .collect::<Vec<_>>(),
            ));
        }
        let item = clean_item(&raw, table);
        if item.is_empty() {
            return None;
        }
        items.push(item);
    }

    let mut output = String::new();
    let lead_in = joined(&tokens[..run[0].start]);
    let lead_trimmed = {
        let chars = t::chars(&lead_in);
        let keep = chars.len()
            - chars
                .iter()
                .rev()
                .take_while(|c| t::is(c, ' ') || t::is(c, '\t'))
                .count();
        t::join(&chars[..keep])
    };
    if t::has_suffix(&lead_trimmed, "\n") {
        output = lead_trimmed;
    } else {
        let mut lead = strip_trailing_connectors(t::trim_whitespaces_and_newlines(&lead_in), table);
        while t::last(&lead).is_some_and(|c| t::is_one_of(c, ",;-–—")) {
            t::remove_last(&mut lead);
            lead = t::trim_whitespaces(&lead).to_owned();
        }
        if let Some(mark) = t::last(&lead) {
            if t::is_letter_or_number(mark) {
                lead.push(':');
            }
            output = lead + "\n";
        }
    }

    let is_bullet = run[0].family == Family::Bullet;
    let lines: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(offset, item)| {
            if is_bullet {
                format!("- {item}")
            } else {
                format!("{}. {item}", offset + 1)
            }
        })
        .collect();
    output.push_str(&lines.join("\n"));

    if items_end < tokens.len() {
        let tail = joined(&tokens[items_end + 1..]);
        let tail = t::trim_whitespaces_and_newlines(&tail);
        if !tail.is_empty() {
            output.push_str(tail_break);
            output.push_str(&listing(tail, table));
        }
    }
    Some(output)
}

const ABBREVIATIONS: [&str; 17] = [
    "mr", "mrs", "ms", "dr", "st", "jr", "sr", "prof", "vs", "etc", "inc", "ltd", "approx", "sra",
    "dra", "ud", "uds",
];

/// Whether `separator` ends a sentence after `word`: ".", "!", "?" or "…" then a space,
/// never after "Dr" or an initial.
fn ends_sentence(separator: &str, word: &str) -> bool {
    let chars = t::chars(separator);
    let Some(mark) = chars.iter().position(|c| t::is_one_of(c, ".!?…")) else {
        return false;
    };
    if !chars.get(mark + 1).is_some_and(|c| t::is_whitespace(c)) {
        return false;
    }
    if t::is(chars[mark], '.')
        && (ABBREVIATIONS.iter().any(|a| t::same(a, word))
            || (t::count(word) == 1 && t::first(word).is_some_and(t::is_letter)))
    {
        return false;
    }
    true
}

/// An item's text: no leading marks, no trailing comma, period, "and" or "or", and a capital
/// first letter.
fn clean_item(raw: &str, table: &Table) -> String {
    let chars = t::chars(raw);
    let skip = chars
        .iter()
        .take_while(|c| t::is_one_of(c, " \t\n,:.;)-–—"))
        .count();
    let mut item = t::join(&chars[skip..]);
    let mut previous = String::new();
    while previous != item {
        previous = item.clone();
        item = t::trim_whitespaces_and_newlines(&item).to_owned();
        if !t::has_suffix(&item, "...") && t::last(&item).is_some_and(|c| t::is_one_of(c, ",;:.")) {
            t::remove_last(&mut item);
        }
        item = strip_trailing_connectors(&item, table);
    }
    capitalizing_first_word(&item)
}

/// `text` without the connectors ("and", "y luego") a next item's marker leaves at its end.
fn strip_trailing_connectors(text: &str, table: &Table) -> String {
    let mut text = text.to_owned();
    loop {
        let chars = t::chars(&text);
        let Some(space) = chars.iter().rposition(|c| t::is(c, ' ') || t::is(c, ',')) else {
            break;
        };
        let last = fold(&t::join(&chars[space + 1..]));
        if !tables::has(table.connectors, &last) && !tables::has(table.alternatives, &last) {
            break;
        }
        let mut kept = t::trim_whitespaces(&t::join(&chars[..space])).to_owned();
        while t::last(&kept).is_some_and(|c| t::is(c, ',')) {
            t::remove_last(&mut kept);
        }
        text = kept;
    }
    text
}

fn capitalizing_first_word(text: &str) -> String {
    let chars = t::chars(text);
    let Some(first) = chars.iter().position(|c| t::is_letter(c)) else {
        return text.to_owned();
    };
    let word: Vec<&str> = chars[first..]
        .iter()
        .copied()
        .take_while(|c| !t::is_whitespace(c))
        .collect();
    if word.iter().any(|c| t::is(c, '@')) || word.iter().skip(1).any(|c| t::is_uppercase(c)) {
        return text.to_owned();
    }
    let mut out = t::join(&chars[..first]);
    out.push_str(&t::uppercased(chars[first]));
    out.push_str(&t::join(&chars[first + 1..]));
    out
}
