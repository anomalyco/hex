//! Labs › Spoken formatting: whole spoken phrases ("new line", "line
//! break", "new paragraph", "open quote", "close quote") become line breaks
//! and quotation marks, in English and Spanish ("nuevo párrafo", "nueva
//! línea", "abre comillas", …). Matching ignores case and Spanish accents
//! and needs word boundaries, so "newline" or "new liner" stay as they are.
//! The commas, periods, and spaces a model puts around a command go with it.

#[derive(Clone, Copy)]
enum Command {
    Break(&'static str),
    OpenQuote,
    CloseQuote,
}

const COMMANDS: &[(&[&str], Command)] = &[
    (&["new", "paragraph"], Command::Break("\n\n")),
    (&["new", "line"], Command::Break("\n")),
    (&["line", "break"], Command::Break("\n")),
    (&["open", "quote"], Command::OpenQuote),
    (&["close", "quote"], Command::CloseQuote),
    (&["nuevo", "parrafo"], Command::Break("\n\n")),
    (&["nueva", "linea"], Command::Break("\n")),
    (&["salto", "de", "linea"], Command::Break("\n")),
    (&["abrir", "comillas"], Command::OpenQuote),
    (&["abre", "comillas"], Command::OpenQuote),
    (&["cerrar", "comillas"], Command::CloseQuote),
    (&["cierra", "comillas"], Command::CloseQuote),
];

/// `c` lowercased with Spanish accents dropped, as the phrase lists are
/// written: "Párrafo" reads as "parrafo".
pub(super) fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    let lower = match (lower.next(), lower.next()) {
        (Some(single), None) => single,
        _ => c,
    };
    match lower {
        'á' => 'a',
        'é' => 'e',
        'í' => 'i',
        'ó' => 'o',
        'ú' | 'ü' => 'u',
        other => other,
    }
}

/// `transcript` with its spoken formatting phrases applied.
pub fn apply(transcript: &str) -> String {
    let chars: Vec<char> = transcript.chars().collect();
    let mut out = String::with_capacity(transcript.len());
    let mut index = 0;
    while index < chars.len() {
        let word_start = index == 0 || !chars[index - 1].is_alphanumeric();
        if word_start && let Some((command, end)) = command_at(&chars, index) {
            index = emit(&mut out, command, &chars, end);
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// The command whose words start at `start`, and where they end.
fn command_at(chars: &[char], start: usize) -> Option<(Command, usize)> {
    COMMANDS.iter().find_map(|(words, command)| {
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
                    Some(&c) if fold(c) == expected => position += 1,
                    _ => return None,
                }
            }
        }
        let word_end = chars
            .get(position)
            .is_none_or(|next| !next.is_alphanumeric());
        word_end.then_some((*command, position))
    })
}

fn is_space(c: char) -> bool {
    c.is_whitespace() && c != '\n'
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

fn skip(chars: &[char], mut index: usize, also: &[char]) -> usize {
    while chars
        .get(index)
        .is_some_and(|&c| is_space(c) || also.contains(&c))
    {
        index += 1;
    }
    index
}

/// Writes `command` and returns where the text continues.
fn emit(out: &mut String, command: Command, chars: &[char], end: usize) -> usize {
    match command {
        Command::Break(breaks) => {
            trim_end(out, &[',', ';']);
            out.push_str(breaks);
            skip(chars, end, &[',', '.', ';', ':'])
        }
        Command::OpenQuote => {
            trim_end(out, &[]);
            if out
                .chars()
                .next_back()
                .is_some_and(|last| !matches!(last, '\n' | '(' | '[' | '"'))
            {
                out.push(' ');
            }
            out.push('"');
            skip(chars, end, &[',', '.'])
        }
        Command::CloseQuote => {
            trim_end(out, &[',']);
            out.push('"');
            let next = skip(chars, end, &[]);
            if chars.get(next).is_some_and(|c| c.is_alphanumeric()) {
                out.push(' ');
            }
            next
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply;

    #[test]
    fn breaks_replace_their_phrases_and_the_punctuation_around_them() {
        assert_eq!(apply("Hello. New line. Next"), "Hello.\nNext");
        assert_eq!(apply("One. New paragraph. Two."), "One.\n\nTwo.");
        assert_eq!(apply("one, line break, two"), "one\ntwo");
        assert_eq!(apply("a NEW LINE new line b"), "a\n\nb");
    }

    #[test]
    fn quotes_become_quotation_marks() {
        assert_eq!(
            apply("He said open quote hi close quote."),
            "He said \"hi\"."
        );
        assert_eq!(
            apply("He said, Open quote, hello, close quote, and left."),
            "He said, \"hello\", and left."
        );
        assert_eq!(apply("open quote yes close quote then"), "\"yes\" then");
    }

    #[test]
    fn spanish_phrases_work_with_or_without_accents_and_in_any_case() {
        assert_eq!(apply("Hola. Nuevo párrafo. Adiós."), "Hola.\n\nAdiós.");
        assert_eq!(apply("Hola. nuevo parrafo. Adiós."), "Hola.\n\nAdiós.");
        assert_eq!(apply("uno, nueva línea, dos"), "uno\ndos");
        assert_eq!(apply("uno NUEVA LINEA dos"), "uno\ndos");
        assert_eq!(apply("uno salto de línea dos"), "uno\ndos");
        assert_eq!(
            apply("dijo abre comillas hola cierra comillas"),
            "dijo \"hola\""
        );
        assert_eq!(
            apply("Dijo Abrir Comillas sí Cerrar Comillas y se fue"),
            "Dijo \"sí\" y se fue"
        );
        assert_eq!(apply("Hola. New paragraph. Adiós."), "Hola.\n\nAdiós.");
    }

    #[test]
    fn other_words_are_left_alone() {
        for text in [
            "comillas",
            "una línea lineal",
            "dos parrafos nuevos",
            "nuevo parrafos",
            "The newline character",
            "a new liner",
            "renew line items",
            "line breaker",
            "open quotes",
            "Plain text, nothing to do.",
            "",
        ] {
            assert_eq!(apply(text), text);
        }
    }
}
