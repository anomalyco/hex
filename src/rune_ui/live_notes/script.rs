//! The canned meeting the live notes stories play: a nine-minute game
//! studio call in four topics (`tests/fixtures/live-notes-replay.ndjson`,
//! from #201), with what an ideal pass would find in it: each topic's name
//! and line, and its decisions, to-dos, and questions with the lines they
//! came from. The pipeline decides when each is found.

use std::sync::OnceLock;

use serde::Deserialize;

const TRANSCRIPT: &str = include_str!("../../../tests/fixtures/live-notes-replay.ndjson");

#[derive(Clone, Debug)]
pub(super) struct Line {
    pub(super) mine: bool,
    pub(super) start_ms: u64,
    pub(super) end_ms: u64,
    pub(super) text: String,
    /// The topic it belongs to, which a pass's "model" reads.
    pub(super) topic: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Topic {
    pub(super) name: &'static str,
    /// Its first line, where the boundary pass after Stop moves a chapter.
    pub(super) first_line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NoteKind {
    Point,
    Decision,
    Action,
    Question,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NoteSpec {
    pub(super) kind: NoteKind,
    pub(super) text: &'static str,
    pub(super) owner: Option<&'static str>,
    /// The transcript lines it came from; the last one dates it.
    pub(super) lines: &'static [usize],
}

pub(super) struct Script {
    pub(super) lines: Vec<Line>,
    pub(super) topics: &'static [Topic],
    pub(super) notes: &'static [NoteSpec],
    /// The title before chapters extend it: the call's.
    pub(super) base_title: &'static str,
    pub(super) summary: &'static str,
}

impl Script {
    /// When the call ends: a few seconds after its last line.
    pub(super) fn end_ms(&self) -> u64 {
        self.lines.last().map_or(0, |line| line.end_ms) + 4_000
    }

    pub(super) fn topic_start(&self, topic: usize) -> u64 {
        self.lines[self.topics[topic].first_line].start_ms
    }

    pub(super) fn note_at(&self, note: usize) -> u64 {
        let spec = &self.notes[note];
        spec.lines
            .last()
            .map_or(0, |&line| self.lines[line].start_ms)
    }

    pub(super) fn words_after(&self, covered_ms: u64, until_ms: u64) -> usize {
        self.lines
            .iter()
            .filter(|line| line.end_ms > covered_ms && line.end_ms <= until_ms)
            .map(|line| line.text.split_whitespace().count())
            .sum()
    }
}

const TOPICS: &[Topic] = &[
    Topic {
        name: "Lantern onboarding",
        first_line: 0,
    },
    Topic {
        name: "Lobby host migration",
        first_line: 19,
    },
    Topic {
        name: "Cave lighting performance",
        first_line: 39,
    },
    Topic {
        name: "Launch date and trailer",
        first_line: 59,
    },
];

const NOTES: &[NoteSpec] = &[
    NoteSpec {
        kind: NoteKind::Decision,
        text: "Lantern-only first room; mirrors wait until room three",
        owner: None,
        lines: &[4, 5, 10],
    },
    NoteSpec {
        kind: NoteKind::Action,
        text: "Rebuild the first room and send Friday's playtest build",
        owner: Some("You"),
        lines: &[11],
    },
    NoteSpec {
        kind: NoteKind::Question,
        text: "Do co-op players share one lantern? Test both on Friday",
        owner: None,
        lines: &[12, 13, 14],
    },
    NoteSpec {
        kind: NoteKind::Decision,
        text: "Snapshot only quantized mirror angles and lantern positions",
        owner: None,
        lines: &[25, 26, 29, 30, 31],
    },
    NoteSpec {
        kind: NoteKind::Action,
        text: "Rewrite the host snapshot, about three days",
        owner: Some("Them"),
        lines: &[32, 33],
    },
    NoteSpec {
        kind: NoteKind::Action,
        text: "Soak test that kills the host every two minutes",
        owner: Some("You"),
        lines: &[33],
    },
    NoteSpec {
        kind: NoteKind::Point,
        text: "Steam Deck drops to 22 fps in the big cave",
        owner: None,
        lines: &[41, 42],
    },
    NoteSpec {
        kind: NoteKind::Decision,
        text: "World lights skip fog on low; bake all static light",
        owner: None,
        lines: &[45, 46, 49],
    },
    NoteSpec {
        kind: NoteKind::Action,
        text: "Write the performance budget: 40 fps on Deck, 60 on PC",
        owner: Some("Them"),
        lines: &[53, 54],
    },
    NoteSpec {
        kind: NoteKind::Decision,
        text: "Launch March 26 if the soak test passes by March 5",
        owner: None,
        lines: &[64, 65, 66],
    },
    NoteSpec {
        kind: NoteKind::Action,
        text: "Email the publisher the date and the condition",
        owner: Some("Them"),
        lines: &[67, 68],
    },
    NoteSpec {
        kind: NoteKind::Question,
        text: "Who makes the trailer, and is there a festival demo?",
        owner: None,
        lines: &[71, 74, 75, 76],
    },
];

#[derive(Deserialize)]
struct Entry {
    source: String,
    start_ms: u64,
    end_ms: u64,
    text: String,
}

/// The Lantern design call.
pub(super) fn lantern() -> &'static Script {
    static SCRIPT: OnceLock<Script> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        let lines = TRANSCRIPT
            .lines()
            .filter(|line| !line.trim().is_empty())
            .enumerate()
            .map(|(index, line)| {
                let entry: Entry = serde_json::from_str(line).expect("the canned call parses");
                let topic = TOPICS
                    .iter()
                    .rposition(|topic| topic.first_line <= index)
                    .unwrap_or(0);
                Line {
                    mine: entry.source == "microphone",
                    start_ms: entry.start_ms,
                    end_ms: entry.end_ms,
                    text: entry.text,
                    topic,
                }
            })
            .collect();
        Script {
            lines,
            topics: TOPICS,
            notes: NOTES,
            base_title: "Zoom call",
            summary: "The team cut Lantern's tutorial to a lantern-only first room and will \
                test shared and separate co-op lanterns in Friday's playtest. Lobbies will \
                send only quantized inputs when the host migrates, the caves get a \
                performance budget, and launch is March 26 if the soak test passes by March 5.",
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canned_call_has_four_topics_and_its_notes_point_at_real_lines() {
        let script = lantern();
        assert_eq!(script.lines.len(), 79);
        assert_eq!(script.lines[19].topic, 1);
        assert_eq!(script.lines[78].topic, 3);
        for note in script.notes {
            assert!(note.lines.iter().all(|&line| line < script.lines.len()));
        }
    }
}
