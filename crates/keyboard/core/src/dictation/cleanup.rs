//! `SpokenCleanupRules.clean` (ios/HEXShared/SpokenCleanupRules.swift): Clean up what I say
//! for Spanish, German and Norwegian, which the English cleanup tagger was never trained on.
//! Hesitation sounds go, a filler phrase set off by commas goes, and a spoken correction ("a
//! las 3, digo, a las 4") drops its marker and the words it replaces. Like the tagger, it
//! only removes words; `reconstruct` repairs the commas and capitals next to a removal.

use super::formatter::filler_pattern;
use super::reconstruct::reconstruct;
use super::{Language, regex, text as t, words};

/// How far back a correction looks for the words it replaces.
pub const MAX_REACH: usize = 8;

struct Rules {
    /// Removed between commas, or at a sentence start before a comma.
    fillers: &'static [&'static [&'static str]],
    /// Start a correction when set off by commas, or by a sentence end and a comma.
    markers: &'static [&'static [&'static str]],
    /// Markers clear enough to count without commas ("3 mejor dicho 4").
    plain_markers: &'static [&'static [&'static str]],
    /// Words of one kind can replace each other ("martes, perdón, miércoles").
    kinds: &'static [&'static [&'static str]],
    /// Words that lose their comma when the filler after them goes ("Y, este, fuimos").
    no_comma: &'static [&'static str],
}

impl Rules {
    /// The index in `kinds` of a bare word, with any word holding a digit a number.
    fn kind(&self, word: &str) -> Option<usize> {
        if t::chars(word).iter().any(|c| t::is_number(c)) {
            return Some(self.kinds.len());
        }
        self.kinds
            .iter()
            .position(|kind| kind.iter().any(|k| t::same(k, word)))
    }
}

fn rules(language: Option<&Language>) -> Option<&'static Rules> {
    match language.and_then(|l| l.code.as_deref()) {
        Some("es") => Some(&SPANISH),
        Some("de") => Some(&GERMAN),
        Some("nb" | "nn" | "no") => Some(&NORWEGIAN),
        _ => None,
    }
}

/// `SpokenCleanupRules.supports`.
pub fn supports(language: Option<&Language>) -> bool {
    rules(language).is_some()
}

/// `text` with its fillers and spoken corrections removed, or `text` itself when nothing
/// goes or the language has no rules.
pub fn clean(text: &str, language: Option<&Language>) -> String {
    let Some(rules) = rules(language) else {
        return text.to_owned();
    };
    let words = words(text);
    let mut cleaned = removing_fillers(&words, rules, language);
    cleaned = resolving_corrections(cleaned, rules);
    let same =
        cleaned.len() == words.len() && cleaned.iter().zip(&words).all(|(a, b)| t::same(a, b));
    if same {
        text.to_owned()
    } else {
        cleaned.join(" ")
    }
}

fn removing_fillers(input: &[String], rules: &Rules, language: Option<&Language>) -> Vec<String> {
    let mut words = input.to_vec();
    let mut deletes = vec![false; words.len()];
    let hesitation = filler_pattern(language);
    let mut index = 0;
    while index < words.len() {
        if let Some(pattern) = hesitation
            && is_hesitation(&words[index], pattern)
        {
            deletes[index] = true;
            index += 1;
            continue;
        }
        let Some(phrase) = rules
            .fillers
            .iter()
            .find(|phrase| matches(phrase, &words, index))
        else {
            index += 1;
            continue;
        };
        let end = index + phrase.len() - 1;
        let opens = index == 0 || is_pause(&words[index - 1]) || ends_sentence(&words[index - 1]);
        if !opens
            || !t::has_suffix(&words[end], ",")
            || end + 1 >= words.len()
            || has_opening_mark(&words[index])
        {
            index += 1;
            continue;
        }
        if index > 0
            && t::has_suffix(&words[index - 1], ",")
            && rules
                .no_comma
                .iter()
                .any(|w| t::same(w, &bare(&words[index - 1])))
        {
            t::remove_last(&mut words[index - 1]);
        }
        for delete in &mut deletes[index..=end] {
            *delete = true;
        }
        index = end + 1;
    }
    if !deletes.contains(&true) {
        return input.to_vec();
    }
    super::words(&reconstruct(&words, &deletes))
}

/// A sound like "eh", unless it asks or exclaims ("¿eh?", "¡Eh, tú!").
fn is_hesitation(word: &str, pattern: &str) -> bool {
    if has_opening_mark(word) {
        return false;
    }
    match t::last(word) {
        Some(last) if !t::is(last, '?') && !t::is(last, '!') => {
            regex::is_filler_word(&bare(word), pattern)
        }
        _ => false,
    }
}

struct Correction {
    /// The first replaced word, the marker's last word, and the repair's first word.
    start: usize,
    marker_end: usize,
    repair: usize,
}

fn resolving_corrections(input: Vec<String>, rules: &Rules) -> Vec<String> {
    let mut words = input;
    let mut from = 0;
    // Each correction removes words, so this ends; the cap only guards against a mistake.
    for _ in 0..words.len() {
        let Some(found) = next_correction(&words, from, rules) else {
            break;
        };
        words[found.repair] = taking(&words[found.start], &words[found.repair]);
        let mut deletes = vec![false; words.len()];
        for delete in &mut deletes[found.start..=found.marker_end] {
            *delete = true;
        }
        words = super::words(&reconstruct(&words, &deletes));
        from = found.start + 1;
    }
    words
}

fn next_correction(words: &[String], from: usize, rules: &Rules) -> Option<Correction> {
    if from >= words.len() {
        return None;
    }
    for marker_start in from.max(1)..words.len() {
        for marker in rules
            .markers
            .iter()
            .filter(|marker| matches(marker, words, marker_start))
        {
            let marker_end = marker_start + marker.len() - 1;
            let repair = marker_end + 1;
            if repair >= words.len() || has_opening_mark(&words[marker_start]) {
                continue;
            }
            let before = &words[marker_start - 1];
            let set_off = (is_pause(before) || ends_sentence(before))
                && t::has_suffix(&words[marker_end], ",");
            let plain = rules.plain_markers.contains(marker)
                && !ends_sentence(before)
                && !is_pause(&words[marker_end])
                && !ends_sentence(&words[marker_end]);
            if !set_off && !plain {
                continue;
            }
            let first = bare(&words[repair]);
            if first.is_empty() {
                continue;
            }
            // The sentence the replaced words are in, at most `MAX_REACH` words back.
            let mut low = marker_start - 1;
            while low > 0 && !ends_sentence(&words[low - 1]) && marker_start - low < MAX_REACH {
                low -= 1;
            }
            let candidates = || (low..marker_start).rev();
            let mut start = candidates().find(|&c| t::same(&bare(&words[c]), &first));
            if start.is_none()
                && let Some(kind) = rules.kind(&first)
            {
                start = candidates().find(|&c| rules.kind(&bare(&words[c])) == Some(kind));
            }
            let Some(start) = start else { continue };
            return Some(Correction {
                start,
                marker_end,
                repair,
            });
        }
    }
    None
}

/// The repair takes the place of the words it corrects: its first word takes the first
/// replaced word's capital and opening mark.
fn taking(replaced: &str, repair: &str) -> String {
    let replaced_chars = t::chars(replaced);
    let marks_count = replaced_chars
        .iter()
        .take_while(|c| t::is_one_of(c, OPENING_MARKS))
        .count();
    let marks = t::join(&replaced_chars[..marks_count]);
    let mut result = repair.to_owned();
    if !marks.is_empty() && !has_opening_mark(&result) {
        result = marks + &result;
    }
    let Some(source) = replaced_chars.get(marks_count) else {
        return result;
    };
    let mut chars = t::owned_chars(&result);
    let Some(index) = chars.iter().position(|c| t::is_letter(c)) else {
        return result;
    };
    if !t::is_letter(source) || chars[index + 1..].iter().any(|c| t::is_uppercase(c)) {
        return result;
    }
    chars[index] = if t::is_uppercase(source) {
        t::uppercased(&chars[index])
    } else {
        t::lowercased(&chars[index])
    };
    t::join(&chars)
}

pub(super) const OPENING_MARKS: &str = "¿¡\"“«(„";
pub(super) const CLOSING_MARKS: &str = ",.;:!?…\"”»)";

/// Lowercased, without opening marks or trailing punctuation.
pub fn bare(word: &str) -> String {
    let chars = t::chars(word);
    let start = chars
        .iter()
        .take_while(|c| t::is_one_of(c, OPENING_MARKS))
        .count();
    let mut end = chars.len();
    while end > start && t::is_one_of(chars[end - 1], CLOSING_MARKS) {
        end -= 1;
    }
    t::lowercased(&t::join(&chars[start..end]))
}

fn matches(phrase: &[&str], words: &[String], index: usize) -> bool {
    if index + phrase.len() > words.len() {
        return false;
    }
    for (offset, part) in phrase.iter().enumerate() {
        let word = &words[index + offset];
        // Inside a phrase only a comma may follow a word ("no, espera,").
        if offset < phrase.len() - 1
            && let Some(last) = t::last(word)
            && !t::is(last, ',')
            && t::is_one_of(last, CLOSING_MARKS)
        {
            return false;
        }
        if !t::same(&bare(word), part) {
            return false;
        }
    }
    true
}

fn is_pause(word: &str) -> bool {
    t::last(word).is_some_and(|c| t::is_one_of(c, ",;:"))
}

pub(super) fn ends_sentence(word: &str) -> bool {
    let mut chars = t::chars(word);
    while chars.last().is_some_and(|c| t::is_one_of(c, "\"”»)")) {
        chars.pop();
    }
    chars.last().is_some_and(|c| t::is_one_of(c, ".!?…"))
}

fn has_opening_mark(word: &str) -> bool {
    t::first(word).is_some_and(|c| t::is_one_of(c, OPENING_MARKS))
}

static SPANISH: Rules = Rules {
    fillers: &[&["este"], &["o", "sea"]],
    markers: &[
        &["digo"],
        &["perdón"],
        &["perdon"],
        &["no"],
        &["no", "espera"],
        &["no", "mejor"],
        &["mejor", "dicho"],
    ],
    plain_markers: &[&["mejor", "dicho"], &["no", "mejor"]],
    kinds: &[
        &[
            "dos", "tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve", "diez", "once",
            "doce",
        ],
        &[
            "lunes",
            "martes",
            "miércoles",
            "jueves",
            "viernes",
            "sábado",
            "domingo",
        ],
        &[
            "enero",
            "febrero",
            "marzo",
            "abril",
            "mayo",
            "junio",
            "julio",
            "agosto",
            "septiembre",
            "setiembre",
            "octubre",
            "noviembre",
            "diciembre",
        ],
        &["hoy", "mañana", "ayer", "anoche"],
    ],
    no_comma: &[
        "y", "o", "pero", "que", "a", "de", "en", "con", "para", "por", "el", "la", "los", "las",
        "un", "una", "mi", "tu", "su", "porque", "si",
    ],
};

static GERMAN: Rules = Rules {
    fillers: &[],
    markers: &[
        &["nein"],
        &["nein", "warte"],
        &["ich", "meine"],
        &["besser", "gesagt"],
        &["sorry"],
        &["entschuldigung"],
    ],
    plain_markers: &[&["besser", "gesagt"]],
    kinds: &[
        &[
            "zwei", "drei", "vier", "fünf", "sechs", "sieben", "acht", "neun", "zehn", "elf",
            "zwölf",
        ],
        &[
            "montag",
            "dienstag",
            "mittwoch",
            "donnerstag",
            "freitag",
            "samstag",
            "sonntag",
        ],
        &[
            "januar",
            "jänner",
            "februar",
            "märz",
            "april",
            "mai",
            "juni",
            "juli",
            "august",
            "september",
            "oktober",
            "november",
            "dezember",
        ],
        &["heute", "morgen", "gestern", "übermorgen"],
    ],
    no_comma: &[
        "und", "oder", "aber", "dass", "der", "die", "das", "ein", "eine", "mit", "zu", "in", "an",
        "auf", "für", "von", "ich", "du", "er", "sie", "wir",
    ],
};

static NORWEGIAN: Rules = Rules {
    fillers: &[],
    markers: &[
        &["nei"],
        &["nei", "vent"],
        &["jeg", "mener"],
        &["rettere", "sagt"],
        &["unnskyld"],
        &["sorry"],
    ],
    plain_markers: &[&["rettere", "sagt"]],
    kinds: &[
        &[
            "to", "tre", "fire", "fem", "seks", "sju", "syv", "åtte", "ni", "ti", "elleve", "tolv",
        ],
        &[
            "mandag", "tirsdag", "onsdag", "torsdag", "fredag", "lørdag", "søndag",
        ],
        &[
            "januar",
            "februar",
            "mars",
            "april",
            "mai",
            "juni",
            "juli",
            "august",
            "september",
            "oktober",
            "november",
            "desember",
        ],
    ],
    no_comma: &[
        "og", "eller", "men", "at", "jeg", "du", "han", "hun", "vi", "en", "et", "den", "det",
        "på", "i", "til", "med", "for", "som",
    ],
};
