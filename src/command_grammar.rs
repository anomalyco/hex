use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use crate::commands::{Action, CommandError, CommandInfo, CommandScope};
use crate::context::{ContextSelector, ContextSnapshot};
use crate::keyboard::Key;
use crate::spoken_text::normalize;

#[derive(Clone, Copy, Debug)]
pub struct Digit;

#[derive(Clone, Copy, Debug)]
pub struct Direction;

#[derive(Clone, Copy, Debug)]
pub struct Count;

#[derive(Clone, Copy, Debug)]
pub struct OptionalCount;

impl Count {
    pub const fn optional(self) -> OptionalCount {
        OptionalCount
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapturedDigit(u8);

impl CapturedDigit {
    pub fn as_char(self) -> char {
        char::from(b'0' + self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturedDirection {
    Left,
    Right,
    Up,
    Down,
}

impl CapturedDirection {
    pub fn key(self) -> Key {
        match self {
            Self::Left => Key::Left,
            Self::Right => Key::Right,
            Self::Up => Key::Up,
            Self::Down => Key::Down,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapturedCount(u8);

impl CapturedCount {
    pub fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapturedValue {
    Digit(u8),
    Letter(char),
    Choice(String),
    Text(String),
}

pub type CapturedValues = BTreeMap<String, CapturedValue>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersonalCapture {
    Digit { min: u8, max: u8 },
    Letter,
    Choice { choices: BTreeMap<String, String> },
    Union { members: Vec<PersonalCapture> },
    Text,
}

/// Longest normalized capture accepted from one completed command line.
pub(crate) const MAX_CAPTURE_WORDS: usize = 24;
pub(crate) const MAX_CAPTURE_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
enum PatternToken {
    Literal(String),
    Digit {
        min: u8,
        max: u8,
    },
    Letter,
    /// Normalized spoken word to canonical output value.
    Choice(BTreeMap<String, String>),
    Union(Vec<PatternToken>),
    Direction,
    Count,
    /// One or more trailing free-text words captured verbatim.
    Rest,
}

impl PatternToken {
    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Rest, _) | (_, Self::Rest) => true,
            (Self::Literal(left), Self::Literal(right)) => left == right,
            (Self::Literal(literal), slot) | (slot, Self::Literal(literal)) => {
                slot.accepts_literal(literal)
            }
            (Self::Direction, Self::Direction) => true,
            (Self::Letter, Self::Letter) => true,
            (Self::Choice(left), Self::Choice(right)) => {
                left.keys().any(|word| right.contains_key(word))
            }
            (Self::Choice(choices), slot) | (slot, Self::Choice(choices)) => {
                choices.keys().any(|word| slot.accepts_literal(word))
            }
            (Self::Union(members), token) | (token, Self::Union(members)) => {
                members.iter().any(|member| member.overlaps(token))
            }
            (
                Self::Digit {
                    min: left_min,
                    max: left_max,
                },
                Self::Digit {
                    min: right_min,
                    max: right_max,
                },
            ) => left_min <= right_max && right_min <= left_max,
            (Self::Digit { max, .. }, Self::Count) | (Self::Count, Self::Digit { max, .. }) => {
                *max >= 1
            }
            (Self::Count, Self::Count) => true,
            _ => false,
        }
    }

    fn accepts_literal(&self, literal: &str) -> bool {
        match self {
            Self::Literal(expected) => expected == literal,
            Self::Digit { min, max } => {
                parse_digit(literal).is_some_and(|digit| digit >= *min && digit <= *max)
            }
            Self::Direction => parse_direction(literal).is_some(),
            Self::Count => parse_count(literal).is_some(),
            Self::Letter => parse_letter(literal).is_some(),
            Self::Choice(choices) => choices.contains_key(literal),
            Self::Union(members) => members.iter().any(|member| member.accepts_literal(literal)),
            Self::Rest => true,
        }
    }
}

impl PersonalCapture {
    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        self.pattern_token().overlaps(&other.pattern_token())
    }

    fn pattern_token(&self) -> PatternToken {
        match self {
            Self::Digit { min, max } => PatternToken::Digit {
                min: *min,
                max: *max,
            },
            Self::Letter => PatternToken::Letter,
            Self::Choice { choices } => PatternToken::Choice(choices.clone()),
            Self::Union { members } => {
                PatternToken::Union(members.iter().map(Self::pattern_token).collect())
            }
            Self::Text => PatternToken::Rest,
        }
    }

    fn parse_word(&self, word: &str) -> Option<CapturedValue> {
        match self {
            Self::Digit { min, max } => {
                let digit = parse_digit(word)?;
                (digit >= *min && digit <= *max).then_some(CapturedValue::Digit(digit))
            }
            Self::Letter => parse_letter(word).map(CapturedValue::Letter),
            Self::Choice { choices } => choices.get(word).cloned().map(CapturedValue::Choice),
            Self::Union { members } => members.iter().find_map(|member| member.parse_word(word)),
            Self::Text => None,
        }
    }
}

type CaptureParser<C> = dyn Fn(&[&str]) -> Option<C> + Send + Sync;

#[doc(hidden)]
pub struct TypedPattern<C> {
    display: String,
    signatures: Vec<Vec<PatternToken>>,
    parse: Box<CaptureParser<C>>,
}

pub trait PatternSpec {
    type Capture: 'static;

    fn compile(self) -> TypedPattern<Self::Capture>;
}

impl PatternSpec for (&'static str, Digit) {
    type Capture = CapturedDigit;

    fn compile(self) -> TypedPattern<Self::Capture> {
        digit_pattern(&[self.0])
    }
}

impl PatternSpec for (&'static str, &'static str, Digit) {
    type Capture = CapturedDigit;

    fn compile(self) -> TypedPattern<Self::Capture> {
        digit_pattern(&[self.0, self.1])
    }
}

impl PatternSpec for (&'static str, &'static str) {
    type Capture = ();

    fn compile(self) -> TypedPattern<Self::Capture> {
        literal_pattern(&[self.0, self.1])
    }
}

impl PatternSpec for (&'static str, &'static str, &'static str) {
    type Capture = ();

    fn compile(self) -> TypedPattern<Self::Capture> {
        literal_pattern(&[self.0, self.1, self.2])
    }
}

impl PatternSpec for (&'static str, Count) {
    type Capture = CapturedCount;

    fn compile(self) -> TypedPattern<Self::Capture> {
        let literal = normalize(self.0);
        let parse_literal = literal.clone();
        TypedPattern {
            display: format!("{} <count>", self.0),
            signatures: vec![vec![PatternToken::Literal(literal), PatternToken::Count]],
            parse: Box::new(move |words| match words {
                [heard_literal, count] if *heard_literal == parse_literal => parse_count(count),
                _ => None,
            }),
        }
    }
}

impl PatternSpec for (&'static str, Direction, OptionalCount) {
    type Capture = (CapturedDirection, Option<CapturedCount>);

    fn compile(self) -> TypedPattern<Self::Capture> {
        let literal = normalize(self.0);
        let parse_literal = literal.clone();
        TypedPattern {
            display: format!("{} <direction> [<count>]", self.0),
            signatures: vec![
                vec![
                    PatternToken::Literal(literal.clone()),
                    PatternToken::Direction,
                ],
                vec![
                    PatternToken::Literal(literal),
                    PatternToken::Direction,
                    PatternToken::Count,
                ],
            ],
            parse: Box::new(move |words| match words {
                [heard_literal, direction] if *heard_literal == parse_literal => {
                    Some((parse_direction(direction)?, None))
                }
                [heard_literal, direction, count] if *heard_literal == parse_literal => {
                    Some((parse_direction(direction)?, Some(parse_count(count)?)))
                }
                _ => None,
            }),
        }
    }
}

fn digit_pattern(literals: &[&'static str]) -> TypedPattern<CapturedDigit> {
    let literals = literals
        .iter()
        .map(|literal| normalize(literal))
        .collect::<Vec<_>>();
    let display = format!("{} <digit>", literals.join(" "));
    let signature = literals
        .iter()
        .cloned()
        .map(PatternToken::Literal)
        .chain([PatternToken::Digit { min: 0, max: 9 }])
        .collect();
    TypedPattern {
        display,
        signatures: vec![signature],
        parse: Box::new(move |words| {
            let (digit, heard_literals) = words.split_last()?;
            (heard_literals == literals)
                .then(|| parse_digit(digit))
                .flatten()
                .map(CapturedDigit)
        }),
    }
}

fn literal_pattern(literals: &[&'static str]) -> TypedPattern<()> {
    let display = literals.join(" ");
    let literals = literals
        .iter()
        .map(|literal| normalize(literal))
        .collect::<Vec<_>>();
    let signature = literals
        .iter()
        .cloned()
        .map(PatternToken::Literal)
        .collect();
    TypedPattern {
        display,
        signatures: vec![signature],
        parse: Box::new(move |words| (words == literals).then_some(())),
    }
}

fn literal_phrase(phrase: impl Into<String>) -> TypedPattern<()> {
    let phrase = phrase.into();
    let words = normalize(&phrase)
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let signature = words.iter().cloned().map(PatternToken::Literal).collect();
    TypedPattern {
        display: phrase,
        signatures: vec![signature],
        parse: Box::new(move |heard| (heard == words).then_some(())),
    }
}

/// Validate a personal phrase and return its trailing capture name, if any.
///
/// A capture phrase has the shape `spoken words {name}`: at least one literal
/// word, exactly one placeholder, placeholder last, and a lowercase
/// `[a-z][a-z0-9_]*` name.
pub(crate) fn phrase_placeholder(phrase: &str) -> Result<Option<String>, String> {
    let opens = phrase.matches('{').count();
    let closes = phrase.matches('}').count();
    if opens == 0 && closes == 0 {
        return Ok(None);
    }
    if opens != 1 || closes != 1 {
        return Err("a phrase may contain at most one {capture} placeholder".into());
    }
    let open = phrase.find('{').expect("counted one opening brace");
    let close = phrase.find('}').expect("counted one closing brace");
    if close < open {
        return Err("the {capture} placeholder is malformed".into());
    }
    if !phrase[close + 1..].trim().is_empty() {
        return Err("the {capture} placeholder must end the phrase".into());
    }
    let name = &phrase[open + 1..close];
    validate_capture_name(name)?;
    if normalize(&phrase[..open]).is_empty() {
        return Err(
            "a capture phrase needs at least one spoken word before the placeholder".into(),
        );
    }
    Ok(Some(name.into()))
}

/// Compile a personal phrase that may end in a `{name}` capture placeholder.
fn capture_pattern(phrase: String) -> Result<TypedPattern<CapturedValues>, String> {
    let Some(name) = phrase_placeholder(&phrase)? else {
        let words = normalize(&phrase)
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let signature = words.iter().cloned().map(PatternToken::Literal).collect();
        return Ok(TypedPattern {
            display: phrase,
            signatures: vec![signature],
            parse: Box::new(move |heard| (heard == words).then_some(BTreeMap::new())),
        });
    };
    let open = phrase.find('{').expect("placeholder was validated");
    let words = normalize(&phrase[..open])
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let signature = words
        .iter()
        .cloned()
        .map(PatternToken::Literal)
        .chain([PatternToken::Rest])
        .collect();
    Ok(TypedPattern {
        display: phrase.trim().to_string(),
        signatures: vec![signature],
        parse: Box::new(move |heard| {
            if heard.len() <= words.len() {
                return None;
            }
            let (prefix, rest) = heard.split_at(words.len());
            if prefix != words || rest.len() > MAX_CAPTURE_WORDS {
                return None;
            }
            let text = rest.join(" ");
            if text.len() > MAX_CAPTURE_BYTES {
                return None;
            }
            Some(BTreeMap::from([(name.clone(), CapturedValue::Text(text))]))
        }),
    })
}

fn schema_capture_pattern(
    phrase: String,
    captures: &BTreeMap<String, PersonalCapture>,
) -> Result<TypedPattern<CapturedValues>, String> {
    let mut tokens = Vec::new();
    let mut names = HashSet::new();
    for raw in phrase.split_whitespace() {
        if raw.starts_with('{') || raw.ends_with('}') {
            if !(raw.starts_with('{')
                && raw.ends_with('}')
                && raw.matches('{').count() == 1
                && raw.matches('}').count() == 1)
            {
                return Err("capture placeholders must be standalone {name} words".into());
            }
            let name = &raw[1..raw.len() - 1];
            validate_capture_name(name)?;
            let Some(capture) = captures.get(name) else {
                return Err(format!("phrase references undeclared capture {{{name}}}"));
            };
            if !names.insert(name.to_string()) {
                return Err(format!("capture {{{name}}} must appear exactly once"));
            }
            tokens.push(capture.pattern_token());
        } else {
            tokens.extend(
                normalize(raw)
                    .split_whitespace()
                    .map(|word| PatternToken::Literal(word.to_string())),
            );
        }
    }
    if names.len() != captures.len() {
        let missing = captures
            .keys()
            .find(|name| !names.contains(*name))
            .expect("capture count differs");
        return Err(format!("capture {{{missing}}} must appear exactly once"));
    }
    if tokens.is_empty() {
        return Err("a command phrase must contain a spoken word".into());
    }
    if let Some(index) = tokens
        .iter()
        .position(|token| matches!(token, PatternToken::Rest))
        && index + 1 != tokens.len()
    {
        return Err("text() captures must be trailing".into());
    }
    let parse_captures = captures.clone();
    Ok(TypedPattern {
        display: phrase.trim().to_string(),
        signatures: vec![tokens],
        parse: Box::new(move |heard| {
            let mut values = BTreeMap::new();
            let mut word_index = 0;
            for part in phrase.split_whitespace() {
                if part.starts_with('{') {
                    let name = &part[1..part.len() - 1];
                    match parse_captures.get(name)? {
                        capture @ (PersonalCapture::Digit { .. }
                        | PersonalCapture::Letter
                        | PersonalCapture::Choice { .. }
                        | PersonalCapture::Union { .. }) => {
                            let value = capture.parse_word(heard.get(word_index)?)?;
                            values.insert(name.to_string(), value);
                            word_index += 1;
                        }
                        PersonalCapture::Text => {
                            let rest = heard.get(word_index..)?;
                            if rest.is_empty() || rest.len() > MAX_CAPTURE_WORDS {
                                return None;
                            }
                            let text = rest.join(" ");
                            if text.len() > MAX_CAPTURE_BYTES {
                                return None;
                            }
                            values.insert(name.to_string(), CapturedValue::Text(text));
                            word_index = heard.len();
                        }
                    }
                } else {
                    for literal in normalize(part).split_whitespace() {
                        if heard.get(word_index).copied() != Some(literal) {
                            return None;
                        }
                        word_index += 1;
                    }
                }
            }
            (word_index == heard.len()).then_some(values)
        }),
    })
}

fn validate_capture_name(name: &str) -> Result<(), String> {
    let mut characters = name.chars();
    if !characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
    {
        return Err("capture names start with a lowercase letter and use lowercase letters, digits, and underscores".into());
    }
    Ok(())
}

fn parse_digit(word: &str) -> Option<u8> {
    match word {
        "zero" | "0" => Some(0),
        "one" | "1" => Some(1),
        "two" | "2" => Some(2),
        "three" | "3" => Some(3),
        "four" | "4" => Some(4),
        "five" | "5" => Some(5),
        "six" | "6" => Some(6),
        "seven" | "7" => Some(7),
        "eight" | "8" => Some(8),
        "nine" | "9" => Some(9),
        _ => None,
    }
}

const LETTER_ALIASES: &[(&str, char)] = &[
    ("a", 'a'),
    ("ay", 'a'),
    ("alpha", 'a'),
    ("b", 'b'),
    ("bee", 'b'),
    ("bravo", 'b'),
    ("c", 'c'),
    ("see", 'c'),
    ("charlie", 'c'),
    ("d", 'd'),
    ("dee", 'd'),
    ("delta", 'd'),
    ("e", 'e'),
    ("echo", 'e'),
    ("f", 'f'),
    ("ef", 'f'),
    ("foxtrot", 'f'),
    ("g", 'g'),
    ("gee", 'g'),
    ("golf", 'g'),
    ("h", 'h'),
    ("aitch", 'h'),
    ("hotel", 'h'),
    ("i", 'i'),
    ("eye", 'i'),
    ("india", 'i'),
    ("j", 'j'),
    ("jay", 'j'),
    ("juliett", 'j'),
    ("k", 'k'),
    ("kay", 'k'),
    ("kilo", 'k'),
    ("l", 'l'),
    ("el", 'l'),
    ("lima", 'l'),
    ("m", 'm'),
    ("em", 'm'),
    ("mike", 'm'),
    ("n", 'n'),
    ("en", 'n'),
    ("november", 'n'),
    ("o", 'o'),
    ("oh", 'o'),
    ("oscar", 'o'),
    ("p", 'p'),
    ("pee", 'p'),
    ("papa", 'p'),
    ("q", 'q'),
    ("cue", 'q'),
    ("quebec", 'q'),
    ("r", 'r'),
    ("are", 'r'),
    ("romeo", 'r'),
    ("s", 's'),
    ("ess", 's'),
    ("sierra", 's'),
    ("t", 't'),
    ("tee", 't'),
    ("tango", 't'),
    ("u", 'u'),
    ("you", 'u'),
    ("uniform", 'u'),
    ("v", 'v'),
    ("vee", 'v'),
    ("victor", 'v'),
    ("w", 'w'),
    ("whiskey", 'w'),
    ("x", 'x'),
    ("xray", 'x'),
    ("y", 'y'),
    ("why", 'y'),
    ("yankee", 'y'),
    ("z", 'z'),
    ("zee", 'z'),
    ("zed", 'z'),
    ("zulu", 'z'),
];

fn parse_letter(word: &str) -> Option<char> {
    LETTER_ALIASES
        .iter()
        .find_map(|(alias, letter)| (*alias == word).then_some(*letter))
}

#[cfg(test)]
mod letter_tests {
    use super::*;

    #[test]
    fn letter_aliases_are_unique_normalized_words_and_cover_the_alphabet() {
        let mut aliases = HashSet::new();
        let mut letters = HashSet::new();
        for (alias, letter) in LETTER_ALIASES {
            assert_eq!(crate::spoken_text::normalize(alias), *alias);
            assert_eq!(alias.split_whitespace().count(), 1);
            assert!(aliases.insert(*alias), "duplicate letter alias {alias}");
            letters.insert(*letter);
        }
        assert_eq!(letters, ('a'..='z').collect());
    }

    #[test]
    fn letter_aliases_parse_to_canonical_lowercase_letters() {
        for (alias, expected) in LETTER_ALIASES {
            assert_eq!(parse_letter(alias), Some(*expected), "alias {alias}");
            if alias.len() == 1 {
                assert_eq!(parse_letter(&alias.to_ascii_uppercase()), None);
            }
        }
        assert_eq!(parse_letter("juliet"), None);
        assert_eq!(parse_letter("x-ray"), None);
    }

    #[test]
    fn letters_overlap_literals_choices_letters_and_text_but_not_digits() {
        let letter = PatternToken::Letter;
        assert!(letter.overlaps(&PatternToken::Literal("alpha".into())));
        assert!(letter.overlaps(&PatternToken::Choice(BTreeMap::from([(
            "bee".into(),
            "insect".into(),
        )]))));
        assert!(letter.overlaps(&PatternToken::Letter));
        assert!(letter.overlaps(&PatternToken::Rest));
        assert!(!letter.overlaps(&PatternToken::Digit { min: 0, max: 9 }));
    }

    #[test]
    fn unions_overlap_every_token_kind_through_their_members() {
        let union = PatternToken::Union(vec![
            PatternToken::Digit { min: 0, max: 2 },
            PatternToken::Letter,
            PatternToken::Choice(BTreeMap::from([("left".into(), "left".into())])),
        ]);
        assert!(union.overlaps(&PatternToken::Literal("alpha".into())));
        assert!(union.overlaps(&PatternToken::Digit { min: 2, max: 4 }));
        assert!(union.overlaps(&PatternToken::Letter));
        assert!(union.overlaps(&PatternToken::Choice(BTreeMap::from([(
            "left".into(),
            "other".into(),
        )]))));
        assert!(union.overlaps(&PatternToken::Direction));
        assert!(union.overlaps(&PatternToken::Count));
        assert!(union.overlaps(&PatternToken::Rest));
        assert!(union.overlaps(&PatternToken::Union(vec![PatternToken::Letter])));
        assert!(!union.overlaps(&PatternToken::Literal("home".into())));
        assert!(!union.overlaps(&PatternToken::Digit { min: 3, max: 9 }));
    }
}

fn parse_direction(word: &str) -> Option<CapturedDirection> {
    match word {
        "left" => Some(CapturedDirection::Left),
        "right" => Some(CapturedDirection::Right),
        "up" => Some(CapturedDirection::Up),
        "down" => Some(CapturedDirection::Down),
        _ => None,
    }
}

fn parse_count(word: &str) -> Option<CapturedCount> {
    word.parse::<u8>()
        .ok()
        .filter(|count| *count > 0)
        .map(CapturedCount)
}

pub struct Command {
    id: String,
    description: String,
}

impl Command {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
        }
    }

    pub fn spoken<P: PatternSpec>(self, pattern: P) -> CommandBuilder<P::Capture> {
        CommandBuilder {
            id: self.id,
            description: self.description,
            context: ContextSelector::Always,
            protected: false,
            patterns: vec![pattern.compile()],
        }
    }

    pub fn phrases(self, phrases: impl IntoIterator<Item = &'static str>) -> CommandBuilder<()> {
        let patterns = phrases.into_iter().map(literal_phrase).collect::<Vec<_>>();
        assert!(!patterns.is_empty(), "command must have a spoken phrase");
        CommandBuilder {
            id: self.id,
            description: self.description,
            context: ContextSelector::Always,
            protected: false,
            patterns,
        }
    }
}

pub struct CommandBuilder<C> {
    id: String,
    description: String,
    context: ContextSelector,
    protected: bool,
    patterns: Vec<TypedPattern<C>>,
}

impl<C: 'static> CommandBuilder<C> {
    pub fn spoken<P: PatternSpec<Capture = C>>(mut self, pattern: P) -> Self {
        self.patterns.push(pattern.compile());
        self
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn when(mut self, context: ContextSelector) -> Self {
        self.context = context;
        self
    }

    pub fn protected(mut self) -> Self {
        self.protected = true;
        self
    }

    pub fn action(self, action: impl Fn(C) -> Action + Send + Sync + 'static) -> ConfiguredCommand {
        for (index, left) in self.patterns.iter().enumerate() {
            for right in &self.patterns[index + 1..] {
                assert!(
                    !typed_patterns_overlap(left, right),
                    "command aliases overlap: {}",
                    self.id
                );
            }
        }
        let action = Arc::new(action);
        ConfiguredCommand {
            id: self.id,
            description: self.description,
            context: self.context,
            protected: self.protected,
            group: None,
            patterns: self
                .patterns
                .into_iter()
                .map(|pattern| {
                    let action = action.clone();
                    ErasedPattern {
                        display: pattern.display,
                        signatures: pattern.signatures,
                        resolve: Arc::new(move |words| {
                            (pattern.parse)(words).map(|capture| (action)(capture))
                        }),
                    }
                })
                .collect(),
        }
    }
}

fn typed_patterns_overlap<C>(left: &TypedPattern<C>, right: &TypedPattern<C>) -> bool {
    signatures_overlap(&left.signatures, &right.signatures)
}

#[derive(Clone)]
struct ErasedPattern {
    display: String,
    signatures: Vec<Vec<PatternToken>>,
    resolve: Arc<ActionResolver>,
}

type ActionResolver = dyn Fn(&[&str]) -> Option<Action> + Send + Sync;

#[derive(Clone)]
pub struct ConfiguredCommand {
    id: String,
    description: String,
    context: ContextSelector,
    protected: bool,
    group: Option<String>,
    patterns: Vec<ErasedPattern>,
}

impl ConfiguredCommand {
    #[allow(dead_code)]
    pub fn literal(
        id: impl Into<String>,
        description: impl Into<String>,
        phrases: impl IntoIterator<Item = impl Into<String>>,
        context: ContextSelector,
        action: Action,
    ) -> Result<Self, CommandError> {
        Self::personal_literal(id, description, phrases, context, action, None)
    }

    pub fn personal_literal(
        id: impl Into<String>,
        description: impl Into<String>,
        phrases: impl IntoIterator<Item = impl Into<String>>,
        context: ContextSelector,
        action: Action,
        group: Option<String>,
    ) -> Result<Self, CommandError> {
        let id = id.into();
        let patterns = phrases
            .into_iter()
            .map(|phrase| literal_phrase(phrase.into()))
            .collect::<Vec<_>>();
        if patterns.is_empty() {
            return Err(CommandError::MissingPhrase { id });
        }
        if patterns
            .iter()
            .any(|pattern| pattern.signatures.iter().any(Vec::is_empty))
        {
            return Err(CommandError::MissingPhrase { id });
        }
        for (index, left) in patterns.iter().enumerate() {
            for right in &patterns[index + 1..] {
                if typed_patterns_overlap(left, right) {
                    return Err(CommandError::OverlappingAliases { id });
                }
            }
        }
        let action = Arc::new(move |()| action.clone());
        Ok(Self {
            id,
            description: description.into(),
            context,
            protected: false,
            group,
            patterns: patterns
                .into_iter()
                .map(|pattern| {
                    let action = action.clone();
                    ErasedPattern {
                        display: pattern.display,
                        signatures: pattern.signatures,
                        resolve: Arc::new(move |words| {
                            (pattern.parse)(words).map(|capture| (action)(capture))
                        }),
                    }
                })
                .collect(),
        })
    }

    /// Compile a personal command whose phrases may end in one `{name}`
    /// capture placeholder. The action factory receives the capture matched
    /// from the completed line, or `None` for plain literal aliases.
    pub fn personal_command(
        id: impl Into<String>,
        description: impl Into<String>,
        phrases: impl IntoIterator<Item = impl Into<String>>,
        context: ContextSelector,
        captures: Option<BTreeMap<String, PersonalCapture>>,
        action: impl Fn(CapturedValues) -> Action + Send + Sync + 'static,
        group: Option<String>,
    ) -> Result<Self, CommandError> {
        let id = id.into();
        let mut patterns = Vec::new();
        for phrase in phrases {
            let phrase = phrase.into();
            let pattern = match &captures {
                Some(captures) => schema_capture_pattern(phrase, captures),
                None => capture_pattern(phrase),
            };
            match pattern {
                Ok(pattern) => patterns.push(pattern),
                Err(message) => return Err(CommandError::InvalidCapture { id, message }),
            }
        }
        if patterns.is_empty()
            || patterns
                .iter()
                .any(|pattern| pattern.signatures.iter().any(Vec::is_empty))
        {
            return Err(CommandError::MissingPhrase { id });
        }
        for (index, left) in patterns.iter().enumerate() {
            for right in &patterns[index + 1..] {
                if typed_patterns_overlap(left, right) {
                    return Err(CommandError::OverlappingAliases { id });
                }
            }
        }
        let action = Arc::new(action);
        Ok(Self {
            id,
            description: description.into(),
            context,
            protected: false,
            group,
            patterns: patterns
                .into_iter()
                .map(|pattern| {
                    let action = action.clone();
                    ErasedPattern {
                        display: pattern.display,
                        signatures: pattern.signatures,
                        resolve: Arc::new(move |words| {
                            (pattern.parse)(words).map(|capture| (action)(capture))
                        }),
                    }
                })
                .collect(),
        })
    }

    pub(crate) fn protocol_literal(
        id: impl Into<String>,
        phrases: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, CommandError> {
        let mut command = Self::personal_literal(
            id,
            "Voice dictation protocol",
            phrases,
            ContextSelector::Always,
            Action::StartDictation,
            None,
        )?;
        command.protected = true;
        Ok(command)
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn conflicts_with(&self, other: &Self) -> bool {
        self.context.can_coexist_with(&other.context)
            && (self.protected
                || other.protected
                || self.context.specificity() == other.context.specificity())
            && self.patterns.iter().any(|left| {
                other
                    .patterns
                    .iter()
                    .any(|right| patterns_overlap(left, right))
            })
    }

    pub(crate) fn conflicts_at_prefix(&self, other: &Self) -> bool {
        self.patterns.iter().any(|left| {
            other
                .patterns
                .iter()
                .any(|right| signatures_prefix_overlap(&left.signatures, &right.signatures))
        })
    }

    pub(crate) fn specificity(&self) -> u8 {
        self.context.specificity()
    }

    pub(crate) fn match_words(&self, words: &[&str], context: &ContextSnapshot) -> Option<Action> {
        if !self.context.matches(context) {
            return None;
        }
        self.patterns
            .iter()
            .find_map(|pattern| (pattern.resolve)(words))
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) fn matches_context(&self, context: &ContextSnapshot) -> bool {
        self.context.matches(context)
    }

    pub(crate) fn catalog(&self) -> CommandInfo {
        let mut phrases = self.patterns.iter().map(|pattern| pattern.display.clone());
        CommandInfo {
            scope: self.context.scope(),
            phrase: phrases.next().expect("command must have a spoken pattern"),
            aliases: phrases.collect(),
            description: self.description.clone(),
            id: self.id.clone(),
            group: self.group.clone(),
        }
    }
}

fn patterns_overlap(left: &ErasedPattern, right: &ErasedPattern) -> bool {
    signatures_overlap(&left.signatures, &right.signatures)
}

fn signatures_overlap(left: &[Vec<PatternToken>], right: &[Vec<PatternToken>]) -> bool {
    left.iter().any(|left| {
        right
            .iter()
            .any(|right| signature_pair_overlaps(left, right))
    })
}

/// Whether one spoken word sequence could match both signatures. A trailing
/// `Rest` capture accepts one or more arbitrary words, so a capture signature
/// overlaps every signature that shares its literal prefix and is long enough
/// to reach the capture.
fn signature_pair_overlaps(left: &[PatternToken], right: &[PatternToken]) -> bool {
    let left_rest = matches!(left.last(), Some(PatternToken::Rest));
    let right_rest = matches!(right.last(), Some(PatternToken::Rest));
    let left_fixed = &left[..left.len() - usize::from(left_rest)];
    let right_fixed = &right[..right.len() - usize::from(right_rest)];
    let lengths_compatible = match (left_rest, right_rest) {
        (false, false) => left.len() == right.len(),
        (true, false) => right_fixed.len() > left_fixed.len(),
        (false, true) => left_fixed.len() > right_fixed.len(),
        (true, true) => true,
    };
    lengths_compatible
        && left_fixed
            .iter()
            .zip(right_fixed)
            .all(|(left, right)| left.overlaps(right))
}

fn signatures_prefix_overlap(left: &[Vec<PatternToken>], right: &[Vec<PatternToken>]) -> bool {
    left.iter().any(|left| {
        right.iter().any(|right| {
            left.iter()
                .zip(right)
                .all(|(left, right)| left.overlaps(right))
        })
    })
}

impl ContextSelector {
    fn scope(&self) -> CommandScope {
        match self {
            Self::Always => CommandScope::Global,
            Self::BrowserHost(host) => CommandScope::Browser(host.clone()),
            Self::Application(application) => CommandScope::Application(application.clone()),
        }
    }
}
