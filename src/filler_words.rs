//! Labs › Remove filler words: the hesitations a model writes down
//! verbatim ("um", "umm", "uh", "uhh", "erm", "er", "ah") leave a dictation,
//! with no model involved. A filler matches as a whole word, lowercase or
//! with only its first letter capitalised, so "umbrella", "ahead", "error",
//! "Uhura", "uh-huh", and "ER" stay as they are. The comma or ellipsis that
//! comes with a filler goes with it, the spaces around it close up, and a
//! capitalised filler that started a sentence hands its capital to the next
//! word. Line breaks are kept.

const FILLERS: &[&str] = &["um", "umm", "uh", "uhh", "erm", "er", "ah"];

/// `transcript` without its filler words.
pub fn apply(transcript: &str) -> String {
    let chars: Vec<char> = transcript.chars().collect();
    let mut out = String::with_capacity(transcript.len());
    // A filler was just removed: the next text decides the spacing.
    let mut removed = false;
    let mut capitalize = false;
    let mut index = 0;
    while index < chars.len() {
        if let Some(end) = filler_at(&chars, index) {
            trim_end(&mut out, &[]);
            if !removed {
                capitalize = chars[index].is_uppercase() && sentence_start(&out);
            }
            index = after_filler(&chars, end);
            removed = true;
            continue;
        }
        let c = chars[index];
        index += 1;
        if removed {
            if is_space(c) {
                continue;
            }
            if c == '\n' {
                removed = false;
                capitalize = false;
                out.push(c);
                continue;
            }
            if matches!(c, '.' | '!' | '?' | ',') && sentence_start(&out) {
                continue;
            }
            removed = false;
            let closing = matches!(
                c,
                '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '”' | '’' | '…'
            ) || (c == '"'
                && chars.get(index).is_none_or(|next| !next.is_alphanumeric()));
            if closing {
                trim_end(&mut out, &[',']);
                capitalize = false;
            } else {
                if needs_space(&out) {
                    out.push(' ');
                }
                if std::mem::take(&mut capitalize) {
                    out.extend(c.to_uppercase());
                    continue;
                }
            }
        }
        out.push(c);
    }
    if removed {
        trim_end(&mut out, &[',']);
    }
    out
}

fn joins_word(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '\'' | '’')
}

/// Where the filler starting at `start` ends, if one does.
fn filler_at(chars: &[char], start: usize) -> Option<usize> {
    if start > 0 && joins_word(chars[start - 1]) {
        return None;
    }
    FILLERS.iter().find_map(|word| {
        let end = start + word.len();
        let candidate = chars.get(start..end)?;
        let matches =
            candidate
                .iter()
                .zip(word.chars())
                .enumerate()
                .all(|(number, (&c, expected))| {
                    c == expected || (number == 0 && c == expected.to_ascii_uppercase())
                });
        let ends = chars.get(end).is_none_or(|&next| !joins_word(next));
        (matches && ends).then_some(end)
    })
}

/// Where the text continues after a filler ending at `end`, past its own
/// comma or ellipsis.
fn after_filler(chars: &[char], end: usize) -> usize {
    let mut index = skip_spaces(chars, end);
    match chars.get(index) {
        Some(',' | '…') => index += 1,
        Some('.') if chars.get(index + 1) == Some(&'.') && chars.get(index + 2) == Some(&'.') => {
            index += 3
        }
        _ => {}
    }
    skip_spaces(chars, index)
}

fn is_space(c: char) -> bool {
    c.is_whitespace() && c != '\n'
}

fn skip_spaces(chars: &[char], mut index: usize) -> usize {
    while chars.get(index).is_some_and(|&c| is_space(c)) {
        index += 1;
    }
    index
}

fn trim_end(out: &mut String, also: &[char]) {
    while let Some(last) = out.chars().next_back() {
        if is_space(last) || also.contains(&last) {
            out.pop();
        } else {
            break;
        }
    }
}

fn sentence_start(out: &str) -> bool {
    out.chars()
        .next_back()
        .is_none_or(|last| matches!(last, '.' | '!' | '?' | '\n'))
}

fn needs_space(out: &str) -> bool {
    let mut reversed = out.chars().rev();
    match reversed.next() {
        None | Some('\n' | '(' | '[' | '{' | '“' | '‘') => false,
        Some('"') => reversed
            .next()
            .is_some_and(|before| !before.is_whitespace()),
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::apply;

    #[test]
    fn fillers_leave_with_their_commas_and_spaces() {
        assert_eq!(apply("So, um, we should go."), "So, we should go.");
        assert_eq!(apply("I think uh we should go."), "I think we should go.");
        assert_eq!(apply("I think, um."), "I think.");
        assert_eq!(apply("Well umm... maybe"), "Well maybe");
        assert_eq!(apply("It was, er, erm, fine"), "It was, fine");
        assert_eq!(apply("Done, uh"), "Done");
        assert_eq!(apply("(um, aside)"), "(aside)");
    }

    #[test]
    fn a_filler_that_started_a_sentence_hands_on_its_capital() {
        assert_eq!(apply("Um, so I think"), "So I think");
        assert_eq!(
            apply("Hello. Uh, we left. Ah. Then"),
            "Hello. We left. Then"
        );
        assert_eq!(apply("Uh, um, okay"), "Okay");
    }

    #[test]
    fn line_breaks_are_kept() {
        assert_eq!(apply("First line.\nUm, second"), "First line.\nSecond");
        assert_eq!(apply("one um\ntwo"), "one\ntwo");
    }

    #[test]
    fn other_words_are_left_alone() {
        for text in [
            "The umbrella is ahead of the error.",
            "Uhura said uh-huh.",
            "Take him to the ER.",
            "UM is a university.",
            "drum, Ahab, ermine",
            "Plain text, nothing to do.",
            "",
        ] {
            assert_eq!(apply(text), text);
        }
    }
}
