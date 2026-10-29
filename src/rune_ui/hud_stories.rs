//! Dot scope's HUD as playground stories (the `playground` experiment, ⇧⌘D,
//! group HUD, while `dot-scope` is on). The HUD is a Rune overlay of its
//! own, not a view in the window, so a story can't draw it: each one drives the real
//! HUD at the top of the screen through the same events dictation sends,
//! and its stage says what it's showing. Every story starts from HUD Lab's
//! tuning ([`dot_scope::lab`]); playing one puts its placement and look into
//! that one tuning, so HUD Lab shows them too.

use rune::{Cx, El, Play, Playground, Story, col, text};

use super::controls::{DESCRIPTION, Paint};
use crate::dictation_indicator::DictationIndicatorEvent as Event;
use crate::dot_scope::{self, Placement, Processing, Tuning};

const SIDE_W: f32 = 240.0;
/// How long a story records its canned voice.
const RECORD_SECS: f64 = 3.2;
/// How long a story processes before it's done.
const PROCESS_SECS: f64 = 2.6;
/// Long enough after the last event for the HUD to settle and hide.
const SETTLE_SECS: f64 = 1.6;
/// The processing story's hold before it finishes by itself.
const HOLD_SECS: f64 = 12.0;
const JOB: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    /// Records, then processes and is done, or is cancelled.
    Whole,
    Processing,
    YourVoice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ending {
    Done,
    Cancel,
}

pub(crate) struct HudStory {
    flow: Flow,
    /// Where the HUD sits and whether the notch is pretend; `None` keeps
    /// HUD Lab's.
    placement: Option<(Placement, bool)>,
    /// The processing look; `None` keeps HUD Lab's.
    look: Option<Processing>,
    ending: Ending,
    /// Bumped by each play, so a replay's steps never meet an older one's.
    run: u64,
    /// What the HUD shows now, for the stage.
    showing: &'static str,
}

/// The placement stories: each plays a whole dictation from one placement.
const PLACEMENTS: [(&str, Placement, bool); 4] = [
    ("HUD/Notch", Placement::Notch, false),
    ("HUD/Pretend notch", Placement::Notch, true),
    ("HUD/Menu bar", Placement::MenuBar, false),
    ("HUD/Below the menu bar", Placement::Below, false),
];

/// Adds the HUD's stories to `playground`. A placement story's variants
/// are each look with each ending (`flow-done`, `sweep-cancel`, …); the
/// Processing and Your voice stories' variants are placements (`notch`,
/// `menu-bar`, …). Links name both, so these names stay put.
pub(crate) fn add<S: 'static>(mut playground: Playground<S>) -> Playground<S> {
    for (name, placement, pretend) in PLACEMENTS {
        playground = playground.story(endings(story(name, move || HudStory {
            placement: Some((placement, pretend)),
            ..HudStory::new(Flow::Whole)
        })));
    }
    playground
        .story(placements(story("HUD/Processing", || {
            HudStory::new(Flow::Processing)
        })))
        .story(placements(story("HUD/Your voice", || {
            HudStory::new(Flow::YourVoice)
        })))
}

impl HudStory {
    fn new(flow: Flow) -> Self {
        Self {
            flow,
            placement: None,
            look: None,
            ending: Ending::Done,
            run: 0,
            showing: "Nothing yet",
        }
    }
}

fn story(name: &'static str, init: impl Fn() -> HudStory + 'static) -> Story<HudStory> {
    Story::new(name, init, view)
        .trigger('r', "Play", play)
        .trigger('p', "Next processing look", |s, play| {
            let look = s.look.unwrap_or(dot_scope::lab().processing);
            let next = Processing::ALL
                .iter()
                .position(|each| *each == look)
                .map_or(0, |index| (index + 1) % Processing::ALL.len());
            s.look = Some(Processing::ALL[next]);
            replay(s, play);
        })
        .trigger('a', "Auto level on or off", |s, play| {
            retune(|t| t.auto_level = !t.auto_level);
            replay(s, play);
        })
        .trigger('s', "Stop", |s, _| stop(s))
        .side(side)
}

/// Every look with every ending.
fn endings(story: Story<HudStory>) -> Story<HudStory> {
    let mut story = story;
    for (name, look, ending) in [
        ("flow-done", Processing::Flow, Ending::Done),
        ("pulse-done", Processing::Pulse, Ending::Done),
        ("sweep-done", Processing::Sweep, Ending::Done),
        ("flow-cancel", Processing::Flow, Ending::Cancel),
        ("pulse-cancel", Processing::Pulse, Ending::Cancel),
        ("sweep-cancel", Processing::Sweep, Ending::Cancel),
    ] {
        story = story.variant(name, move |s| {
            s.look = Some(look);
            s.ending = ending;
        });
    }
    story
}

/// HUD Lab's placement, then each placement.
fn placements(story: Story<HudStory>) -> Story<HudStory> {
    story
        .variant("hud-lab", |s| s.placement = None)
        .variant("notch", |s| s.placement = Some((Placement::Notch, false)))
        .variant("pretend-notch", |s| {
            s.placement = Some((Placement::Notch, true))
        })
        .variant("menu-bar", |s| {
            s.placement = Some((Placement::MenuBar, false))
        })
        .variant("below", |s| s.placement = Some((Placement::Below, false)))
}

fn send(event: Event) {
    if let Some(runtime) = crate::macos_rune::runtime() {
        runtime.indicator(event);
    }
}

/// Changes the shared tuning and shows it on the HUD.
fn retune(change: impl FnOnce(&mut Tuning)) {
    let mut tuning = dot_scope::lab();
    change(&mut tuning);
    dot_scope::set_lab(tuning);
    send(Event::ConfigureDotScope(tuning));
}

/// Puts this story's placement and look into the shared tuning.
fn apply(s: &HudStory) {
    retune(|t| {
        if let Some((placement, pretend_notch)) = s.placement {
            t.placement = placement;
            t.pretend_notch = pretend_notch;
        }
        if let Some(look) = s.look {
            t.processing = look;
        }
    });
}

fn stop(s: &mut HudStory) {
    s.run += 1;
    s.showing = "Nothing";
    send(Event::Reset);
}

fn replay(s: &mut HudStory, play: &mut Play<'_, HudStory>) {
    if s.flow != Flow::YourVoice {
        self::play(s, play);
    }
}

/// Runs `step` `secs` from now unless a newer play or Stop came since.
fn then(
    s: &HudStory,
    play: &mut Play<'_, HudStory>,
    secs: f64,
    step: impl FnOnce(&mut HudStory, &mut Play<'_, HudStory>) + 'static,
) {
    let run = s.run;
    play.after(secs, move |s, play| {
        if s.run == run {
            step(s, play);
        }
    });
}

fn record(s: &mut HudStory) {
    send(Event::Reset);
    apply(s);
    send(Event::Started);
    send(Event::DemoVoice);
    s.showing = "Recording a canned voice";
}

fn process(s: &mut HudStory) {
    send(Event::Submitted { job_id: JOB });
    send(Event::Transcribing { job_id: JOB });
    s.showing = "Processing";
}

fn done(s: &mut HudStory, play: &mut Play<'_, HudStory>) {
    send(Event::JobCompleted { job_id: JOB });
    s.showing = "Done";
    then(s, play, SETTLE_SECS, |s, _| {
        // Ends the canned voice, so a real dictation meters the microphone.
        send(Event::Reset);
        s.showing = "Hidden";
    });
}

fn play(s: &mut HudStory, play: &mut Play<'_, HudStory>) {
    s.run += 1;
    match s.flow {
        Flow::Whole => {
            record(s);
            then(s, play, RECORD_SECS, |s, play| match s.ending {
                Ending::Done => {
                    process(s);
                    then(s, play, PROCESS_SECS, done);
                }
                Ending::Cancel => {
                    send(Event::Cancelled);
                    s.showing = "Cancelled";
                    then(s, play, SETTLE_SECS, |s, _| {
                        send(Event::Reset);
                        s.showing = "Hidden";
                    });
                }
            });
        }
        Flow::Processing => {
            record(s);
            process(s);
            then(s, play, HOLD_SECS, done);
        }
        Flow::YourVoice => {
            send(Event::Reset);
            apply(s);
            s.showing = "Waiting for your dictation shortcut";
        }
    }
}

fn view(s: &HudStory, cx: &mut Cx<HudStory>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let tuning = dot_scope::lab();
    let how = if s.flow == Flow::YourVoice {
        "Hold your dictation shortcut and talk: the HUD meters your microphone \
         with this tuning, and the words paste where you are, as always."
    } else {
        "The HUD draws in its own window at the top of the screen with the \
         pointer, not here. R plays this again."
    };
    col()
        .key("hud-story")
        .w(420.0)
        .gap(10.0)
        .padding(24.0)
        .child(
            text(s.showing)
                .key("showing")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
        .child(
            text(format!(
                "{}{} · {} · {} · auto level {}",
                tuning.placement.label(),
                if tuning.pretend_notch {
                    " (pretend)"
                } else {
                    ""
                },
                tuning.processing.label(),
                match s.ending {
                    Ending::Done => "done",
                    Ending::Cancel => "cancel",
                },
                if tuning.auto_level { "on" } else { "off" },
            ))
            .key("tuning")
            .font(DESCRIPTION)
            .color(p.muted()),
        )
        .child(
            text(how)
                .key("how")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap(),
        )
}

fn side(_: &HudStory, cx: &mut Cx<HudStory>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    text(
        "HUD Lab's sliders apply here: change them there and play again. \
         Save in HUD Lab keeps them for the next launch.",
    )
    .key("hud-story-side")
    .font(p.k.ty.small)
    .color(p.muted())
    .wrap()
    .w(SIDE_W)
}
