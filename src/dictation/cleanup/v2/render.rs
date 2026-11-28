//! The cleaned line: the words as heard with the decoded deletions taken
//! out, the tagged spans written, and the edges of each gap repaired.
//!
//! Untouched words keep the recogniser's own punctuation and capitals
//! unless the model's case and punctuation heads are asked to rewrite them.
//! Around a deletion a promoted sentence start is capitalized, a dangling
//! leading comma is dropped, and a deleted word's sentence mark moves onto
//! the word before it. If every word would go, the line stays as heard.
//!
//! A line-for-line port of the reference's `apply_cleanup` (`itn.py`),
//! held to it by `parity_tests`: 600 recorded cases across every tag.

use std::sync::LazyLock;

use regex::Regex;

use super::decode::{Action, Case, Decode, Span, Tag, guard_corrections};
use super::itn::{self, Kind, format_with_punctuation, norm};

static ENDS_IN_PUNCTUATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[,.?!;:]$").expect("a fixed pattern compiles"));
static PATH_LIKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[@/:_]").expect("a fixed pattern compiles"));

const CLOSERS: &[char] = &['"', '\'', '(', ')', '[', ']', '}', '\u{201d}', '\u{2019}'];
const DANGLING: &[char] = &[',', ';', ':', '-', '\u{2014}', '\u{2013}'];
const TRAILING: &[char] = &[',', '.', '?', '!', ';', ':'];

/// The sentence mark a word ends with, behind any closing quote or bracket.
fn terminal(word: &str) -> Option<char> {
    word.trim_end_matches(CLOSERS)
        .chars()
        .next_back()
        .filter(|last| ".?!".contains(*last))
}

fn ends_sentence(word: &str) -> bool {
    terminal(word).is_some()
}

fn capitalize_first_letter(word: &str) -> String {
    match word.char_indices().find(|(_, ch)| ch.is_alphabetic()) {
        Some((at, ch)) if !ch.is_uppercase() => {
            let mut out = String::with_capacity(word.len() + 2);
            out.push_str(&word[..at]);
            out.extend(ch.to_uppercase());
            out.push_str(&word[at + ch.len_utf8()..]);
            out
        }
        _ => word.to_owned(),
    }
}

/// A kept word rewritten by the case and punctuation heads.
fn recased(word: &str, tag: Tag) -> String {
    let core = word.trim_end_matches(TRAILING);
    let mut out = match tag.case {
        Case::Keep => core.to_owned(),
        Case::Lower => core.to_lowercase(),
        Case::Upper => core.to_uppercase(),
        Case::Title => {
            let mut chars = core.chars();
            match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.as_str().to_lowercase().chars())
                    .collect(),
                None => String::new(),
            }
        }
    };
    out.push_str(tag.punct.text());
    out
}

/// A tagged span starting at `start`, formatted, and how many words it
/// took: the span's own words, or a span the model split in two and this
/// joins ("twenty" + "five milligrams", "eighteen hundred" + "and" + "fifty
/// dollars", "eight" + "twenty one"). `out` is what was kept so far: a link
/// or address may take its last word back ("notebook.app" + "slash new").
fn span_at(
    words: &[&str],
    actions: &[Action],
    spans: &[Span],
    start: usize,
    kind: Kind,
    out: &mut Vec<String>,
) -> Option<(String, usize)> {
    let count = words.len();
    let live = |index: usize| index < count && !actions[index].is_delete();
    let run_from = |from: usize, inside: Span| {
        let mut end = from;
        while live(end) && spans[end] == inside {
            end += 1;
        }
        end
    };
    let end = run_from(start + 1, Span::Inside(kind));
    let formatted = |kind: Kind, to: usize| format_with_punctuation(kind, &words[start..to]);
    let open_end = |index: usize| !ENDS_IN_PUNCTUATION.is_match(words[index - 1]);

    if matches!(kind, Kind::Number | Kind::Unit)
        && live(end)
        && matches!(
            spans[end],
            Span::Begin(Kind::Unit) | Span::Inside(Kind::Unit)
        )
    {
        let to = run_from(end + 1, Span::Inside(Kind::Unit));
        if let Some(piece) = formatted(Kind::Unit, to) {
            return Some((piece, to - start));
        }
    }
    if kind == Kind::Number
        && end + 1 < count
        && live(end)
        && spans[end] == Span::Outside
        && norm(words[end]) == "and"
        && open_end(end)
        && live(end + 1)
        && matches!(
            spans[end + 1],
            Span::Begin(Kind::Number) | Span::Inside(Kind::Number)
        )
    {
        let to = run_from(end + 2, Span::Inside(Kind::Number));
        if let Some(piece) = formatted(Kind::Number, to) {
            return Some((piece, to - start));
        }
    }
    if kind == Kind::Number
        && spans[start] == Span::Inside(Kind::Number)
        && live(end)
        && spans[end] == Span::Begin(Kind::Number)
        && open_end(end)
    {
        let to = run_from(end + 1, Span::Inside(Kind::Number));
        if let Some(piece) = formatted(Kind::Number, to) {
            return Some((piece, to - start));
        }
    }
    if matches!(kind, Kind::Url | Kind::Email)
        && let Some(previous) = out.last()
        && !ends_sentence(previous)
        && matches!(
            norm(words[start]).as_str(),
            "slash" | "forward" | "dot" | "at"
        )
    {
        let mut tokens: Vec<&str> = vec![previous.as_str()];
        tokens.extend_from_slice(&words[start..end]);
        if let Some(piece) = format_with_punctuation(kind, &tokens) {
            out.pop();
            return Some((piece, end - start));
        }
    }
    if matches!(spans[start], Span::Begin(_)) || end - start >= 2 {
        // Never the tail of a spoken compound: "twenty [six hours]" is not
        // "twenty 6 hours".
        let previous = out.last().map(|word| norm(word)).unwrap_or_default();
        let splits_compound = matches!(kind, Kind::Number | Kind::Unit)
            && itn::is_digit_word(&norm(words[start]))
            && (itn::is_tens(&previous)
                || matches!(previous.as_str(), "hundred" | "thousand" | "million"));
        if !splits_compound && let Some(piece) = formatted(kind, end) {
            return Some((piece, end - start));
        }
    }
    None
}

/// What happens to each word once openers are kept (unless the decode
/// deletes them) and every correction carries its cue.
fn guarded(words: &[&str], tags: &[Tag], decode: &Decode) -> Vec<Action> {
    let mut actions: Vec<Action> = tags
        .iter()
        .map(|tag| match tag.action {
            Action::DeleteOpener if !decode.delete_openers => Action::Keep,
            action => action,
        })
        .collect();
    guard_corrections(words, &mut actions);
    actions
}

/// What happens to each word as [`render`] writes the line. When every
/// word would go, none does.
pub(crate) fn actions(words: &[&str], tags: &[Tag], decode: &Decode) -> Vec<Action> {
    let mut actions = guarded(words, tags, decode);
    if actions.iter().all(|action| action.is_delete()) {
        actions.fill(Action::Keep);
    }
    actions
}

/// The cleaned line for `words` as heard and each word's tag.
pub(crate) fn render(words: &[&str], tags: &[Tag], decode: &Decode) -> String {
    let count = words.len();
    if count == 0 {
        return String::new();
    }
    let actions = guarded(words, tags, decode);
    if actions.iter().all(|action| action.is_delete()) {
        return words.join(" ");
    }
    if actions.iter().all(|action| !action.is_delete())
        && tags
            .iter()
            .all(|tag| tag.span == Span::Outside || !decode.use_itn)
        && !decode.apply_case_punct
    {
        return words.join(" ");
    }
    let spans: Vec<Span> = tags.iter().map(|tag| tag.span).collect();

    let mut out: Vec<String> = Vec::with_capacity(count);
    let mut index = 0;
    let mut after_gap = false;
    let mut gap_terminal: Option<char> = None;
    let mut sentence_start = true;
    while index < count {
        if actions[index].is_delete() {
            after_gap = true;
            if let Some(mark) = terminal(words[index]) {
                gap_terminal = Some(mark);
            }
            index += 1;
            continue;
        }
        let span = spans[index]
            .kind()
            .filter(|_| decode.use_itn)
            .and_then(|kind| span_at(words, &actions, &spans, index, kind, &mut out));
        if let Some((mut piece, taken)) = span {
            if sentence_start && after_gap {
                piece = piece.trim_start_matches(DANGLING).to_owned();
            }
            if !piece.is_empty() {
                sentence_start = ends_sentence(&piece);
                out.push(piece);
            }
            after_gap = false;
            gap_terminal = None;
            index += taken;
            continue;
        }
        let mut word = words[index].to_owned();
        if decode.apply_case_punct {
            word = recased(&word, tags[index]);
        } else if after_gap && sentence_start {
            word = word.trim_start_matches(DANGLING).to_owned();
            if !word.is_empty() && !PATH_LIKE.is_match(&word) {
                word = capitalize_first_letter(&word);
            }
        }
        if !word.is_empty() {
            sentence_start = ends_sentence(&word) || word.ends_with('\n');
            out.push(word);
        }
        after_gap = false;
        gap_terminal = None;
        index += 1;
    }
    // "Ship it, um." keeps its period.
    if after_gap
        && let (Some(mark), Some(last)) = (gap_terminal, out.last_mut())
        && !ends_sentence(last)
    {
        let kept = last.trim_end_matches([',', ';', ':']).len();
        last.truncate(kept);
        last.push(mark);
    }
    if out.is_empty() {
        return words.join(" ");
    }
    let mut line = String::new();
    for word in &out {
        line.push_str(word);
        if !(word.ends_with('\n') || word.ends_with("\n- ")) {
            line.push(' ');
        }
    }
    line.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::super::decode::{Punct, Tag};
    use super::*;

    fn tagged(actions: &[Action], spans: &[Span]) -> Vec<Tag> {
        actions
            .iter()
            .zip(spans)
            .map(|(&action, &span)| Tag {
                action,
                span,
                ..Tag::KEEP
            })
            .collect()
    }

    fn cleaned(words: &str, actions: &[Action], spans: &[Span]) -> String {
        let words: Vec<&str> = words.split_whitespace().collect();
        render(&words, &tagged(actions, spans), &Decode::default())
    }

    #[test]
    fn a_deletion_leaves_a_sentence_that_reads_right() {
        use Action::{DeleteFiller as F, Keep as K};
        let o = Span::Outside;
        assert_eq!(
            cleaned(
                "Done. Um, ,next we ship it, uh.",
                &[K, F, K, K, K, K, F],
                &[o; 7]
            ),
            "Done. Next we ship it."
        );
        // Every word would go: the line stays as heard.
        assert_eq!(cleaned("Um, uh.", &[F, F], &[o; 2]), "Um, uh.");
        // An opener stays unless the decode deletes openers.
        let opener = [Action::DeleteOpener, Action::DeleteOpener, K, K];
        assert_eq!(
            cleaned("Okay, so ship it.", &opener, &[o; 4]),
            "Okay, so ship it."
        );
        let words: Vec<&str> = "Okay, so write a function.".split_whitespace().collect();
        let openers = Decode {
            delete_openers: true,
            ..Decode::default()
        };
        let actions = [Action::DeleteOpener, Action::DeleteOpener, K, K, K];
        assert_eq!(
            render(&words, &tagged(&actions, &[o; 5]), &openers),
            "Write a function."
        );
    }

    #[test]
    fn only_tagged_spans_are_written_and_split_spans_are_joined() {
        use Kind::{Email, Number, Unit, Url};
        use Span::{Begin as B, Inside as I, Outside as O};
        let keep = |count| vec![Action::Keep; count];
        assert_eq!(
            cleaned(
                "Latency is two hundred fifty milliseconds, not two hundred.",
                &keep(9),
                &[O, O, B(Unit), I(Unit), I(Unit), I(Unit), O, O, O]
            ),
            "Latency is 250 milliseconds, not two hundred."
        );
        assert_eq!(
            cleaned(
                "Call five five five zero one.",
                &keep(6),
                &[O, I(Number), I(Number), I(Number), I(Number), I(Number)]
            ),
            "Call 55501."
        );
        for middle in [B(Unit), I(Unit)] {
            assert_eq!(
                cleaned(
                    "not twenty five milligrams",
                    &keep(4),
                    &[O, B(Number), middle, I(Unit)]
                ),
                "not 25 milligrams"
            );
        }
        assert_eq!(
            cleaned("from twenty six hours", &keep(4), &[O, O, B(Unit), I(Unit)]),
            "from twenty six hours"
        );
        assert_eq!(
            cleaned(
                "Open notebook.app slash new.",
                &keep(4),
                &[O, O, I(Url), I(Url)]
            ),
            "Open notebook.app/new."
        );
        assert_eq!(
            cleaned(
                "Email ops at northwind.io.",
                &keep(4),
                &[O, O, I(Email), I(Email)]
            ),
            "Email ops@northwind.io."
        );
        assert_eq!(
            cleaned(
                "at eight twenty one.",
                &keep(4),
                &[O, I(Number), B(Number), I(Number)]
            ),
            "at 8:21."
        );
        assert_eq!(
            cleaned(
                "rent is eighteen hundred and fifty dollars,",
                &keep(7),
                &[O, O, B(Number), I(Number), O, B(Number), I(Number)]
            ),
            "rent is $1,850,"
        );
        // Without ITN the words stay as heard.
        let words: Vec<&str> = vec!["two", "hundred"];
        let plain = Decode {
            use_itn: false,
            ..Decode::default()
        };
        assert_eq!(
            render(&words, &tagged(&keep(2), &[B(Number), I(Number)]), &plain),
            "two hundred"
        );
    }

    #[test]
    fn the_case_and_punctuation_heads_rewrite_kept_words_when_asked() {
        let words = ["hello", "WORLD", "how", "are", "you", "ok"];
        let mut tags = vec![Tag::KEEP; 6];
        tags[0].case = Case::Title;
        tags[1].case = Case::Lower;
        tags[1].punct = Punct::Period;
        tags[2].case = Case::Title;
        tags[4].punct = Punct::Question;
        tags[5].case = Case::Upper;
        tags[5].punct = Punct::Bullet;
        let rewrite = Decode {
            apply_case_punct: true,
            ..Decode::default()
        };
        assert_eq!(
            render(&words, &tags, &rewrite),
            "Hello world. How are you? OK\n-"
        );
        assert_eq!(
            render(&words, &tags, &Decode::default()),
            "hello WORLD how are you ok"
        );
    }
}
