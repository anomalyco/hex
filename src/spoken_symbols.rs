//! Labs › Spoken symbols: whole spoken words for punctuation and symbols
//! become the characters, joined to their neighbours as written text joins
//! them: "hashtag release" is "#release", "my underscore variable" is
//! "my_variable", and "done comma thanks" is "done, thanks". Matching
//! ignores ASCII case and needs word boundaries, so "commas" or "hashtags"
//! stay as they are. The commas, periods, and spaces a model puts around a
//! spoken symbol go with it.

/// Which neighbour a symbol attaches to.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Join {
    /// The next word: "#release".
    Next,
    /// Both words: "my_variable".
    Both,
    /// The previous word, with one space after: "done, thanks".
    Previous,
}

const SYMBOLS: &[(&[&str], char, Join)] = &[
    (&["hashtag"], '#', Join::Next),
    (&["hash", "sign"], '#', Join::Next),
    (&["at", "sign"], '@', Join::Next),
    (&["underscore"], '_', Join::Both),
    (&["hyphen"], '-', Join::Both),
    (&["slash"], '/', Join::Both),
    (&["comma"], ',', Join::Previous),
    (&["semicolon"], ';', Join::Previous),
    (&["colon"], ':', Join::Previous),
];

/// `transcript` with its spoken symbols applied.
pub fn apply(transcript: &str) -> String {
    let chars: Vec<char> = transcript.chars().collect();
    let mut out = String::with_capacity(transcript.len());
    // Where the last symbol written ends, so trimming never removes it.
    let mut floor = 0;
    let mut index = 0;
    while index < chars.len() {
        let word_start = index == 0 || !chars[index - 1].is_alphanumeric();
        if word_start && let Some((symbol, join, end)) = symbol_at(&chars, index) {
            index = emit(&mut out, &mut floor, symbol, join, &chars, end);
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// The symbol whose words start at `start`, and where they end.
fn symbol_at(chars: &[char], start: usize) -> Option<(char, Join, usize)> {
    SYMBOLS.iter().find_map(|(words, symbol, join)| {
        let mut position = start;
        for (number, word) in words.iter().enumerate() {
            if number > 0 {
                let spaces = chars[position..]
                    .iter()
                    .take_while(|c| c.is_whitespace())
                    .count();
                if spaces == 0 {
                    return None;
                }
                position += spaces;
            }
            for expected in word.chars() {
                match chars.get(position) {
                    Some(c) if c.to_ascii_lowercase() == expected => position += 1,
                    _ => return None,
                }
            }
        }
        let word_end = chars
            .get(position)
            .is_none_or(|next| !next.is_alphanumeric());
        word_end.then_some((*symbol, *join, position))
    })
}

fn is_space(c: char) -> bool {
    c.is_whitespace() && c != '\n'
}

/// Removes trailing spaces and model commas, never below `floor`.
fn trim_end(out: &mut String, floor: usize) {
    while out.len() > floor
        && let Some(last) = out.chars().next_back()
        && (is_space(last) || last == ',')
    {
        out.pop();
    }
}

fn skip(chars: &[char], mut index: usize) -> usize {
    while chars
        .get(index)
        .is_some_and(|&c| is_space(c) || matches!(c, ',' | '.'))
    {
        index += 1;
    }
    index
}

/// Writes `symbol` and returns where the text continues.
fn emit(
    out: &mut String,
    floor: &mut usize,
    symbol: char,
    join: Join,
    chars: &[char],
    end: usize,
) -> usize {
    if join != Join::Next {
        trim_end(out, *floor);
    }
    out.push(symbol);
    *floor = out.len();
    let next = skip(chars, end);
    if join == Join::Previous
        && chars.get(next).is_some_and(|&c| {
            !matches!(
                c,
                '\n' | ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '"'
            )
        })
    {
        out.push(' ');
    }
    next
}

#[cfg(test)]
mod tests {
    use super::apply;

    #[test]
    fn symbols_join_their_neighbours() {
        assert_eq!(apply("hashtag release"), "#release");
        assert_eq!(apply("my underscore variable"), "my_variable");
        assert_eq!(apply("done comma thanks"), "done, thanks");
        assert_eq!(apply("tag it hash sign release"), "tag it #release");
        assert_eq!(apply("mail me at sign home"), "mail me @home");
        assert_eq!(apply("well hyphen known"), "well-known");
        assert_eq!(apply("and slash or"), "and/or");
        assert_eq!(apply("one semicolon two"), "one; two");
        assert_eq!(apply("the list colon apples"), "the list: apples");
        assert_eq!(apply("Done, Comma, thanks."), "Done, thanks.");
        assert_eq!(apply("the list COLON."), "the list:");
        assert_eq!(apply("a comma comma b"), "a,, b");
    }

    #[test]
    fn other_words_are_left_alone() {
        for text in [
            "commas",
            "slashed prices",
            "hashtags everywhere",
            "a hash browns sign",
            "the colonel",
            "Plain text, nothing to do.",
            "",
        ] {
            assert_eq!(apply(text), text);
        }
    }
}
