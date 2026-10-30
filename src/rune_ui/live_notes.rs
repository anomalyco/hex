//! Live meeting notes as a playground story (the `playground` experiment,
//! ⇧⌘D, Prototypes › Meetings › Live notes): a canned nine-minute call
//! plays in accelerated time through a stand-in for the live pipeline, on a
//! mock screen. The recording capsule's title follows the chapter, and the
//! chapters so far float under it with the current one's notes; a note
//! opens the transcript lines it came from. The capsule copies the whole
//! transcript, then only what is new since; a chapter copies itself or
//! everything from it on. The chevron hides the notes, and the capsule drags
//! to another corner with them. Stop finishes the call: the last pass, the
//! boundary pass, then the final title and summary. Nothing here touches a real meeting; it is for
//! working out how live notes should look on screen
//! (`hex://story/prototypes/meetings/live-notes?play=1`).

mod copy;
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
/// How long the capsule says what a copy did, in real seconds.
const FLASH_SECS: f64 = 1.6;

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
    /// Whether the chapters show under the capsule; its chevron hides them.
    notes_open: bool,
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
    /// The chapter under the pointer, which shows its copy actions.
    hovered_chapter: Option<usize>,
    /// An earlier chapter opened to its notes; the current one always is.
    open_chapter: Option<usize>,
    open_note: Option<usize>,
    /// How far the transcript is copied, once a copy reached the live
    /// edge: Copy new copies what came after it, and shows only then.
    copied_through: Option<u64>,
    /// What the last copy did, shown in place of the clock, and until when.
    flash: Option<(String, f64)>,
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
            notes_open: true,
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
            hovered_chapter: None,
            open_chapter: None,
            open_note: None,
            copied_through: None,
            flash: None,
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
                self.pipeline.pass(script, self.clock_ms);
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
        self.hovered_chapter = None;
        self.open_note = None;
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
        let shown = chapters >= 2 && self.notes_open;
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
    if s.flash.as_ref().is_some_and(|(_, until)| now >= *until) {
        s.flash = None;
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
        .trigger('h', "Hide or show the notes", |s, _| {
            s.take_over();
            s.notes_open = !s.notes_open;
        })
        .trigger('c', "Copy all, then copy new", |s, play| {
            s.take_over();
            let what = if s.copied_through.is_some() {
                copy::Copy::New
            } else {
                copy::Copy::All
            };
            s.copy(what, play);
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
        .variant("Notes open", |s| s.notes_open = true)
        .variant("Notes hidden", |s| s.notes_open = false)
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
        for variant in ["1", "2"] {
            rt.open_playground(slug, Some(variant));
            assert!(rt.playground_state::<super::LiveNotes>().is_some());
        }
    }

    /// The first copy takes the whole transcript; after it, Copy new takes
    /// only what was said since.
    #[test]
    fn copy_new_takes_what_came_after_the_first_copy() {
        let app = playground::attach(App::new(playground::spec(spec())));
        let mut rt = app.headless_runtime(WINDOW_SIZE, rune::Env::new(false, 1.0));
        rt.frame(0.0);
        rt.open_playground("prototypes-meetings-live-notes", None);
        rt.playground_trigger('n');
        rt.playground_trigger('c');
        let all = rt.copy_request.take().expect("Copy all copies");
        let all: &str = &all;
        assert!(all.starts_with("## Lantern onboarding"));
        let state = rt.playground_state::<super::LiveNotes>().unwrap();
        let through = state.copied_through.expect("Copy all marks the edge");
        assert_eq!(through, state.clock_ms);

        rt.playground_trigger('n');
        rt.playground_trigger('c');
        let new = rt.copy_request.take().expect("Copy new copies");
        let new: &str = &new;
        let first_new = new.lines().find(|line| line.starts_with('[')).unwrap();
        assert!(!all.contains(first_new), "{first_new} was copied twice");
    }
}
