//! A clock time the recogniser wrote with a dot, written with a colon:
//! "at 2.30" is "at 2:30", while "version 2.30", "2.30 meters", and
//! "trading at 2.30" keep their dot.
//!
//! The reference's `_clock_times` (`itn.py`), run over the kept words
//! once the line is written.

use std::sync::LazyLock;

use regex::Regex;

use super::itn::{is_amount_word, norm};
use super::render::ends_sentence;

static CLOCK_DOT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{1,2})\.([0-5]\d)(am|pm)?([,.?!;:]*)$").expect("a fixed pattern compiles")
});

/// Words before a time: "at 2.30", "by 4.45".
const LEADS: &[&str] = &[
    "at", "by", "around", "until", "till", "til", "before", "after", "since",
];
const MERIDIEMS: &[&str] = &["am", "pm", "a.m", "p.m", "a.m.", "p.m."];
/// Words before "at" that make it a price: "trading at 2.30".
const AMOUNT_LEADS: &[&str] = &[
    "priced", "trading", "trades", "traded", "valued", "sold", "selling", "costs", "cost", "rated",
    "capped", "pegged",
];
/// Words after a number that make it an amount: "2.30 a share".
const AMOUNT_FOLLOWERS: &[&str] = &[
    "percent",
    "percentage",
    "per",
    "times",
    "x",
    "a",
    "an",
    "each",
    "apiece",
    "points",
    "point",
];

/// `word` written as a clock time, when it reads as one.
fn clock_time(before_lead: &str, lead: &str, word: &str, next: &str) -> Option<String> {
    let found = CLOCK_DOT.captures(word)?;
    let hour: u32 = found[1].parse().ok()?;
    if !(1..=23).contains(&hour) {
        return None;
    }
    let minute = &found[2];
    let meridiem = found.get(3).map_or("", |at| at.as_str());
    let trail = &found[4];
    let follows = if trail.is_empty() {
        norm(next)
    } else {
        String::new()
    };
    if is_amount_word(&follows) || AMOUNT_FOLLOWERS.contains(&follows.as_str()) {
        return None;
    }
    let timed =
        !meridiem.is_empty() || MERIDIEMS.contains(&follows.as_str()) || follows == "o'clock";
    if !timed
        && (!LEADS.contains(&norm(lead).as_str())
            || AMOUNT_LEADS.contains(&norm(before_lead).as_str()))
    {
        return None;
    }
    Some(format!("{hour}:{minute}{meridiem}{trail}"))
}

/// The written words with every dotted clock time given its colon.
pub(super) fn clock_times(words: &mut [String]) {
    let read: Vec<String> = words.to_vec();
    for (at, word) in words.iter_mut().enumerate() {
        let mut lead = at.checked_sub(1).map_or("", |before| read[before].as_str());
        let mut before_lead = at.checked_sub(2).map_or("", |before| read[before].as_str());
        if !lead.is_empty() && ends_sentence(lead) {
            lead = "";
            before_lead = "";
        }
        let next = read.get(at + 1).map_or("", String::as_str);
        if let Some(written) = clock_time(before_lead, lead, &read[at], next) {
            *word = written;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(line: &str) -> String {
        let mut words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        clock_times(&mut words);
        words.join(" ")
    }

    #[test]
    fn a_dotted_clock_time_gets_a_colon_and_an_amount_keeps_its_dot() {
        for (heard, want) in [
            (
                "Let's meet on Thursday at 2.30.",
                "Let's meet on Thursday at 2:30.",
            ),
            ("Call me at 10.15 tomorrow", "Call me at 10:15 tomorrow"),
            ("done by 4.45, then lunch", "done by 4:45, then lunch"),
            ("Lunch is 12.30 pm", "Lunch is 12:30 pm"),
            ("Dinner at 7.05pm.", "Dinner at 7:05pm."),
        ] {
            assert_eq!(written(heard), want);
        }
        for kept in [
            "Upgrade to version 2.30 today",
            "The pole is at 2.30 meters",
            "It costs $2.30 now",
            "Shares trading at 2.30 today",
            "Sold at 2.30 a share",
            "Rates at 0.50 percent",
            "Pi is about 3.14",
            "We ran at 2.305 load",
            "Done at. 2.30 left",
        ] {
            assert_eq!(written(kept), kept);
        }
    }
}
