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

/// Free text captured by a trailing `{name}` placeholder in a personal
/// command phrase. The text is the normalized spoken remainder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedPhrase {
    pub name: String,
    pub text: String,
}

/// Longest normalized capture accepted from one completed command line.
pub(crate) const MAX_CAPTURE_WORDS: usize = 24;
pub(crate) const MAX_CAPTURE_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
enum PatternToken {
    Literal(String),
    Digit,
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
            Self::Rest => true,
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
    let mut characters = name.chars();
    let starts_lowercase = characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase());
    if !starts_lowercase
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(
            "capture names start with a lowercase letter and use lowercase letters, digits, and underscores"
                .into(),
        );
    }
    if normalize(&phrase[..open]).is_empty() {
        return Err(
            "a capture phrase needs at least one spoken word before the placeholder".into(),
        );
    }
    Ok(Some(name.into()))
}

/// Compile a personal phrase that may end in a `{name}` capture placeholder.
fn capture_pattern(phrase: String) -> Result<TypedPattern<Option<CapturedPhrase>>, String> {
    let Some(name) = phrase_placeholder(&phrase)? else {
        let words = normalize(&phrase)
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let signature = words.iter().cloned().map(PatternToken::Literal).collect();
        return Ok(TypedPattern {
            display: phrase,
            signatures: vec![signature],
            parse: Box::new(move |heard| (heard == words).then_some(None)),
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
            Some(Some(CapturedPhrase {
                name: name.clone(),
                text,
            }))
        }),
    })
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
        action: impl Fn(Option<CapturedPhrase>) -> Action + Send + Sync + 'static,
        group: Option<String>,
    ) -> Result<Self, CommandError> {
        let id = id.into();
        let mut patterns = Vec::new();
        for phrase in phrases {
            match capture_pattern(phrase.into()) {
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
