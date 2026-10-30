//! Live meeting notes as a playground story (the `playground` experiment,
//! ⇧⌘D, Prototypes › Meetings › Live notes): a canned nine-minute call
//! plays in accelerated time through a stand-in for the live pipeline, on a
//! mock screen. The recording capsule's title follows the chapter, the
//! notes heard so far float under it, and a note opens the transcript lines
//! it came from. Drag the capsule to another corner and the notes follow.
//! Stop finishes the call: the last pass, the boundary pass, then the final
//! title and summary. Nothing here touches a real meeting; it is for
//! working out how live notes should look on screen
//! (`hex://story/prototypes/meetings/live-notes?play=1`).

mod notes;
mod pipeline;
mod script;
mod views;

use rune::{Play, Playground, Story, Tx};

use pipeline::{NOTES_INTERVAL_MS, Pipeline};
use script::lantern;

const NAME: &str = "Prototypes/Meetings/Live notes";
/// How many times faster than real time the call plays.
const SPEEDS: [f64; 3] = [15.0, 30.0, 60.0];
/// How long the notes still being written after Stop take, in real seconds.
const TRANSCRIBING_SECS: f64 = 1.4;
const WRITING_SECS: f64 = 1.6;
/// How long a new note stays under the capsule in [`Under::Fade`].
const FADE_SECS: f64 = 4.0;

/// What floats under the capsule while it records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Under {
    /// The newest notes, always.
    Stack,
    /// Each new note for a few seconds, then nothing.
    Fade,
    /// Nothing until the pointer is on the capsule; then every note.
    Hover,
}

/// The screen corner the capsule sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Corner {
    right: bool,
    bottom: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Recording,
    /// The final transcript; the last pass and the boundary pass follow.
    Transcribing,
    /// The final title and summary.
    Writing,
    Done,
}

/// Something the story does by itself while it plays, once, unless a
/// trigger took over.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cue {
    Waiting,
    At(f64),
    Done,
}

impl Cue {
    /// True once, `delay` after `ready` first holds.
    fn fire(&mut self, now: f64, ready: bool, delay: f64) -> bool {
        match *self {
            Cue::Waiting if ready => *self = Cue::At(now + delay),
            Cue::At(at) if now >= at => {
                *self = Cue::Done;
                return true;
            }
            _ => {}
        }
        false
    }
}

pub(crate) struct LiveNotes {
    under: Under,
    corner: Corner,
    /// How far the capsule is dragged from its corner, while it is.
    drag: (f32, f32),
    clock_ms: u64,
    playing: bool,
    speed: usize,
    /// The story's clock, from its poll.
    now: f64,
    last_poll: Option<f64>,
    phase: Phase,
    phase_since: f64,
    pipeline: Pipeline,
    hovered: bool,
    open_note: Option<usize>,
    /// The newest note and when [`Under::Fade`] lets it go.
    fresh: Option<(usize, f64)>,
    autopilot: bool,
    /// Opens the newest note's lines, then closes them.
    show_note: Cue,
    hide_note: Cue,
    shown_note: Option<usize>,
    /// Moves the capsule to the other side, as a drag would.
    move_capsule: Cue,
}

impl LiveNotes {
    fn new() -> Self {
        Self {
            under: Under::Stack,
            corner: Corner {
                right: true,
                bottom: false,
            },
            drag: (0.0, 0.0),
            clock_ms: 0,
            playing: false,
            speed: 1,
            now: 0.0,
            last_poll: None,
            phase: Phase::Recording,
            phase_since: 0.0,
            pipeline: Pipeline::default(),
            hovered: false,
            open_note: None,
            fresh: None,
            autopilot: true,
            show_note: Cue::Waiting,
            hide_note: Cue::Waiting,
            shown_note: None,
            move_capsule: Cue::Waiting,
        }
    }

    /// Runs the call to `target_ms`, a second at a time, so passes fall
    /// where they would however far one frame jumps.
    fn advance_to(&mut self, target_ms: u64) -> bool {
        let script = lantern();
        let target_ms = target_ms.min(script.end_ms());
        let before = (self.clock_ms / 1_000, self.pipeline.passes);
        while self.clock_ms < target_ms {
            self.clock_ms = (self.clock_ms + 1_000).min(target_ms);
            if self.pipeline.due(script, self.clock_ms, NOTES_INTERVAL_MS) {
                let found = self.pipeline.found.len();
                self.pipeline.pass(script, self.clock_ms);
                if found < self.pipeline.found.len() {
                    self.fresh = self.newest_note().map(|note| (note, self.now + FADE_SECS));
                }
            }
        }
        if self.clock_ms >= script.end_ms() {
            self.stop();
        }
        before != (self.clock_ms / 1_000, self.pipeline.passes)
    }

    fn stop(&mut self) {
        if self.phase != Phase::Recording {
            return;
        }
        self.pipeline.stop(lantern());
        self.phase = Phase::Transcribing;
        self.phase_since = self.now;
        self.playing = false;
        self.hovered = false;
        self.open_note = None;
        self.fresh = None;
    }

    /// The notes found so far, newest first.
    fn notes_newest_first(&self) -> Vec<usize> {
        let script = lantern();
        let mut notes = self.pipeline.found.clone();
        notes.sort_by_key(|&note| std::cmp::Reverse(script.note_at(note)));
        notes
    }

    fn newest_note(&self) -> Option<usize> {
        self.notes_newest_first().first().copied()
    }

    fn take_over(&mut self) {
        self.autopilot = false;
    }

    fn cues(&mut self) -> bool {
        if !self.autopilot || self.phase != Phase::Recording {
            return false;
        }
        let now = self.now;
        let mut changed = false;
        let chapters = self.pipeline.chapters.len();
        // Hover shows nothing to open until the pointer is there.
        let shown = chapters >= 2 && (self.under != Under::Hover || self.hovered);
        if self.show_note.fire(now, shown, 1.0) {
            self.shown_note = self.newest_note();
            self.open_note = self.shown_note;
            changed = true;
        }
        if self.hide_note.fire(now, self.show_note == Cue::Done, 3.0)
            && self.open_note == self.shown_note
        {
            self.open_note = None;
            changed = true;
        }
        if self.move_capsule.fire(now, chapters >= 3, 0.6) {
            self.corner = Corner {
                right: !self.corner.right,
                bottom: true,
            };
            changed = true;
        }
        changed
    }
}

fn poll(s: &mut LiveNotes, now: f64, _: &mut Tx) -> bool {
    let elapsed = s.last_poll.map_or(0.0, |last| (now - last).max(0.0));
    s.last_poll = Some(now);
    s.now = now;
    let mut changed = false;
    if s.phase == Phase::Recording && s.playing {
        let step = (elapsed * SPEEDS[s.speed] * 1_000.0) as u64;
        changed |= s.advance_to(s.clock_ms + step);
    }
    changed |= s.cues();
    let script = lantern();
    let since = now - s.phase_since;
    match s.phase {
        Phase::Transcribing if since >= TRANSCRIBING_SECS => {
            s.pipeline.tail(script);
            s.pipeline.refine(script);
            s.phase = Phase::Writing;
            s.phase_since = now;
            changed = true;
        }
        Phase::Writing if since >= WRITING_SECS => {
            s.phase = Phase::Done;
            s.phase_since = now;
            changed = true;
        }
        _ => {}
    }
    if s.fresh.is_some_and(|(_, until)| now >= until) {
        s.fresh = None;
        changed = true;
    }
    changed
}

/// R makes the state again before this runs, so it only starts the clock.
fn play(s: &mut LiveNotes, _: &mut Play<'_, LiveNotes>) {
    s.playing = true;
}

fn next_topic(s: &mut LiveNotes) {
    if s.phase != Phase::Recording {
        return;
    }
    let chapters = s.pipeline.chapters.len();
    while s.phase == Phase::Recording && s.pipeline.chapters.len() == chapters {
        s.advance_to(s.clock_ms + 1_000);
    }
}

/// Adds the live notes story to `playground`.
pub(crate) fn add<S: 'static>(playground: Playground<S>) -> Playground<S> {
    playground.story(story())
}

fn story() -> Story<LiveNotes> {
    Story::new(NAME, LiveNotes::new, views::stage)
        .stage(rune::Stage::Fill)
        .poll(poll)
        .trigger('r', "Play from the start", play)
        .trigger('p', "Pause or resume", |s, _| {
            if s.phase == Phase::Recording {
                s.playing = !s.playing;
            }
        })
        .trigger('f', "Faster or slower", |s, _| {
            s.speed = (s.speed + 1) % SPEEDS.len();
        })
        .trigger('n', "Jump to the next chapter", |s, _| next_topic(s))
        .trigger('m', "Move to the next corner", |s, _| {
            s.take_over();
            s.corner = match (s.corner.right, s.corner.bottom) {
                (true, false) => Corner {
                    right: true,
                    bottom: true,
                },
                (true, true) => Corner {
                    right: false,
                    bottom: true,
                },
                (false, true) => Corner {
                    right: false,
                    bottom: false,
                },
                (false, false) => Corner {
                    right: true,
                    bottom: false,
                },
            };
        })
        .trigger('l', "Show the newest note's lines", |s, _| {
            s.take_over();
            let newest = s.newest_note();
            s.open_note = if s.open_note == newest { None } else { newest };
        })
        .trigger('s', "Stop", |s, _| {
            s.take_over();
            s.stop();
        })
        .variant("Newest notes", |s| s.under = Under::Stack)
        .variant("Each note fades", |s| s.under = Under::Fade)
        .variant("Notes on hover", |s| s.under = Under::Hover)
        .side(views::side)
}

#[cfg(test)]
mod tests {
    use rune::App;

    use super::super::{WINDOW_SIZE, playground, spec};

    /// `hex://story/prototypes/meetings/live-notes` opens the story on each
    /// variant.
    #[test]
    fn its_story_link_opens_each_variant() {
        let slug = "prototypes-meetings-live-notes";
        #[cfg(target_os = "macos")]
        assert_eq!(
            super::super::story_link::parse("hex://story/prototypes/meetings/live-notes", "hex")
                .map(|link| link.story),
            Ok(slug.to_owned())
        );
        let app = playground::attach(App::new(playground::spec(spec())));
        let mut rt = app.headless_runtime(WINDOW_SIZE, rune::Env::new(false, 1.0));
        rt.frame(0.0);
        for variant in ["1", "2", "3"] {
            rt.open_playground(slug, Some(variant));
            assert!(rt.playground_state::<super::LiveNotes>().is_some());
        }
    }
}
