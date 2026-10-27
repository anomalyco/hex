//! `hex capsule-playground`: the real capsule in a window of its own, on a
//! plain backdrop, driven by a fake host. Buttons step it through every
//! state, "Play the whole flow" runs a capture end to end, and the panel
//! tunes its timings, springs, and sounds live, then copies
//! them back as the Rust constants.
//!
//! It runs beside the installed Hex without touching it: no capture host,
//! settings, captures, tap, or network. Nothing records and nothing sends;
//! the fake host only answers the capsule's actions with the inputs a real
//! capture would bring.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rune::{Cx, El, KeyEvent, Spring, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant};

use super::flow::{Action, Cue, Delivered, Dest, Failure, Input, Route, Undone};
use super::surface::{Id, PANEL_H, PANEL_W};
use super::tuning::{self, Tuning};
use super::{CapsuleState, Mode, capsule, tone};
use crate::rune_ui::controls::{DESCRIPTION, Paint};
use crate::rune_ui::lists::slider_in;

/// How much slow motion slows the capsule.
const SLOW: f64 = 0.25;
const SIDEBAR_W: f32 = 360.0;
const WINDOW: (f32, f32) = (PANEL_W + SIDEBAR_W, 800.0);
const SLIDER_W: f32 = SIDEBAR_W - 48.0;

/// Something the playground does to the capsule at a time.
#[derive(Clone, Debug)]
enum Step {
    Input(Input),
    Click(Id),
    /// Starts the whole flow again, when looping.
    Again,
}

pub(crate) struct Playground {
    /// The capsule's clock: wall time, slowed in slow motion.
    clock: f64,
    wall: Option<f64>,
    slow: bool,
    looping: bool,
    reduced: bool,
    /// Whether a saved rule routes the capture (sent at once) rather than
    /// suggestions.
    rule: bool,
    sound: bool,
    volume: f32,
    tuning: Tuning,
    /// Steps due, by the capsule's clock.
    queue: Vec<(f64, Step)>,
    /// What the capsule asked its host to do, for the fake host.
    actions: Arc<Mutex<Vec<Action>>>,
    /// The latest actions and cues, newest last.
    log: Vec<String>,
    copied: bool,
}

/// Opens the playground and runs it until its window closes.
pub(crate) fn run(rune_args: Vec<String>) {
    let _ = crate::feedback::preload();
    crate::feedback::set_volume(0.5);
    let spec = rune::AppSpec {
        title: "Hex capsule playground",
        min_size: Some(WINDOW),
        scenes: SCENES,
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        poll: Some(poll),
        on_key: Some(on_key),
        ..rune::AppSpec::new("Hex capsule playground", WINDOW, init, super::view)
    };
    rune::App::new(spec).args(rune_args).run();
}

fn init() -> CapsuleState {
    let mut state = CapsuleState::new(true);
    let actions: Arc<Mutex<Vec<Action>>> = Arc::default();
    state.act = Some(Arc::new({
        let actions = actions.clone();
        move |action| {
            actions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(action)
        }
    }));
    let cues: Arc<Mutex<Vec<Cue>>> = Arc::default();
    state.sound = Some(Arc::new(move |cue| {
        cues.lock().unwrap_or_else(|e| e.into_inner()).push(cue);
        crate::feedback::play(tone(cue));
    }));
    state.backdrop = true;
    state.playground = Some(Box::new(Playground {
        clock: 0.0,
        wall: None,
        slow: false,
        looping: false,
        reduced: false,
        rule: false,
        sound: true,
        volume: 0.5,
        tuning: tuning::get(),
        queue: Vec::new(),
        actions,
        log: Vec::new(),
        copied: false,
    }));
    state
}

/// Stills of the playground for the review page (`-- --snap … --scene …`).
type Scene = (&'static str, fn(&mut CapsuleState));

const SCENES: &[Scene] = &[
    ("rule", |s| scene(s, &rule_sent())),
    ("suggestions", |s| {
        scene(s, &[record(), Step::Input(stop()), done(), routed(guess())])
    }),
    ("always", |s| {
        let mut steps = vec![record(), Step::Input(stop()), done(), routed(guess())];
        steps.push(Step::Click(Id::Always));
        scene(s, &steps)
    }),
    ("search", |s| {
        scene(
            s,
            &[
                record(),
                Step::Input(stop()),
                done(),
                routed(guess()),
                Step::Input(Input::OpenList),
            ],
        )
    }),
    ("picked", |s| {
        scene(
            s,
            &[
                record(),
                Step::Input(stop()),
                done(),
                routed(guess()),
                Step::Input(Input::SendRow(0)),
            ],
        )
    }),
    ("taken-back", |s| scene(s, &taken_back())),
    ("recording", |s| {
        scene(s, &[record(), Step::Input(Input::Chips)])
    }),
    ("discard", |s| {
        scene(s, &[record(), Step::Click(Id::Cancel)])
    }),
];

/// A saved rule's answer, which the router sends at once.
fn rule_sent() -> Vec<Step> {
    vec![
        record(),
        Step::Input(stop()),
        done(),
        routed(rule()),
        Step::Input(Input::Settled),
    ]
}

/// A saved rule's send, taken back at once (Undo, after the inputs before
/// it, where a click would jump the queue): the suggestions again.
fn taken_back() -> Vec<Step> {
    let mut steps = rule_sent();
    steps.push(Step::Input(Input::Escape));
    steps
}

fn scene(s: &mut CapsuleState, steps: &[Step]) {
    if let Some(pg) = s.playground.as_mut() {
        pg.queue = steps.iter().cloned().map(|step| (0.0, step)).collect();
    }
}

// ─── The fake capture ────────────────────────────────────────────────────────

fn record() -> Step {
    Step::Input(Input::Stage(scrawl_capture::capture::Stage::Recording))
}

fn stop() -> Input {
    Input::Stage(scrawl_capture::capture::Stage::Stopping)
}

fn done() -> Step {
    Step::Input(Input::Stage(scrawl_capture::capture::Stage::Done))
}

fn routed(route: Route) -> Step {
    Step::Input(Input::Routed(route))
}

fn recent() -> Vec<Dest> {
    vec![
        Dest::new("RUNE", "2 min ago"),
        Dest::new("SCRAWL", "an hour ago"),
        Dest::new("HOME", "yesterday"),
    ]
}

/// A saved rule names no runner-up: its own session is all it suggests.
fn rule() -> Route {
    let to = Dest::new("SEER", "rule: problems.kitlangton.com");
    Route::Rule {
        to: to.clone(),
        suggested: vec![to],
        recent: recent(),
        site: Some("problems.kitlangton.com".into()),
    }
}

fn guess() -> Route {
    Route::Guess {
        suggested: vec![
            Dest::new("HEX", "you were just in it"),
            Dest::new("SEER", "same page"),
            Dest::new("HOME", "mentioned"),
        ],
        recent: recent(),
        site: Some("problems.kitlangton.com".into()),
    }
}

impl Playground {
    fn at(&mut self, delay: f64, step: Step) {
        self.queue.push((self.clock + delay, step));
    }

    fn note(&mut self, line: String) {
        self.log.push(line);
        let extra = self.log.len().saturating_sub(6);
        self.log.drain(..extra);
    }

    /// Answers the capsule's actions as a capture host would, a moment
    /// later, without recording or sending anything.
    fn answer(&mut self, action: Action) {
        self.note(format!("→ {action:?}"));
        match action {
            Action::Record(_) | Action::Start(_) | Action::Switch(_) => self.at(0.15, record()),
            Action::Finish => {
                self.at(0.0, Step::Input(stop()));
                self.at(1.4, done());
                if self.rule {
                    self.at(1.8, routed(rule()));
                    self.at(2.0, Step::Input(Input::Settled));
                } else {
                    self.at(1.8, routed(guess()));
                }
            }
            Action::Undo { .. } => self.at(0.25, Step::Input(Input::Undone(Undone::TakenBack))),
            Action::Cancel
            | Action::Discard
            | Action::Send { .. }
            | Action::Paste
            | Action::Inspect => {}
        }
    }

    /// The whole flow: record, the chips, stop, write, then the
    /// suggestions and a pick (or a saved rule's send), its Undo, and the
    /// capsule leaving.
    fn play(&mut self) {
        self.queue.clear();
        self.at(0.0, Step::Input(Input::Dismiss));
        self.at(0.6, record());
        self.at(1.4, Step::Input(Input::Chips));
        self.at(3.2, Step::Input(Input::Worked));
        self.at(4.6, Step::Click(Id::Finish));
        let routed_at = 4.6 + 1.8;
        let gone = if self.rule {
            routed_at + self.tuning.undo_s + 2.5
        } else {
            let picked = routed_at + 1.8;
            self.at(picked, Step::Input(Input::SendRow(0)));
            picked + 2.5
        };
        self.at(gone, Step::Input(Input::Dismiss));
        if self.looping {
            self.at(gone + 1.0, Step::Again);
        }
    }

    fn set_tuning(&mut self, tuning: Tuning) {
        self.tuning = tuning;
        self.copied = false;
        tuning::set(tuning);
    }
}

// ─── Clock and steps ─────────────────────────────────────────────────────────

fn poll(s: &mut CapsuleState, wall: f64, tx: &mut Tx) -> bool {
    let Some(pg) = s.playground.as_mut() else {
        return super::poll(s, wall, tx);
    };
    let scale = if pg.slow { SLOW } else { 1.0 };
    let dt = pg.wall.map_or(0.0, |last| (wall - last).max(0.0));
    pg.wall = Some(wall);
    pg.clock += dt * scale;
    let now = pg.clock;
    let actions = std::mem::take(&mut *pg.actions.lock().unwrap_or_else(|e| e.into_inner()));
    for action in actions {
        pg.answer(action);
    }
    pg.queue.sort_by(|a, b| a.0.total_cmp(&b.0));
    let due = pg.queue.partition_point(|(at, _)| *at <= now);
    let steps: Vec<Step> = pg.queue.drain(..due).map(|(_, step)| step).collect();
    let mut changed = !steps.is_empty();
    for step in steps {
        match step {
            Step::Input(input) => s.inbox.push(input),
            Step::Click(id) => {
                s.now = now;
                s.click(id, tx);
            }
            Step::Again => {
                if let Some(pg) = s.playground.as_mut() {
                    pg.play();
                }
            }
        }
    }
    changed |= super::poll(s, now, tx);
    if let Some(pg) = s.playground.as_mut() {
        if let Some((next, _)) = pg.queue.first() {
            tx.wake_after(Duration::from_secs_f64(((next - now) / scale).max(0.0)));
        }
        // Actions from a click land on the next frame.
        if !pg
            .actions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
        {
            tx.wake_after(Duration::ZERO);
        }
    }
    changed
}

/// The keys Hex's tap would hand the capsule, from the window instead.
fn on_key(s: &mut CapsuleState, key: &KeyEvent, tx: &mut Tx) -> bool {
    use rune::Key;
    let Some(keys) = s.flow.keys() else {
        return false;
    };
    let list = keys == crate::capture_host::CapsuleKeys::List;
    let digits = match keys {
        crate::capture_host::CapsuleKeys::List => 9,
        crate::capture_host::CapsuleKeys::Suggest { rows } => usize::from(rows),
        _ => 0,
    };
    let escape = matches!(
        keys,
        crate::capture_host::CapsuleKeys::List | crate::capture_host::CapsuleKeys::Picker
    );
    let digit = match &key.key {
        Key::Text(t) if !key.mods.alt => {
            t.parse::<usize>().ok().filter(|d| (1..=digits).contains(d))
        }
        _ => None,
    };
    let input = match &key.key {
        _ if digit.is_some() => Input::SendRow(digit.unwrap_or(1) - 1),
        Key::Escape if escape => Input::Escape,
        Key::Text(t) if key.mods.alt && key.mods.shift && t.eq_ignore_ascii_case("v") => {
            Input::OpenList
        }
        Key::Up if list => Input::Move(-1),
        Key::Down if list => Input::Move(1),
        Key::Enter if list => Input::Send {
            always: key.mods.shift,
        },
        Key::Backspace if list => Input::Backspace,
        Key::Text(t) if list => Input::Type(t.clone()),
        _ => return false,
    };
    s.inbox.push(input);
    tx.wake_after(Duration::ZERO);
    true
}

// ─── View ────────────────────────────────────────────────────────────────────

pub(super) fn view(s: &CapsuleState, cx: &mut Cx<CapsuleState>) -> El {
    let Some(pg) = s.playground.as_deref() else {
        return el();
    };
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    if pg.slow {
        cx.time_scale(SLOW as f32);
    }
    if pg.reduced {
        cx.reduced_motion(true);
    }
    s.reduced.set(cx.is_reduced_motion());
    let stage = el()
        .key("stage")
        .size(PANEL_W, PANEL_H)
        .fill(p.k.c.layer_03)
        .child(capsule(p, &s.surface, &s.live, cx));
    let left = col()
        .key("left")
        .w(PANEL_W)
        .gap(14.0)
        .child(stage)
        .child(states(p))
        .child(flow_controls(p, pg))
        .child(log(p, pg));
    row()
        .key("playground")
        .size(WINDOW.0, WINDOW.1)
        .fill(p.k.c.bg_base)
        .child(left)
        .child(panel(p, pg))
}

fn button(
    p: Paint,
    key: &str,
    label: &str,
    on: bool,
    press: impl Fn(&mut CapsuleState, &mut Tx) + 'static,
) -> El {
    Button::new(label)
        .variant(if on {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::Neutral
        })
        .on_press_tx(press)
        .build(p.k)
        .key(key)
}

fn heading(p: Paint, title: &str) -> El {
    text(title)
        .key(format!("h-{title}"))
        .font(p.k.ty.ui_medium.lh(16.0))
        .color(p.k.c.text_muted)
}

/// A button that sends the capsule straight to a state.
fn state_button(p: Paint, label: &'static str, steps: fn() -> Vec<Step>) -> El {
    button(p, label, label, false, move |s, tx| {
        if let Some(pg) = s.playground.as_mut() {
            pg.queue.clear();
            for step in steps() {
                pg.at(0.0, step);
            }
        }
        tx.wake_after(Duration::ZERO);
    })
}

fn states(p: Paint) -> El {
    col()
        .key("states")
        .gap(8.0)
        .px(14.0)
        .child(heading(p, "States"))
        .child(
            row()
                .key("row-1")
                .gap(6.0)
                .flex_wrap()
                .child(state_button(p, "Record", || vec![record()]))
                .child(state_button(p, "Chips", || {
                    vec![record(), Step::Input(Input::Chips)]
                }))
                .child(state_button(p, "Discard?", || {
                    vec![record(), Step::Click(Id::Cancel)]
                }))
                .child(state_button(p, "Picker", || {
                    vec![record(), Step::Click(Id::Mode(Mode::Area))]
                }))
                .child(state_button(p, "Stop", || {
                    vec![record(), Step::Input(stop())]
                }))
                .child(state_button(p, "Copied", || {
                    vec![record(), Step::Input(stop()), done()]
                }))
                .child(state_button(p, "Nothing heard", || {
                    vec![
                        record(),
                        Step::Input(stop()),
                        done(),
                        Step::Input(Input::NothingHeard),
                        routed(Route::Nothing { recent: recent() }),
                    ]
                })),
        )
        .child(
            row()
                .key("row-2")
                .gap(6.0)
                .flex_wrap()
                .child(state_button(p, "Suggestions", || {
                    vec![record(), Step::Input(stop()), done(), routed(guess())]
                }))
                .child(state_button(p, "Pick 1", || {
                    vec![Step::Input(Input::SendRow(0))]
                }))
                .child(state_button(p, "Search all", || {
                    vec![Step::Input(Input::OpenList)]
                }))
                .child(state_button(p, "Saved rule", rule_sent))
                .child(state_button(p, "Undo", || vec![Step::Click(Id::Undo)]))
                .child(state_button(p, "Rule, then Undo", taken_back))
                .child(state_button(p, "Queued", || {
                    vec![
                        record(),
                        Step::Input(stop()),
                        done(),
                        Step::Input(Input::Delivered(Delivered::Queued {
                            to: "SEER".into(),
                            why: "OpenCode can't see this Mac's files".into(),
                        })),
                    ]
                }))
                .child(state_button(p, "Failed", || {
                    vec![
                        record(),
                        Step::Input(stop()),
                        done(),
                        Step::Input(Input::Failed(
                            Failure::Send("SEER".into()),
                            Some("OpenCode isn't running".into()),
                        )),
                    ]
                }))
                .child(state_button(p, "Hide", || {
                    vec![Step::Input(Input::Dismiss)]
                })),
        )
}

fn flow_controls(p: Paint, pg: &Playground) -> El {
    col()
        .key("flow")
        .gap(8.0)
        .px(14.0)
        .child(heading(p, "Flow"))
        .child(
            row()
                .key("flow-row")
                .gap(6.0)
                .flex_wrap()
                .child(button(p, "play", "Play the whole flow", false, |s, tx| {
                    if let Some(pg) = s.playground.as_mut() {
                        pg.play();
                    }
                    tx.wake_after(Duration::ZERO);
                }))
                .child(button(p, "rule", "Saved rule", pg.rule, |s, _| {
                    if let Some(pg) = s.playground.as_mut() {
                        pg.rule = !pg.rule;
                    }
                }))
                .child(button(p, "loop", "Loop", pg.looping, |s, tx| {
                    if let Some(pg) = s.playground.as_mut() {
                        pg.looping = !pg.looping;
                        if pg.looping && pg.queue.is_empty() {
                            pg.play();
                        }
                    }
                    tx.wake_after(Duration::ZERO);
                }))
                .child(button(p, "slow", "Slow motion", pg.slow, |s, _| {
                    if let Some(pg) = s.playground.as_mut() {
                        pg.slow = !pg.slow;
                    }
                }))
                .child(button(p, "reduced", "Reduce Motion", pg.reduced, |s, _| {
                    if let Some(pg) = s.playground.as_mut() {
                        pg.reduced = !pg.reduced;
                    }
                })),
        )
}

fn log(p: Paint, pg: &Playground) -> El {
    let mut lines = col().key("log").gap(2.0).px(14.0);
    for (i, line) in pg.log.iter().enumerate() {
        lines = lines.child(
            text(line.clone())
                .key(format!("log-{i}"))
                .font(p.k.ty.mono)
                .color(p.faint()),
        );
    }
    lines
}

// ─── Tuning ──────────────────────────────────────────────────────────────────

/// A slider over `lo..=hi` for one tuning value.
fn knob(
    p: Paint,
    key: &str,
    label: &str,
    value: f32,
    (lo, hi): (f32, f32),
    unit: &str,
    set: fn(&mut Tuning, f32),
) -> El {
    let display = format!("{value:.3}{unit}");
    slider_in(
        p,
        key,
        label,
        (value - lo) / (hi - lo),
        display,
        SLIDER_W,
        move |s: &mut CapsuleState, at: f32, _| {
            if let Some(pg) = s.playground.as_mut() {
                let mut tuning = pg.tuning;
                set(&mut tuning, lo + at * (hi - lo));
                pg.set_tuning(tuning);
            }
        },
    )
}

/// A spring's duration and bounce, as two sliders.
macro_rules! spring {
    ($p:expr, $t:expr, $field:ident, $label:expr) => {
        col()
            .key(stringify!($field))
            .gap(6.0)
            .child(knob(
                $p,
                "duration",
                &format!("{} duration", $label),
                $t.$field.duration,
                (0.06, 0.9),
                " s",
                |t, v| t.$field = Spring::new(v, t.$field.bounce),
            ))
            .child(knob(
                $p,
                "bounce",
                &format!("{} bounce", $label),
                $t.$field.bounce,
                (-0.3, 0.6),
                "",
                |t, v| t.$field = Spring::new(t.$field.duration, v),
            ))
    };
}

fn panel(p: Paint, pg: &Playground) -> El {
    let t = pg.tuning;
    let mut body = col()
        .key("tuning")
        .w(SIDEBAR_W)
        .gap(12.0)
        .padding(16.0)
        .child(heading(p, "Undo"))
        .child(knob(
            p,
            "undo",
            "Undo window",
            t.undo_s as f32,
            (2.0, 10.0),
            " s",
            |t, v| {
                t.undo_s = f64::from((v * 2.0).round() / 2.0);
            },
        ))
        .child(heading(p, "Sound"))
        .child(
            row()
                .key("sound-row")
                .gap(6.0)
                .flex_wrap()
                .child(button(p, "sound", "Sound", pg.sound, |s, _| {
                    if let Some(pg) = s.playground.as_mut() {
                        pg.sound = !pg.sound;
                        crate::feedback::set_enabled(pg.sound);
                    }
                }))
                .children(
                    [
                        ("Start", Cue::Start),
                        ("Stop", Cue::Stop),
                        ("Sent", Cue::Sent),
                        ("Undo", Cue::TakenBack),
                        ("Failed", Cue::Failed),
                    ]
                    .into_iter()
                    .map(|(label, cue)| {
                        button(p, &format!("tone-{label}"), label, false, move |s, _| {
                            if let Some(pg) = s.playground.as_mut() {
                                pg.note(format!("♪ {cue:?}"));
                            }
                            crate::feedback::play(tone(cue));
                        })
                    }),
                ),
        )
        .child(slider_in(
            p,
            "volume",
            "Volume",
            pg.volume,
            format!("{:.0}%", pg.volume * 100.0),
            SLIDER_W,
            |s: &mut CapsuleState, at: f32, _| {
                if let Some(pg) = s.playground.as_mut() {
                    pg.volume = at;
                    crate::feedback::set_volume(at);
                }
            },
        ))
        .child(heading(p, "Holds"))
        .child(knob(
            p,
            "done-hold",
            "Finished capsule stays",
            t.done_hold_s as f32,
            (3.0, 30.0),
            " s",
            |t, v| {
                t.done_hold_s = f64::from(v.round());
            },
        ))
        .child(knob(
            p,
            "brief-hold",
            "Taken back / failed stays",
            t.brief_hold_s as f32,
            (1.0, 10.0),
            " s",
            |t, v| {
                t.brief_hold_s = f64::from((v * 2.0).round() / 2.0);
            },
        ))
        .child(knob(
            p,
            "chips-open",
            "Chips stay open",
            t.chips_open_s as f32,
            (1.0, 8.0),
            " s",
            |t, v| {
                t.chips_open_s = f64::from((v * 2.0).round() / 2.0);
            },
        ))
        .child(heading(p, "Timings"))
        .child(knob(
            p,
            "shell-wait",
            "Walls wait (narrowing)",
            t.shell_wait_s,
            (0.0, 0.2),
            " s",
            |t, v| {
                t.shell_wait_s = v;
            },
        ))
        .child(knob(
            p,
            "stagger",
            "Chip stagger",
            t.stagger_s,
            (0.0, 0.05),
            " s",
            |t, v| {
                t.stagger_s = v;
            },
        ))
        .child(knob(
            p,
            "reveal-enter",
            "Content fade-in",
            t.reveal_enter_s,
            (0.05, 0.4),
            " s",
            |t, v| {
                t.reveal_enter_s = v;
            },
        ))
        .child(heading(p, "Springs"))
        .child(spring!(p, t, island, "Walls"))
        .child(spring!(p, t, island_scale, "Entry"))
        .child(spring!(p, t, reveal_open, "Chips open"))
        .child(spring!(p, t, reveal_close, "Chips close"))
        .child(spring!(p, t, switch_width, "Mode switch"))
        .child(spring!(p, t, switch_slide, "Switch slide"))
        .child(spring!(p, t, highlight, "Highlight"))
        .child(spring!(p, t, presence_exit, "Exit"));
    body = body.child(
        row()
            .key("copy-row")
            .gap(6.0)
            .child(button(
                p,
                "copy",
                if pg.copied { "Copied" } else { "Copy values" },
                true,
                |s, tx| {
                    if let Some(pg) = s.playground.as_mut() {
                        tx.copy(pg.tuning.to_rust());
                        pg.copied = true;
                    }
                },
            ))
            .child(button(p, "reset", "Reset", false, |s, _| {
                if let Some(pg) = s.playground.as_mut() {
                    pg.set_tuning(tuning::DEFAULT);
                }
            })),
    );
    body = body.child(
        text("Values apply to the next change. Copy values copies tuning.rs's DEFAULT.")
            .key("hint")
            .font(DESCRIPTION)
            .color(p.faint())
            .wrap(),
    );
    col()
        .key("panel-scroll")
        .w(SIDEBAR_W)
        .h(WINDOW.1)
        .fill(p.k.c.layer_01)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .child(body)
}
