//! `SpokenLists`' words per language (SpokenLists.swift, `Table`), as written there: they're
//! compared with the folded words of a transcript.

use super::Language;
use super::lists::small_number;
use super::text as t;

pub(crate) struct Table {
    /// Phrases that break a line.
    pub(super) breaks: &'static [(&'static [&'static str], &'static str)],
    /// Words before a phrase that make it a noun, not a command: "a new line", "the bullet".
    pub(super) determiners: &'static [&'static str],
    /// Words after "new line" that make it a noun: "new line of shoes".
    pub(super) break_followers: &'static [&'static str],
    pub(super) connectors: &'static [&'static str],
    /// "or", stripped from an item's end like a connector.
    pub(super) alternatives: &'static [&'static str],
    ordinals: &'static [(&'static str, usize)],
    /// Ordinal suffixes written after digits: "1st", "2nd".
    digit_ordinal_suffixes: &'static [&'static str],
    pub(super) ordinal_phrases: &'static [(&'static [&'static str], usize)],
    /// Words right after an ordinal that keep it prose: "first time", "second guess".
    pub(super) ordinal_followers: &'static [&'static str],
    /// "first of all".
    pub(super) of_all: &'static [&'static str],
    /// "finally", "lastly"; the flag asks for a comma after ("last, …").
    pub(super) finals: &'static [(&'static [&'static str], bool)],
    pub(super) cardinals: &'static [(&'static str, usize)],
    /// "number one", "step one".
    pub(super) keywords: &'static [&'static str],
    /// Bullet phrases, longest first; the flag asks for a clause start.
    pub(super) bullets: &'static [(&'static [&'static str], bool)],
    /// Words right after "bullet" that keep it prose: "bullet points", "bullet train".
    pub(super) bullet_followers: &'static [&'static str],
}

/// `set.contains(word)`, as Swift compares strings.
pub(super) fn has(set: &[&str], word: &str) -> bool {
    set.iter().any(|entry| t::same(entry, word))
}

pub(super) fn find(set: &[&'static str], word: &str) -> Option<&'static str> {
    set.iter().find(|entry| t::same(entry, word)).copied()
}

pub(super) fn lookup(map: &[(&str, usize)], word: &str) -> Option<usize> {
    map.iter()
        .find(|(key, _)| t::same(key, word))
        .map(|&(_, value)| value)
}

impl Table {
    pub(super) fn ordinal(&self, word: &str) -> Option<usize> {
        if let Some(value) = lookup(self.ordinals, word) {
            return Some(value);
        }
        for suffix in self.digit_ordinal_suffixes {
            if t::has_suffix(word, suffix) {
                let chars = t::chars(word);
                let digits = t::join(&chars[..chars.len() - t::count(suffix)]);
                if !digits.is_empty()
                    && let Some(value) = small_number(&digits)
                {
                    return Some(value);
                }
            }
        }
        None
    }

    pub(super) fn for_language(language: Option<&Language>) -> Option<&'static Table> {
        match super::code_or_english(language) {
            "en" => Some(&ENGLISH),
            "es" => Some(&SPANISH),
            _ => None,
        }
    }
}

static ENGLISH: Table = Table {
    breaks: &[
        (&["new", "paragraph"], "\n\n"),
        (&["new", "line"], "\n"),
        (&["line", "break"], "\n"),
    ],
    determiners: &[
        "a", "an", "the", "this", "that", "these", "those", "my", "your", "his", "her", "its",
        "our", "their", "one", "another", "every", "each", "any", "some", "no", "to", "of", "in",
        "on", "with", "for", "at", "by", "from", "silver", "magic", "rubber", "plastic", "stray",
        "whole", "entire",
    ],
    break_followers: &[
        "of", "items", "item", "is", "was", "are", "were", "has", "had", "will", "would", "that",
        "which", "for", "from", "cook", "cooks", "worker", "workers", "manager", "managers", "up",
        "breaks",
    ],
    connectors: &["and", "then", "also", "plus"],
    alternatives: &["or"],
    ordinals: &[
        ("first", 1),
        ("firstly", 1),
        ("second", 2),
        ("secondly", 2),
        ("third", 3),
        ("thirdly", 3),
        ("fourth", 4),
        ("fourthly", 4),
        ("fifth", 5),
        ("fifthly", 5),
        ("sixth", 6),
        ("seventh", 7),
        ("eighth", 8),
        ("ninth", 9),
        ("tenth", 10),
    ],
    digit_ordinal_suffixes: &["st", "nd", "rd", "th"],
    ordinal_phrases: &[],
    ordinal_followers: &[
        "time",
        "times",
        "place",
        "thing",
        "things",
        "one",
        "ones",
        "day",
        "days",
        "week",
        "weeks",
        "month",
        "months",
        "year",
        "years",
        "half",
        "class",
        "person",
        "people",
        "hand",
        "floor",
        "grade",
        "round",
        "prize",
        "guess",
        "guessed",
        "guessing",
        "nature",
        "rate",
        "aid",
        "base",
        "cousin",
        "cousins",
        "date",
        "name",
        "impression",
        "choice",
        "language",
        "lady",
        "world",
        "edition",
        "gear",
        "love",
        "sight",
        "glance",
        "mate",
        "chance",
        "try",
        "attempt",
        "look",
        "night",
        "kiss",
        "draft",
        "opinion",
        "wave",
        "quarter",
        "season",
        "episode",
        "chapter",
        "page",
        "row",
        "team",
        "degree",
        "letter",
        "word",
        "words",
        "course",
        "act",
        "part",
        "period",
        "hour",
        "minute",
        "thought",
        "thoughts",
        "off",
        "up",
        "to",
        "in",
        "on",
        "at",
        "and",
        "or",
        "is",
        "was",
        "came",
        "come",
        "served",
        "last",
        "of",
        "string",
        "fiddle",
        "semester",
        "term",
        "born",
        "child",
        "son",
        "daughter",
        "baby",
        "wife",
        "husband",
        "job",
        "line",
        "generation",
        "step",
        "steps",
        "officer",
        "lieutenant",
        "amendment",
        "street",
        "avenue",
        "best",
        "most",
        "largest",
        "biggest",
    ],
    of_all: &["of", "all"],
    finals: &[
        (&["last", "but", "not", "least"], false),
        (&["finally"], false),
        (&["lastly"], false),
        (&["last"], true),
    ],
    cardinals: &[
        ("one", 1),
        ("two", 2),
        ("three", 3),
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
        ("ten", 10),
    ],
    keywords: &["number", "step", "item", "point"],
    bullets: &[
        (&["bullet", "point"], false),
        (&["next", "bullet"], false),
        (&["next", "item"], false),
        (&["bullet"], false),
        (&["dash"], true),
    ],
    bullet_followers: &[
        "points", "proof", "proofs", "train", "trains", "wound", "wounds", "hole", "holes",
        "journal", "journals", "casing", "casings", "shell", "shells", "ant", "ants", "is", "was",
        "hit", "hits", "went", "flew", "of", "in", "list", "lists",
    ],
};

static SPANISH: Table = Table {
    breaks: &[
        (&["nuevo", "parrafo"], "\n\n"),
        (&["nueva", "linea"], "\n"),
        (&["salto", "de", "linea"], "\n"),
    ],
    determiners: &[
        "un", "una", "el", "la", "los", "las", "este", "esta", "estos", "estas", "ese", "esa",
        "eso", "mi", "mis", "tu", "tus", "su", "sus", "nuestra", "nuestro", "otra", "otro", "cada",
        "de", "del", "con", "en", "para", "por", "al", "a",
    ],
    break_followers: &["de", "del", "es", "fue", "que", "para", "y"],
    connectors: &["y", "e", "luego", "despues", "tambien"],
    alternatives: &["o", "u"],
    ordinals: &[
        ("primero", 1),
        ("primera", 1),
        ("primeramente", 1),
        ("segundo", 2),
        ("segunda", 2),
        ("tercero", 3),
        ("tercera", 3),
        ("cuarto", 4),
        ("cuarta", 4),
        ("quinto", 5),
        ("quinta", 5),
        ("sexto", 6),
        ("sexta", 6),
        ("septimo", 7),
        ("septima", 7),
        ("octavo", 8),
        ("octava", 8),
        ("noveno", 9),
        ("novena", 9),
        ("decimo", 10),
        ("decima", 10),
    ],
    digit_ordinal_suffixes: &["º", "ª"],
    ordinal_phrases: &[
        (&["en", "primer", "lugar"], 1),
        (&["en", "segundo", "lugar"], 2),
        (&["en", "tercer", "lugar"], 3),
        (&["en", "cuarto", "lugar"], 4),
        (&["en", "quinto", "lugar"], 5),
    ],
    ordinal_followers: &[
        "vez",
        "veces",
        "lugar",
        "dia",
        "dias",
        "plato",
        "piso",
        "mano",
        "grado",
        "premio",
        "parte",
        "nombre",
        "ano",
        "anos",
        "mes",
        "semana",
        "momento",
        "paso",
        "pasos",
        "hijo",
        "hija",
        "ministro",
        "ministra",
        "actor",
        "plano",
        "mundo",
        "tiempo",
        "ciclo",
        "curso",
        "nivel",
        "puesto",
        "de",
        "del",
        "en",
        "que",
        "es",
        "fue",
        "y",
        "o",
        "a",
        "con",
        "amor",
        "intento",
        "capitulo",
        "temporada",
        "clase",
        "fila",
    ],
    of_all: &["de", "todo"],
    finals: &[
        (&["en", "ultimo", "lugar"], false),
        (&["por", "ultimo"], false),
        (&["finalmente"], false),
    ],
    cardinals: &[
        ("uno", 1),
        ("una", 1),
        ("dos", 2),
        ("tres", 3),
        ("cuatro", 4),
        ("cinco", 5),
        ("seis", 6),
        ("siete", 7),
        ("ocho", 8),
        ("nueve", 9),
        ("diez", 10),
    ],
    keywords: &["numero", "paso"],
    bullets: &[
        (&["siguiente", "vineta"], false),
        (&["siguiente", "elemento"], false),
        (&["vineta"], false),
        (&["guion"], true),
    ],
    bullet_followers: &["de", "del", "es", "fue", "que"],
};
