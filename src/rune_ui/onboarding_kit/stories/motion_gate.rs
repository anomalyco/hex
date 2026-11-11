//! The first run under Rune's motion gate: the story "Whole run, slow
//! download, a sketch" recorded headless around each beat, in dark and
//! light and under Reduce Motion, and checked for pops, jumps, blinks and
//! flickers ([`rune::motion_gate`]). `scripts/motion-audit` in Rune reads
//! the same variables: `MOTION_GATE=<cell part>`, `MOTION_GATE_OUT`,
//! `MOTION_GATE_DUMP` and `MOTION_GATE_SHEET`.

use rune::motion_gate::{self, AllowRow, Job, Recording, Run};
use rune::render::{Gpu, Offscreen, Renderer};
use rune::text::TextSystem;
use rune::{Env, Runtime};

use super::*;

const HZ: f64 = 60.0;

/// Findings that are the design, each with its reason, scoped to the cells
/// they happen in and capped at how many they cover. Authored bounces,
/// kicks, shakes and typing are marked where they're written
/// (`El::motion_intent`), so only shared-element hand-offs stay here.
const ALLOW: &[AllowRow] = &[
    AllowRow::new(
        "never-settles",
        "",
        "",
        "the story plays on past each window",
    ),
    // Shared-element hand-offs: the element taking over sits in the one
    // leaving's place on the same frame.
    AllowRow::new(
        "pop-out",
        "download-progress-pill",
        "entrance|parks",
        "flies up as the top bar's pill",
    )
    .max(6),
    AllowRow::new(
        "pop-out",
        "download-bar",
        "sketch",
        "flies up as the top bar's pill",
    )
    .max(3),
    AllowRow::new(
        "pop-out",
        "download-pill-shell",
        "almost-ready",
        "flies down as the bar",
    )
    .max(3),
    AllowRow::new(
        "overshoot",
        "track/fill",
        "almost-ready",
        "fills while the bar it rides lands",
    )
    .max(3),
    AllowRow::new(
        "appear-then-jump",
        "thread/hex-bubble",
        "words",
        "a sent line arrives whole and the thread makes room",
    )
    .max(3),
    AllowRow::new(
        "appear-then-jump",
        "thread/sent-bubble",
        "handoff",
        "a sent line arrives whole and the thread makes room",
    )
    .max(3),
];

/// The story as the playground opens it, before Play.
fn opened() -> Onboard {
    let mut s = Onboard::new(Flow::FirstRun);
    s.speech = Speech::AfterDownload;
    s.login = super::super::login::LoginAsk::preview();
    s.rehearsal = Some(Rehearsal::new(true));
    s.autoplay = true;
    s.download_secs = SKETCH_DOWNLOAD_SECS;
    s.manual_override = true;
    s
}

/// Play: the first run starts, its beats due on the story's own clock.
fn play(s: &mut Onboard, now: f64) {
    s.auto_due_at = begin_first_run(s, now).map(|secs| now + secs);
}

/// `s` in a window at its own `now`.
fn runtime_at(s: Onboard, dark: bool, reduced: bool) -> Run<Onboard> {
    let env = Env {
        system_dark: dark,
        scale: 1.0,
    };
    let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let now = s.now;
    let mut rt: Runtime<Onboard> =
        crate::rune_ui::offscreen::runtime(s, view, crate::rune_ui::WINDOW_SIZE, env, text);
    rt.clock = now;
    rt.reduced_motion = reduced;
    rt.poll = Some(poll);
    Run::new(rt, HZ)
}

/// The story played headless, without a window, until `beat` first holds;
/// `back` seconds before that moment.
fn before(reduced: bool, beat: fn(&Onboard) -> bool, back: f64) -> Onboard {
    let run = |until: &mut dyn FnMut(&Onboard, f64) -> bool| {
        let mut s = opened();
        s.reduced.set(reduced);
        play(&mut s, 0.0);
        let mut tx = Tx::default();
        let mut now = 0.0;
        while !until(&s, now) {
            assert!(now < 200.0, "the beat never came");
            now += 1.0 / HZ;
            poll(&mut s, now, &mut tx);
        }
        (s, now)
    };
    let (_, at) = run(&mut |s, _| beat(s));
    run(&mut |_, now| now >= at - back).0
}

/// How long a window plays before it records, so what mounted with it has
/// settled.
const SETTLE: f64 = 1.0;

/// An input the window gives the story, named.
type Act = (&'static str, fn(&mut Onboard, &mut Tx));

/// Plays the run up to `secs` before `beat`, then records `length` seconds,
/// with `act` (if any) as the input `before` seconds in.
struct Window {
    beat: fn(&Onboard) -> bool,
    before: f64,
    length: f64,
    act: Option<Act>,
}

fn record(w: &Window, dark: bool, reduced: bool) -> Recording {
    let s = before(reduced, w.beat, w.before + SETTLE);
    let mut r = runtime_at(s, dark, reduced);
    r.idle(SETTLE);
    r.step();
    match w.act {
        Some((what, act)) => {
            r.play(w.before);
            r.act(what, |rt| rt.update(act));
            r.play(w.length - w.before);
        }
        None => r.play(w.length),
    }
    r.rec
}

/// The window opening on `"Setting up Hex"`: the story as it opens, then
/// Play.
fn entrance(dark: bool, reduced: bool) -> Recording {
    let mut r = runtime_at(opened(), dark, reduced);
    r.idle(0.25);
    r.step();
    r.act("play", |rt| {
        rt.update(|s, _| {
            let now = s.now;
            play(s, now);
        })
    });
    // The download flies up to the top bar 1 s in.
    r.play(3.0);
    r.rec
}

fn parked(s: &Onboard) -> bool {
    s.step == Step::Parked
}

fn beat(beat: fn(&Onboard) -> bool, before: f64, length: f64) -> Window {
    Window {
        beat,
        before,
        length,
        act: None,
    }
}

fn windows() -> Vec<(&'static str, Window)> {
    vec![
        // The pill flies up and the conversation rises in under it.
        ("parks", beat(parked, 0.6, 3.0)),
        // Jay's messages land in it, and he answers his own question.
        (
            "opens",
            Window {
                beat: |s| s.parked_from.is_some_and(|at| s.now >= at + 1.0),
                before: 0.0,
                length: 3.0,
                act: None,
            },
        ),
        // Autocorrect's hit: the camera pushes in, shakes and pulls out.
        (
            "autocorrect",
            beat(
                |s| {
                    s.typing_from.is_some_and(|from| {
                        s.scene
                            .slams()
                            .next()
                            .is_some_and(|hit| s.now - from >= hit)
                    })
                },
                1.0,
                2.5,
            ),
        ),
        ("with-hex", beat(|s| s.step == Step::YourTurn, 0.5, 3.0)),
        ("words", beat(|s| s.step == Step::Hearing, 0.2, 4.0)),
        (
            "almost-ready",
            beat(|s| s.step == Step::Microphone, 0.5, 5.0),
        ),
        ("sketch", beat(super::super::sketches::playing, 1.5, 3.5)),
        (
            "handoff",
            beat(|s| s.download.is_some_and(|d| d.done()), 0.5, 9.0),
        ),
        ("game", beat(|s| s.step == Step::Try, 0.5, 8.0)),
    ]
}

fn cells() -> Vec<(String, Job)> {
    let mut cells: Vec<(String, Job)> = Vec::new();
    for (theme, dark, reduced) in [
        ("dark", true, false),
        ("light", false, false),
        ("reduced", true, true),
    ] {
        cells.push((
            format!("entrance/{theme}"),
            Box::new(move || entrance(dark, reduced)),
        ));
        for (name, window) in windows() {
            cells.push((
                format!("{name}/{theme}"),
                Box::new(move || record(&window, dark, reduced)),
            ));
        }
    }
    cells
}

#[test]
fn the_first_run_moves_cleanly() {
    motion_gate::gate_rows(cells(), ALLOW);
}

/// The window drawn as it shows now, 1× sRGB RGBA.
fn draw(
    gpu: &Gpu,
    renderer: &mut Renderer,
    target: &Offscreen,
    rt: &mut Runtime<Onboard>,
) -> Vec<u8> {
    let size = (target.width, target.height);
    renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut rt.text.atlas,
        &rt.instances,
        size,
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.draw(&mut encoder, &target.view, [0.0, 0.0, 0.0, 1.0]);
    target.copy(&mut encoder);
    gpu.queue.submit([encoder.finish()]);
    target.read(&gpu.device)
}

/// Mean luma of `rect` (`x, y, w, h` in points) in `rgba`, `width` wide.
fn luma(rgba: &[u8], width: u32, [x, y, w, h]: [u32; 4]) -> f32 {
    let mut sum = 0.0;
    for row in y..y + h {
        for col in x..x + w {
            let i = ((row * width + col) * 4) as usize;
            let [r, g, b] = [rgba[i], rgba[i + 1], rgba[i + 2]].map(f32::from);
            sum += 0.2126 * r + 0.7152 * g + 0.0722 * b;
        }
    }
    sum / (w * h) as f32
}

/// The motion gate sees rects and opacity, not paint: this one looks at the
/// pixels of the conversation's empty middle while the Launch Room's row
/// grows into it, in both appearances. Its surface must never blink, as it
/// did when the plate it grew on handed over to the card's own surface and
/// that faded in. `HEX_ONBOARDING_FRAMES=<folder>` also writes the frames.
#[test]
fn the_conversation_opens_without_a_blink() {
    let Ok(gpu) = Gpu::try_new(Gpu::instance(), None) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let folder = std::env::var_os("HEX_ONBOARDING_FRAMES").map(std::path::PathBuf::from);
    let (width, height) = (
        crate::rune_ui::WINDOW_SIZE.0 as u32,
        crate::rune_ui::WINDOW_SIZE.1 as u32,
    );
    let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
    let target = crate::rune_ui::offscreen::target(&gpu.device, width, height);
    // Inside the card, above the thread's first line and right of where
    // the row's faces and title fly up.
    const MIDDLE: [u32; 4] = [520, 190, 180, 90];
    for dark in [true, false] {
        let w = &windows()[1].1;
        let s = before(false, w.beat, w.before + SETTLE);
        let mut r = runtime_at(s, dark, false);
        r.idle(SETTLE);
        // The row is pressed 1.5 s in and has grown 0.75 s later.
        r.idle(1.45);
        let mut seen = Vec::new();
        for frame in 0..75 {
            r.rt.frame(r.dt);
            let rgba = draw(&gpu, &mut renderer, &target, &mut r.rt);
            seen.push(luma(&rgba, width, MIDDLE));
            if let Some(folder) = &folder {
                let theme = if dark { "dark" } else { "light" };
                let path = folder.join(format!("open-{theme}-{frame:03}.png"));
                rune::render::write_png(&path, width, height, &rgba).unwrap();
            }
        }
        // A blink: a frame darker or lighter than the one before, which the
        // frames after come at least halfway back from.
        for k in 1..seen.len() {
            let (was, is) = (seen[k - 1], seen[k]);
            let back = seen[k..(k + 20).min(seen.len())]
                .iter()
                .any(|&v| (v - was).abs() < (is - was).abs() * 0.5);
            assert!(
                (is - was).abs() < 1.5 || !back,
                "the conversation blinks at frame {k} (dark {dark}): luma {was:.1} → {is:.1}, then back",
            );
        }
    }
}
