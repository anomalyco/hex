//! Spoken numbers, amounts, times, units, emails, links, and code, written
//! the way they are read: "two hundred fifty milliseconds" becomes "250
//! milliseconds", "priya at example dot com" becomes "priya@example.com".
//!
//! Only spans the model tagged are formatted, and each grammar is
//! deterministic and closed: a span it does not recognise gives `None`, and
//! the caller keeps the words as heard. A formatter only rewrites the words
//! it is given into digits and symbols; it never adds a word.
//!
//! A line-for-line port of the tagger's Python reference (`itn.py`),
//! checked against it by `parity_tests`. Python's integers are unbounded:
//! here a number past `u128` is not formatted.

use std::sync::LazyLock;

use regex::Regex;

/// What a span is, by its tag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Number,
    Unit,
    Email,
    Url,
    Code,
}

const ONES: &[(&str, u128)] = &[
    ("zero", 0),
    ("oh", 0),
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
    ("eleven", 11),
    ("twelve", 12),
    ("thirteen", 13),
    ("fourteen", 14),
    ("fifteen", 15),
    ("sixteen", 16),
    ("seventeen", 17),
    ("eighteen", 18),
    ("nineteen", 19),
];

const TENS: &[(&str, u128)] = &[
    ("twenty", 20),
    ("thirty", 30),
    ("forty", 40),
    ("fifty", 50),
    ("sixty", 60),
    ("seventy", 70),
    ("eighty", 80),
    ("ninety", 90),
];

const SCALES: &[(&str, u128)] = &[
    ("thousand", 1_000),
    ("million", 1_000_000),
    ("billion", 1_000_000_000),
];

const ORDINALS: &[(&str, u128, &str)] = &[
    ("first", 1, "st"),
    ("second", 2, "nd"),
    ("third", 3, "rd"),
    ("fourth", 4, "th"),
    ("fifth", 5, "th"),
    ("sixth", 6, "th"),
    ("seventh", 7, "th"),
    ("eighth", 8, "th"),
    ("ninth", 9, "th"),
    ("tenth", 10, "th"),
    ("eleventh", 11, "th"),
    ("twelfth", 12, "th"),
    ("thirteenth", 13, "th"),
    ("fourteenth", 14, "th"),
    ("fifteenth", 15, "th"),
    ("sixteenth", 16, "th"),
    ("seventeenth", 17, "th"),
    ("eighteenth", 18, "th"),
    ("nineteenth", 19, "th"),
    ("twentieth", 20, "th"),
    ("thirtieth", 30, "th"),
    ("fortieth", 40, "th"),
    ("fiftieth", 50, "th"),
    ("sixtieth", 60, "th"),
    ("seventieth", 70, "th"),
    ("eightieth", 80, "th"),
    ("ninetieth", 90, "th"),
];

const CURRENCIES: &[(&str, &str)] = &[
    ("dollar", "$"),
    ("dollars", "$"),
    ("buck", "$"),
    ("bucks", "$"),
    ("euro", "\u{20ac}"),
    ("euros", "\u{20ac}"),
    ("pound", "\u{a3}"),
    ("pounds", "\u{a3}"),
];

/// A spoken unit and how it is written.
const UNITS: &[(&str, &str)] = &[
    ("millisecond", "millisecond"),
    ("milliseconds", "milliseconds"),
    ("ms", "ms"),
    ("second", "second"),
    ("seconds", "seconds"),
    ("sec", "s"),
    ("secs", "s"),
    ("minute", "minute"),
    ("minutes", "minutes"),
    ("min", "min"),
    ("mins", "min"),
    ("hour", "hour"),
    ("hours", "hours"),
    ("hr", "hr"),
    ("hrs", "hrs"),
    ("kilogram", "kg"),
    ("kilograms", "kg"),
    ("kilo", "kg"),
    ("kilos", "kg"),
    ("gram", "g"),
    ("grams", "g"),
    ("milligram", "milligram"),
    ("milligrams", "milligrams"),
    ("pound", "lb"),
    ("pounds", "lb"),
    ("ounce", "oz"),
    ("ounces", "oz"),
    ("kilometer", "km"),
    ("kilometers", "km"),
    ("kilometre", "km"),
    ("kilometres", "km"),
    ("meter", "m"),
    ("meters", "m"),
    ("metre", "m"),
    ("metres", "m"),
    ("centimeter", "cm"),
    ("centimeters", "cm"),
    ("millimeter", "mm"),
    ("millimeters", "mm"),
    ("mile", "mi"),
    ("miles", "mi"),
    ("foot", "ft"),
    ("feet", "ft"),
    ("inch", "in"),
    ("inches", "in"),
    ("byte", "B"),
    ("bytes", "B"),
    ("kilobyte", "KB"),
    ("kilobytes", "KB"),
    ("megabyte", "MB"),
    ("megabytes", "MB"),
    ("meg", "MB"),
    ("megs", "MB"),
    ("gigabyte", "GB"),
    ("gigabytes", "GB"),
    ("gig", "GB"),
    ("gigs", "GB"),
    ("terabyte", "TB"),
    ("terabytes", "TB"),
    ("hertz", "Hz"),
    ("kilohertz", "kHz"),
    ("megahertz", "MHz"),
    ("gigahertz", "GHz"),
    ("watt", "W"),
    ("watts", "W"),
    ("kilowatt", "kW"),
    ("kilowatts", "kW"),
    ("volt", "V"),
    ("volts", "V"),
    ("amp", "A"),
    ("amps", "A"),
    ("pixel", "px"),
    ("pixels", "px"),
    ("mph", "mph"),
];

const SYMBOLS: &[(&str, &str)] = &[
    ("at", "@"),
    ("dot", "."),
    ("period", "."),
    ("point", "."),
    ("slash", "/"),
    ("backslash", "\\"),
    ("colon", ":"),
    ("dash", "-"),
    ("hyphen", "-"),
    ("minus", "-"),
    ("underscore", "_"),
    ("plus", "+"),
    ("hash", "#"),
    ("pound", "#"),
    ("question", "?"),
    ("equals", "="),
    ("ampersand", "&"),
    ("tilde", "~"),
];

const WEB_TLDS: &[&str] = &[
    "com", "org", "net", "io", "dev", "ai", "co", "app", "edu", "gov", "ly", "gg", "xyz", "me",
    "uk", "ca", "de", "fr", "jp", "sh", "so", "to",
];

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a fixed pattern compiles")
}

static INTEGER: LazyLock<Regex> = LazyLock::new(|| regex(r"^(?:\d{1,3}(?:,\d{3})+|\d+)$"));
static DIGITS: LazyLock<Regex> = LazyLock::new(|| regex(r"^\d+$"));
static DIGIT: LazyLock<Regex> = LazyLock::new(|| regex(r"^\d$"));
static DECIMAL: LazyLock<Regex> = LazyLock::new(|| regex(r"^(?:\d{1,3}(?:,\d{3})+|\d+)\.\d+$"));
static WRITTEN: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"^(?:[$\x{20ac}\x{a3}]?\d[\d,]*(?:\.\d+)?(?:%|k|st|nd|rd|th)?|\d{1,2}:\d{2}(?:am|pm)?)$")
});
static HOST_TLD: LazyLock<Regex> = LazyLock::new(|| regex(r"[a-z0-9]\.([a-z]{2,})$"));
static CODE_MARK: LazyLock<Regex> = LazyLock::new(|| regex(r"[_/\-.:@#=~+]"));
static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| regex(r"^[a-z][a-z0-9]*$"));
static LEAD: LazyLock<Regex> =
    LazyLock::new(|| regex(r#"^[(\[{"\x{201c}\x{2018}\x{2014}\x{2013}]+"#));
static TRAIL: LazyLock<Regex> = LazyLock::new(|| regex(r#"[,.?!:;)\]}\x{201d}\x{2019}"]+$"#));

fn lookup<T: Copy>(table: &[(&str, T)], word: &str) -> Option<T> {
    table
        .iter()
        .find(|(said, _)| *said == word)
        .map(|&(_, value)| value)
}

fn ones(word: &str) -> Option<u128> {
    lookup(ONES, word)
}

/// A digit word: zero through nine, and "oh".
pub(super) fn is_digit_word(word: &str) -> bool {
    ones(word).is_some_and(|value| value < 10)
}

fn digit_word(word: &str) -> Option<u128> {
    ones(word).filter(|&value| value < 10)
}

pub(super) fn is_ones(word: &str) -> bool {
    ones(word).is_some()
}

pub(super) fn is_tens(word: &str) -> bool {
    lookup(TENS, word).is_some()
}

fn symbol(word: &str) -> Option<&'static str> {
    lookup(SYMBOLS, word)
}

/// A word as the grammars read it: quotes and brackets off both ends,
/// trailing punctuation off, lowercased.
/// Whether a word names a unit or a currency ("meters", "dollars").
pub(super) fn is_amount_word(word: &str) -> bool {
    UNITS.iter().any(|(spoken, _)| *spoken == word)
        || CURRENCIES.iter().any(|(spoken, _)| *spoken == word)
}

pub(super) fn norm(token: &str) -> String {
    token
        .trim_matches(|ch| "\"'()[]{}\u{201c}\u{201d}\u{2018}\u{2019}`".contains(ch))
        .trim_end_matches(|ch| ",.?!;:".contains(ch))
        .to_lowercase()
}

fn norms(tokens: &[&str]) -> Vec<String> {
    tokens
        .iter()
        .map(|token| norm(token))
        .filter(|norm| !norm.is_empty())
        .collect()
}

fn parse_digits(text: &str) -> Option<u128> {
    text.replace(',', "").parse().ok()
}

/// `1234567` as `1,234,567`.
fn grouped(value: u128) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Two or more spoken single digits: "eight eight four one".
fn is_digit_string(words: &[String]) -> bool {
    words.len() >= 2
        && words
            .iter()
            .all(|word| is_digit_word(word) || DIGIT.is_match(word))
}

fn digit_string(words: &[String]) -> String {
    words
        .iter()
        .map(|word| digit_word(word).map_or_else(|| word.clone(), |value| value.to_string()))
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Last {
    One,
    Teen,
    Ten,
    Hundred,
    Scale,
    Num,
}

/// English cardinal words, round hundreds included ("twenty four hundred"
/// is 2400). Spoken single digits are a digit string, not a number.
fn int_words(words: &[String]) -> Option<u128> {
    if words.is_empty() {
        return None;
    }
    if words.len() == 1 && INTEGER.is_match(&words[0]) {
        return parse_digits(&words[0]);
    }
    if is_digit_string(words) {
        return None;
    }
    let (mut total, mut current) = (0_u128, 0_u128);
    let mut last: Option<Last> = None;
    let mut any = false;
    for word in words {
        let word = word.as_str();
        if word == "and" {
            if !any || !matches!(last, Some(Last::Hundred | Last::Scale)) {
                return None;
            }
            continue;
        }
        if DIGITS.is_match(word) {
            if last.is_some() {
                return None;
            }
            current = word.parse().ok()?;
            last = Some(Last::Num);
        } else if let Some(value) = ones(word) {
            if word == "oh" || matches!(last, Some(Last::One | Last::Teen | Last::Num)) {
                return None;
            }
            if last == Some(Last::Ten) && value >= 10 {
                return None;
            }
            current = current.checked_add(value)?;
            last = Some(if value >= 10 { Last::Teen } else { Last::One });
        } else if let Some(value) = lookup(TENS, word) {
            if matches!(last, Some(Last::One | Last::Teen | Last::Ten | Last::Num)) {
                return None;
            }
            current = current.checked_add(value)?;
            last = Some(Last::Ten);
        } else if word == "hundred" {
            if last == Some(Last::Hundred) {
                return None;
            }
            current = match current {
                0 => 100,
                below if below < 100 => below * 100,
                _ => return None,
            };
            last = Some(Last::Hundred);
        } else {
            let scale = lookup(SCALES, word)?;
            if last == Some(Last::Scale) {
                return None;
            }
            total = total.checked_add(current.max(1).checked_mul(scale)?)?;
            current = 0;
            last = Some(Last::Scale);
        }
        any = true;
    }
    if any {
        total.checked_add(current)
    } else {
        None
    }
}

/// A number's value when it is whole, and how it is written.
struct Parsed {
    whole: Option<u128>,
    written: String,
}

fn number_value(words: &[String]) -> Option<Parsed> {
    if words.is_empty() {
        return None;
    }
    if words.len() == 1 && DECIMAL.is_match(&words[0]) {
        return Some(Parsed {
            whole: None,
            written: words[0].replace(',', ""),
        });
    }
    if let Some(point) = words.iter().position(|word| word == "point") {
        let (left, right) = (&words[..point], &words[point + 1..]);
        if right.is_empty() {
            return None;
        }
        let left = if left.is_empty() { 0 } else { int_words(left)? };
        let mut fraction = String::new();
        for word in right {
            if let Some(digit) = digit_word(word) {
                fraction.push_str(&digit.to_string());
            } else if DIGITS.is_match(word) {
                fraction.push_str(word);
            } else {
                return None;
            }
        }
        return Some(Parsed {
            whole: None,
            written: format!("{left}.{fraction}"),
        });
    }
    int_words(words).map(|value| Parsed {
        whole: Some(value),
        written: value.to_string(),
    })
}

fn amount(symbol: &str, parsed: &Parsed) -> String {
    match parsed.whole {
        Some(value) if value >= 1000 => format!("{symbol}{}", grouped(value)),
        Some(value) => format!("{symbol}{value}"),
        None => format!("{symbol}{}", parsed.written),
    }
}

fn hour(word: &str) -> Option<u128> {
    int_words(&[word.to_owned()]).filter(|hour| (1..=12).contains(hour))
}

/// A clock time: "seven thirty" is 7:30, "seven oh five pm" is 7:05 pm.
fn time(words: &[String]) -> Option<String> {
    if words.len() < 2 {
        return None;
    }
    let mut core = words.to_vec();
    let mut ampm = String::new();
    if matches!(
        core.last().map(String::as_str),
        Some("am" | "pm" | "a.m." | "p.m.")
    ) {
        ampm = core.pop().unwrap_or_default().replace('.', "");
    } else if core.len() >= 3 {
        let tail = [core[core.len() - 2].as_str(), core[core.len() - 1].as_str()];
        if tail == ["a", "m"] || tail == ["p", "m"] {
            ampm = tail.concat();
            core.truncate(core.len() - 2);
        }
    }
    if core.is_empty() {
        return None;
    }
    let suffixed = |time: String| {
        if ampm.is_empty() {
            time
        } else {
            format!("{time} {ampm}")
        }
    };
    if core.len() == 2 && core[1] == "o'clock" {
        return hour(&core[0]).map(|hh| suffixed(format!("{hh}:00")));
    }
    if !ampm.is_empty() && core.len() == 1 {
        return hour(&core[0]).map(|hh| format!("{hh} {ampm}"));
    }
    if core.len() >= 2 {
        let hh = hour(&core[0])?;
        let rest = &core[1..];
        if rest[0] == "oh" && rest.len() == 2 {
            // "four oh one" stays a digit string unless am or pm says it
            // is a time.
            if ampm.is_empty() {
                return None;
            }
            let mm = int_words(&rest[1..]).filter(|mm| (1..=9).contains(mm))?;
            return Some(format!("{hh}:{mm:02} {ampm}"));
        }
        let mm = int_words(rest).filter(|mm| (10..=59).contains(mm))?;
        return Some(suffixed(format!("{hh}:{mm:02}")));
    }
    None
}

/// A NUMBER span: amounts, percentages, times, ordinals, digit strings,
/// and cardinals.
pub(super) fn number(tokens: &[&str]) -> Option<String> {
    let norms = norms(tokens);
    let count = norms.len();
    if count == 0 {
        return None;
    }
    // Already written: "250", "$500", "7:30", "3.5", "20%".
    if count == 1 && WRITTEN.is_match(&norms[0]) {
        return Some(tokens[0].to_owned());
    }
    // "forty two dollars and fifty cents" is $42.50.
    if count >= 4 && matches!(norms[count - 1].as_str(), "cent" | "cents") {
        let currency = norms
            .iter()
            .position(|word| lookup(CURRENCIES, word).is_some());
        if let Some(at) = currency.filter(|&at| at > 0) {
            let symbol = lookup(CURRENCIES, &norms[at]).unwrap_or_default();
            let mut cents = &norms[at + 1..count - 1];
            if cents.first().is_some_and(|word| word == "and") {
                cents = &cents[1..];
            }
            if let (Some(whole), Some(cents)) = (int_words(&norms[..at]), int_words(cents))
                && cents < 100
            {
                let whole = if whole >= 1000 {
                    grouped(whole)
                } else {
                    whole.to_string()
                };
                return Some(format!("{symbol}{whole}.{cents:02}"));
            }
        }
    }
    if count >= 2
        && let Some(symbol) = lookup(CURRENCIES, &norms[count - 1])
        && let Some(parsed) = number_value(&norms[..count - 1])
    {
        return Some(amount(symbol, &parsed));
    }
    if count >= 2
        && matches!(norms[count - 1].as_str(), "percent" | "percentage")
        && let Some(parsed) = number_value(&norms[..count - 1])
    {
        return Some(format!("{}%", parsed.written));
    }
    if let Some(time) = time(&norms) {
        return Some(time);
    }
    if let Some(&(_, value, suffix)) = ORDINALS.iter().find(|(said, ..)| *said == norms[count - 1])
    {
        if count == 1 {
            return Some(format!("{value}{suffix}"));
        }
        if let Some(prefix) = int_words(&norms[..count - 1]) {
            return Some(format!("{}{suffix}", prefix.checked_add(value)?));
        }
    }
    if count >= 2
        && matches!(norms[count - 1].as_str(), "k" | "grand")
        && let Some(value) = int_words(&norms[..count - 1])
    {
        return Some(format!("{value}k"));
    }
    if is_digit_string(&norms) {
        return Some(digit_string(&norms));
    }
    // "twelve oh eight" is 1208.
    if count >= 3 && norms[count - 2] == "oh" {
        let last = &norms[count - 1];
        let digit = digit_word(last).or_else(|| {
            if DIGIT.is_match(last) {
                last.parse().ok()
            } else {
                None
            }
        });
        if let (Some(prefix), Some(digit)) = (int_words(&norms[..count - 2]), digit)
            && (10..=99).contains(&prefix)
        {
            return Some(format!("{prefix}0{digit}"));
        }
    }
    number_value(&norms).map(|parsed| parsed.written)
}

/// A UNIT span: "two hundred fifty milliseconds" is 250 milliseconds.
pub(super) fn unit(tokens: &[&str]) -> Option<String> {
    let norms = norms(tokens);
    let count = norms.len();
    if count < 2 {
        return None;
    }
    if count >= 3
        && matches!(norms[count - 2].as_str(), "degree" | "degrees")
        && matches!(norms[count - 1].as_str(), "fahrenheit" | "celsius")
        && let Some(parsed) = number_value(&norms[..count - 2])
    {
        let scale = if norms[count - 1] == "fahrenheit" {
            "\u{b0}F"
        } else {
            "\u{b0}C"
        };
        return Some(format!("{} {scale}", parsed.written));
    }
    let written = lookup(UNITS, &norms[count - 1])?;
    let parsed = number_value(&norms[..count - 1])?;
    // "twenty second" is the 22nd, not 20 second.
    if matches!(
        norms[count - 1].as_str(),
        "second" | "minute" | "hour" | "milligram"
    ) && parsed.whole.is_some_and(|value| value != 1)
    {
        return None;
    }
    Some(format!("{} {written}", parsed.written))
}

fn symbol_join(norms: &[String]) -> String {
    let mut out = String::new();
    let mut index = 0;
    while index < norms.len() {
        let word = norms[index].as_str();
        let next = norms.get(index + 1).map(String::as_str);
        if word == "forward" && next == Some("slash") {
            out.push('/');
            index += 2;
            continue;
        }
        if word == "colon" && next == Some("colon") {
            out.push_str("::");
            index += 2;
            continue;
        }
        out.push_str(symbol(word).unwrap_or(word));
        index += 1;
    }
    out
}

/// An EMAIL span: "john at foo dot com" is john@foo.com.
pub(super) fn email(tokens: &[&str]) -> Option<String> {
    let norms = norms(tokens);
    if norms.is_empty() {
        return None;
    }
    let at = norms.iter().position(|word| word == "at");
    if at.is_none() && !norms.concat().contains('@') {
        return None;
    }
    // A prose word before the address ("be ops at example.io") is no part
    // of it.
    if let Some(at) = at {
        let local = &norms[..at];
        if local.is_empty()
            || local
                .windows(2)
                .any(|pair| symbol(&pair[0]).is_none() && symbol(&pair[1]).is_none())
        {
            return None;
        }
    }
    let joined = symbol_join(&norms);
    if joined.matches('@').count() != 1 {
        return None;
    }
    let (local, domain) = joined.split_once('@')?;
    if local.is_empty() || !domain.contains('.') {
        return None;
    }
    let (_, tld) = domain.rsplit_once('.')?;
    if tld.chars().count() < 2 || !tld.chars().all(char::is_alphabetic) {
        return None;
    }
    Some(joined)
}

/// A URL span: "foo dot com slash docs" is foo.com/docs.
pub(super) fn url(tokens: &[&str]) -> Option<String> {
    let norms = norms(tokens);
    if norms.is_empty() {
        return None;
    }
    let joined = symbol_join(&norms);
    let scheme = ["http://", "https://", "www."]
        .iter()
        .any(|prefix| joined.starts_with(prefix));
    let host = joined.split('/').next().unwrap_or_default();
    let bare = HOST_TLD.captures(host).is_some_and(|tld| {
        WEB_TLDS.contains(&&tld[1])
            && (joined.contains('/') || norms.iter().any(|word| word == "dot"))
    });
    (scheme || bare).then_some(joined)
}

/// A CODE span: "foo underscore bar" is foo_bar, "use state" is useState.
pub(super) fn code(tokens: &[&str]) -> Option<String> {
    let norms = norms(tokens);
    if norms.is_empty() {
        return None;
    }
    if norms
        .iter()
        .any(|word| symbol(word).is_some() || word == "forward")
    {
        let joined = symbol_join(&norms);
        return CODE_MARK.is_match(&joined).then_some(joined);
    }
    if (2..=6).contains(&norms.len()) && norms.iter().all(|word| IDENTIFIER.is_match(word)) {
        let mut out = norms[0].clone();
        for word in &norms[1..] {
            out.push_str(&word[..1].to_ascii_uppercase());
            out.push_str(&word[1..]);
        }
        return Some(out);
    }
    None
}

/// A span formatted by its kind's grammar, or `None` to keep it as heard.
pub(super) fn format(kind: Kind, tokens: &[&str]) -> Option<String> {
    match kind {
        Kind::Number => number(tokens),
        Kind::Unit => unit(tokens),
        Kind::Email => email(tokens),
        Kind::Url => url(tokens),
        Kind::Code => code(tokens),
    }
}

/// A span formatted with the punctuation around it kept: "(two hundred
/// fifty dollars)," is "($250),".
pub(super) fn format_with_punctuation(kind: Kind, tokens: &[&str]) -> Option<String> {
    let mut cores: Vec<&str> = tokens.to_vec();
    let Some(&first) = cores.first() else {
        return format(kind, &cores);
    };
    let mut lead = "";
    if let Some(found) = LEAD.find(first)
        && found.len() < first.len()
    {
        lead = found.as_str();
        cores[0] = &first[found.len()..];
    }
    let last_index = cores.len() - 1;
    let last = cores[last_index];
    let mut trail = "";
    if let Some(found) = TRAIL.find(last)
        && found.len() < last.len()
    {
        trail = found.as_str();
        cores[last_index] = &last[..found.start()];
    }
    format(kind, &cores).map(|formatted| format!("{lead}{formatted}{trail}"))
}

#[cfg(test)]
#[path = "itn_tests.rs"]
mod tests;
