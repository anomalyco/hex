use crate::commands::{Action, CommandInfo, CommandScope};
use crate::context::{ContextSelector, ContextSnapshot};
use crate::keyboard::Key;

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
enum PatternToken {
    Literal(String),
    Digit,
    Direction,
    Count,
}

impl PatternToken {
    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Literal(left), Self::Literal(right)) => left == right,
            (Self::Literal(literal), slot) | (slot, Self::Literal(literal)) => {
                slot.accepts_literal(literal)
            }
            (Self::Direction, Self::Direction) => true,
            (Self::Digit, Self::Digit | Self::Count) | (Self::Count, Self::Digit | Self::Count) => {
                true
            }
            _ => false,
        }
    }

    fn accepts_literal(&self, literal: &str) -> bool {
        match self {
            Self::Literal(expected) => expected == literal,
            Self::Digit => literal.len() == 1 && literal.as_bytes()[0].is_ascii_digit(),
            Self::Direction => parse_direction(literal).is_some(),
            Self::Count => parse_count(literal).is_some(),
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
        .chain([PatternToken::Digit])
        .collect();
    TypedPattern {
        display,
        signatures: vec![signature],
        parse: Box::new(move |words| {
            let (digit, heard_literals) = words.split_last()?;
            (heard_literals == literals && digit.len() == 1)
                .then(|| digit.as_bytes()[0])
                .filter(u8::is_ascii_digit)
                .map(|digit| CapturedDigit(digit - b'0'))
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

fn literal_phrase(phrase: &'static str) -> TypedPattern<()> {
    let words = normalize(phrase)
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let signature = words.iter().cloned().map(PatternToken::Literal).collect();
    TypedPattern {
        display: phrase.into(),
        signatures: vec![signature],
        parse: Box::new(move |heard| (heard == words).then_some(())),
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
    id: &'static str,
    description: &'static str,
}

impl Command {
    pub fn new(id: &'static str, description: &'static str) -> Self {
        Self { id, description }
    }

    pub fn spoken<P: PatternSpec>(self, pattern: P) -> CommandBuilder<P::Capture> {
        CommandBuilder {
            id: self.id,
            description: self.description,
            context: ContextSelector::Always,
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
            patterns,
        }
    }
}

pub struct CommandBuilder<C> {
    id: &'static str,
    description: &'static str,
    context: ContextSelector,
    patterns: Vec<TypedPattern<C>>,
}

impl<C: 'static> CommandBuilder<C> {
    pub fn spoken<P: PatternSpec<Capture = C>>(mut self, pattern: P) -> Self {
        self.patterns.push(pattern.compile());
        self
    }

    pub fn when(mut self, context: ContextSelector) -> Self {
        self.context = context;
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
        let action = std::sync::Arc::new(action);
        ConfiguredCommand {
            id: self.id,
            description: self.description,
            context: self.context,
            patterns: self
                .patterns
                .into_iter()
                .map(|pattern| {
                    let action = action.clone();
                    ErasedPattern {
                        display: pattern.display,
                        signatures: pattern.signatures,
                        resolve: Box::new(move |words| {
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

struct ErasedPattern {
    display: String,
    signatures: Vec<Vec<PatternToken>>,
    resolve: Box<ActionResolver>,
}

type ActionResolver = dyn Fn(&[&str]) -> Option<Action> + Send + Sync;

pub struct ConfiguredCommand {
    id: &'static str,
    description: &'static str,
    context: ContextSelector,
    patterns: Vec<ErasedPattern>,
}

impl ConfiguredCommand {
    pub(crate) fn id(&self) -> &'static str {
        self.id
    }

    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        self.context.overlaps(&other.context)
            && self.patterns.iter().any(|left| {
                other
                    .patterns
                    .iter()
                    .any(|right| patterns_overlap(left, right))
            })
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
            description: self.description,
            id: self.id,
        }
    }
}

fn patterns_overlap(left: &ErasedPattern, right: &ErasedPattern) -> bool {
    signatures_overlap(&left.signatures, &right.signatures)
}

fn signatures_overlap(left: &[Vec<PatternToken>], right: &[Vec<PatternToken>]) -> bool {
    left.iter().any(|left| {
        right.iter().any(|right| {
            left.len() == right.len()
                && left
                    .iter()
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

    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Always, _) | (_, Self::Always) => true,
            (Self::BrowserHost(left), Self::BrowserHost(right))
            | (Self::Application(left), Self::Application(right)) => left == right,
            (Self::BrowserHost(_), Self::Application(_))
            | (Self::Application(_), Self::BrowserHost(_)) => true,
        }
    }
}

pub(crate) fn normalize(text: &str) -> String {
    text.trim()
        .trim_matches(|character: char| character.is_ascii_punctuation())
        .split_whitespace()
        .map(|word| match word.to_ascii_lowercase().as_str() {
            "zero" => "0".to_string(),
            "one" => "1".to_string(),
            "two" => "2".to_string(),
            "three" => "3".to_string(),
            "four" => "4".to_string(),
            "five" => "5".to_string(),
            "six" => "6".to_string(),
            "seven" => "7".to_string(),
            "eight" => "8".to_string(),
            "nine" => "9".to_string(),
            _ => word.to_ascii_lowercase(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}
