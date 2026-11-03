//! Subtitle credits a speech model invents at the end of a clip (#1141).
//! Models trained on subtitled video (Whisper, Cohere Transcribe) learned
//! that broadcast audio ends with a credit line, and on a trailing pause or
//! breath they can append one nobody said. Only a credit at the very end is
//! removed; the same words anywhere else are kept.

/// Credits seen appended to dictations, matched case-insensitively on their
/// letters and digits alone, so punctuation and hyphens may differ.
const CREDITS: &[&str] = &[
    "Sous-titrage Société Radio-Canada",
    "Sous-titrage ST' 501",
    "Sous-titres réalisés par la communauté d'Amara.org",
    "Sous-titres par la communauté d'Amara.org",
    "Subtitles by the Amara.org community",
    "Untertitel der Amara.org-Community",
];

/// Each run of letters or digits in `text`, lowercased, with its byte offset.
fn tokens(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            out.push((begin, text[begin..index].to_lowercase()));
        }
    }
    if let Some(begin) = start {
        out.push((begin, text[begin..].to_lowercase()));
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
    fn counts_letter_and_digit_runs() {
        assert_eq!(token_count("Radio-Canada."), 2);
        assert_eq!(token_count("d'Amara.org"), 3);
        assert_eq!(token_count(" , "), 0);
    }
}
