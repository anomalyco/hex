//! `SpokenPunctuation` (ios/HEXShared/SpokenPunctuation.swift): punctuation said by name
//! written as the mark, in English, Spanish, French, German, Italian and Portuguese. Runs
//! inside a style, after its fillers and Format lists, before the style's own punctuation.

mod tables;

use std::collections::HashSet;
use std::sync::OnceLock;

use super::Language;
use super::fold::fold;
use super::text as t;

pub fn format(text: &str, language: Option<&Language>) -> String {
    let Some(table) = Table::for_language(language) else {
        return text.to_owned();
    };
    if text.is_empty() {
        return text.to_owned();
    }
    let mut writer = Writer {
        tokens: tokenize(text),
        table,
        out: String::new(),
        capitalize_next: false,
        lowercase_next: false,
    };
    writer.write();
    writer.out
}

// MARK: Words

struct Token {
    text: String,
    is_word: bool,
    /// Lowercased, accents folded, curly apostrophes straight.
    folded: String,
}

fn folded(text: &str) -> String {
    t::chars(&fold(text))
        .into_iter()
        .map(|c| if t::is(c, '’') { "'" } else { c })
        .collect()
}

/// Alternating words (letters, digits, apostrophes) and the separators between them.
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_is_word: Option<bool> = None;
    let flush = |tokens: &mut Vec<Token>, current: &str, is_word: Option<bool>| {
        if let Some(is_word) = is_word {
            let folded = if is_word {
                folded(current)
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

/// Marks a speech model puts around a spoken name: the name's mark replaces them.
const PAUSE_MARKS: &str = ",.;:!?…";
const OPENERS: &str = "([“«„¿¡\"‘";
const CLOSERS: &str = ")]”»“\"’";
/// Endings after "apostrophe": "John apostrophe S" is "John's".
const CONTRACTIONS: [&str; 7] = ["s", "t", "d", "m", "ll", "re", "ve"];

// MARK: Writing

struct Writer {
    tokens: Vec<Token>,
    table: &'static Table,
    out: String,
    capitalize_next: bool,
    lowercase_next: bool,
}

impl Writer {
    fn write(&mut self) {
        let mut index = 0;
        while index < self.tokens.len() {
            if self.tokens[index].is_word
                && let Some((command, end)) = self.command(index)
            {
                if self.allows(command, index, end) {
                    index = self.apply(command, end);
                } else {
                    // A name kept as said stays whole: "a dos puntos" never loses its "puntos".
                    for position in index..end {
                        self.emit(position);
                    }
                    index = end;
                }
                continue;
            }
            self.emit(index);
            index += 1;
        }
    }

    fn emit(&mut self, index: usize) {
        let token = &self.tokens[index];
        if !token.is_word {
            self.out.push_str(&token.text);
            return;
        }
        if self.lowercase_next && CONTRACTIONS.iter().any(|c| t::same(c, &token.folded)) {
            self.out.push_str(&t::lowercased(&token.text));
        } else if self.capitalize_next {
            self.out.push_str(&capitalized(&token.text));
        } else {
            self.out.push_str(&token.text);
        }
        self.capitalize_next = false;
        self.lowercase_next = false;
    }

    /// The command whose words start at `index`, longest first, and where its words end.
    fn command(&self, index: usize) -> Option<(&'static Command, usize)> {
        self.table.commands.iter().find_map(|command| {
            self.phrase_end(&command.words, index)
                .map(|end| (command, end))
        })
    }

    /// Where `words` said one after another start at `index` end. Between them only spaces,
    /// a hyphen or one comma a model heard as a pause.
    fn phrase_end(&self, words: &[String], index: usize) -> Option<usize> {
        let mut position = index;
        for (number, word) in words.iter().enumerate() {
            if number > 0 {
                let token = self.tokens.get(position).filter(|token| !token.is_word)?;
                let separator = t::chars(&token.text);
                if separator.len() > 3
                    || !separator.iter().all(|c| t::is_one_of(c, " -,"))
                    || separator.iter().filter(|c| t::is(c, ',')).count() > 1
                {
                    return None;
                }
                position += 1;
            }
            let token = self.tokens.get(position).filter(|token| token.is_word)?;
            if !t::same(&token.folded, word) {
                return None;
            }
            position += 1;
        }
        Some(position)
    }

    fn count(&self, mark: char) -> usize {
        t::chars(&self.out)
            .iter()
            .filter(|c| t::is(c, mark))
            .count()
    }

    fn is_open(&self, pair: Pair) -> bool {
        match pair {
            Pair::Paren => self.count('(') > self.count(')'),
            Pair::Quote => {
                self.count(self.table.quotes.0) > self.count(self.table.quotes.1)
                    || self.count('"') % 2 == 1
            }
        }
    }

    /// Whether a closing quote name follows `end`, with words between.
    fn closing_quote(&self, end: usize, toggles: bool) -> bool {
        let mut index = end;
        let mut words_between = false;
        while index < self.tokens.len() {
            if t::contains(&self.tokens[index].text, "\n") {
                return false;
            }
            if self.tokens[index].is_word
                && let Some((found, command_end)) = self.command(index)
            {
                match found.mark {
                    Mark::Close(Pair::Quote) => return words_between,
                    Mark::ToggleQuote { .. } => return toggles && words_between,
                    _ => index = command_end,
                }
                words_between = true;
                continue;
            }
            words_between = words_between || self.tokens[index].is_word;
            index += 1;
        }
        false
    }

    fn allows(&self, command: &Command, index: usize, end: usize) -> bool {
        let tokens = &self.tokens;
        let table = self.table;
        let before = if index >= 1 {
            tokens[index - 1].text.as_str()
        } else {
            ""
        };
        let after = tokens.get(end).map_or("", |token| token.text.as_str());
        let previous = (index >= 2 && !t::contains(before, "\n")).then(|| &tokens[index - 2]);
        let next = (end + 1 < tokens.len() && !t::contains(after, "\n")).then(|| &tokens[end + 1]);
        let next_direct = next.is_some()
            && t::chars(after)
                .iter()
                .all(|c| t::is(c, ' ') || t::is(c, '\t'));
        let mark = command.mark;
        let family = command.family;

        // A compound: "comma-separated", "pre-period", "and/or".
        if t::first(after).is_some_and(|c| t::is(c, '-') || t::is(c, '/')) {
            return false;
        }
        if family != Family::Hyphen
            && t::last(before).is_some_and(|c| t::is(c, '-') || t::is(c, '/'))
        {
            return false;
        }

        // A name: "I'll text Dash later", "George Smiley".
        if table.capitalizes_sentences
            && t::first(&tokens[index].text).is_some_and(t::is_uppercase)
            && let Some(previous) = previous
        {
            if family == Family::Dash && !t::chars(before).iter().any(|c| t::is_one_of(c, ".!?")) {
                return false;
            }
            if family == Family::Smiley && t::first(&previous.text).is_some_and(t::is_uppercase) {
                return false;
            }
        }

        let mut opens = false;
        let mut closes = false;
        match mark {
            Mark::Open(_) | Mark::Prefix(_) => opens = true,
            Mark::ToggleQuote { toggles } => {
                opens = !self.is_open(Pair::Quote);
                closes = !opens;
                if opens && (next.is_none() || !self.closing_quote(end, toggles)) {
                    return false;
                }
            }
            Mark::Close(pair) => {
                if !self.is_open(pair) {
                    return false;
                }
                closes = true;
            }
            _ => {}
        }

        if !opens {
            // Attached to something said before it on the line; a quote or bracket that is
            // open closes after any word ("hello, their close quote").
            let Some(previous) = previous else {
                return false;
            };
            if !closes && self.is_noun_phrase(previous, index, family) {
                return false;
            }
        } else if matches!(mark, Mark::Prefix(_))
            && let Some(previous) = previous
            && self.is_noun_phrase(previous, index, family)
        {
            return false;
        }

        if matches!(mark, Mark::Open(_)) {
            // "The open quote was missing."
            if previous
                .is_some_and(|previous| table.blockers.contains(&*t::canon(&previous.folded)))
            {
                return false;
            }
            if next_direct
                && next.is_some_and(|next| table.followers.contains(&*t::canon(&next.folded)))
            {
                return false;
            }
            return next.is_some();
        }
        if matches!(mark, Mark::ToggleQuote { .. }) && opens {
            return true;
        }

        // A decimal or a time: "two comma five", "3 colon 30".
        if matches!(family, Family::Comma | Family::Period | Family::Colon)
            && let (Some(previous), true, Some(next)) = (previous, next_direct, next)
            && self.is_number(previous)
            && self.is_number(next)
        {
            return false;
        }

        if next_direct && let Some(next) = next {
            let word = t::canon(&next.folded);
            if table.followers.contains(&*word)
                || table
                    .family_followers(family)
                    .is_some_and(|set| set.contains(&*word))
                || table
                    .follower_prefixes
                    .iter()
                    .any(|prefix| t::has_prefix(&next.folded, prefix))
            {
                return false;
            }
        }

        match mark {
            Mark::Join(_) => {
                let joinable = if family == Family::Hyphen {
                    " \t-"
                } else {
                    " \t"
                };
                if !t::chars(before).iter().all(|c| t::is_one_of(c, joinable)) || !next_direct {
                    return false;
                }
            }
            Mark::Prefix(_) if !next_direct => return false,
            _ => {}
        }

        if command.needs_following && next.is_none() {
            return false;
        }
        if command.strict {
            let at_line_end = next.is_none();
            let pause_after = t::chars(after)
                .into_iter()
                .find(|c| !t::is(c, ' ') && !t::is(c, '\t'))
                .is_some_and(|c| t::is_one_of(c, PAUSE_MARKS) || t::is_one_of(c, CLOSERS));
            let starts_clause = next_direct
                && next.is_some_and(|next| {
                    table.starters.contains(&*t::canon(&next.folded))
                        || table
                            .starter_prefixes
                            .iter()
                            .any(|prefix| t::has_prefix(&next.folded, prefix))
                        || (table.capitalizes_sentences
                            && t::first(&next.text).is_some_and(t::is_uppercase))
                });
            let another_command = next.is_some() && self.command(end + 1).is_some();
            if !(at_line_end || pause_after || starts_clause || another_command) {
                return false;
            }
        }
        true
    }

    fn is_number(&self, token: &Token) -> bool {
        t::chars(&token.folded).iter().all(|c| t::is_number(c))
            || self.table.numbers.contains(&*t::canon(&token.folded))
    }

    /// Whether the word before a name makes the name a noun: "a period", "the comma",
    /// "trial period", "Sarah's colon", "ein wichtiger Punkt".
    fn is_noun_phrase(&self, previous: &Token, index: usize, family: Family) -> bool {
        let table = self.table;
        let word = t::canon(&previous.folded);
        if table.blockers.contains(&*word)
            || table
                .family_blockers(family)
                .is_some_and(|set| set.contains(&*word))
        {
            return true;
        }
        // An English or German sentence may end on a preposition or a particle, a noun
        // phrase not.
        if table.prepositions.contains(&*word)
            && !(table.prepositions_end_sentences
                && matches!(
                    family,
                    Family::Question | Family::Exclamation | Family::Period
                ))
        {
            return true;
        }
        if table.possessives
            && t::has_suffix(&previous.folded, "'s")
            && !table.starters.contains(&*word)
        {
            return true;
        }
        // German nouns are capitalized: a lowercase word between an article and the name is
        // an adjective ("ein wichtiger Punkt").
        table.adjectives_before_nouns
            && t::first(&previous.text).is_some_and(t::is_lowercase)
            && index >= 4
            && t::chars(&self.tokens[index - 3].text)
                .iter()
                .all(|c| t::is(c, ' '))
            && table
                .blockers
                .contains(&*t::canon(&self.tokens[index - 4].folded))
    }

    /// Writes the command's mark and returns where reading goes on.
    fn apply(&mut self, command: &Command, end: usize) -> usize {
        let after = self
            .tokens
            .get(end)
            .map_or(String::new(), |token| token.text.clone());
        let resume = if end < self.tokens.len() {
            end + 1
        } else {
            end
        };
        let word_after = end + 1 < self.tokens.len();
        let rest = |set: &str| {
            let chars = t::chars(&after);
            let skip = chars.iter().take_while(|c| t::is_one_of(c, set)).count();
            t::join(&chars[skip..])
        };
        let spaced = |rest: String| {
            if rest.is_empty() && word_after {
                " ".to_owned()
            } else {
                rest
            }
        };
        let pauses = format!(" \t{PAUSE_MARKS}");

        match command.mark {
            Mark::Attach(mark) => {
                self.trim_end(&pauses);
                self.out.push_str(mark);
                self.out.push_str(&spaced(rest(&pauses)));
                self.capitalize_next = t::contains(".?!", mark);
            }
            Mark::LineEnd(mark, breaks) => {
                self.trim_end(&pauses);
                self.out.push_str(mark);
                self.out.push_str(breaks);
                self.out.push_str(&rest(&format!(" \t\n{PAUSE_MARKS}")));
                self.capitalize_next = true;
            }
            Mark::Open(mark) => {
                self.open_with(mark);
                self.out.push_str(&rest(&pauses));
            }
            Mark::Close(pair) => {
                self.close_with(pair);
                self.out.push_str(&spaced(after.clone()));
            }
            Mark::ToggleQuote { .. } => {
                if self.is_open(Pair::Quote) {
                    self.close_with(Pair::Quote);
                    self.out.push_str(&spaced(after.clone()));
                } else {
                    let open = self.table.quotes.0.to_string();
                    self.open_with(&open);
                    self.out.push_str(&rest(&pauses));
                }
            }
            Mark::Join(mark) => {
                self.trim_end(if command.family == Family::Hyphen {
                    " \t-"
                } else {
                    " \t"
                });
                self.out.push_str(mark);
                self.out.push_str(&rest(" \t"));
                self.lowercase_next = command.family == Family::Apostrophe;
            }
            Mark::Prefix(mark) => {
                self.open_with(mark);
                self.out.push_str(&rest(" \t"));
            }
            Mark::Spaced(mark) => {
                self.trim_end(" \t,;:");
                self.out.push(' ');
                self.out.push_str(mark);
                self.out.push_str(&spaced(rest(" \t,;:")));
            }
            Mark::Emoticon(face) => {
                self.trim_end(" \t,");
                self.out.push(' ');
                self.out.push_str(face);
                self.out.push_str(&spaced(rest(" \t,.")));
            }
        }
        resume
    }

    fn trim_end(&mut self, set: &str) {
        while t::last(&self.out).is_some_and(|c| t::is_one_of(c, set)) {
            t::remove_last(&mut self.out);
        }
    }

    /// An opening mark with a space before it, none after.
    fn open_with(&mut self, mark: &str) {
        self.trim_end(" \t");
        if t::last(&self.out).is_some_and(|c| !t::is_newline(c) && !t::is_one_of(c, OPENERS)) {
            self.out.push(' ');
        }
        self.out.push_str(mark);
    }

    /// A closing mark against the word before it; a straight quote closes a straight one.
    fn close_with(&mut self, pair: Pair) {
        let straight = pair == Pair::Quote && self.count('"') % 2 == 1;
        self.trim_end(" \t,;:");
        match pair {
            Pair::Paren => self.out.push(')'),
            Pair::Quote => self
                .out
                .push(if straight { '"' } else { self.table.quotes.1 }),
        }
    }
}

fn capitalized(word: &str) -> String {
    let Some(first) = t::first(word) else {
        return word.to_owned();
    };
    let rest = &word[first.len()..];
    if !t::is_lowercase(first) || t::chars(rest).iter().any(|c| t::is_uppercase(c)) {
        return word.to_owned();
    }
    t::uppercased(first) + rest
}

// MARK: Commands

#[derive(Clone, Copy, PartialEq)]
enum Pair {
    Paren,
    Quote,
}

#[derive(Clone, Copy)]
enum Mark {
    /// Against the word before: , . ? ! : ; …
    Attach(&'static str),
    /// A mark, then line breaks: "punto y aparte".
    LineEnd(&'static str, &'static str),
    /// Before the next word: ( ¿ ¡ “
    Open(&'static str),
    Close(Pair),
    /// A bare "quote": opens, or closes the quote that is open. `toggles`: two of them make a
    /// pair; English needs "end quote".
    ToggleQuote {
        toggles: bool,
    },
    /// Between two words, no spaces: - ' @
    Join(&'static str),
    /// On the next word: # $
    Prefix(&'static str),
    /// With a space each side: –
    Spaced(&'static str),
    Emoticon(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Family {
    Comma,
    Period,
    Question,
    Exclamation,
    Colon,
    Semicolon,
    Ellipsis,
    Dash,
    Hyphen,
    Apostrophe,
    At,
    Hashtag,
    Dollar,
    Quote,
    Paren,
    Smiley,
}

#[derive(Clone, Copy, PartialEq)]
enum Placement {
    Lenient,
    /// Needs command-like placement: the line's end, a mark after it, or a clause start.
    Strict,
    /// Needs words after it on the line.
    Following,
    StrictFollowing,
}

struct Command {
    words: Vec<String>,
    mark: Mark,
    family: Family,
    strict: bool,
    needs_following: bool,
}

// MARK: Words per language

/// A language's words as SpokenPunctuation.swift writes them.
struct Spec {
    commands: &'static [(&'static str, Mark, Family, Placement)],
    blockers: &'static [&'static str],
    numbers: &'static [&'static str],
    prepositions: &'static [&'static str],
    prepositions_end_sentences: bool,
    family_blockers: &'static [(Family, &'static [&'static str])],
    followers: &'static [&'static str],
    family_followers: &'static [(Family, &'static [&'static str])],
    follower_prefixes: &'static [&'static str],
    starters: &'static [&'static str],
    starter_prefixes: &'static [&'static str],
    quotes: (char, char),
    capitalizes_sentences: bool,
    adjectives_before_nouns: bool,
    possessives: bool,
}

/// A language's words folded, as the Swift `Table` holds them.
struct Table {
    commands: Vec<Command>,
    /// Words before a name that make it a noun: articles, possessives, numbers, adjectives.
    blockers: HashSet<String>,
    prepositions: HashSet<String>,
    numbers: HashSet<String>,
    prepositions_end_sentences: bool,
    family_blockers: Vec<(Family, HashSet<String>)>,
    /// Words after a name that continue the noun: "is", "of", "de".
    followers: HashSet<String>,
    family_followers: Vec<(Family, HashSet<String>)>,
    follower_prefixes: Vec<String>,
    /// Words that start a clause after a name said at its end.
    starters: HashSet<String>,
    starter_prefixes: Vec<String>,
    quotes: (char, char),
    capitalizes_sentences: bool,
    adjectives_before_nouns: bool,
    possessives: bool,
}

/// The table's form of a word: folded, and compared as Swift compares strings.
fn key(word: &str) -> String {
    t::canon(&folded(word)).into_owned()
}

fn set(words: &[&str]) -> HashSet<String> {
    words.iter().map(|w| key(w)).collect()
}

impl Table {
    fn new(spec: &Spec) -> Table {
        let mut commands: Vec<Command> = spec
            .commands
            .iter()
            .map(|&(phrase, mark, family, placement)| Command {
                words: t::split_on(&folded(phrase), ' ', false)
                    .into_iter()
                    .flat_map(|part| t::split_on(part, '-', false))
                    .map(str::to_owned)
                    .collect(),
                mark,
                family,
                strict: matches!(placement, Placement::Strict | Placement::StrictFollowing),
                needs_following: matches!(
                    placement,
                    Placement::Following | Placement::StrictFollowing
                ),
            })
            .collect();
        commands.sort_by_key(|command| std::cmp::Reverse(command.words.len()));
        Table {
            commands,
            blockers: set(spec.blockers),
            prepositions: set(spec.prepositions),
            numbers: set(spec.numbers),
            prepositions_end_sentences: spec.prepositions_end_sentences,
            family_blockers: spec
                .family_blockers
                .iter()
                .map(|&(family, words)| (family, set(words)))
                .collect(),
            followers: set(spec.followers),
            family_followers: spec
                .family_followers
                .iter()
                .map(|&(family, words)| (family, set(words)))
                .collect(),
            follower_prefixes: spec.follower_prefixes.iter().map(|w| folded(w)).collect(),
            starters: set(spec.starters),
            starter_prefixes: spec.starter_prefixes.iter().map(|w| folded(w)).collect(),
            quotes: spec.quotes,
            capitalizes_sentences: spec.capitalizes_sentences,
            adjectives_before_nouns: spec.adjectives_before_nouns,
            possessives: spec.possessives,
        }
    }

    fn family_blockers(&self, family: Family) -> Option<&HashSet<String>> {
        self.family_blockers
            .iter()
            .find(|(f, _)| *f == family)
            .map(|(_, set)| set)
    }

    fn family_followers(&self, family: Family) -> Option<&HashSet<String>> {
        self.family_followers
            .iter()
            .find(|(f, _)| *f == family)
            .map(|(_, set)| set)
    }

    fn for_language(language: Option<&Language>) -> Option<&'static Table> {
        static TABLES: [OnceLock<Table>; 6] = [const { OnceLock::new() }; 6];
        let (slot, spec) = match super::code_or_english(language) {
            "en" => (0, &tables::ENGLISH),
            "es" => (1, &tables::SPANISH),
            "fr" => (2, &tables::FRENCH),
            "de" => (3, &tables::GERMAN),
            "it" => (4, &tables::ITALIAN),
            "pt" => (5, &tables::PORTUGUESE),
            _ => return None,
        };
        Some(TABLES[slot].get_or_init(|| Table::new(spec)))
    }
}
