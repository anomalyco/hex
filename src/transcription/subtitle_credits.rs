//! Subtitle credits a speech model invents at the end of a clip (#1141).
//! Models trained on subtitled video (Whisper, Cohere Transcribe) learned
//! that broadcast audio ends with a credit line, and on a trailing pause or
//! breath they can append one nobody said. Whisper in Turkish adds a thanks
//! for watching, or the subtitle editor's credit (#1770). Only a credit at
//! the very end is removed; the same words anywhere else are kept. The thanks
//! is a real sentence too, so a dictation that genuinely ends with it loses
//! it.

/// Credits seen appended to dictations, matched case-insensitively on their
/// letters and digits alone, so punctuation and hyphens may differ. Turkish
/// `İ` and `I` both fold to `i`: lowercasing leaves a combining dot that is
/// dropped, and the dotless `ı` counts as `i`.
const CREDITS: &[&str] = &[
    "Sous-titrage Société Radio-Canada",
    "Sous-titrage ST' 501",
    "Sous-titres réalisés par la communauté d'Amara.org",
    "Sous-titres par la communauté d'Amara.org",
    "Subtitles by the Amara.org community",
    "Untertitel der Amara.org-Community",
    "İzlediğiniz için teşekkür ederim",
    "Altyazı M.K.",
    "Altyazı M.",
];

/// A combining mark such as the dot Turkish lowercasing leaves behind (`İ`
/// becomes `i` + U+0307).
fn is_combining(character: char) -> bool {
    matches!(character, '\u{0300}'..='\u{036F}')
}

/// `text` lowercased and folded so Turkish case and dot rules do not matter.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|character| !is_combining(*character))
        .map(|character| match character {
            'ı' => 'i',
            other => other,
        })
        .collect()
}

/// Each run of letters or digits in `text`, folded, with its byte offset.
fn tokens(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
        } else if is_combining(character) && start.is_some() {
            // A mark inside a word never splits the token, so the offsets of
            // the tokens after it stay on the original text.
        } else if let Some(begin) = start.take() {
            out.push((begin, fold(&text[begin..index])));
        }
    }
    if let Some(begin) = start {
        out.push((begin, fold(&text[begin..])));
    }
    out
}

/// How many letter-or-digit runs `text` has.
pub(crate) fn token_count(text: &str) -> usize {
    tokens(text).len()
}

/// `text` without the credits it ends with, and how many letter-or-digit
/// runs were removed; `None` when it ends with none.
pub(crate) fn strip(text: &str) -> Option<(String, usize)> {
    let tokens = tokens(text);
    let credits = CREDITS
        .iter()
        .map(|credit| tokens_of(credit))
        .collect::<Vec<_>>();
    let mut end = tokens.len();
    'credits: loop {
        for credit in &credits {
            let length = credit.len();
            if length <= end
                && tokens[end - length..end]
                    .iter()
                    .map(|(_, token)| token)
                    .eq(credit.iter())
            {
                end -= length;
                continue 'credits;
            }
        }
        break;
    }
    if end == tokens.len() {
        return None;
    }
    let kept = text[..tokens[end].0].trim_end_matches(|character: char| {
        character.is_whitespace() || matches!(character, ',' | ';' | ':' | '-' | '–' | '—')
    });
    Some((kept.to_string(), tokens.len() - end))
}

fn tokens_of(text: &str) -> Vec<String> {
    tokens(text).into_iter().map(|(_, token)| token).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_subtitle_credit_is_removed() {
        assert_eq!(
            strip("La réunion est reportée. Sous-titrage Société Radio-Canada"),
            Some(("La réunion est reportée.".into(), 5))
        );
        assert_eq!(
            strip("on se voit demain, sous-titrage société radio canada."),
            Some(("on se voit demain".into(), 5))
        );
        assert_eq!(
            strip("Merci. Sous-titres réalisés par la communauté d'Amara.org"),
            Some(("Merci.".into(), 9))
        );
    }

    #[test]
    fn a_credit_alone_leaves_nothing() {
        assert_eq!(
            strip(" Sous-titrage Société Radio-Canada "),
            Some((String::new(), 5))
        );
    }

    #[test]
    fn speech_without_a_trailing_credit_is_kept() {
        assert_eq!(strip("Bonjour à tous."), None);
        assert_eq!(
            strip("Sous-titrage Société Radio-Canada, c'est ce qu'on voit à la fin."),
            None
        );
        assert_eq!(strip("J'écoute Radio-Canada"), None);
        assert_eq!(strip(""), None);
    }

    #[test]
    fn a_turkish_thanks_for_watching_is_removed() {
        assert_eq!(
            strip("Toplantı yarına kaldı. İzlediğiniz için teşekkür ederim."),
            Some(("Toplantı yarına kaldı.".into(), 4))
        );
        // The dotted capital as a plain `I`, and all lowercase.
        assert_eq!(
            strip("Toplantı yarına kaldı. Izlediğiniz için teşekkür ederim"),
            Some(("Toplantı yarına kaldı.".into(), 4))
        );
        assert_eq!(
            strip("Toplantı yarına kaldı. izlediğiniz için teşekkür ederim"),
            Some(("Toplantı yarına kaldı.".into(), 4))
        );
        assert_eq!(
            strip("Toplantı yarına kaldı. İZLEDİĞİNİZ İÇİN TEŞEKKÜR EDERİM"),
            Some(("Toplantı yarına kaldı.".into(), 4))
        );
    }

    #[test]
    fn a_truncated_turkish_credit_is_removed() {
        assert_eq!(
            strip("Toplantı yarına kaldı. Altyazı M.K."),
            Some(("Toplantı yarına kaldı.".into(), 3))
        );
        assert_eq!(
            strip("Toplantı yarına kaldı. Altyazı M."),
            Some(("Toplantı yarına kaldı.".into(), 2))
        );
        assert_eq!(
            strip("Toplantı yarına kaldı. ALTYAZI M.K."),
            Some(("Toplantı yarına kaldı.".into(), 3))
        );
    }

    #[test]
    fn the_same_words_mid_sentence_are_kept() {
        assert_eq!(
            strip("İzlediğiniz için teşekkür ederim demek istemedim."),
            None
        );
        assert_eq!(strip("Altyazı M.K. yazısını gördüm ama okumadım."), None);
    }

    #[test]
    fn a_combining_mark_inside_a_word_does_not_split_it() {
        // `İ` written as `I` plus a combining dot, and a decomposed accent:
        // the marks stay inside the word, so tokens and their byte offsets do.
        assert_eq!(token_count("I\u{0307}zledig\u{0306}iniz"), 1);
        assert_eq!(token_count("cafe\u{0301}"), 1);
        assert_eq!(
            strip("cafe\u{0301}. Altyazı M.K."),
            Some(("cafe\u{0301}.".into(), 3))
        );
    }

    #[test]
    fn counts_letter_and_digit_runs() {
        assert_eq!(token_count("Radio-Canada."), 2);
        assert_eq!(token_count("d'Amara.org"), 3);
        assert_eq!(token_count("İzlediğiniz için teşekkür ederim"), 4);
        assert_eq!(token_count("Altyazı M.K."), 3);
        assert_eq!(token_count(" , "), 0);
    }
}
