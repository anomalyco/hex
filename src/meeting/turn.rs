use super::TranscriptEntry;

/// Adjacent entries on the same track from the same speaker within this
/// pause form one turn.
pub(crate) const TURN_GAP_MS: u64 = 3_000;

/// Approximate per-turn prefix overhead (`[mm:ss · HH:MM:SS] Them: `) used
/// when budgeting rendered transcript pages by character count.
const TURN_PREFIX_CHARS: usize = 26;

/// Limits applied while coalescing adjacent same-speaker transcript entries.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TurnOptions {
    /// Start a new turn rather than merging when adding an entry would exceed
    /// this many whitespace-delimited words.
    pub max_words: Option<usize>,
    /// Stop accepting further entries once at least one turn exists and adding
    /// the next entry would exceed this character budget.
    pub max_chars: Option<usize>,
}

/// Coalesced turns plus the index of the first entry excluded by
/// `TurnOptions::max_chars`, if any.
pub(crate) struct CoalescedTurns {
    pub turns: Vec<TranscriptEntry>,
    pub stopped_at: Option<usize>,
}

/// Coalesces adjacent same-speaker entries within `TURN_GAP_MS` (3 s) using
/// the canonical spacing/punctuation join rules.
pub fn coalesce_transcript(
    entries: impl IntoIterator<Item = TranscriptEntry>,
) -> Vec<TranscriptEntry> {
    coalesce_with_options(entries, TurnOptions::default()).turns
}

/// Coalesces adjacent same-speaker entries within `TURN_GAP_MS` (3 s),
/// optionally bounding words per turn or total rendered characters.
pub(crate) fn coalesce_with_options(
    entries: impl IntoIterator<Item = TranscriptEntry>,
    options: TurnOptions,
) -> CoalescedTurns {
    let mut turns: Vec<TranscriptEntry> = Vec::new();
    let mut current_words: usize = 0;
    let mut used_chars: usize = 0;
    let mut stopped_at = None;

    for (index, mut entry) in entries.into_iter().enumerate() {
        let trimmed = entry.text.trim();
        let entry_words = trimmed.split_whitespace().count();
        let cost = trimmed.len() + 1;
        if let Some(max_chars) = options.max_chars
            && !turns.is_empty()
            && used_chars + cost > max_chars
        {
            stopped_at = Some(index);
            break;
        }
        let can_merge = turns.last().is_some_and(|turn| {
            turn.source == entry.source
                && turn.speaker == entry.speaker
                && entry.start_ms.saturating_sub(turn.end_ms) <= TURN_GAP_MS
                && options
                    .max_words
                    .is_none_or(|limit| current_words + entry_words <= limit)
        });
        if can_merge {
            let turn = turns.last_mut().expect("checked above");
            append_text(&mut turn.text, trimmed);
            turn.end_ms = turn.end_ms.max(entry.end_ms);
            current_words += entry_words;
            used_chars += cost;
        } else {
            entry.text = trimmed.to_string();
            turns.push(entry);
            current_words = entry_words;
            used_chars += cost + TURN_PREFIX_CHARS;
        }
    }

    CoalescedTurns { turns, stopped_at }
}

fn append_text(target: &mut String, addition: &str) {
    let addition = addition.trim();
    if addition.is_empty() {
        return;
    }
    if target.is_empty() {
        target.push_str(addition);
        return;
    }
    let needs_space = target
        .chars()
        .next_back()
        .is_some_and(|ch| !ch.is_whitespace() && !is_opening_punctuation(ch))
        && addition
            .chars()
            .next()
            .is_some_and(|ch| !ch.is_whitespace() && !is_closing_punctuation(ch));
    if needs_space {
        target.push(' ');
    }
    target.push_str(addition);
}

fn is_opening_punctuation(ch: char) -> bool {
    matches!(ch, '(' | '[' | '{')
}

fn is_closing_punctuation(ch: char) -> bool {
    matches!(ch, ',' | '.' | '?' | '!' | ';' | ':' | ')' | ']' | '}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::MeetingSource;

    fn entry(source: MeetingSource, start_ms: u64, end_ms: u64, text: &str) -> TranscriptEntry {
        TranscriptEntry::new(source, start_ms, end_ms, text)
    }

    /// One track carries everyone remote, so a turn ends where the speaker
    /// changes: between two named people, and between a named person and
    /// speech nobody was named for.
    #[test]
    fn a_turn_splits_where_the_speaker_changes() {
        let mut speakers = crate::meeting::Speakers::default();
        let from = crate::meeting::NameSource::CallApp;
        let mut add = |name| {
            let name = crate::meeting::SpeakerName::parse(name).unwrap();
            Some(speakers.add(name, from))
        };
        let priya = add("Priya Nair");
        let marcus = add("Marcus Webb");
        let said = |speaker: &Option<crate::meeting::SpeakerId>, start_ms, text| TranscriptEntry {
            speaker: speaker.clone(),
            ..entry(MeetingSource::System, start_ms, start_ms + 900, text)
        };
        let turns = coalesce_transcript([
            said(&priya, 0, "Thursday works"),
            said(&priya, 1_000, "if the migration lands."),
            said(&None, 2_000, "Yeah."),
            said(&marcus, 3_000, "I can take it."),
            said(&priya, 4_000, "Thanks."),
        ]);
        let heard = turns
            .iter()
            .map(|turn| (turn.speaker.clone(), turn.text.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            heard,
            [
                (priya.clone(), "Thursday works if the migration lands."),
                (None, "Yeah."),
                (marcus, "I can take it."),
                (priya, "Thanks."),
            ]
        );
    }

    #[test]
    fn coalesces_within_three_seconds_and_splits_after() {
        let turns = coalesce_transcript([
            entry(MeetingSource::Microphone, 0, 1_000, "First."),
            entry(MeetingSource::Microphone, 4_000, 5_000, "At boundary."),
            entry(MeetingSource::Microphone, 8_001, 9_000, "Past boundary."),
            entry(MeetingSource::System, 9_500, 10_000, "Other speaker."),
        ]);
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].text, "First. At boundary.");
        assert_eq!(turns[0].start_ms, 0);
        assert_eq!(turns[0].end_ms, 5_000);
        assert_eq!(turns[1].text, "Past boundary.");
        assert_eq!(turns[2].text, "Other speaker.");
    }

    #[test]
    fn trims_whitespace_and_avoids_double_or_dangling_spaces() {
        let turns = coalesce_transcript([
            entry(MeetingSource::Microphone, 0, 500, "  First.  "),
            entry(MeetingSource::Microphone, 600, 1_000, "   "),
            entry(MeetingSource::Microphone, 1_100, 1_500, "\tSecond.\n"),
        ]);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].text, "First. Second.");
        assert_eq!(turns[0].end_ms, 1_500);

        let leading_empty = coalesce_transcript([
            entry(MeetingSource::System, 0, 400, "   "),
            entry(MeetingSource::System, 500, 1_000, "  Hello. "),
        ]);
        assert_eq!(leading_empty.len(), 1);
        assert_eq!(leading_empty[0].text, "Hello.");
    }

    #[test]
    fn joins_punctuation_without_spurious_spaces() {
        let turns = coalesce_transcript([
            entry(MeetingSource::System, 0, 500, "Hello"),
            entry(MeetingSource::System, 600, 1_000, ", world"),
            entry(MeetingSource::System, 1_100, 1_500, "!"),
            entry(MeetingSource::System, 1_600, 2_000, "("),
            entry(MeetingSource::System, 2_100, 2_500, "aside"),
            entry(MeetingSource::System, 2_600, 3_000, ")"),
            entry(MeetingSource::System, 3_100, 3_500, "Next sentence."),
        ]);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].text, "Hello, world! (aside) Next sentence.");
    }

    #[test]
    fn respects_max_words_cap_without_dropping_entries() {
        let result = coalesce_with_options(
            [
                entry(MeetingSource::Microphone, 0, 500, "one two three"),
                entry(MeetingSource::Microphone, 600, 1_000, "four five"),
                entry(MeetingSource::Microphone, 1_100, 1_500, "six seven"),
            ],
            TurnOptions {
                max_words: Some(5),
                max_chars: None,
            },
        );
        assert_eq!(result.stopped_at, None);
        assert_eq!(result.turns.len(), 2);
        assert_eq!(result.turns[0].text, "one two three four five");
        assert_eq!(result.turns[1].text, "six seven");
    }

    #[test]
    fn respects_max_chars_budget_and_reports_cursor_index() {
        let result = coalesce_with_options(
            [
                entry(MeetingSource::System, 0, 500, "First line here."),
                entry(MeetingSource::System, 600, 1_000, "Second line here."),
                entry(MeetingSource::System, 1_100, 1_500, "Third line overflows."),
            ],
            TurnOptions {
                max_words: None,
                max_chars: Some(65),
            },
        );
        assert_eq!(result.turns.len(), 1);
        assert_eq!(result.turns[0].text, "First line here. Second line here.");
        assert_eq!(result.stopped_at, Some(2));
    }
}
