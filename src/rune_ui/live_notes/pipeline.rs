//! A stand-in for the live pipeline, so the stories show notes arriving the
//! way they would. The "model" is perfect (it names the topic most of a
//! window is about and finds every note in it); the rules around it are
//! live chapters' (`meeting_chapters`): one pass per interval of new
//! meeting time with at least 40 new words, the first name opens chapter 1
//! at 00:00, and a departure opens a chapter, dated at its window's first
//! line, only when the next pass departs too and the current chapter has
//! lasted 90 s. After Stop a waiting departure opens, and the boundary pass
//! moves each chapter to where its topic begins.
//!
//! One step is proposed, not built: after Stop a last pass reads the lines
//! no live pass did, for their notes and a chapter they may open.

use super::script::Script;

/// Live chapters' interval.
#[cfg(test)]
const CHAPTERS_INTERVAL_MS: u64 = 180_000;
/// #201's live notes interval.
pub(super) const NOTES_INTERVAL_MS: u64 = 75_000;
const MIN_NEW_WORDS: usize = 40;
const MIN_CHAPTER_MS: u64 = 90_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Chapter {
    pub(super) topic: usize,
    pub(super) start_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Pipeline {
    pub(super) chapters: Vec<Chapter>,
    /// A departure heard once, waiting for a second pass.
    pub(super) pending: Option<Chapter>,
    pub(super) covered_ms: u64,
    pub(super) passes: u32,
    /// Notes found, by index into the script's, in the order found.
    pub(super) found: Vec<usize>,
    /// What each step did, newest last, for the side column.
    pub(super) log: Vec<String>,
    /// The boundaries moved after Stop.
    pub(super) refined: bool,
}

impl Pipeline {
    pub(super) fn due(&self, script: &Script, clock_ms: u64, interval_ms: u64) -> bool {
        clock_ms >= self.covered_ms + interval_ms
            && script.words_after(self.covered_ms, clock_ms) >= MIN_NEW_WORDS
    }

    /// One pass over the lines heard since the last; true when the
    /// chapters changed.
    pub(super) fn pass(&mut self, script: &Script, clock_ms: u64) -> bool {
        let Some((topic, start_ms, end_ms)) = self.window(script, clock_ms) else {
            return false;
        };
        self.passes += 1;
        let found = self.find_notes(script, end_ms);
        let name = script.topics[topic].name;
        let at = clock(clock_ms);
        let changed = self.merge(topic, start_ms);
        let what = if changed && self.chapters.len() == 1 {
            format!("opens chapter 1, “{name}”")
        } else if changed {
            let opened = self.chapters.last().map_or(topic, |chapter| chapter.topic);
            format!(
                "confirms “{}”, from {}",
                script.topics[opened].name,
                clock(self.chapters.last().map_or(0, |c| c.start_ms))
            )
        } else if self.pending.is_some_and(|p| p.topic == topic) {
            format!("hears “{name}” once; waits for a second pass")
        } else {
            "still the same chapter".into()
        };
        self.log.push(format!(
            "{at} pass {}: {what}{}",
            self.passes,
            notes_suffix(found)
        ));
        self.covered_ms = end_ms;
        changed
    }

    /// Stop: a departure still waiting opens if the chapter before it had
    /// its 90 s.
    pub(super) fn stop(&mut self, script: &Script) {
        if let Some(pending) = self.pending.take()
            && let Some(current) = self.chapters.last()
            && pending.start_ms >= current.start_ms + MIN_CHAPTER_MS
        {
            self.chapters.push(pending);
            self.log.push(format!(
                "Stop: opens the waiting “{}”",
                script.topics[pending.topic].name
            ));
        }
    }

    /// Proposed: one pass over the lines no live pass read.
    pub(super) fn tail(&mut self, script: &Script) {
        let end_ms = script.end_ms();
        let Some((topic, start_ms, _)) = self.window(script, end_ms) else {
            return;
        };
        self.passes += 1;
        let found = self.find_notes(script, end_ms);
        let opens = self.chapters.last().is_some_and(|current| {
            current.topic != topic && start_ms >= current.start_ms + MIN_CHAPTER_MS
        });
        if opens {
            self.chapters.push(Chapter { topic, start_ms });
        }
        self.log.push(format!(
            "After Stop, last pass: {}{}",
            if opens {
                format!("opens “{}”", script.topics[topic].name)
            } else {
                "no new chapter".into()
            },
            notes_suffix(found)
        ));
        self.covered_ms = end_ms;
    }

    /// The boundary pass: each chapter after the first moves to the line
    /// where its topic begins, if that stays after the chapter before.
    pub(super) fn refine(&mut self, script: &Script) {
        let mut moved = 0;
        for index in 1..self.chapters.len() {
            let start = script.topic_start(self.chapters[index].topic);
            if start > self.chapters[index - 1].start_ms && start != self.chapters[index].start_ms {
                self.chapters[index].start_ms = start;
                moved += 1;
            }
        }
        self.refined = true;
        self.log.push(format!(
            "Boundary pass: {moved} {} moved to where the topic begins",
            if moved == 1 { "chapter" } else { "chapters" }
        ));
    }

    /// The topic most of the new lines are about, and the window's span.
    fn window(&self, script: &Script, until_ms: u64) -> Option<(usize, u64, u64)> {
        let fresh: Vec<_> = script
            .lines
            .iter()
            .filter(|line| line.end_ms > self.covered_ms && line.end_ms <= until_ms)
            .collect();
        let start_ms = fresh.first()?.start_ms;
        let end_ms = fresh.iter().map(|line| line.end_ms).max()?;
        let mut weight = vec![0usize; script.topics.len()];
        for line in &fresh {
            weight[line.topic] += line.text.len();
        }
        let topic = (0..weight.len()).max_by_key(|&t| (weight[t], usize::MAX - t))?;
        Some((topic, start_ms, end_ms))
    }

    fn find_notes(&mut self, script: &Script, end_ms: u64) -> usize {
        let before = self.found.len();
        for (index, _) in script.notes.iter().enumerate() {
            let at = script.note_at(index);
            if at <= end_ms && !self.found.contains(&index) {
                self.found.push(index);
            }
        }
        self.found.len() - before
    }

    /// Live chapters' rule (`Chaptering::merge`), with topics known by
    /// index rather than by comparing names.
    fn merge(&mut self, topic: usize, window_start_ms: u64) -> bool {
        let Some(current) = self.chapters.last().copied() else {
            self.chapters.push(Chapter { topic, start_ms: 0 });
            return true;
        };
        if current.topic == topic {
            self.pending = None;
            return false;
        }
        match self.pending.take() {
            Some(pending) if pending.start_ms >= current.start_ms + MIN_CHAPTER_MS => {
                self.chapters.push(pending);
                if pending.topic != topic {
                    self.pending = Some(Chapter {
                        topic,
                        start_ms: window_start_ms,
                    });
                }
                true
            }
            _ => {
                self.pending = Some(Chapter {
                    topic,
                    start_ms: window_start_ms,
                });
                false
            }
        }
    }
}

fn notes_suffix(found: usize) -> String {
    match found {
        0 => String::new(),
        1 => " · 1 note".into(),
        n => format!(" · {n} notes"),
    }
}

/// `mm:ss`.
pub(super) fn clock(ms: u64) -> String {
    let seconds = ms / 1_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::super::script::lantern;
    use super::*;

    fn run(interval_ms: u64) -> Pipeline {
        let script = lantern();
        let mut pipeline = Pipeline::default();
        let mut clock_ms = 0;
        while clock_ms < script.end_ms() {
            clock_ms += 1_000;
            if pipeline.due(script, clock_ms, interval_ms) {
                pipeline.pass(script, clock_ms);
            }
        }
        pipeline.stop(script);
        pipeline
    }

    /// The caves come and go between two three-minute passes, so no
    /// chapter ever names them.
    #[test]
    fn three_minute_passes_miss_the_third_topic() {
        let pipeline = run(CHAPTERS_INTERVAL_MS);
        let topics: Vec<_> = pipeline.chapters.iter().map(|c| c.topic).collect();
        assert_eq!(topics, [0, 1, 3]);
    }

    #[test]
    fn faster_passes_find_every_topic_and_the_boundary_pass_dates_them() {
        let script = lantern();
        let mut pipeline = run(NOTES_INTERVAL_MS);
        pipeline.tail(script);
        pipeline.refine(script);
        let topics: Vec<_> = pipeline.chapters.iter().map(|c| c.topic).collect();
        assert_eq!(topics, [0, 1, 2, 3]);
        assert_eq!(pipeline.chapters[1].start_ms, script.topic_start(1));
        assert_eq!(pipeline.found.len(), script.notes.len());
    }
}
