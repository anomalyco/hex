//! `CleanupReconstruction` (ios/HEXShared/CleanupReconstruction.swift, itself a port of the
//! cleanup tagger's `js/reconstruct.js`): rebuilds a transcript from KEEP/DELETE labels,
//! joining the kept words and repairing the punctuation and capitals next to a deletion. It
//! works on scalars with JavaScript's semantics: a word's "first letter" is its first UTF-16
//! unit, so an astral first character is never cased.

type Word = Vec<char>;

/// Words that never keep a trailing comma when the filler after them is removed.
const NO_COMMA: &[&str] = &[
    "a", "an", "the", "to", "of", "for", "with", "at", "in", "on", "from", "by", "about", "into",
    "onto", "my", "your", "his", "her", "our", "their", "its", "i", "we", "you", "he", "she",
    "they", "can", "could", "would", "will", "should", "shall", "may", "might", "must", "do",
    "does", "did", "is", "are", "was", "were", "be", "been", "am", "have", "has", "had", "and",
    "or", "but", "if", "that", "because", "than", "just", "gonna", "wanna", "want", "need",
    "think", "thinking", "guess", "get", "got", "let's", "i'm", "we're", "you're", "they're",
    "it's", "there's", "i'll", "we'll", "you'll", "i'd", "we'd", "i've", "we've", "really", "very",
    "so", "like", "isn't", "don't", "can't", "won't", "didn't", "this", "these", "those", "some",
    "any", "every", "what", "where", "when", "how", "which", "who", "also", "go", "going", "s",
];
/// Function words a correction's repair may start with; lowercased when a deleted sentence
/// end capitalized them.
const LOWERABLE: &[&str] = &[
    "at", "on", "in", "the", "a", "an", "to", "for", "with", "from", "by", "and", "or", "but",
    "about", "around", "before", "after", "until", "into", "my", "your", "our", "their", "his",
    "her", "this", "that", "these", "those",
];
const FILLER_WORDS: &[&str] = &[
    "um", "uh", "er", "ah", "hmm", "umm", "uhh", "erm", "uhm", "mm", "like", "you", "know", "i",
    "mean",
];
const TRAILING: [char; 7] = [',', '.', '?', '!', ';', ':', '\u{2026}'];
const PAUSES: [char; 3] = [',', ';', ':'];
const SENTENCE_ENDS: [char; 3] = ['.', '?', '!'];

/// `words` are the transcript's whitespace-separated words; `deletes` has one flag per word.
pub fn reconstruct<S: AsRef<str>>(words: &[S], deletes: &[bool]) -> String {
    let mut out: Vec<Word> = Vec::new();
    let mut pending: Vec<Word> = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let mut scalars: Word = word.as_ref().chars().collect();
        if deletes.get(index).copied().unwrap_or(false) {
            pending.push(scalars);
            continue;
        }
        if !pending.is_empty() {
            scalars = after_gap(&mut out, &pending, scalars);
            pending.clear();
        }
        out.push(scalars);
    }
    if !pending.is_empty() && !out.is_empty() {
        at_end(&mut out, &pending);
    }
    out.iter()
        .map(|word| word.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

fn after_gap(out: &mut [Word], deleted: &[Word], next: Word) -> Word {
    let mut b = next;
    let Some(last) = out.last() else {
        return capitalized(&b);
    };
    let mut a = strip_ellipsis(last);
    let term = terminal(&deleted[deleted.len() - 1]);
    if a.last() == Some(&',') && contains(NO_COMMA, &bare(&a)) {
        a.pop();
    }
    if !term.is_empty() && !ends_sentence(&a) && is_upper(first_unit(&b)) {
        if contains(LOWERABLE, &bare(&b)) {
            b = lowercased_first(&b);
        } else if deleted
            .iter()
            .all(|word| contains(FILLER_WORDS, &bare(word)))
        {
            a = trimming(&a, &PAUSES);
            a.extend(&term);
        }
    }
    let ends = a.is_empty() || ends_sentence(&a);
    let count = out.len();
    out[count - 1] = a;
    if ends {
        b = capitalized(&b);
    }
    b
}

fn at_end(out: &mut [Word], deleted: &[Word]) {
    let count = out.len();
    let mut a = strip_ellipsis(&out[count - 1]);
    if !ends_sentence(&a) {
        a = trimming(&a, &PAUSES);
        a.extend(terminal(&deleted[deleted.len() - 1]));
    }
    out[count - 1] = a;
}

/// `t[0]`: the first UTF-16 unit; an astral first character is half a surrogate pair.
fn first_unit(t: &[char]) -> Option<char> {
    t.first().copied().filter(|&c| (c as u32) <= 0xFFFF)
}

fn maps_to_itself(mut mapping: impl Iterator<Item = char>, c: char) -> bool {
    mapping.next() == Some(c) && mapping.next().is_none()
}

/// `ch === ch.toUpperCase() && ch !== ch.toLowerCase()`
fn is_upper(c: Option<char>) -> bool {
    c.is_some_and(|c| maps_to_itself(c.to_uppercase(), c) && !maps_to_itself(c.to_lowercase(), c))
}

/// `ch === ch.toLowerCase() && ch !== ch.toUpperCase()`
fn is_lower(c: Option<char>) -> bool {
    c.is_some_and(|c| maps_to_itself(c.to_lowercase(), c) && !maps_to_itself(c.to_uppercase(), c))
}

fn has_ellipsis(t: &[char]) -> bool {
    t.len() >= 3 && t[t.len() - 3..] == ['.', '.', '.']
}

fn ends_sentence(t: &[char]) -> bool {
    t.last().is_some_and(|c| SENTENCE_ENDS.contains(c)) && !has_ellipsis(t)
}

/// The sentence-final mark a deleted word carries, ignoring ellipses.
fn terminal(t: &[char]) -> Word {
    if ends_sentence(t) {
        vec![t[t.len() - 1]]
    } else {
        Vec::new()
    }
}

/// `t.replace(/[,.?!;:…]+$/, "").toLowerCase()`
fn bare(t: &[char]) -> String {
    let mut end = t.len();
    while end > 0 && TRAILING.contains(&t[end - 1]) {
        end -= 1;
    }
    t[..end].iter().flat_map(|c| c.to_lowercase()).collect()
}

/// Every listed word is ASCII, so byte equality is JavaScript's `Set.has`.
fn contains(set: &[&str], word: &str) -> bool {
    word.is_ascii() && set.contains(&word)
}

fn capitalized(t: &[char]) -> Word {
    match first_unit(t) {
        Some(first) if is_lower(Some(first)) && !t[1..].iter().any(|&c| is_upper(Some(c))) => {
            first.to_uppercase().chain(t[1..].iter().copied()).collect()
        }
        _ => t.to_vec(),
    }
}

fn lowercased_first(t: &[char]) -> Word {
    match first_unit(t) {
        Some(first) => first.to_lowercase().chain(t[1..].iter().copied()).collect(),
        None => t.to_vec(),
    }
}

fn strip_ellipsis(t: &[char]) -> Word {
    if has_ellipsis(t) {
        return t[..t.len() - 3].to_vec();
    }
    if t.last() == Some(&'\u{2026}') {
        return t[..t.len() - 1].to_vec();
    }
    t.to_vec()
}

fn trimming(t: &[char], set: &[char]) -> Word {
    let mut end = t.len();
    while end > 0 && set.contains(&t[end - 1]) {
        end -= 1;
    }
    t[..end].to_vec()
}
