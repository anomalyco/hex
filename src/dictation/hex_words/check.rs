//! The check pass: one walk over the text before any parser builds values
//! (the spec's "Reading, step by step", step 5). It refuses with the first
//! problem it meets: nesting deeper than [`MAX_DEPTH`], an object that
//! repeats a key (compared after escapes are decoded), or anything strict
//! JSON (RFC 8259) doesn't allow. The walk keeps its own stack of open
//! brackets and never recurses, so no depth of nesting can exhaust a stack.

use std::collections::HashSet;
use std::ops::Range;

use super::{MAX_DEPTH, Refusal};

/// An open `{` with the keys read so far, or an open `[`.
enum Open {
    Object(HashSet<String>),
    List,
}

/// Checks `text`. On success, returns where the numbers too large for a
/// double are (`1e400`): RFC 8259 allows them, but `serde_json` refuses
/// to build them, so the reader reads each as a value of the wrong type,
/// as a reader that holds it as infinity would.
pub(super) fn check(text: &str) -> Result<Vec<Range<usize>>, Refusal> {
    let mut walk = Walk {
        text,
        bytes: text.as_bytes(),
        at: 0,
        open: Vec::new(),
        huge: Vec::new(),
    };
    walk.run()?;
    Ok(walk.huge)
}

struct Walk<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    open: Vec<Open>,
    huge: Vec<Range<usize>>,
}

impl Walk<'_> {
    fn run(&mut self) -> Result<(), Refusal> {
        'value: loop {
            self.space();
            match self.peek() {
                Some(b'{') => {
                    self.enter(Open::Object(HashSet::new()))?;
                    self.space();
                    if !self.eat(b'}') {
                        self.key()?;
                        continue 'value;
                    }
                    self.open.pop();
                }
                Some(b'[') => {
                    self.enter(Open::List)?;
                    self.space();
                    if !self.eat(b']') {
                        continue 'value;
                    }
                    self.open.pop();
                }
                Some(b'"') => self.string(None)?,
                Some(b'-' | b'0'..=b'9') => self.number()?,
                Some(b't') => self.literal(b"true")?,
                Some(b'f') => self.literal(b"false")?,
                Some(b'n') => self.literal(b"null")?,
                _ => return Err(Refusal::NotJson),
            }
            // After a value: a comma, a closing bracket, or the end.
            loop {
                self.space();
                match self.open.last() {
                    None if self.at == self.bytes.len() => return Ok(()),
                    None => return Err(Refusal::NotJson),
                    Some(Open::Object(_)) => {
                        if self.eat(b',') {
                            self.space();
                            self.key()?;
                            continue 'value;
                        }
                        if !self.eat(b'}') {
                            return Err(Refusal::NotJson);
                        }
                    }
                    Some(Open::List) => {
                        if self.eat(b',') {
                            continue 'value;
                        }
                        if !self.eat(b']') {
                            return Err(Refusal::NotJson);
                        }
                    }
                }
                self.open.pop();
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        if found {
            self.at += 1;
        }
        found
    }

    /// JSON's whitespace: space, tab, LF, and CR, nothing else.
    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    /// Opens a bracket, refusing a ninth level.
    fn enter(&mut self, open: Open) -> Result<(), Refusal> {
        if self.open.len() >= MAX_DEPTH {
            return Err(Refusal::TooDeep);
        }
        self.open.push(open);
        self.at += 1;
        Ok(())
    }

    /// An object's key and its colon, refusing a key the object already has.
    fn key(&mut self) -> Result<(), Refusal> {
        if self.peek() != Some(b'"') {
            return Err(Refusal::NotJson);
        }
        let mut key = String::new();
        self.string(Some(&mut key))?;
        if let Some(Open::Object(keys)) = self.open.last_mut()
            && !keys.insert(key)
        {
            return Err(Refusal::DuplicateKey);
        }
        self.space();
        if self.eat(b':') {
            Ok(())
        } else {
            Err(Refusal::NotJson)
        }
    }

    /// A string from its opening quote, decoded into `decoded` when given.
    /// A raw control character, an unknown escape, or a lone surrogate
    /// refuses it.
    fn string(&mut self, mut decoded: Option<&mut String>) -> Result<(), Refusal> {
        self.at += 1;
        loop {
            let Some(byte) = self.peek() else {
                return Err(Refusal::NotJson);
            };
            match byte {
                b'"' => {
                    self.at += 1;
                    return Ok(());
                }
                b'\\' => {
                    let escape = self.bytes.get(self.at + 1).copied();
                    self.at += 2;
                    let character = match escape {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => self.unicode_escape()?,
                        _ => return Err(Refusal::NotJson),
                    };
                    if let Some(decoded) = decoded.as_deref_mut() {
                        decoded.push(character);
                    }
                }
                0x00..=0x1f => return Err(Refusal::NotJson),
                _ => {
                    // The text is UTF-8 and `at` sits on a character's start.
                    let character = self.text[self.at..]
                        .chars()
                        .next()
                        .ok_or(Refusal::NotJson)?;
                    self.at += character.len_utf8();
                    if let Some(decoded) = decoded.as_deref_mut() {
                        decoded.push(character);
                    }
                }
            }
        }
    }

    /// The character a `\u` escape names, after its `\u`: a high surrogate
    /// must be followed by an escaped low one.
    fn unicode_escape(&mut self) -> Result<char, Refusal> {
        let unit = self.hex4()?;
        let code = match unit {
            0xd800..=0xdbff => {
                if self.bytes.get(self.at..self.at + 2) != Some(b"\\u".as_slice()) {
                    return Err(Refusal::NotJson);
                }
                self.at += 2;
                let low = self.hex4()?;
                if !(0xdc00..=0xdfff).contains(&low) {
                    return Err(Refusal::NotJson);
                }
                0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00)
            }
            0xdc00..=0xdfff => return Err(Refusal::NotJson),
            _ => unit,
        };
        char::from_u32(code).ok_or(Refusal::NotJson)
    }

    fn hex4(&mut self) -> Result<u32, Refusal> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or(Refusal::NotJson)?;
        let mut value = 0;
        for &digit in digits {
            value = value * 16 + char::from(digit).to_digit(16).ok_or(Refusal::NotJson)?;
        }
        self.at += 4;
        Ok(value)
    }

    /// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`; what follows it is
    /// checked as whatever comes after a value.
    fn number(&mut self) -> Result<(), Refusal> {
        let start = self.at;
        self.eat(b'-');
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(Refusal::NotJson),
        }
        if self.eat(b'.') && !self.digits() {
            return Err(Refusal::NotJson);
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !self.digits() {
                return Err(Refusal::NotJson);
            }
        }
        if self.text[start..self.at]
            .parse::<f64>()
            .is_ok_and(f64::is_infinite)
        {
            self.huge.push(start..self.at);
        }
        Ok(())
    }

    /// Skips digits; false when there were none.
    fn digits(&mut self) -> bool {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at > start
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), Refusal> {
        if self.bytes[self.at..].starts_with(word) {
            self.at += word.len();
            Ok(())
        } else {
            Err(Refusal::NotJson)
        }
    }
}
