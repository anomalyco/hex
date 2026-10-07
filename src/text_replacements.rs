use std::sync::OnceLock;

use regex::{Regex, RegexBuilder};

use crate::app_settings::TextReplacement;

#[derive(Clone)]
struct CompiledRule {
    matcher: Regex,
    output: String,
}

struct Candidate {
    start: usize,
    end: usize,
    rule: usize,
}

#[derive(Clone, Default)]
pub(crate) struct ReplacementSet {
    rules: Vec<CompiledRule>,
}

impl ReplacementSet {
    pub(crate) fn new(rules: &[TextReplacement]) -> Self {
        let rules = rules
            .iter()
            .filter_map(|rule| {
                let matched_phrase = rule.matched_phrase.trim();
                if matched_phrase.is_empty() {
                    return None;
                }
                RegexBuilder::new(&regex::escape(matched_phrase))
                    .case_insensitive(true)
                    .unicode(true)
                    .build()
                    .ok()
                    .map(|matcher| CompiledRule {
                        matcher,
                        output: rule.output.clone(),
                    })
            })
            .collect();
        Self { rules }
    }

    pub(crate) fn replace(&self, text: &str) -> String {
        let mut candidates = self
            .rules
            .iter()
            .enumerate()
            .flat_map(|(rule, replacement)| {
                replacement
                    .matcher
                    .find_iter(text)
                    .filter(|matched| phrase_boundary(text, matched.start(), matched.end()))
                    .map(move |matched| Candidate {
                        start: matched.start(),
                        end: matched.end(),
                        rule,
                    })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            (right.end - right.start)
                .cmp(&(left.end - left.start))
                .then_with(|| left.start.cmp(&right.start))
                .then_with(|| left.rule.cmp(&right.rule))
        });
        let mut selected = Vec::<Candidate>::new();
        for candidate in candidates {
            if selected
                .iter()
                .any(|existing| candidate.start < existing.end && existing.start < candidate.end)
            {
                continue;
            }
            selected.push(candidate);
        }
        if selected.is_empty() {
            return text.into();
        }
        selected.sort_by_key(|candidate| candidate.start);

        let mut output = String::with_capacity(text.len());
        let mut cursor = 0;
        let mut capitalize_next = false;
        for (index, candidate) in selected.iter().enumerate() {
            if cursor < candidate.start {
                push_segment(
                    &mut output,
                    &text[cursor..candidate.start],
                    &mut capitalize_next,
                );
            }
            let replacement = &self.rules[candidate.rule].output;
            if !replacement.is_empty() {
                push_segment(&mut output, replacement, &mut capitalize_next);
                cursor = candidate.end;
                continue;
            }
            let next_limit = selected
                .get(index + 1)
                .map_or(text.len(), |next| next.start);
            let between = &text[candidate.end..next_limit];
            if at_sentence_start(&output) {
                if text[candidate.start..candidate.end]
                    .chars()
                    .find(|character| character.is_alphabetic())
                    .is_some_and(char::is_uppercase)
                {
                    capitalize_next = true;
                }
                let after_punct = between.trim_start_matches(CLAUSE_PUNCTUATION);
                let after_spaces = after_punct.trim_start_matches(HORIZONTAL_WHITESPACE);
                cursor = candidate.end + (between.len() - after_spaces.len());
                if (after_spaces.is_empty() && next_limit == text.len())
                    || after_spaces.starts_with('\n')
                {
                    trim_trailing_horizontal_spaces(&mut output);
                }
            } else {
                let remaining = text[candidate.end..].trim_start_matches(HORIZONTAL_WHITESPACE);
                if between.starts_with(CLAUSE_PUNCTUATION)
                    || remaining.is_empty()
                    || remaining.starts_with('\n')
                {
                    trim_trailing_horizontal_spaces(&mut output);
                    if output.ends_with(',') {
                        output.pop();
                    }
                    if remaining.is_empty() || remaining.starts_with('\n') {
                        let after_spaces = between.trim_start_matches(HORIZONTAL_WHITESPACE);
                        cursor = candidate.end + (between.len() - after_spaces.len());
                    } else {
                        cursor = candidate.end;
                    }
                } else if output.ends_with(HORIZONTAL_WHITESPACE) {
                    let after_spaces = between.trim_start_matches(HORIZONTAL_WHITESPACE);
                    cursor = candidate.end + (between.len() - after_spaces.len());
                } else {
                    cursor = candidate.end;
                }
            }
        }
        if cursor < text.len() {
            push_segment(&mut output, &text[cursor..], &mut capitalize_next);
        }
        output
    }
}

const HORIZONTAL_WHITESPACE: [char; 2] = [' ', '\t'];
const CLAUSE_PUNCTUATION: [char; 6] = ['.', '!', '?', ',', ';', ':'];
const SENTENCE_ENDINGS: [char; 4] = ['.', '!', '?', '\n'];

fn at_sentence_start(output: &str) -> bool {
    let trimmed = output.trim_end_matches(HORIZONTAL_WHITESPACE);
    trimmed.is_empty() || trimmed.ends_with(SENTENCE_ENDINGS)
}

fn trim_trailing_horizontal_spaces(output: &mut String) {
    let trimmed_len = output.trim_end_matches(HORIZONTAL_WHITESPACE).len();
    output.truncate(trimmed_len);
}

fn push_segment(output: &mut String, segment: &str, capitalize_next: &mut bool) {
    if !*capitalize_next || segment.is_empty() {
        output.push_str(segment);
        if segment.chars().any(|character| !character.is_whitespace()) {
            *capitalize_next = false;
        }
        return;
    }
    let Some((index, character)) = segment
        .char_indices()
        .find(|(_, character)| !character.is_whitespace())
    else {
        output.push_str(segment);
        return;
    };
    output.push_str(&segment[..index]);
    if character.is_lowercase() && at_sentence_start(output) {
        output.extend(character.to_uppercase());
    } else {
        output.push(character);
    }
    output.push_str(&segment[index + character.len_utf8()..]);
    *capitalize_next = false;
}

fn phrase_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(word_character) && !after.is_some_and(word_character)
}

fn word_character(character: char) -> bool {
    static WORD_CHARACTER: OnceLock<Regex> = OnceLock::new();
    let mut encoded = [0; 4];
    WORD_CHARACTER
        .get_or_init(|| Regex::new(r"^\w$").expect("Unicode word regex must compile"))
        .is_match(character.encode_utf8(&mut encoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(matched_phrase: &str, output: &str) -> TextReplacement {
        TextReplacement {
            matched_phrase: matched_phrase.into(),
            output: output.into(),
        }
    }

    #[test]
    fn replaces_globally_on_unicode_phrase_boundaries_without_disturbing_punctuation() {
        let replacements = ReplacementSet::new(&[
            rule("cafe au lait", "café au lait"),
            rule("open code", "OpenCode"),
            rule("über", "Über"),
        ]);

        assert_eq!(
            replacements.replace("OPEN CODE, open code. café au lait; cafe au lait! ÜBER."),
            "OpenCode, OpenCode. café au lait; café au lait! Über."
        );
        assert_eq!(
            replacements.replace("reopen code, open codes, übercool"),
            "reopen code, open codes, übercool"
        );
    }

    #[test]
    fn longest_match_wins_and_replacements_do_not_recurse() {
        let replacements = ReplacementSet::new(&[
            rule("open", "closed"),
            rule("open code", "OpenCode"),
            rule("code then open", "combined"),
            rule("closed", "shut"),
        ]);

        assert_eq!(
            replacements.replace("open code then open"),
            "closed combined"
        );
    }

    #[test]
    fn outputs_do_not_trigger_other_rules_during_the_same_pass() {
        let replacements = ReplacementSet::new(&[
            rule("open code", "OpenCode"),
            rule("alpha", "beta release"),
            rule("beta", "gamma"),
        ]);

        let corrected = replacements.replace("OPEN CODE alpha");
        assert_eq!(corrected, "OpenCode beta release");
        assert_eq!(replacements.replace("beta release"), "gamma release");
    }

    #[test]
    fn duplicate_phrases_use_settings_order_deterministically() {
        let replacements = ReplacementSet::new(&[rule("hex", "first"), rule("HEX", "second")]);

        assert_eq!(replacements.replace("hex"), "first");
    }

    #[test]
    fn empty_replacements_delete_phrases_and_collapse_surrounding_spacing_and_punctuation() {
        let replacements =
            ReplacementSet::new(&[rule("um", ""), rule("uh", ""), rule("thank you", "")]);

        assert_eq!(replacements.replace("I um think so."), "I think so.");
        assert_eq!(replacements.replace("I, um, think so."), "I, think so.");
        assert_eq!(replacements.replace("I, um, uh, think so."), "I, think so.");
        assert_eq!(
            replacements.replace("Um, the build passed."),
            "The build passed."
        );
        assert_eq!(
            replacements.replace("Um, uh, the build passed."),
            "The build passed."
        );
        assert_eq!(
            replacements.replace("um, the build passed."),
            "the build passed."
        );
        assert_eq!(replacements.replace("I think so um."), "I think so.");
        assert_eq!(replacements.replace("I think so, um."), "I think so.");
        assert_eq!(replacements.replace("I think so, um"), "I think so");
        assert_eq!(replacements.replace("Thank you."), "");
        assert_eq!(replacements.replace("Hello. Thank you."), "Hello.");
        assert_eq!(
            replacements.replace("Hello. Thank you. The build passed."),
            "Hello. The build passed."
        );
    }
}
