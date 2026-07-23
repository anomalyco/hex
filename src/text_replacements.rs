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
        selected.sort_by_key(|candidate| candidate.start);

        let mut output = String::with_capacity(text.len());
        let mut cursor = 0;
        for candidate in selected {
            output.push_str(&text[cursor..candidate.start]);
            output.push_str(&self.rules[candidate.rule].output);
            cursor = candidate.end;
        }
        if cursor == 0 {
            return text.into();
        }
        output.push_str(&text[cursor..]);
        output
    }
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
}
