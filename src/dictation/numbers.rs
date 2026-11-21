//! Whether a transcript is nothing but numbers, the one check the OpenCode
//! rewrite, the Apple Intelligence cleanup, and Labs › Neutral voice share
//! before they skip a dictation.

/// Whether a transcript says nothing but numbers: digits or English number
/// words, with punctuation between them.
pub(crate) fn numbers_only(transcript: &str) -> bool {
    const WORDS: &[&str] = &[
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
        "thirty",
        "forty",
        "fifty",
        "sixty",
        "seventy",
        "eighty",
        "ninety",
        "hundred",
        "thousand",
        "million",
        "billion",
    ];
    /// Words that join numbers but are not numbers themselves.
    const JOINERS: &[&str] = &["and", "point", "minus", "negative"];
    let mut any_number = false;
    for token in transcript.split(|c: char| c.is_whitespace() || c == '-') {
        let word = token.trim_matches(|c: char| !c.is_alphanumeric());
        if word.is_empty() {
            continue;
        }
        if word
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
            || WORDS.iter().any(|w| word.eq_ignore_ascii_case(w))
        {
            any_number = true;
        } else if !JOINERS.iter().any(|w| word.eq_ignore_ascii_case(w)) {
            return false;
        }
    }
    any_number
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_number_words_are_numbers() {
        for numbers in [
            "One, two, three, four, five, six, seven, eight, nine, ten.",
            "1355",
            "Twenty-one point five.",
            "one hundred and forty two",
        ] {
            assert!(numbers_only(numbers), "{numbers}");
        }
        for speech in [
            "",
            "And.",
            "Point one, the dataset has 1355 evaluation windows.",
            "Call me at five.",
        ] {
            assert!(!numbers_only(speech), "{speech}");
        }
    }
}
