//! The parity fixture's dictionary, as the Swift exporter's `ACStubDictionary` and
//! `ACStubChecker` (`ios/HEXMobileTests/AutocorrectParity/AutocorrectParityStub.swift`) answer:
//! the same words from the same data, and every call logged the same way.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use hex_keyboard_core::autocorrect::{TextChecker, text};

pub fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn key(s: &str) -> String {
    text::nfc(s).into_owned()
}

pub struct StubDictionary {
    rank: HashMap<String, usize>,
    names: HashSet<String>,
    upper_names: HashSet<String>,
    name_by_lower: HashMap<String, (usize, String)>,
    alphabet: Vec<String>,
    by_first: HashMap<String, Vec<(usize, String)>>,
    memo: RefCell<HashMap<String, Vec<String>>>,
}

impl StubDictionary {
    pub fn new(words: &[String], name_list: &[String]) -> Self {
        let name_lowers: HashSet<String> = name_list
            .iter()
            .map(|n| key(&text::lowercased(n)))
            .collect();
        let mut rank = HashMap::new();
        let mut ranked = Vec::new();
        for word in words {
            let k = key(word);
            if rank.contains_key(&k) || name_lowers.contains(&k) {
                continue;
            }
            rank.insert(k, ranked.len());
            ranked.push(word.clone());
        }
        let mut name_by_lower: HashMap<String, (usize, String)> = HashMap::new();
        for (i, name) in name_list.iter().enumerate() {
            name_by_lower
                .entry(key(&text::lowercased(name)))
                .or_insert((ranked.len() + i, name.clone()));
        }
        let mut letters: HashSet<String> = HashSet::new();
        let mut by_first: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        for (i, word) in ranked.iter().enumerate() {
            letters.extend(text::chars(word).into_iter().map(key));
            let f = text::first(word);
            if !f.is_empty() {
                by_first.entry(key(f)).or_default().push((i, word.clone()));
            }
        }
        for (lower, entry) in &name_by_lower {
            letters.extend(text::chars(lower).into_iter().map(key));
            let f = text::first(lower);
            if !f.is_empty() {
                by_first.entry(key(f)).or_default().push(entry.clone());
            }
        }
        for list in by_first.values_mut() {
            list.sort_by_key(|e| e.0);
        }
        Self {
            rank,
            names: name_list.iter().map(|n| key(n)).collect(),
            upper_names: name_list
                .iter()
                .map(|n| key(&text::uppercased(n)))
                .collect(),
            name_by_lower,
            alphabet: letters.into_iter().collect(),
            by_first,
            memo: RefCell::default(),
        }
    }

    pub fn is_word_valid(&self, word: &str) -> bool {
        if word.is_empty() {
            return true;
        }
        if text::count(word) == 1 && text::is_letter(word) {
            return true;
        }
        self.rank.contains_key(&key(&text::lowercased(word)))
            || self.names.contains(&key(word))
            || self.upper_names.contains(&key(word))
    }

    pub fn guesses(&self, word: &str) -> Vec<String> {
        let lower = text::lowercased(word);
        if let Some(memo) = self.memo.borrow().get(&lower) {
            return memo.clone();
        }
        let chars: Vec<String> = text::chars(&lower).into_iter().map(str::to_owned).collect();
        let n = chars.len();
        if n == 0 {
            return Vec::new();
        }
        let mut found: HashMap<usize, String> = HashMap::new();
        let mut consider = |c: &[String]| {
            let s: String = c.concat();
            if text::eq(&s, &lower) {
                return;
            }
            if let Some(&r) = self.rank.get(&key(&s)) {
                found.insert(r, s);
            } else if let Some((r, spelled)) = self.name_by_lower.get(&key(&s)) {
                found.insert(*r, spelled.clone());
            }
        };
        if n > 1 {
            for i in 0..n {
                let mut c = chars.clone();
                c.remove(i);
                consider(&c);
            }
            for i in 0..n - 1 {
                let mut c = chars.clone();
                c.swap(i, i + 1);
                consider(&c);
            }
        }
        for i in 0..n {
            for a in &self.alphabet {
                if text::eq(a, &chars[i]) {
                    continue;
                }
                let mut c = chars.clone();
                c[i] = a.clone();
                consider(&c);
            }
        }
        for i in 0..=n {
            for a in &self.alphabet {
                let mut c = chars.clone();
                c.insert(i, a.clone());
                consider(&c);
            }
        }
        let mut found: Vec<(usize, String)> = found.into_iter().collect();
        found.sort_by_key(|e| e.0);
        let mut out: Vec<String> = found.into_iter().take(6).map(|e| e.1).collect();
        let mut splits = 0;
        if n >= 3 {
            for i in 1..n {
                if splits >= 2 {
                    break;
                }
                let (left, right) = (chars[..i].concat(), chars[i..].concat());
                if !self.rank.contains_key(&key(&left)) || !self.rank.contains_key(&key(&right)) {
                    continue;
                }
                if fnv(lower.as_bytes()).is_multiple_of(2) {
                    out.push(format!("{left}-{right}"));
                }
                out.push(format!("{left} {right}"));
                splits += 1;
            }
        }
        if n == 2 {
            out.push(format!("{}.{}.", chars[0], chars[1]));
        }
        out.truncate(10);
        self.memo.borrow_mut().insert(lower, out.clone());
        out
    }

    pub fn completions(&self, prefix: &str) -> Vec<String> {
        let lower = text::lowercased(prefix);
        let f = text::first(&lower);
        let Some(list) = (!f.is_empty())
            .then(|| self.by_first.get(&key(f)))
            .flatten()
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (_, word) in list {
            if out.len() >= 10 {
                break;
            }
            let l = text::lowercased(word);
            if !text::eq(&l, &lower) && text::has_prefix(&l, &lower) {
                out.push(word.clone());
            }
        }
        out
    }
}

/// Every call, hashed in order as the Swift `ACCallLog` does.
pub struct CallLog {
    pub hash: u64,
    pub count: usize,
    pub lines: Option<Vec<String>>,
}

impl CallLog {
    pub fn new(keep: bool) -> Self {
        Self {
            hash: 0xcbf2_9ce4_8422_2325,
            count: 0,
            lines: keep.then(Vec::new),
        }
    }

    fn record(&mut self, line: String) {
        self.count += 1;
        for &b in line.as_bytes().iter().chain(b"\n") {
            self.hash ^= u64::from(b);
            self.hash = self.hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        if let Some(lines) = &mut self.lines {
            lines.push(line);
        }
    }
}

/// The dictionary a `UITextChecker` code opens.
pub fn dictionary_code(code: &str) -> String {
    let base: String = code.chars().take_while(|&c| c != '_' && c != '-').collect();
    if base == "no" { "nb".into() } else { base }
}

pub struct StubChecker {
    pub tag: String,
    pub log: Rc<RefCell<CallLog>>,
    pub dictionaries: Rc<HashMap<String, StubDictionary>>,
    pub accepts_everything: bool,
    pub code: String,
}

impl StubChecker {
    fn dictionary(&self) -> Option<&StubDictionary> {
        self.dictionaries.get(&dictionary_code(&self.code))
    }
}

impl TextChecker for StubChecker {
    fn is_word_valid(&self, word: &str) -> bool {
        let answer =
            self.accepts_everything || self.dictionary().is_some_and(|d| d.is_word_valid(word));
        self.log
            .borrow_mut()
            .record(format!("{}|v|{word}|{}", self.tag, u8::from(answer)));
        answer
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        let answer = self
            .dictionary()
            .map(|d| d.guesses(word))
            .unwrap_or_default();
        self.log
            .borrow_mut()
            .record(format!("{}|g|{word}|{}", self.tag, answer.join("\u{1F}")));
        answer
    }

    fn completions(&self, prefix: &str) -> Vec<String> {
        let answer = self
            .dictionary()
            .map(|d| d.completions(prefix))
            .unwrap_or_default();
        self.log
            .borrow_mut()
            .record(format!("{}|c|{prefix}|{}", self.tag, answer.join("\u{1F}")));
        answer
    }

    fn use_language(&mut self, code: &str) {
        self.code = code.to_owned();
        self.log
            .borrow_mut()
            .record(format!("{}|u|{code}|", self.tag));
    }
}
