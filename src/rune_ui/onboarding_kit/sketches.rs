//! "Watch another one": short `"Without Hex"` sketches for a slow download.
//!
//! Everyone gets the same opener. When the speech model is still
//! downloading on `"Almost ready"`, a button beside the download's bar plays
//! one of these (`scenes/sketch-*.scene`, no `with`), up to all three; the
//! bar moves up to the top pill while one plays. Once the model lands, the
//! first run goes on to the game at the sketch's next stop
//! ([`Scene::stops`]): after a reply has gone out, never mid-line.

use std::collections::VecDeque;
use std::sync::OnceLock;

use rune::{El, TextStyle, Transition, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use super::super::controls::Paint;
use super::cards::CHAT_W;
use super::fixture::Download;
use super::live;
use super::pill::{CAPSULE, FLY};
use super::scene_script::{Script, parse_sketch};
use super::stories::{Onboard, Step};
use super::typing::Scene;
use super::versus;

const SKETCHES: [(&str, &str); 3] = [
    (
        "sketch-first-date.scene",
        include_str!("scenes/sketch-first-date.scene"),
    ),
    (
        "sketch-dream-job.scene",
        include_str!("scenes/sketch-dream-job.scene"),
    ),
    (
        "sketch-family-group.scene",
        include_str!("scenes/sketch-family-group.scene"),
    ),
];

/// From the press to the sketch's first new line: its conversation opens
/// with what was already said.
const LEAD_IN: f64 = 0.6;
/// How long a sketch's last line holds before `"Almost ready"` comes back.
const HOLD: f64 = 1.6;
/// How far back the download's pace is measured.
const PACE_WINDOW: f64 = 8.0;
/// How long the pace has to be measured over before a time left shows.
const PACE_MIN: f64 = 3.0;
const BAR_W: f32 = 220.0;

#[derive(Default)]
pub(crate) struct Sketches {
    playing: Option<Playing>,
    /// How many have started this run.
    played: usize,
    /// How many of the playing sketch's replies have flown out of the field.
    sent_seen: usize,
    /// The download's bytes over the last few seconds, for its time left.
    samples: VecDeque<(f64, u64)>,
}

struct Playing {
    index: usize,
    scene: Scene,
    /// When the sketch's own clock reads zero.
    from: f64,
}

fn script(index: usize) -> &'static Script<'static> {
    static PARSED: [OnceLock<Script<'static>>; SKETCHES.len()] =
        [const { OnceLock::new() }; SKETCHES.len()];
    PARSED[index].get_or_init(|| {
        let (name, src) = SKETCHES[index];
        parse_sketch(src).unwrap_or_else(|errors| panic!("{name}:\n{errors}"))
    })
}

fn on(s: &Onboard) -> Option<&Playing> {
    s.sketches
        .playing
        .as_ref()
        .filter(|p| s.now < p.from + p.scene.duration() + HOLD)
        // A download that stopped says so instead.
        .filter(|_| s.live.as_ref().is_none_or(|live| !live.failed))
}

/// A sketch is on screen.
pub(crate) fn playing(s: &Onboard) -> bool {
    on(s).is_some()
}

/// `"Watch another one"` would play something.
fn offered(s: &Onboard) -> bool {
    s.step == Step::Waiting
        && !s.can_hear()
        && !playing(s)
        && s.sketches.played < SKETCHES.len()
        && s.live.as_ref().is_none_or(|live| !live.failed)
}

/// Plays the next sketch.
pub(crate) fn watch(s: &mut Onboard) {
    if !offered(s) {
        return;
    }
    let index = s.sketches.played;
    let scene = Scene::play(&s.timing, script(index));
    let from = s.now + LEAD_IN;
    s.sfx.on_without_hex(from, &scene);
    s.sketches.playing = Some(Playing { index, scene, from });
    s.sketches.played += 1;
    s.sketches.sent_seen = 0;
}

/// Moves the sketches to `now`: the download's pace, and a sent reply
/// lifting out of the field into its bubble.
pub(super) fn advance(s: &mut Onboard, now: f64, tx: &mut Tx) {
    if let Some(d) = s.download {
        let samples = &mut s.sketches.samples;
        if samples
            .back()
            .is_none_or(|&(_, bytes)| bytes != d.downloaded)
        {
            samples.push_back((now, d.downloaded));
        }
        while samples.len() > 2
            && samples
                .front()
                .is_some_and(|&(at, _)| now - at > PACE_WINDOW)
        {
            samples.pop_front();
        }
    }
    let Some(playing) = s.sketches.playing.as_ref() else {
        return;
    };
    if now >= playing.from + playing.scene.duration() + HOLD {
        s.sketches.playing = None;
        return;
    }
    let sent = playing.scene.sent(now - playing.from);
    for i in s.sketches.sent_seen..sent {
        tx.launch("typed/line0", format!("sent-bubble-{i}"));
    }
    s.sketches.sent_seen = sent;
}

/// With the model in, how long the playing sketch has to its next stop;
/// `None` once it's there (or none plays), and it's put away.
pub(super) fn hold(s: &mut Onboard, now: f64) -> Option<f64> {
    let playing = s.sketches.playing.as_ref()?;
    let t = now - playing.from;
    let next = playing
        .scene
        .stops()
        .into_iter()
        .find(|&stop| stop >= t - 1e-3)
        .filter(|_| t >= 0.0);
    if let Some(stop) = next
        && stop - t > 0.02
    {
        return Some(stop - t);
    }
    s.sketches.playing = None;
    s.sfx.clear();
    None
}

/// The playing sketch's lead, under `"Without Hex"`.
pub(super) fn lead(s: &Onboard) -> Option<&'static str> {
    on(s).map(|p| p.scene.script().lead)
}

/// The playing sketch's conversation.
pub(super) fn card(p: Paint, s: &Onboard) -> Option<El> {
    on(s).map(|playing| {
        el().key(format!("sketch-{}", playing.index))
            .w(CHAT_W)
            .transition(Transition::opacity())
            .child(versus::sketch_card(
                p,
                s,
                &playing.scene,
                s.now - playing.from,
            ))
    })
}

/// Scenes: `"Watch another one"` pressed on `"Almost ready"`, `secs` into
/// the first sketch (`HEX_SKETCH_SECS` moves it).
pub(in crate::rune_ui) fn fixture(ui: &mut super::super::Ui, secs: f64) {
    let Some(s) = ui.first_run.as_mut() else {
        return;
    };
    let secs = std::env::var("HEX_SKETCH_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(secs);
    watch(s);
    s.now += LEAD_IN + secs;
    s.sfx.clear();
}

/// The download's bar on `"Almost ready"`, with `"Watch another one"`
/// beside it while there's one left.
pub(super) fn progress(p: Paint, s: &Onboard, d: Download) -> El {
    let fill = d.fraction();
    let bar = col()
        .key("download-bar")
        .gap(8.0)
        .w(BAR_W)
        .matched(CAPSULE)
        .motion(FLY)
        .child(
            el().key("track")
                .w_full()
                .h(4.0)
                .radius(2.0)
                .fill(p.k.c.border_weak)
                .child(
                    el().key("fill")
                        .w(BAR_W * fill)
                        .h(4.0)
                        .radius(2.0)
                        .fill(p.muted()),
                ),
        )
        .child(
            text(status(s, d))
                .key("bytes")
                .font(TextStyle::new(12.0, 450.0).lh(16.0).features("'tnum' 1"))
                .color(p.faint()),
        );
    let button = offered(s).then(|| {
        let label = "Watch another one";
        let button = Button::new(label)
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Small);
        let button = if s.live.is_some() {
            button.on_press(live::on_ui(watch))
        } else {
            button.on_press(watch)
        };
        button
            .build(p.k)
            .key("watch-another")
            .transition(Transition::opacity())
    });
    row()
        .key("download-row")
        .gap(18.0)
        .center_y()
        .child(bar)
        .children(button)
}

/// `"312 of 731 MB · about 1 min left"`, the time only once the pace is
/// known.
fn status(s: &Onboard, d: Download) -> String {
    let mb = |bytes: u64| (bytes as f64 / 1e6).round();
    let bytes = format!("{} of {} MB", mb(d.downloaded), mb(d.total));
    match left(&s.sketches.samples, d) {
        Some(secs) if secs < 50.0 => format!("{bytes} · less than a minute left"),
        Some(secs) => format!(
            "{bytes} · about {} min left",
            (secs / 60.0).round().max(1.0)
        ),
        None => bytes,
    }
}

/// Seconds left at the pace of the last few seconds.
fn left(samples: &VecDeque<(f64, u64)>, d: Download) -> Option<f64> {
    let (&(t0, b0), &(t1, b1)) = (samples.front()?, samples.back()?);
    if t1 - t0 < PACE_MIN || b1 <= b0 || d.done() {
        return None;
    }
    let rate = (b1 - b0) as f64 / (t1 - t0);
    Some(d.total.saturating_sub(d.downloaded) as f64 / rate)
}

#[cfg(test)]
mod tests {
    use super::super::fixture::{Language, Speech};
    use super::super::stories::Flow;
    use super::super::typing::Timing;
    use super::*;

    #[test]
    fn every_sketch_is_short_and_stops_between_replies() {
        for (i, (name, _)) in SKETCHES.iter().enumerate() {
            let scene = Scene::play(&Timing::default(), script(i));
            let secs = scene.duration();
            assert!(
                (15.0..=18.5).contains(&secs),
                "{name} runs {secs:.1} s; keep it within 15–18 s"
            );
            let stops = scene.stops();
            assert!(stops.len() >= 3, "{name} stops only at {stops:?}");
            assert!(stops.windows(2).all(|w| w[0] < w[1]), "{name}: {stops:?}");
            for &stop in &stops[..stops.len() - 1] {
                assert!(
                    scene.draft(stop).0.is_empty(),
                    "{name} stops at {stop:.2} s with a draft in the field"
                );
            }
        }
    }

    fn waiting() -> Onboard {
        let mut s = Onboard::new(Flow::FirstRun);
        s.speech = Speech::AfterDownload;
        s.step = Step::Waiting;
        s.download = Some(Download {
            language: Language::English,
            downloaded: 300_000_000,
            total: 731_000_000,
        });
        s
    }

    #[test]
    fn a_finished_download_waits_for_the_next_stop() {
        let mut s = waiting();
        s.now = 10.0;
        watch(&mut s);
        assert!(playing(&s));
        let scene = Scene::play(&Timing::default(), script(0));
        let first = scene.stops()[0];
        s.download.as_mut().unwrap().downloaded = 731_000_000;
        let now = 10.0 + LEAD_IN + first - 0.5;
        s.now = now;
        let left = hold(&mut s, now).expect("plays on to its stop");
        assert!((left - 0.5).abs() < 1e-6, "{left}");
        s.now = now + left;
        assert_eq!(hold(&mut s, now + left), None);
        assert!(!playing(&s));
    }

    #[test]
    fn three_sketches_then_the_button_goes() {
        let mut s = waiting();
        for i in 0..SKETCHES.len() {
            assert!(offered(&s), "sketch {i}");
            watch(&mut s);
            s.now += 60.0;
        }
        assert!(!offered(&s));
    }

    #[test]
    fn time_left_shows_once_the_pace_is_known() {
        let d = |downloaded| Download {
            language: Language::English,
            downloaded,
            total: 731_000_000,
        };
        let mut samples = VecDeque::from([(0.0, 300_000_000)]);
        assert_eq!(left(&samples, d(300_000_000)), None);
        samples.push_back((4.0, 312_000_000));
        let secs = left(&samples, d(312_000_000)).unwrap();
        assert!((secs - 139.67).abs() < 0.1, "{secs}");
    }
}
