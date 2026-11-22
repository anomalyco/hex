//! `SwipeDecoderTests` (`ios/HEXMobileTests/SwipeDecoderTests.swift`), test for test, on the
//! bundled English and Spanish models, with swipes drawn through Hex's own key geometry
//! (`KeyboardLayout(width: 390)`'s letter keys, as `TypingDecoder.Keys` has them).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::*;

/// `KeyboardLayout(width: 390, language: .english)`.
const ENGLISH_390: [(char, f64, f64); 26] = [
    ('a', 41.800000000000004, 121.5),
    ('b', 233.30000000000004, 175.5),
    ('c', 156.70000000000002, 175.5),
    ('d', 118.4, 121.5),
    ('e', 99.25, 67.5),
    ('f', 156.70000000000002, 121.5),
    ('g', 195.00000000000003, 121.5),
    ('h', 233.30000000000004, 121.5),
    ('i', 290.75, 67.5),
    ('j', 271.6, 121.5),
    ('k', 309.90000000000003, 121.5),
    ('l', 348.20000000000005, 121.5),
    ('m', 309.90000000000003, 175.5),
    ('n', 271.6, 175.5),
    ('o', 329.05, 67.5),
    ('p', 367.35, 67.5),
    ('q', 22.65, 67.5),
    ('r', 137.54999999999998, 67.5),
    ('s', 80.1, 121.5),
    ('t', 175.85, 67.5),
    ('u', 252.45000000000002, 67.5),
    ('v', 195.00000000000003, 175.5),
    ('w', 60.949999999999996, 67.5),
    ('x', 118.4, 175.5),
    ('y', 214.15, 67.5),
    ('z', 80.10000000000002, 175.5),
];

/// `KeyboardLayout(width: 390, language: .spanish)`.
const SPANISH_390: [(char, f64, f64); 27] = [
    ('a', 22.65, 121.5),
    ('b', 233.30000000000004, 175.5),
    ('c', 156.70000000000002, 175.5),
    ('d', 99.25, 121.5),
    ('e', 99.25, 67.5),
    ('f', 137.54999999999998, 121.5),
    ('g', 175.85, 121.5),
    ('h', 214.15, 121.5),
    ('i', 290.75, 67.5),
    ('j', 252.45000000000002, 121.5),
    ('k', 290.75, 121.5),
    ('l', 329.05, 121.5),
    ('m', 309.90000000000003, 175.5),
    ('n', 271.6, 175.5),
    ('o', 329.05, 67.5),
    ('p', 367.35, 67.5),
    ('q', 22.65, 67.5),
    ('r', 137.54999999999998, 67.5),
    ('s', 60.949999999999996, 121.5),
    ('t', 175.85, 67.5),
    ('u', 252.45000000000002, 67.5),
    ('v', 195.00000000000003, 175.5),
    ('w', 60.949999999999996, 67.5),
    ('x', 118.4, 175.5),
    ('y', 214.15, 67.5),
    ('z', 80.10000000000002, 175.5),
    ('ñ', 367.35, 121.5),
];

/// `AutocorrectEngine.englishContractions`.
const ENGLISH_CONTRACTIONS: [&str; 59] = [
    "I'd",
    "I'll",
    "I'm",
    "I've",
    "ain't",
    "aren't",
    "c'mon",
    "can't",
    "could've",
    "couldn't",
    "didn't",
    "doesn't",
    "don't",
    "hadn't",
    "hasn't",
    "haven't",
    "he'll",
    "he's",
    "here's",
    "how's",
    "isn't",
    "it'll",
    "it's",
    "let's",
    "ma'am",
    "might've",
    "must've",
    "mustn't",
    "needn't",
    "o'clock",
    "she'll",
    "she's",
    "should've",
    "shouldn't",
    "that's",
    "there's",
    "they'd",
    "they'll",
    "they're",
    "they've",
    "wasn't",
    "we'd",
    "we'll",
    "we're",
    "we've",
    "weren't",
    "what's",
    "when's",
    "where's",
    "who's",
    "why's",
    "won't",
    "would've",
    "wouldn't",
    "y'all",
    "you'd",
    "you'll",
    "you're",
    "you've",
];

#[derive(Clone, Copy, PartialEq)]
enum Language {
    English,
    Spanish,
}

impl Language {
    fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
        }
    }
}

fn bundled_model(language: &str) -> Arc<Model> {
    let path = format!(
        "{}/../../../ios/HEXKeyboard/TypingModel/{language}.hextm",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    Arc::new(Model::new(Box::new(bytes)).expect("a valid model"))
}

/// `OffensiveWords.bundled(language)`: the shipped `never-suggest.tsv`'s spellings for the
/// language and every layout (`*`), each a slur (true) or a swear word.
fn offensive_words(language: &str) -> HashMap<String, bool> {
    let path = format!(
        "{}/../../../ios/HEXShared/FullKeyboard/never-suggest.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let tsv = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut entries: HashMap<String, bool> = HashMap::new();
    for line in tsv.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 3 || (fields[0] != "*" && fields[0] != language) {
            continue;
        }
        let slur = match fields[1] {
            "slur" => true,
            "swear" => false,
            _ => continue,
        };
        let spelling = fields[2]
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !spelling.is_empty() && entries.get(&spelling) != Some(&true) {
            entries.insert(spelling, slur);
        }
    }
    entries
}

/// `OffensiveWords.blocks(_:allowSwearing:)` for one word.
fn blocks(words: &HashMap<String, bool>, word: &str, allow_swearing: bool) -> bool {
    word.split([' ', '-']).filter(|p| !p.is_empty()).any(|p| {
        words
            .get(&p.to_lowercase())
            .is_some_and(|&slur| slur || !allow_swearing)
    })
}

/// The decoder and keys `SwipeDecoderTests.decoder` makes.
fn decoder(language: Language, swearing: bool) -> (SwipeDecoder, Keys) {
    let model = bundled_model(language.code());
    let blocked: Vec<String> = offensive_words(language.code())
        .into_iter()
        .filter(|&(_, slur)| slur || !swearing)
        .map(|(word, _)| word)
        .collect();
    let never_suggest = NeverSuggest::new(blocked.iter().map(String::as_str), &model);
    let extra = if language == Language::English {
        ENGLISH_CONTRACTIONS
            .iter()
            .map(|&w| ExtraWord {
                text: w.to_owned(),
                cost: cost_of_contraction(w, CONTRACTION_COST),
                contraction: false,
            })
            .collect()
    } else {
        Vec::new()
    };
    let centres = match language {
        Language::English => ENGLISH_390.to_vec(),
        Language::Spanish => SPANISH_390.to_vec(),
    };
    let keys = Keys {
        centres,
        pitch: (38.3, 54.0),
    };
    (
        SwipeDecoder::new(model, Parameters::default(), never_suggest, extra),
        keys,
    )
}

/// `SwipePath`'s key centres for `word`: an accented letter on its base letter's key, an
/// apostrophe on none, repeats run together.
fn centres(word: &str, keys: &Keys) -> Vec<(f64, f64)> {
    use unicode_normalization::UnicodeNormalization;
    let find = |c: char| keys.centres.iter().find(|k| k.0 == c).map(|k| (k.1, k.2));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for letter in word.to_lowercase().chars().filter(|&c| c != '\'') {
        let base = std::iter::once(letter).nfd().next();
        let Some(centre) = find(letter).or_else(|| base.and_then(find)) else {
            continue;
        };
        if out.last() != Some(&centre) {
            out.push(centre);
        }
    }
    out
}

/// `SwipePath.through`: straight strokes through the centres of `word`'s keys, a point every
/// 6 points, moved by `offset`.
fn through(word: &str, keys: &Keys, offset: (f64, f64)) -> Vec<(f64, f64)> {
    let centres = centres(word, keys);
    let Some(&first) = centres.first() else {
        return Vec::new();
    };
    let mut points = vec![first];
    for pair in centres.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let count = 1.max(((b.0 - a.0).hypot(b.1 - a.1) / 6.0).ceil() as usize);
        for i in 1..=count {
            let t = i as f64 / count as f64;
            points.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    points
        .iter()
        .map(|p| (p.0 + offset.0, p.1 + offset.1))
        .collect()
}

/// `SwipePath.timed`: as `through`, at an even 750 points a second, the finger staying
/// 0.15 s on the keys at `pauses` (indexes into the word's keys), sending nothing meanwhile.
fn timed(word: &str, keys: &Keys, pauses: &[usize]) -> (Vec<(f64, f64)>, Vec<f64>) {
    let centres = centres(word, keys);
    let mut points = vec![centres[0]];
    let mut times = vec![0.0];
    for (index, pair) in centres.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let count = 1.max(((b.0 - a.0).hypot(b.1 - a.1) / 6.0).ceil() as usize);
        for i in 1..=count {
            let t = i as f64 / count as f64;
            points.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            // The first point after a paused key comes only once the finger moves on.
            let pause = if i == 1 && pauses.contains(&index) {
                0.15
            } else {
                0.0
            };
            times.push(times[times.len() - 1] + 0.008 + pause);
        }
    }
    (points, times)
}

fn texts(reading: Reading) -> Vec<String> {
    reading.candidates.into_iter().map(|c| c.text).collect()
}

fn read(
    word: &str,
    decoder: &SwipeDecoder,
    keys: &Keys,
    previous: Option<&str>,
    offset: (f64, f64),
) -> Vec<String> {
    texts(decoder.read(&through(word, keys, offset), None, keys, previous, 4))
}

fn first(word: &str, decoder: &SwipeDecoder, keys: &Keys) -> Option<String> {
    read(word, decoder, keys, None, (0.0, 0.0))
        .into_iter()
        .next()
}

#[test]
fn clean_swipes_read_as_their_words() {
    let (en, keys) = decoder(Language::English, false);
    for word in [
        "hello", "the", "keyboard", "world", "typing", "people", "because", "quick", "with", "you",
        "and",
    ] {
        assert_eq!(first(word, &en, &keys).as_deref(), Some(word));
    }
}

/// A finger that runs low all the way (as taps land below the keys' centres), or low and to
/// the left.
#[test]
fn a_swipe_off_the_centres_still_reads() {
    let (en, keys) = decoder(Language::English, false);
    let (w, h) = keys.pitch;
    for off in [(0.0, h * 0.3), (-w / 4.0, h / 4.0)] {
        for word in [
            "hello", "keyboard", "people", "world", "typing", "because", "quick",
        ] {
            let read = read(word, &en, &keys, None, off);
            assert_eq!(read.first().map(String::as_str), Some(word), "{off:?}");
        }
    }
}

#[test]
fn spanish_swipes_keep_their_accents_and_their_enye() {
    let (es, keys) = decoder(Language::Spanish, false);
    for word in ["año", "también", "gracias", "mañana", "niño", "teclado"] {
        assert_eq!(first(word, &es, &keys).as_deref(), Some(word));
    }
    let both: HashSet<String> = read("esta", &es, &keys, None, (0.0, 0.0))
        .into_iter()
        .take(2)
        .collect();
    assert_eq!(
        both,
        HashSet::from(["esta".to_owned(), "está".to_owned()]),
        "one path, both readings"
    );
}

/// Google Books splits "don't" into "do n't", so the model barely counts it: the decoder
/// reads Hex's English contractions as common words.
#[test]
fn contractions_read_as_common_words() {
    let (en, keys) = decoder(Language::English, false);
    assert_eq!(first("dont", &en, &keys).as_deref(), Some("don't"));
    assert_eq!(first("im", &en, &keys).as_deref(), Some("I'm"));
}

#[test]
fn the_word_before_picks_between_words_on_one_path() {
    let (en, keys) = decoder(Language::English, false);
    assert_eq!(read("to", &en, &keys, None, (0.0, 0.0))[..2], ["to", "too"]);
    assert_eq!(read("too", &en, &keys, Some("far"), (0.0, 0.0))[0], "too");
}

#[test]
fn slurs_are_never_read_and_swear_words_only_when_asked() {
    for language in [Language::English, Language::Spanish] {
        let words = offensive_words(language.code());
        for swearing in [false, true] {
            let (decoder, keys) = decoder(language, swearing);
            let probes: &[&str] = match language {
                Language::English => &["fuck", "shit", "bitch", "nigger", "faggot"],
                Language::Spanish => &["mierda", "cabron", "maricón"],
            };
            for probe in probes {
                for reading in read(probe, &decoder, &keys, None, (0.0, 0.0)) {
                    assert!(
                        !blocks(&words, &reading, swearing),
                        "{probe} read as {reading}, swear words {swearing}"
                    );
                }
            }
        }
    }
    let (swearing, keys) = decoder(Language::English, true);
    assert_eq!(
        first("fuck", &swearing, &keys).as_deref(),
        Some("fuck"),
        "with Suggest swear words on"
    );
}

/// A straight stroke runs over the letters between its ends: "tired" and "tied" draw one line
/// from i to e, "wire" and "write" one from w to i and back to e. Only where the finger slowed
/// tells them apart, so a pause on a letter the stroke crosses reads it, and the same stroke
/// without one doesn't.
#[test]
fn a_pause_on_a_letter_the_stroke_crosses_reads_it() {
    let (en, keys) = decoder(Language::English, false);
    let read = |word: &str, pauses: &[usize]| {
        let (points, times) = timed(word, &keys, pauses);
        texts(en.read(&points, Some(&times), &keys, None, 4))
            .into_iter()
            .next()
    };
    // The pauses are on the letters' keys in order (t i r e d: r is 2).
    assert_eq!(read("tired", &[2]).as_deref(), Some("tired"));
    assert_eq!(
        read("tired", &[]).as_deref(),
        Some("tied"),
        "the same path, no pause on r"
    );
    assert_eq!(read("tire", &[2]).as_deref(), Some("tire"));
    assert_eq!(read("tie", &[]).as_deref(), Some("tie"));
    assert_eq!(read("wire", &[2]).as_deref(), Some("wire"));
    assert_eq!(read("write", &[1, 3]).as_deref(), Some("write"));
    assert_eq!(read("greet", &[1]).as_deref(), Some("greet"));
    assert_eq!(read("get", &[]).as_deref(), Some("get"));
}

/// A pause off the word's keys counts against it: p to t along the top row is "put" when the
/// finger slows on u and "pit" on i; slowing on o and u offers the much rarer "pout".
#[test]
fn a_pause_off_the_words_keys_counts_against_it() {
    let (en, keys) = decoder(Language::English, false);
    let read = |word: &str, pauses: &[usize]| {
        let (points, times) = timed(word, &keys, pauses);
        texts(en.read(&points, Some(&times), &keys, None, 4))
    };
    assert_eq!(read("put", &[1])[0], "put");
    assert_eq!(read("pit", &[1])[0], "pit");
    assert!(read("pout", &[1, 2]).iter().take(3).any(|w| w == "pout"));
    assert!(
        !read("put", &[1]).iter().take(3).any(|w| w == "pout"),
        "no pause on o, no pout"
    );
    // Touches that all arrive at once (a test's, or no clock) read as before.
    let (points, _) = timed("pout", &keys, &[1, 2]);
    let still = vec![0.0; points.len()];
    assert_eq!(
        texts(en.read(&points, Some(&still), &keys, None, 4)),
        texts(en.read(&points, None, &keys, None, 4))
    );
}

#[test]
fn a_path_too_short_reads_as_nothing() {
    let (en, keys) = decoder(Language::English, false);
    let h = keys
        .centres
        .iter()
        .find(|k| k.0 == 'h')
        .map(|k| (k.1, k.2))
        .unwrap();
    assert!(
        en.read(&[h, (h.0 + 4.0, h.1)], None, &keys, None, 4)
            .candidates
            .is_empty()
    );
    assert!(en.read(&[h], None, &keys, None, 4).candidates.is_empty());
}

#[test]
fn contraction_costs_and_heads() {
    assert_eq!(cost_of_contraction("I'm", CONTRACTION_COST), 6.0);
    assert_eq!(cost_of_contraction("We're", CONTRACTION_COST), 7.5);
    assert_eq!(cost_of_contraction("ma'am", CONTRACTION_COST), 9.5);
    assert_eq!(head_of("don't"), Some("do"));
    assert_eq!(
        head_of("can't"),
        Some("ca"),
        "as Google Books splits it: ca n't"
    );
    assert_eq!(head_of("i'm"), Some("i"));
    assert_eq!(head_of("hello"), None);
}
