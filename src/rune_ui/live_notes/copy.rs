//! Copying the transcript from the capsule and its chapters: all of it,
//! what came after the last copy, one chapter, or a chapter and everything
//! after it. A copy that reaches the live edge marks how far the transcript
//! is copied, which is what Copy new copies from and why it shows only
//! after one.

use rune::Tx;

use super::pipeline::clock;
use super::script::lantern;
use super::{FLASH_SECS, LiveNotes, Phase};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Copy {
    All,
    New,
    Chapter(usize),
    From(usize),
}

impl LiveNotes {
    /// Where the transcript ends: the clock while recording, the call's
    /// end after.
    fn edge_ms(&self) -> u64 {
        if self.phase == Phase::Recording {
            self.clock_ms
        } else {
            lantern().end_ms()
        }
    }

    /// Where chapter `index` starts and ends.
    fn chapter_span(&self, index: usize) -> (u64, u64) {
        let chapters = &self.pipeline.chapters;
        let start = chapters[index].start_ms;
        let end = chapters
            .get(index + 1)
            .map_or(self.edge_ms(), |next| next.start_ms);
        (start, end)
    }

    pub(super) fn copy(&mut self, what: Copy, tx: &mut Tx) {
        let edge = self.edge_ms();
        let last = self.pipeline.chapters.len().saturating_sub(1);
        let (from, to) = match what {
            Copy::All => (0, edge),
            Copy::New => (self.copied_through.unwrap_or(0), edge),
            Copy::Chapter(index) => self.chapter_span(index),
            Copy::From(index) => (self.chapter_span(index).0, edge),
        };
        let reaches_edge = match what {
            Copy::All | Copy::New | Copy::From(_) => true,
            Copy::Chapter(index) => index == last,
        };
        let (text, lines) = self.transcript(from, to, !matches!(what, Copy::Chapter(_)));
        let flash = if lines == 0 {
            "Nothing new yet".to_owned()
        } else {
            tx.copy(text);
            match what {
                Copy::New => format!("Copied {lines} new {}", plural(lines)),
                Copy::Chapter(_) => "Copied the chapter".to_owned(),
                _ => format!("Copied {lines} {}", plural(lines)),
            }
        };
        if reaches_edge {
            self.copied_through = Some(edge);
        }
        self.flash = Some((flash, self.now + FLASH_SECS));
    }

    /// The lines that ended in `from..to`, a heading before each chapter
    /// when `headings`, and how many there were.
    fn transcript(&self, from: u64, to: u64, headings: bool) -> (String, usize) {
        let script = lantern();
        let mut out = String::new();
        let mut lines = 0;
        let mut chapter = None;
        for line in script
            .lines
            .iter()
            .filter(|line| line.end_ms > from && line.end_ms <= to)
        {
            let index = self
                .pipeline
                .chapters
                .iter()
                .rposition(|c| c.start_ms <= line.start_ms);
            if headings
                && let Some(index) = index
                && chapter != Some(index)
            {
                chapter = Some(index);
                let topic = self.pipeline.chapters[index].topic;
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("## {}\n\n", script.topics[topic].name));
            }
            out.push_str(&format!(
                "[{}] {}: {}\n",
                clock(line.start_ms),
                if line.mine { "You" } else { "Them" },
                line.text
            ));
            lines += 1;
        }
        (out, lines)
    }
}

fn plural(lines: usize) -> &'static str {
    if lines == 1 { "line" } else { "lines" }
}
