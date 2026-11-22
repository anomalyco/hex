//! Autocorrect's built-in word lists (`AutocorrectEngine+Dictionaries`, and the context
//! groups of `AutocorrectEngine`). Keys are ASCII and looked up composed and lowercased.

use super::text;

/// Valid words where an apostrophe form exists, offered without clobbering the word.
pub fn suggest_only_contraction(lower: &str) -> Option<&'static str> {
    Some(match text::nfc(lower).as_ref() {
        "ill" => "I'll",
        "id" => "I'd",
        "were" => "we're",
        "wed" => "we'd",
        "well" => "we'll",
        "its" => "it's",
        "lets" => "let's",
        "hell" => "he'll",
        "shell" => "she'll",
        _ => return None,
    })
}

const SUGGEST_ONLY_VALUES: &[&str] = &[
    "I'll", "I'd", "we're", "we'd", "we'll", "it's", "let's", "he'll", "she'll",
];

/// Words a period after them makes an abbreviation ("approx.", "etc.").
pub const ABBREVIATIONS: &[&str] = &[
    "approx", "appt", "apt", "assn", "asst", "ave", "blvd", "cf", "co", "corp", "dept", "dr", "eg",
    "est", "etc", "fig", "govt", "ie", "inc", "jr", "lb", "lbs", "ltd", "misc", "mr", "mrs", "ms",
    "mt", "no", "nr", "oz", "pp", "prof", "pt", "rd", "sr", "st", "vol", "vs", "wk", "yr",
];

pub fn is_abbreviation(lower: &str) -> bool {
    ABBREVIATIONS.contains(&text::nfc(lower).as_ref())
}

const SHORTHAND: &[&str] = &[
    "ok", "okay", "ty", "thx", "pls", "plz", "tmrw", "tbh", "idk", "imo", "imho", "lol", "lmao",
    "omg", "brb", "btw", "afaik", "fyi", "nvm", "rn", "irl", "jk", "np", "yw", "smh", "ikr", "ngl",
    "fr", "gg", "dm", "pr", "prs", "ci", "ui", "ux", "ios", "macos", "api", "apis", "url", "urls",
    "json", "sql", "ssh", "cli", "gpu", "cpu", "ram", "sdk", "hex", "wisp", "seer", "rune", "kit",
    "gonna", "wanna", "gotta", "kinda", "sorta", "yeah", "yep", "nope", "nah", "hey", "yo",
];

/// Common chat and engineering shorthand that cautious autocorrect never rewrites.
pub fn is_shorthand(lower: &str) -> bool {
    SHORTHAND.contains(&text::nfc(lower).as_ref())
}

const UNAMBIGUOUS: &[(&str, &str)] = &[
    ("i", "I"),
    ("im", "I'm"),
    ("ive", "I've"),
    ("dont", "don't"),
    ("cant", "can't"),
    ("wont", "won't"),
    ("didnt", "didn't"),
    ("doesnt", "doesn't"),
    ("isnt", "isn't"),
    ("arent", "aren't"),
    ("wasnt", "wasn't"),
    ("werent", "weren't"),
    ("hasnt", "hasn't"),
    ("havent", "haven't"),
    ("hadnt", "hadn't"),
    ("wouldnt", "wouldn't"),
    ("couldnt", "couldn't"),
    ("shouldnt", "shouldn't"),
    ("mustnt", "mustn't"),
    ("neednt", "needn't"),
    ("youre", "you're"),
    ("youve", "you've"),
    ("youd", "you'd"),
    ("youll", "you'll"),
    ("theyre", "they're"),
    ("theyve", "they've"),
    ("theyd", "they'd"),
    ("theyll", "they'll"),
    ("weve", "we've"),
    ("shes", "she's"),
    ("hes", "he's"),
    ("itll", "it'll"),
    ("thats", "that's"),
    ("theres", "there's"),
    ("heres", "here's"),
    ("wheres", "where's"),
    ("whos", "who's"),
    ("whats", "what's"),
    ("hows", "how's"),
    ("whens", "when's"),
    ("whys", "why's"),
    ("aint", "ain't"),
    ("yall", "y'all"),
    ("oclock", "o'clock"),
    ("maam", "ma'am"),
    ("cmon", "c'mon"),
    ("wouldve", "would've"),
    ("couldve", "could've"),
    ("shouldve", "should've"),
    ("mightve", "might've"),
    ("mustve", "must've"),
    ("teh", "the"),
    ("hte", "the"),
    ("taht", "that"),
    ("thta", "that"),
    ("adn", "and"),
    ("nad", "and"),
    ("wiht", "with"),
    ("iwth", "with"),
    ("waht", "what"),
    ("hwat", "what"),
    ("wich", "which"),
    ("whcih", "which"),
    ("thier", "their"),
    ("theri", "their"),
    ("recieve", "receive"),
    ("definately", "definitely"),
    ("seperate", "separate"),
    ("occured", "occurred"),
    ("untill", "until"),
    ("abotu", "about"),
    ("baout", "about"),
    ("becuase", "because"),
    ("beacuse", "because"),
    ("jsut", "just"),
    ("jstu", "just"),
    ("knwo", "know"),
    ("konw", "know"),
    ("tiem", "time"),
    ("tmie", "time"),
    ("shoudl", "should"),
    ("hsould", "should"),
    ("woudl", "would"),
    ("owuld", "would"),
    ("coudl", "could"),
    ("oculd", "could"),
    ("ahve", "have"),
    ("hvae", "have"),
    ("maek", "make"),
    ("mkae", "make"),
    ("thign", "thing"),
    ("hting", "thing"),
    ("peopel", "people"),
    ("poeple", "people"),
    ("workign", "working"),
    ("goign", "going"),
    ("doign", "doing"),
    ("runnign", "running"),
    ("comign", "coming"),
    ("thnaks", "thanks"),
    ("htanks", "thanks"),
    ("plase", "please"),
    ("pleaes", "please"),
    ("yuor", "your"),
    ("yoru", "your"),
    ("yuo", "you"),
    ("oyu", "you"),
    ("fomr", "from"),
    ("verry", "very"),
    ("evrey", "every"),
    ("realyl", "really"),
    ("realy", "really"),
    ("actualyl", "actually"),
    ("probalby", "probably"),
    ("tomorow", "tomorrow"),
    ("tommorow", "tomorrow"),
    ("tonihgt", "tonight"),
    ("tongiht", "tonight"),
    ("mesasge", "message"),
    ("messgae", "message"),
    ("keybaord", "keyboard"),
    ("keboard", "keyboard"),
    ("iphnoe", "iPhone"),
    ("ipohne", "iPhone"),
];

/// Unambiguous missing-apostrophe contractions and common QWERTY slips.
pub fn unambiguous_fix(lower: &str) -> Option<&'static str> {
    let key = text::nfc(lower);
    UNAMBIGUOUS
        .iter()
        .find(|(k, _)| *k == key.as_ref())
        .map(|(_, v)| *v)
}

/// The English contractions Hex corrects to or offers, sorted (`englishContractions`).
pub fn english_contractions() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = UNAMBIGUOUS
        .iter()
        .map(|(_, v)| *v)
        .filter(|v| v.contains('\''))
        .chain(SUGGEST_ONLY_VALUES.iter().copied())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// English words chosen by the words around them (`contextGroups`).
pub const CONTEXT_GROUPS: &[&[&str]] = &[
    &["id", "I'd"],
    &["ill", "I'll"],
    &["hell", "he'll"],
    &["shell", "she'll"],
    &["wed", "we'd"],
    &["lets", "let's"],
    &["well", "we'll"],
    &["its", "it's"],
    &["your", "you're"],
    &["there", "their", "they're"],
    &["then", "than"],
];

/// The group `lower` belongs to (`contextGroupOf`).
pub fn context_group(lower: &str) -> Option<&'static [&'static str]> {
    let key = text::nfc(lower);
    CONTEXT_GROUPS
        .iter()
        .copied()
        .find(|group| group.iter().any(|w| text::lowercased(w) == key.as_ref()))
}

/// A contraction scored as the words it stands for (`contractionWords`).
pub fn contraction_words(lower: &str) -> Option<&'static [&'static [&'static str]]> {
    Some(match text::nfc(lower).as_ref() {
        "i'd" => &[&["i", "would"], &["i", "had"]],
        "i'll" => &[&["i", "will"]],
        "he'll" => &[&["he", "will"]],
        "she'll" => &[&["she", "will"]],
        "we'd" => &[&["we", "would"], &["we", "had"]],
        "let's" => &[&["let", "us"]],
        "we'll" => &[&["we", "will"]],
        "it's" => &[&["it", "is"], &["it", "has"]],
        "you're" => &[&["you", "are"]],
        "they're" => &[&["they", "are"]],
        _ => return None,
    })
}
