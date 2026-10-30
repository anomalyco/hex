//! The capsule's states at rest (goldens, and `HEX_CAPSULE_SNAPS` stills)
//! and its changes played frame by frame (motion checks, and with
//! `HEX_CAPSULE_MOTION` the frames and motion-lint traces).

use std::sync::{Arc, Mutex};

use rune::Runtime;
use rune::text::TextSystem;
use scrawl_capture::capture::Stage;

use super::flow::{
    Action, Delivered, Dest, Failure, Flow, Grant, Input, Mode, Pres, Route, Undone, undo_s,
};
use super::surface::{Id, PANEL_H, PANEL_W, TOP};
use super::{CapsuleState, poll, trace_channels, view};
use crate::capture_host::CaptureNeed;

fn guess() -> Route {
    Route::Guess {
        suggested: vec![
            Dest::new("SEER", "same page"),
            Dest::new("HEX", "last sent"),
        ],
        recent: vec![
            Dest::new("HOME", "yesterday"),
            Dest::new("RUNE", "2 days ago"),
        ],
        site: Some("problems.kitlangton.com".into()),
    }
}

/// Shaped like the router's answer: a saved rule names no runner-up, so
/// its own session is all it suggests, and the recent ones leave it out.
fn rule() -> Route {
    let to = Dest::new("SEER", "sent · rule");
    Route::Rule {
        to: to.clone(),
        suggested: vec![to],
        recent: vec![
            Dest::new("HOME", "yesterday"),
            Dest::new("RUNE", "2 days ago"),
        ],
        site: Some("problems.kitlangton.com".into()),
    }
}

/// An ask with more sessions than it has chips for.
fn guess_five() -> Route {
    Route::Guess {
        suggested: [
            ("SEER", "you were just in it"),
            ("HEX", "same page"),
            ("HOME", "mentioned"),
            ("OPENCODE IOS", "2 min ago"),
            ("RUNE", "an hour ago"),
        ]
        .map(|(name, why)| Dest::new(name, why))
        .to_vec(),
        recent: Vec::new(),
        site: None,
    }
}

fn no_answer() -> Route {
    Route::NoAnswer {
        recent: vec![
            Dest::new("HOME", "yesterday"),
            Dest::new("HEX", "2 days ago"),
        ],
    }
}

/// What a script does at a moment.
#[derive(Clone, Debug)]
enum Beat {
    In(Input),
    Click(Id),
}

use Beat::{Click, In};

/// `pres` after `beats`, each a second apart, read just after the last.
fn after(beats: &[Beat]) -> Pres {
    let mut flow = Flow::new();
    let mut now = 0.0;
    for beat in beats {
        now += 1.0;
        match beat {
            In(input) => {
                flow.input(input.clone(), now);
            }
            Click(id) => {
                flow.click(*id, now);
            }
        }
    }
    flow.pres(now)
}

fn failed() -> Input {
    Input::Failed(
        Failure::Save,
        Some("writing the capture: the window closed".into()),
    )
}

/// The way to the picker in `mode`, as Scrawl has it: the shortcut's
/// recording, its Window chip, then the picker's own mode chip.
fn picking(mode: Mode) -> Vec<Beat> {
    let mut beats = vec![
        In(Input::Phone(mode == Mode::Iphone)),
        In(Input::Stage(Stage::Recording)),
        In(Input::Chips),
        Click(Id::Mode(Mode::Window)),
    ];
    if mode != Mode::Window {
        beats.push(Click(Id::Mode(mode)));
    }
    beats
}

fn processing() -> Beat {
    In(Input::Stage(Stage::Transcribing))
}

/// Every state the capsule rests in, by name.
fn stills() -> Vec<(&'static str, Pres)> {
    let routed = |route: Route| vec![processing(), In(Input::Routed(route))];
    let open = |route: Route| {
        let mut beats = routed(route);
        beats.push(In(Input::OpenList));
        beats
    };
    vec![
        ("picker-area", after(&picking(Mode::Area))),
        ("picker-window", after(&picking(Mode::Window))),
        ("picker-screen", after(&picking(Mode::Screen))),
        ("picker-iphone", after(&picking(Mode::Iphone))),
        ("recording", after(&[In(Input::Stage(Stage::Recording))])),
        (
            "recording-discard",
            after(&[In(Input::Stage(Stage::Recording)), Click(Id::Cancel)]),
        ),
        (
            "recording-chips",
            after(&[In(Input::Stage(Stage::Recording)), In(Input::Chips)]),
        ),
        (
            "recording-chips-iphone",
            after(&[
                In(Input::Phone(true)),
                In(Input::Stage(Stage::Recording)),
                In(Input::Chips),
            ]),
        ),
        ("processing", after(&[processing()])),
        ("saved", after(&[In(Input::Stage(Stage::Done))])),
        ("failed", after(&[In(failed())])),
        (
            "failed-start",
            after(&[In(Input::Failed(
                Failure::Start,
                Some("Screen Recording is off".into()),
            ))]),
        ),
        ("rule-sent", after(&routed(rule()))),
        ("rule-final", {
            let mut flow = Flow::new();
            flow.input(Input::Routed(rule()), 0.0);
            flow.input(Input::Settled, 0.2);
            flow.pres(undo_s() + 1.0)
        }),
        ("guess", after(&routed(guess()))),
        ("shift", {
            let mut beats = routed(guess());
            beats.push(In(Input::Shift(true)));
            after(&beats)
        }),
        ("sent-rule", {
            let mut beats = routed(guess());
            beats.push(In(Input::Shift(true)));
            beats.push(In(Input::SendRow(0)));
            beats.push(In(Input::Shift(false)));
            after(&beats)
        }),
        ("guess-five", after(&routed(guess_five()))),
        (
            "guess-long",
            after(&routed(Route::Guess {
                suggested: [
                    "OPENCODE IOS RELEASE",
                    "SCRAWL IN HEX BUILDER",
                    "CAPTURE CAPSULE",
                    "HOME",
                ]
                .map(|name| Dest::new(name, ""))
                .to_vec(),
                recent: Vec::new(),
                site: None,
            })),
        ),
        (
            "nothing",
            after(&routed(Route::Nothing {
                recent: vec![Dest::new("HOME", "yesterday")],
            })),
        ),
        ("no-answer", after(&routed(no_answer()))),
        ("nothing-heard", {
            let mut beats = vec![In(Input::NothingHeard)];
            beats.extend(routed(Route::Nothing {
                recent: vec![Dest::new("HOME", "yesterday")],
            }));
            after(&beats)
        }),
        ("list-guess", after(&open(guess()))),
        ("recording-drawing", {
            let mut flow = Flow::new();
            flow.input(Input::Stage(Stage::Recording), 0.0);
            flow.input(Input::Drawing(true), 0.5);
            flow.pres(1.0)
        }),
        ("list-rule", after(&open(rule()))),
        ("list-no-answer", after(&open(no_answer()))),
        ("list-typed", {
            let mut beats = open(guess());
            beats.push(In(Input::Type("he".into())));
            after(&beats)
        }),
        ("sent", {
            let mut beats = open(guess());
            beats.push(In(Input::Move(1)));
            beats.push(In(Input::Send { always: false }));
            after(&beats)
        }),
        ("rule-saved", {
            let mut beats = open(guess());
            beats.push(In(Input::Send { always: true }));
            after(&beats)
        }),
        (
            "delivered",
            after(&[In(Input::Delivered(Delivered::Sent { to: "SEER".into() }))]),
        ),
        (
            "queued",
            after(&[In(Input::Delivered(Delivered::Queued {
                to: "SEER".into(),
                why: "OpenCode can't see files".into(),
            }))]),
        ),
        ("taken-back", {
            let mut beats = routed(rule());
            beats.push(In(Input::Undone(Undone::TakenBack)));
            after(&beats)
        }),
        ("too-late", {
            let mut beats = routed(rule());
            beats.push(In(Input::Undone(Undone::TooLate)));
            after(&beats)
        }),
        (
            "permission",
            after(&[In(Input::NeedsPermission(CaptureNeed::ScreenRecording))]),
        ),
        (
            "permission-asked",
            after(&[
                In(Input::NeedsPermission(CaptureNeed::ScreenRecording)),
                Click(Id::Ask),
            ]),
        ),
        ("permission-restart", permission(Grant::Restart)),
        ("permission-ready", permission(Grant::Ready)),
        (
            "permission-microphone",
            after(&[In(Input::NeedsPermission(CaptureNeed::Microphone))]),
        ),
    ]
}

/// The permission helper for Screen Recording once its grant is `grant`.
fn permission(grant: Grant) -> Pres {
    after(&[
        In(Input::NeedsPermission(CaptureNeed::ScreenRecording)),
        In(Input::Permission(CaptureNeed::ScreenRecording, grant)),
    ])
}

fn text() -> TextSystem {
    TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES)
}

fn env(dark: bool) -> rune::Env {
    rune::Env {
        system_dark: dark,
        scale: 2.0,
    }
}

/// Every scene's first frame: a real capture's keyframe (an OpenCode
/// settings screen), cropped and scaled to the thumbnail's 2× size.
fn thumb() -> Option<rune::Image> {
    static IMAGES: std::sync::OnceLock<rune::ImageCache> = std::sync::OnceLock::new();
    let images = IMAGES.get_or_init(|| rune::ImageCache::new(Arc::new(|| {})));
    let image =
        images.load(include_bytes!("../../../tests/fixtures/capture-thumbnail.png").to_vec());
    image.wait().ok()?;
    // Decoding marks the image ready a moment before its pixels hold it.
    while image.pixels().size() == (0, 0) {
        std::thread::yield_now();
    }
    Some(image)
}

/// `pres` at rest.
fn runtime(pres: Pres, dark: bool) -> Runtime<CapsuleState> {
    let state = CapsuleState::still(pres, "0:07", thumb());
    let mut rt =
        crate::rune_ui::offscreen::runtime(state, view, (PANEL_W, PANEL_H), env(dark), text());
    rt.settle_all();
    rt.frame(0.0);
    rt
}

/// Live element keys, without their generations.
fn keys(rt: &Runtime<CapsuleState>) -> Vec<String> {
    rt.elements()
        .iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.key.as_deref())
        .map(|key| key.split('#').next().unwrap_or(key).to_owned())
        .collect()
}

#[test]
fn every_state_draws_its_pieces() {
    let expect: &[(&str, &[&str])] = &[
        (
            "picker-area",
            &["chip:Area", "chip:Window", "chip:Screen", "close"],
        ),
        ("picker-window", &["chip:Window", "close"]),
        ("picker-screen", &["record", "close"]),
        ("picker-iphone", &["chip:iPhone", "record", "close"]),
        ("recording", &["lead", "cancel", "finish"]),
        ("recording-discard", &["lead", "keep", "discard"]),
        (
            "recording-chips",
            &["lead", "chip:Area", "chip:Screen", "close"],
        ),
        (
            "recording-chips-iphone",
            &["lead", "chip:Area", "chip:Screen", "chip:iPhone", "close"],
        ),
        ("processing", &["processing"]),
        ("saved", &["done", "paste", "inspect", "close"]),
        ("failed", &["done", "close"]),
        ("rule-sent", &["done", "why", "undo", "close"]),
        ("rule-final", &["done", "why", "paste", "inspect", "close"]),
        ("guess", &["done", "ask", "list", "row-SEER", "remembers"]),
        ("guess-five", &["done", "ask", "list", "row-SEER"]),
        ("no-answer", &["done", "why", "ask", "list"]),
        (
            "list-guess",
            &["done", "field", "list", "row-SEER", "footer"],
        ),
        ("sent", &["done", "undo", "close"]),
        // Taken back, the suggestions again, the wrong one last.
        (
            "taken-back",
            &["done", "list", "row-HOME", "row-RUNE", "row-SEER"],
        ),
        ("queued", &["done", "why", "paste", "close"]),
    ];
    // Paste and Inspect are for a written capture, and the open list's
    // field takes their place. The top line holds one action at most: Undo
    // before Paste and Inspect.
    let without: &[(&str, &[&str])] = &[
        ("recording-discard", &["cancel", "finish"]),
        ("failed", &["paste", "inspect"]),
        ("failed-start", &["paste", "inspect"]),
        ("list-guess", &["paste", "inspect"]),
        // Trimmed to Search and ✕: the capture is already copied.
        ("guess", &["field", "paste", "inspect", "footer"]),
        ("rule-sent", &["paste", "ask", "inspect"]),
        ("rule-final", &["ask"]),
        ("sent", &["paste", "inspect"]),
    ];
    let all = stills();
    for (name, pieces) in expect {
        let pres = all
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.clone())
            .unwrap();
        let rt = runtime(pres, false);
        let keys = keys(&rt);
        for piece in *pieces {
            assert!(
                keys.iter().any(|k| k == piece),
                "{name} has no {piece}: {keys:?}"
            );
        }
        assert!(keys.iter().any(|k| k == "shell"), "{name} has no shell");
    }
    for (name, pieces) in without {
        let pres = all
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.clone())
            .unwrap();
        let keys = keys(&runtime(pres, false));
        for piece in *pieces {
            assert!(!keys.iter().any(|k| k == piece), "{name} has {piece}");
        }
    }
    let rt = runtime(Pres::Hidden, false);
    let elements = rt.elements();
    let shell = elements
        .iter()
        .find(|e| e.key.as_deref() == Some("shell"))
        .unwrap();
    assert!(shell.opacity < 0.01, "hidden shows its shell");
}

#[test]
fn the_widest_states_fit_the_panel() {
    let long = |n: usize| format!("A VERY LONG SESSION TITLE {n}");
    let rule = Route::Rule {
        to: Dest::new(long(0), ""),
        suggested: vec![Dest::new(long(0), "")],
        recent: vec![Dest::new(long(1), "")],
        site: None,
    };
    let ask = Route::Guess {
        suggested: (0..5).map(|n| Dest::new(long(n), "")).collect(),
        recent: Vec::new(),
        site: None,
    };
    let chips = [
        In(Input::Phone(true)),
        In(Input::Stage(Stage::Recording)),
        In(Input::Chips),
    ];
    for pres in [
        after(&[processing(), In(Input::Routed(rule))]),
        after(&[processing(), In(Input::Routed(ask))]),
        after(&chips),
    ] {
        let (shell, items) = super::surface::layout(&pres, "10:00:00");
        assert!(
            shell[0] >= 0.0 && shell[0] + shell[2] <= PANEL_W,
            "{shell:?} for {pres:?}"
        );
        // Long names give way before the one action does.
        if matches!(pres, Pres::Done(_)) {
            assert!(
                items
                    .iter()
                    .any(|item| matches!(item.id, Id::Paste | Id::Undo | Id::Ask)),
                "no action for {pres:?}"
            );
        }
    }
}

/// The capsule says its shortcuts in quiet text ("⏎", "esc"), a hint's
/// keys in one small rect at most. The one keycap tray is Search's: the
/// capture shortcut that opens it, drawn as Settings draws it.
#[test]
fn the_capsule_draws_keycaps_only_on_search() {
    for (name, pres) in stills() {
        let rt = runtime(pres, false);
        for e in rt.elements() {
            let keycap = e
                .key
                .as_deref()
                .is_some_and(|k| k == "keys" || k.starts_with("keycap"));
            assert!(
                !keycap || e.path.contains("ask-keys"),
                "{name}: {} is a keycap",
                e.path
            );
        }
    }
}

/// Pixels of the panel's top `height` points, at 2×.
fn render(gpu: &rune::render::Gpu, pres: Pres, dark: bool) -> (u32, u32, Vec<u8>) {
    let height = match &pres {
        Pres::Done(super::flow::Done {
            open: Some((_, list)),
            ..
        }) => super::surface::LIST_Y + super::list_height(list) + 28.0,
        _ => {
            // A card's guesses hang under the line.
            use super::surface::{Anchor, anchor_for, layout_at};
            let (shell, _) = layout_at(&pres, "0:00", anchor_for(&pres, Anchor::Top));
            (shell[1] + shell[3] + 36.0).max(TOP + 40.0 + 36.0)
        }
    };
    let rt = runtime(pres, dark);
    let (w, h) = ((PANEL_W * 2.0) as u32, (PANEL_H * 2.0) as u32);
    let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
    let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
    let mut rt = rt;
    renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut rt.text.atlas,
        &rt.instances,
        (w, h),
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.draw(&mut encoder, &target.view, rt.clear_color());
    target.copy(&mut encoder);
    gpu.queue.submit([encoder.finish()]);
    let full = target.read(&gpu.device);
    let crop = ((height * 2.0).ceil() as u32).min(h);
    (w, crop, full[..(4 * w * crop) as usize].to_vec())
}

fn read_png(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    (info.width, info.height, pixels)
}

/// The goldens' states: one of each kind of capsule.
const GOLDENS: &[&str] = &[
    "picker-area",
    "picker-window",
    "recording",
    "recording-discard",
    "recording-chips",
    "recording-chips-iphone",
    "recording-drawing",
    "picker-iphone",
    "processing",
    "saved",
    "rule-sent",
    "rule-final",
    "guess",
    "shift",
    "sent-rule",
    "guess-five",
    "no-answer",
    "list-guess",
    "sent",
    "queued",
    "permission",
    "permission-restart",
    "permission-ready",
];

/// States against `tests/golden/capture-capsule-*.png`;
/// `HEX_UPDATE_GOLDENS=1` rewrites them. Without a GPU adapter they skip.
#[test]
fn capture_capsule_goldens() {
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        eprintln!("skipping capture capsule goldens: no GPU adapter");
        return;
    };
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut misses = Vec::new();
    for (name, pres) in stills().into_iter().filter(|(n, _)| GOLDENS.contains(n)) {
        for dark in [false, true] {
            let theme = if dark { "dark" } else { "light" };
            let path = folder.join(format!("capture-capsule-{name}-{theme}.png"));
            let (w, h, pixels) = render(&gpu, pres.clone(), dark);
            if std::env::var_os("HEX_UPDATE_GOLDENS").is_some() {
                rune::render::write_png(&path, w, h, &pixels).unwrap();
                continue;
            }
            assert!(
                path.exists(),
                "{} is missing; run with HEX_UPDATE_GOLDENS=1",
                path.display()
            );
            let (golden_w, golden_h, golden) = read_png(&path);
            assert_eq!((golden_w, golden_h), (w, h), "{name}-{theme} size changed");
            // GPUs differ in the last bit of blending and glyph coverage; a
            // layout or colour change moves far more.
            let differing = pixels
                .chunks(4)
                .zip(golden.chunks(4))
                .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 12))
                .count();
            if differing > (w * h) as usize / 500 {
                let actual =
                    std::env::temp_dir().join(format!("capture-capsule-{name}-{theme}-actual.png"));
                rune::render::write_png(&actual, w, h, &pixels).unwrap();
                misses.push(format!(
                    "{name}-{theme}: {differing} pixels differ; actual at {}",
                    actual.display()
                ));
            }
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

/// `HEX_CAPSULE_TOP_LINE=quiet|icons` draws the finished capsule's top
/// line that way and `HEX_CAPSULE_FOOTER=band|well` the list's footer, to
/// compare them.
fn top_line_from_env() {
    let mut tuning = super::tuning::DEFAULT;
    if let Ok(name) = std::env::var("HEX_CAPSULE_TOP_LINE") {
        tuning.top_line = super::tuning::TopLine::ALL
            .into_iter()
            .find(|m| format!("{m:?}").eq_ignore_ascii_case(&name))
            .expect("HEX_CAPSULE_TOP_LINE is quiet or icons");
    }
    if let Ok(name) = std::env::var("HEX_CAPSULE_FOOTER") {
        tuning.list_footer = super::tuning::ListFooter::ALL
            .into_iter()
            .find(|m| format!("{m:?}").eq_ignore_ascii_case(&name))
            .expect("HEX_CAPSULE_FOOTER is band or well");
    }
    // `HEX_CAPSULE_FLOW=list|toast|menu_bar|cursor|notch` plays that flow,
    // and `HEX_CAPSULE_UNDO_S` shortens Undo's window for a short clip.
    if let Ok(name) = std::env::var("HEX_CAPSULE_FLOW") {
        tuning.flow = super::tuning::CapsuleFlow::ALL
            .into_iter()
            .find(|f| {
                format!("{f:?}")
                    .replace('_', "")
                    .eq_ignore_ascii_case(&name.replace('_', ""))
            })
            .expect("HEX_CAPSULE_FLOW is list, toast, menu_bar, cursor or notch");
    }
    if let Ok(secs) = std::env::var("HEX_CAPSULE_UNDO_S") {
        tuning.undo_s = secs.parse().expect("HEX_CAPSULE_UNDO_S is seconds");
    }
    if tuning != super::tuning::DEFAULT {
        super::tuning::set(tuning);
    }
}

/// `HEX_CAPSULE_SNAPS=<folder>` writes every state, light and dark.
#[test]
fn capture_capsule_snapshots() {
    let Some(folder) = std::env::var_os("HEX_CAPSULE_SNAPS") else {
        return;
    };
    top_line_from_env();
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        eprintln!("skipping capture capsule snapshots: no GPU adapter");
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    for (name, pres) in stills() {
        for dark in [false, true] {
            let (w, h, pixels) = render(&gpu, pres.clone(), dark);
            let theme = if dark { "dark" } else { "light" };
            let path = folder.join(format!("{name}-{theme}.png"));
            rune::render::write_png(&path, w, h, &pixels).unwrap();
        }
    }
}

// ── Motion ──────────────────────────────────────────────────────────────

const FPS: f64 = 60.0;

/// A capture's moments, played on the frame clock.
struct Script {
    name: &'static str,
    beats: Vec<(f64, Beat)>,
    end: f64,
}

fn scripts() -> Vec<Script> {
    let stage = |s| In(Input::Stage(s));
    vec![
        Script {
            name: "picker",
            beats: vec![
                (0.2, stage(Stage::Recording)),
                (0.2, In(Input::Chips)),
                (1.2, Click(Id::Mode(Mode::Area))),
                (2.0, Click(Id::Mode(Mode::Window))),
                (2.8, Click(Id::Mode(Mode::Screen))),
                (3.6, Click(Id::Mode(Mode::Area))),
                (4.4, Click(Id::Close)),
            ],
            end: 5.4,
        },
        Script {
            name: "record-window",
            beats: vec![
                (0.2, stage(Stage::Recording)),
                (0.2, In(Input::Chips)),
                (1.2, Click(Id::Mode(Mode::Window))),
                // The pick's own recording: a window, so no chips.
                (2.2, stage(Stage::Recording)),
                (3.6, Click(Id::Finish)),
                (3.6, stage(Stage::Stopping)),
                (4.8, stage(Stage::Done)),
                (6.2, In(Input::Dismiss)),
            ],
            end: 7.2,
        },
        Script {
            name: "iphone-switch",
            beats: vec![
                (0.1, In(Input::Phone(true))),
                (0.2, stage(Stage::Recording)),
                (0.2, In(Input::Chips)),
                (1.6, Click(Id::Mode(Mode::Iphone))),
                // The phone's capture takes the capsule over.
                (1.9, stage(Stage::Recording)),
                (3.2, Click(Id::Finish)),
                (3.2, stage(Stage::Stopping)),
            ],
            end: 4.4,
        },
        Script {
            name: "chips-close",
            beats: vec![(0.2, stage(Stage::Recording)), (1.0, In(Input::Chips))],
            end: 5.6,
        },
        // Escape ends nothing while recording; ✕ asks, and Discard ends it.
        Script {
            name: "cancel",
            beats: vec![
                (0.2, stage(Stage::Recording)),
                (1.0, In(Input::Escape)),
                (1.4, Click(Id::Cancel)),
                (2.2, Click(Id::Discard)),
            ],
            end: 3.2,
        },
        // Nothing clicked: the ask goes back to recording on its own.
        Script {
            name: "discard-ask",
            beats: vec![(0.2, stage(Stage::Recording)), (1.0, Click(Id::Cancel))],
            end: 1.0 + super::flow::DISCARD_ASK_S + 1.2,
        },
        Script {
            name: "rule-undo",
            beats: vec![
                (0.2, stage(Stage::Recording)),
                (1.2, stage(Stage::Transcribing)),
                (2.4, In(Input::Routed(rule()))),
                (3.6, Click(Id::Undo)),
                (4.0, In(Input::Undone(Undone::TakenBack))),
            ],
            end: 8.8,
        },
        // A new capture over a sent one: the finished capsule's content
        // gives way to the recording's, then the picker's.
        Script {
            name: "sent-then-record",
            beats: vec![
                (0.2, stage(Stage::Transcribing)),
                (1.2, In(Input::Routed(rule()))),
                (2.4, stage(Stage::Recording)),
                (2.6, In(Input::Chips)),
                (3.4, Click(Id::Mode(Mode::Area))),
            ],
            end: 5.0,
        },
        // The main path: the suggestions, Shift down, then a digit.
        Script {
            name: "suggest-pick",
            beats: vec![
                (0.2, stage(Stage::Transcribing)),
                (1.2, In(Input::Routed(guess()))),
                (2.2, In(Input::Shift(true))),
                (2.8, In(Input::SendRow(1))),
                (3.0, In(Input::Shift(false))),
            ],
            end: 6.4,
        },
        Script {
            name: "suggest-escape",
            beats: vec![
                (0.2, stage(Stage::Transcribing)),
                (1.2, In(Input::Routed(guess_five()))),
                (2.6, In(Input::Escape)),
            ],
            end: 4.4,
        },
        Script {
            name: "rule-sends",
            beats: vec![
                (0.2, stage(Stage::Transcribing)),
                (1.2, In(Input::Routed(rule()))),
                (1.5, In(Input::Settled)),
            ],
            // "Sent" at once, its Undo, then the list chip once it ends.
            end: 1.2 + undo_s() + 1.9,
        },
        Script {
            name: "guess-list-send",
            beats: vec![
                (0.2, stage(Stage::Transcribing)),
                (1.2, In(Input::Routed(guess()))),
                (2.2, In(Input::OpenList)),
                (3.0, In(Input::Move(1))),
                (3.5, In(Input::Move(1))),
                (4.0, In(Input::Type("ho".into()))),
                (4.8, In(Input::Send { always: false })),
            ],
            end: 9.0,
        },
        Script {
            name: "no-answer",
            beats: vec![
                (0.2, stage(Stage::Transcribing)),
                (1.2, In(Input::Routed(no_answer()))),
                (2.4, In(Input::OpenList)),
                (3.4, In(Input::Escape)),
                (4.4, In(Input::Dismiss)),
            ],
            end: 5.4,
        },
        // Every flow's whole path (`HEX_CAPSULE_FLOW`): a recording that
        // ⌥⇧S stops (the host's stages), the guesses come, one is picked (the toast's ⇥ then ⏎,
        // the rest a digit), Sent with Undo, gone; then a saved rule sends
        // on its own and leaves by itself.
        Script {
            name: "flow",
            beats: flow_beats(),
            end: 9.6,
        },
        Script {
            name: "failed",
            beats: vec![(0.2, stage(Stage::Transcribing)), (1.2, In(failed()))],
            end: 6.0,
        },
    ]
}

fn flow_beats() -> Vec<(f64, Beat)> {
    let stage = |s| In(Input::Stage(s));
    let toast = super::tuning::get().flow == super::tuning::CapsuleFlow::Toast;
    let mut beats = vec![
        (0.2, stage(Stage::Recording)),
        (1.6, stage(Stage::Stopping)),
        (1.8, stage(Stage::Transcribing)),
        (2.4, stage(Stage::Done)),
        (2.4, In(Input::Routed(guess()))),
    ];
    if toast {
        beats.push((3.1, In(Input::Cycle(1))));
        beats.push((3.8, In(Input::Send { always: false })));
    } else {
        beats.push((3.8, In(Input::SendRow(0))));
    }
    beats.extend([
        (6.4, stage(Stage::Transcribing)),
        (7.0, stage(Stage::Done)),
        (7.0, In(Input::Routed(rule()))),
        (7.2, In(Input::Settled)),
    ]);
    beats
}

#[derive(Clone, Copy, PartialEq)]
enum Variant {
    Calm,
    Reduced,
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Variant::Calm => "calm",
            Variant::Reduced => "reduced",
        }
    }
}

/// One frame of the shell.
struct Sample {
    t: f64,
    /// Rect, opacity.
    shell: Option<([f32; 4], f32)>,
    animating: bool,
    pres: Pres,
}

/// Plays `script` at 60 fps on the frame clock, as the overlay would, with
/// `trace` attached, handing every frame to `frame`.
fn play(
    script: &Script,
    variant: Variant,
    dark: bool,
    trace: Option<rune_motion::MotionTrace>,
    mut frame: impl FnMut(&mut Runtime<CapsuleState>, usize),
) -> (Vec<Sample>, Vec<Action>) {
    let mut state = CapsuleState::new(false);
    state.backdrop = true;
    let actions: Arc<Mutex<Vec<Action>>> = Arc::default();
    let record = actions.clone();
    state.act = Some(Arc::new(move |action| {
        record.lock().unwrap().push(action);
    }));
    let mut rt =
        crate::rune_ui::offscreen::runtime(state, view, (PANEL_W, PANEL_H), env(dark), text());
    rt.poll = Some(poll);
    rt.set_system_reduced_motion(variant == Variant::Reduced);
    rt.scene.set_trace(trace);
    rt.frame(0.0);
    let mut next = 0;
    let mut samples = Vec::new();
    let frames = (script.end * FPS).round() as usize;
    for index in 0..=frames {
        let t = index as f64 / FPS;
        while next < script.beats.len() && script.beats[next].0 <= t + 1e-9 {
            let beat = script.beats[next].1.clone();
            // As the feed does, the first frame lands just ahead of
            // "Copied".
            let frame = matches!(beat, In(Input::Stage(Stage::Done)))
                .then(thumb)
                .flatten();
            rt.update(move |s, tx| match beat {
                In(input) => {
                    if frame.is_some() {
                        s.live.thumb = frame;
                    }
                    s.inbox.push(input);
                    tx.wake_after(std::time::Duration::ZERO);
                }
                Click(id) => s.click(id, tx),
            });
            next += 1;
        }
        // The meter thread, on the frame clock.
        rt.state.live.meter.advance(t);
        let animating = rt.frame(if index == 0 { 0.0 } else { 1.0 / FPS });
        trace_channels(&mut rt);
        let shell = rt
            .elements()
            .iter()
            .find(|e| e.key.as_deref() == Some("shell"))
            .map(|e| (e.rect, e.opacity));
        samples.push(Sample {
            t,
            shell,
            animating,
            pres: rt.state.surface.pres.clone(),
        });
        frame(&mut rt, index);
    }
    if let Some(mut trace) = rt.scene.take_trace() {
        trace.flush().unwrap();
    }
    let actions = actions.lock().unwrap().clone();
    (samples, actions)
}

/// What's wrong with a script's motion: the shell leaving the panel or
/// jumping in a frame while it shows, or frames still asked for once
/// everything has landed.
fn faults(script: &Script, variant: Variant, samples: &[Sample]) -> Vec<String> {
    let mut faults = Vec::new();
    let mut previous: Option<([f32; 4], Option<[f32; 4]>)> = None;
    for sample in samples {
        let at = format!("{} {} at {:.3} s", script.name, variant.name(), sample.t);
        let Some((rect, opacity)) = sample.shell else {
            previous = None;
            continue;
        };
        let [x, y, w, h] = rect;
        if x < 0.0 || y < 0.0 || x + w > PANEL_W || y + h > PANEL_H {
            faults.push(format!("{at}: the shell {rect:?} leaves the panel"));
        }
        if let Some((prev, before)) = previous
            && opacity > 0.05
            && variant == Variant::Calm
        {
            // A spring leaves rest gently: a big first step is a jump.
            for (k, side) in ["x", "y", "w", "h"].iter().enumerate() {
                let step = (rect[k] - prev[k]).abs();
                let was = before.map_or(0.0, |b: [f32; 4]| (prev[k] - b[k]).abs());
                if step > 40.0 && was < 0.5 {
                    faults.push(format!(
                        "{at}: the shell's {side} jumps from rest {} → {} pt",
                        prev[k], rect[k]
                    ));
                }
            }
        }
        previous = Some((rect, previous.map(|(p, _)| p)));
    }
    // A second after the last change, with nothing looping, nothing asks
    // for a frame.
    let mut changed = 0.0;
    let mut late = None;
    for pair in samples.windows(2) {
        let s = &pair[1];
        if s.pres != pair[0].pres {
            changed = s.t;
        }
        if late.is_none()
            && s.t > changed + 1.0
            && s.animating
            && matches!(s.pres, Pres::Hidden | Pres::Selecting { .. })
        {
            late = Some(s);
        }
    }
    if let Some(late) = late {
        faults.push(format!(
            "{} {}: still asks for frames at {:.3} s",
            script.name,
            variant.name(),
            late.t
        ));
    }
    faults
}

#[test]
fn the_capsule_moves_inside_its_panel_and_comes_to_rest() {
    let mut faults = Vec::new();
    for script in scripts() {
        for variant in [Variant::Calm, Variant::Reduced] {
            let (samples, _) = play(&script, variant, true, None, |_, _| {});
            faults.extend(self::faults(&script, variant, &samples));
        }
    }
    assert!(faults.is_empty(), "{faults:#?}");
}

#[test]
fn presses_reach_the_host() {
    let script = scripts()
        .into_iter()
        .find(|s| s.name == "record-window")
        .unwrap();
    let (_, actions) = play(&script, Variant::Calm, true, None, |_, _| {});
    assert_eq!(actions, vec![Action::Discard, Action::Finish]);
    let script = scripts()
        .into_iter()
        .find(|s| s.name == "rule-undo")
        .unwrap();
    let (_, actions) = play(&script, Variant::Calm, true, None, |_, _| {});
    assert_eq!(actions, vec![Action::Undo { to: "SEER".into() }]);
    for (name, expected) in [
        ("cancel", vec![Action::Cancel]),
        ("discard-ask", Vec::new()),
    ] {
        let script = scripts().into_iter().find(|s| s.name == name).unwrap();
        let (samples, actions) = play(&script, Variant::Calm, true, None, |_, _| {});
        assert_eq!(actions, expected, "{name}");
        let last = &samples.last().unwrap().pres;
        match name {
            "cancel" => assert_eq!(*last, Pres::Hidden),
            _ => assert!(matches!(
                last,
                Pres::Recording {
                    discarding: false,
                    ..
                }
            )),
        }
    }
}

/// The live capsule after `inputs`, at rest, with every action it hands the
/// host recorded.
fn live(inputs: Vec<Input>) -> (Runtime<CapsuleState>, Arc<Mutex<Vec<Action>>>) {
    let mut state = CapsuleState::new(false);
    let actions: Arc<Mutex<Vec<Action>>> = Arc::default();
    let record = actions.clone();
    state.act = Some(Arc::new(move |action| record.lock().unwrap().push(action)));
    let mut rt =
        crate::rune_ui::offscreen::runtime(state, view, (PANEL_W, PANEL_H), env(true), text());
    rt.poll = Some(poll);
    rt.frame(0.0);
    for input in inputs {
        rt.update(move |s, tx| {
            s.inbox.push(input);
            tx.wake_after(std::time::Duration::ZERO);
        });
        settle(&mut rt);
    }
    (rt, actions)
}

/// A second of frames, enough for anything to land.
fn settle(rt: &mut Runtime<CapsuleState>) {
    for _ in 0..FPS as usize {
        rt.frame(1.0 / FPS);
    }
}

/// Clicks the middle of the live element keyed `key` as a Mac delivers it:
/// the pointer gets there, Hex's session tap hears the press first and
/// tells the capsule it may be a click away (`CapsuleKey::Press`), and only
/// then does the overlay get the press and the release.
fn click_on(rt: &mut Runtime<CapsuleState>, key: &str, nth: usize) {
    let [x, y, w, h] = rt
        .elements()
        .iter()
        .filter(|e| !e.exiting && e.key.as_deref().and_then(|k| k.split('#').next()) == Some(key))
        .nth(nth)
        .unwrap_or_else(|| panic!("no {key} {nth} in {:?}", keys(rt)))
        .rect;
    let (cx, cy) = (x + w * 0.5, y + h * 0.5);
    assert!(
        rt.hit_regions()
            .iter()
            .any(|r| cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3]),
        "{key} at ({cx}, {cy}) is outside the overlay's hit regions {:?}",
        rt.hit_regions()
    );
    rt.pointer_move(cx, cy);
    rt.frame(1.0 / FPS);
    rt.update(|s, tx| {
        s.inbox.push(Input::ClickAway);
        tx.wake_after(std::time::Duration::ZERO);
    });
    rt.frame(1.0 / FPS);
    rt.pointer_down(cx, cy);
    rt.frame(1.0 / FPS);
    rt.pointer_up(cx, cy);
    settle(rt);
}

/// Clicking any of the capsule's choices does what it says, in every flow:
/// a suggestion, a search result, Search, Undo and the list it brings
/// back, ✕, and each flow's picks. The tap's press
/// arrives before the click, so a click on the list must not read as a
/// click away from it.
#[test]
fn clicks_on_the_capsule_do_what_they_say() {
    use super::tuning::{self, CapsuleFlow};
    let routed = |route: Route| vec![processing_input(), Input::Routed(route)];
    let send = |to: &str| Action::Send {
        to: to.into(),
        always: None,
    };
    tuning::with_flow(CapsuleFlow::List, || {
        // A suggestion in the list under the capsule.
        let (mut rt, actions) = live(routed(guess()));
        click_on(&mut rt, "row-HEX", 0);
        assert_eq!(*actions.lock().unwrap(), vec![send("HEX")]);

        // A search result, opened over the suggestions.
        let mut inputs = routed(guess());
        inputs.push(Input::OpenList);
        let (mut rt, actions) = live(inputs);
        click_on(&mut rt, "row-RUNE", 0);
        assert_eq!(*actions.lock().unwrap(), vec![send("RUNE")]);

        // Search, beside "Copied", opens the search.
        let (mut rt, _) = live(routed(guess()));
        click_on(&mut rt, "ask", 0);
        assert!(keys(&rt).iter().any(|k| k == "field"), "{:?}", keys(&rt));

        // A rule's send: Undo takes it back and offers the list again, where
        // a click redirects it.
        let (mut rt, actions) = live(routed(rule()));
        click_on(&mut rt, "undo", 0);
        assert_eq!(
            *actions.lock().unwrap(),
            vec![Action::Undo { to: "SEER".into() }]
        );
        click_on(&mut rt, "row-HOME", 0);
        assert_eq!(
            *actions.lock().unwrap(),
            vec![Action::Undo { to: "SEER".into() }, send("HOME")]
        );

        // ✕ closes.
        let (mut rt, _) = live(routed(rule()));
        click_on(&mut rt, "close", 0);
        assert_eq!(rt.state.surface.pres, Pres::Hidden);
    });
    for flow in [
        CapsuleFlow::Cursor,
        CapsuleFlow::Notch,
        CapsuleFlow::MenuBar,
    ] {
        tuning::with_flow(flow, || {
            let (mut rt, actions) = live(routed(guess()));
            click_on(&mut rt, "pick", 1);
            assert_eq!(*actions.lock().unwrap(), vec![send("HEX")], "{flow:?}");
        });
    }
}

fn processing_input() -> Input {
    Input::Stage(Stage::Transcribing)
}

/// `HEX_CAPSULE_MOTION=<folder>` plays every script, light and dark, calm
/// and under Reduce Motion: motion-lint traces in
/// `<folder>/runs/<script>-<variant>/trace.jsonl` (`motion-lint analyze
/// <folder>`), and with `HEX_CAPSULE_FRAMES=1` each frame as a PNG in
/// `<folder>/frames/<script>-<variant>-<theme>/`. `HEX_CAPSULE_SCRIPTS`
/// picks scripts by name, and `HEX_CAPSULE_FLOW` the flow they play.
#[test]
fn capture_capsule_motion() {
    let Some(folder) = std::env::var_os("HEX_CAPSULE_MOTION") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    let frames = std::env::var_os("HEX_CAPSULE_FRAMES").is_some();
    let gpu = if frames {
        rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).ok()
    } else {
        None
    };
    let (w, h) = ((PANEL_W * 2.0) as u32, (PANEL_H * 2.0) as u32);
    let mut gpu_parts = gpu.as_ref().map(|gpu| {
        (
            crate::rune_ui::offscreen::renderer(&gpu.device),
            crate::rune_ui::offscreen::target(&gpu.device, w, h),
        )
    });
    let only = std::env::var("HEX_CAPSULE_SCRIPTS").ok();
    // `HEX_CAPSULE_FRAMES=all` writes Reduce Motion's frames too.
    let reduced_frames = std::env::var("HEX_CAPSULE_FRAMES").is_ok_and(|v| v == "all");
    top_line_from_env();
    let mut report = Vec::new();
    for script in scripts() {
        if only
            .as_deref()
            .is_some_and(|only| !only.split(',').any(|name| name == script.name))
        {
            continue;
        }
        for variant in [Variant::Calm, Variant::Reduced] {
            // Frames show the calm motion; Reduce Motion is checked by its
            // trace, unless asked for.
            let frames = frames && (reduced_frames || matches!(variant, Variant::Calm));
            for dark in [true, false] {
                let theme = if dark { "dark" } else { "light" };
                // `HEX_CAPSULE_THEME=light|dark` writes one theme's frames.
                let frames = frames
                    && std::env::var("HEX_CAPSULE_THEME")
                        .ok()
                        .is_none_or(|t| t == theme);
                // One trace per script and variant: the motion is the same
                // in either theme.
                let trace = dark.then(|| {
                    let run =
                        folder
                            .join("runs")
                            .join(format!("{}-{}", script.name, variant.name()));
                    std::fs::create_dir_all(&run).unwrap();
                    let header = rune_motion::TraceHeader {
                        launch: format!("{}-{}", script.name, variant.name()),
                        width: PANEL_W,
                        height: PANEL_H,
                        hz: FPS as f32,
                        scale: 2.0,
                    };
                    let file = std::fs::File::create(run.join("trace.jsonl")).unwrap();
                    rune_motion::MotionTrace::new(file, &header)
                        .unwrap()
                        .host("capsule")
                });
                if trace.is_none() && !frames {
                    continue;
                }
                let out = folder.join("frames").join(format!(
                    "{}-{}-{theme}",
                    script.name,
                    variant.name()
                ));
                if frames {
                    std::fs::create_dir_all(&out).unwrap();
                }
                let (samples, _) = play(&script, variant, dark, trace, |rt, index| {
                    let (true, Some(gpu), Some((renderer, target))) =
                        (frames, &gpu, &mut gpu_parts)
                    else {
                        return;
                    };
                    renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
                    renderer.prepare(
                        &gpu.device,
                        &gpu.queue,
                        &mut rt.text.atlas,
                        &rt.instances,
                        (w, h),
                    );
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    renderer.draw(&mut encoder, &target.view, rt.clear_color());
                    target.copy(&mut encoder);
                    gpu.queue.submit([encoder.finish()]);
                    let pixels = target.read(&gpu.device);
                    rune::render::write_png(&out.join(format!("{index:04}.png")), w, h, &pixels)
                        .unwrap();
                });
                if dark {
                    let found = faults(&script, variant, &samples);
                    report.push(format!(
                        "{} {}: {} frames, {}",
                        script.name,
                        variant.name(),
                        samples.len(),
                        if found.is_empty() {
                            "no faults".to_owned()
                        } else {
                            format!("{} faults:\n  {}", found.len(), found.join("\n  "))
                        }
                    ));
                }
            }
        }
    }
    std::fs::write(folder.join("report.txt"), report.join("\n") + "\n").unwrap();
}

// ── The ink overlay ─────────────────────────────────────────────────────

/// The ink overlay over a plain screen, as a still or a clip shows it.
fn ink_on_screen(s: &super::ink::InkState, cx: &mut rune::Cx<super::ink::InkState>) -> rune::El {
    let screen = if cx.system_dark() {
        rune::color::hex("#1d1e22")
    } else {
        rune::color::hex("#ececf0")
    };
    let [w, h] = s.scene().size;
    rune::el()
        .size(w, h)
        .fill(screen.unwrap_or(rune::Color::BLACK))
        .child(super::ink::view(s, cx))
}

/// A full-screen capture as it plays out: the start glow, ink drawn with
/// Shift held and lifted, then a laser sweep.
fn ink_at(now: u64) -> super::ink::Scene {
    use scrawl_capture::capture::InkSnapshot;

    use super::ink::tests::{frame, stroke, wave};
    let until = |mut stroke: scrawl_capture::model::Stroke| {
        let t0 = stroke.t0;
        let pts: Vec<i32> = stroke
            .pts
            .chunks(3)
            .filter(|p| t0 + p[2] as u64 <= now)
            .flatten()
            .copied()
            .collect();
        let last = pts.chunks(3).last()?[2] as u64;
        stroke.t1 = t0 + last;
        stroke.pts = pts;
        Some(stroke)
    };
    let ink = stroke(&wave([0.1, 0.32], [0.52, 0.3], 32, 0.06), 200, 12, false);
    let lifted = ink.t1;
    let laser = stroke(&wave([0.18, 0.72], [0.86, 0.52], 52, 0.07), 900, 13, true);
    let mut snapshot = InkSnapshot {
        target: frame(),
        strokes: Vec::new(),
        live: None,
        laser: until(laser),
        glow: true,
    };
    if now < lifted {
        snapshot.live = until(ink);
    } else {
        snapshot.strokes.push(ink);
    }
    let tip = (200..lifted).contains(&now).then(|| {
        let live = snapshot.live.as_ref().map(|s| s.normalized_points());
        let [x, y] = live.and_then(|p| p.last().copied()).unwrap_or([0.1, 0.32]);
        [x * 560.0, y * 320.0]
    });
    super::ink::Scene::at(&snapshot, now, tip)
}

/// `HEX_CAPSULE_SNAPS=<folder>` writes the ink overlay's moments as stills,
/// and with `HEX_CAPSULE_FRAMES=1` and `HEX_CAPSULE_MOTION=<folder>` the
/// whole capture as frames in `<folder>/frames/ink-<theme>/`.
#[test]
fn ink_overlay_stills_and_frames() {
    let snaps = std::env::var_os("HEX_CAPSULE_SNAPS").map(std::path::PathBuf::from);
    let motion = std::env::var_os("HEX_CAPSULE_MOTION")
        .filter(|_| std::env::var_os("HEX_CAPSULE_FRAMES").is_some())
        .map(std::path::PathBuf::from);
    if snaps.is_none() && motion.is_none() {
        return;
    }
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        eprintln!("skipping ink overlay stills: no GPU adapter");
        return;
    };
    let size = super::ink::tests::frame();
    let (pw, ph) = (size.width as f32, size.height as f32);
    let (w, h) = ((pw * 2.0) as u32, (ph * 2.0) as u32);
    let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
    let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
    let mut draw = |rt: &mut Runtime<super::ink::InkState>, path: &std::path::Path| {
        renderer.prepare(
            &gpu.device,
            &gpu.queue,
            &mut rt.text.atlas,
            &rt.instances,
            (w, h),
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.draw(&mut encoder, &target.view, rt.clear_color());
        target.copy(&mut encoder);
        gpu.queue.submit([encoder.finish()]);
        rune::render::write_png(path, w, h, &target.read(&gpu.device)).unwrap();
    };
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let state = super::ink::InkState::still(ink_at(0));
        let mut rt =
            crate::rune_ui::offscreen::runtime(state, ink_on_screen, (pw, ph), env(dark), text());
        rt.frame(0.0);
        if let Some(folder) = &snaps {
            std::fs::create_dir_all(folder).unwrap();
            for (name, ms) in [
                ("ink-glow", 160),
                ("ink-drawing", 470),
                ("ink-laser", 1_420),
                ("ink-pin", 1_760),
            ] {
                rt.update(|s, tx| {
                    tx.instant();
                    s.set_scene(ink_at(ms));
                });
                rt.frame(0.0);
                draw(&mut rt, &folder.join(format!("{name}-{theme}.png")));
            }
        }
        if let Some(folder) = &motion {
            let out = folder.join("frames").join(format!("ink-{theme}"));
            std::fs::create_dir_all(&out).unwrap();
            for index in 0..(2_400.0 / (1000.0 / FPS)) as u64 {
                let ms = (index as f64 * 1000.0 / FPS) as u64;
                rt.update(|s, tx| {
                    tx.instant();
                    s.set_scene(ink_at(ms));
                });
                rt.frame(1.0 / FPS);
                draw(&mut rt, &out.join(format!("{index:04}.png")));
            }
        }
    }
}

// ── The on-screen picker ────────────────────────────────────────────────

use super::picker::{self, Aim, PickerState};

/// The picker's display in the stills and clips, in points.
const DISPLAY: [f32; 2] = [960.0, 600.0];

fn picker_aim(mode: Mode, pointer: [f32; 2]) -> Aim {
    picker::tests::aim(mode, pointer)
}

fn picker_stills() -> Vec<(&'static str, Aim)> {
    vec![
        ("onscreen-window", picker_aim(Mode::Window, [800.0, 470.0])),
        (
            "onscreen-area",
            Aim {
                drag: Some([300.0, 180.0]),
                ..picker_aim(Mode::Area, [640.0, 420.0])
            },
        ),
        ("onscreen-screen", picker_aim(Mode::Screen, [480.0, 300.0])),
    ]
}

/// What a picker clip does, on the frame clock.
#[derive(Clone)]
enum Do {
    Want(Option<Mode>),
    /// Glides the pointer to here over `.1` seconds.
    Move([f32; 2], f64),
    Press([f32; 2]),
    /// Drags to here over `.1` seconds, then releases.
    Drag([f32; 2], f64),
}

struct PickerScript {
    name: &'static str,
    start: Aim,
    beats: Vec<(f64, Do)>,
    end: f64,
}

fn picker_scripts() -> Vec<PickerScript> {
    vec![
        PickerScript {
            name: "onscreen-window",
            start: picker_aim(Mode::Window, [80.0, 520.0]),
            beats: vec![
                (0.1, Do::Want(Some(Mode::Window))),
                (0.7, Do::Move([300.0, 200.0], 0.5)),
                (1.5, Do::Move([780.0, 480.0], 0.5)),
                (2.3, Do::Move([60.0, 560.0], 0.5)),
                (3.1, Do::Move([480.0, 320.0], 0.5)),
                (3.9, Do::Press([480.0, 320.0])),
                // The capture records: the capsule stops picking.
                (4.2, Do::Want(None)),
            ],
            end: 5.0,
        },
        PickerScript {
            name: "onscreen-area",
            start: picker_aim(Mode::Area, [240.0, 160.0]),
            beats: vec![
                (0.1, Do::Want(Some(Mode::Area))),
                // Too small to record: it closes onto the release.
                (0.6, Do::Press([240.0, 160.0])),
                (0.6, Do::Drag([262.0, 178.0], 0.15)),
                (1.0, Do::Move([300.0, 180.0], 0.5)),
                (1.8, Do::Press([300.0, 180.0])),
                (1.8, Do::Drag([660.0, 440.0], 0.8)),
                (3.0, Do::Want(None)),
            ],
            end: 3.8,
        },
        PickerScript {
            name: "onscreen-modes",
            start: picker_aim(Mode::Window, [380.0, 260.0]),
            beats: vec![
                (0.1, Do::Want(Some(Mode::Window))),
                (1.0, Do::Want(Some(Mode::Area))),
                (1.8, Do::Want(Some(Mode::Screen))),
                (2.6, Do::Want(Some(Mode::Window))),
                (3.4, Do::Want(None)),
            ],
            end: 4.2,
        },
    ]
}

/// A hand's glide: it starts and lands without a jolt.
fn ease(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (t * t * t * (t * (6.0 * t - 15.0) + 10.0)) as f32
}

fn lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// A glide or drag under way, and whether it's a drag that releases at
/// the end.
#[derive(Clone, Copy)]
struct Glide {
    from: [f32; 2],
    to: [f32; 2],
    start: f64,
    secs: f64,
    drag: bool,
}

impl Glide {
    fn new(from: [f32; 2], to: [f32; 2], start: f64, secs: f64, drag: bool) -> Self {
        Self {
            from,
            to,
            start,
            secs,
            drag,
        }
    }
}

/// The picker's samples: its opacity and the target's rect.
struct PickerSample {
    t: f64,
    opacity: f32,
    target: Option<[f32; 4]>,
    animating: bool,
}

/// Plays `script` at 60 fps, handing every frame to `frame`.
fn play_picker(
    script: &PickerScript,
    variant: Variant,
    dark: bool,
    trace: Option<rune_motion::MotionTrace>,
    mut frame: impl FnMut(&mut Runtime<PickerState>, usize),
) -> (Vec<PickerSample>, Vec<Action>) {
    let mut state = PickerState::still(script.start.clone(), false);
    let actions: Arc<Mutex<Vec<Action>>> = Arc::default();
    let record = actions.clone();
    state.set_act(Arc::new(move |action| record.lock().unwrap().push(action)));
    let mut rt = crate::rune_ui::offscreen::runtime(
        state,
        picker::view,
        (DISPLAY[0], DISPLAY[1]),
        env(dark),
        text(),
    );
    rt.poll = Some(picker::poll);
    rt.set_system_reduced_motion(variant == Variant::Reduced);
    rt.scene.set_trace(trace);
    rt.frame(0.0);
    let mut pointer = script.start.pointer.unwrap_or([0.0, 0.0]);
    let mut glide: Option<Glide> = None;
    let mut next = 0;
    let mut samples = Vec::new();
    let frames = (script.end * FPS).round() as usize;
    for index in 0..=frames {
        let t = index as f64 / FPS;
        while next < script.beats.len() && script.beats[next].0 <= t + 1e-9 {
            match script.beats[next].1.clone() {
                Do::Want(mode) => rt.update(move |s, tx| {
                    s.want(mode);
                    tx.wake_after(std::time::Duration::ZERO);
                }),
                Do::Move(to, secs) => glide = Some(Glide::new(pointer, to, t, secs, false)),
                Do::Drag(to, secs) => glide = Some(Glide::new(pointer, to, t, secs, true)),
                Do::Press(at) => {
                    pointer = at;
                    rt.update(move |s, tx| s.dragged(rune::Phase::Down, at, false, tx));
                }
            }
            next += 1;
        }
        if let Some(Glide {
            from,
            to,
            start,
            secs,
            drag,
        }) = glide
        {
            let k = ((t - start) / secs).min(1.0);
            pointer = lerp(from, to, ease(k));
            let at = pointer;
            rt.update(move |s, tx| {
                if drag {
                    s.dragged(rune::Phase::Move, at, false, tx);
                } else {
                    s.moved(at, tx);
                }
            });
            if k >= 1.0 {
                glide = None;
                if drag {
                    rt.update(move |s, tx| s.dragged(rune::Phase::Up, at, false, tx));
                }
            }
        }
        let animating = rt.frame(if index == 0 { 0.0 } else { 1.0 / FPS });
        let elements = rt.elements();
        let opacity = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("layer"))
            .map_or(0.0, |e| e.opacity);
        let target = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("target"))
            .map(|e| e.rect);
        rt.trace_channel("picker.alpha", "level", opacity);
        samples.push(PickerSample {
            t,
            opacity,
            target,
            animating,
        });
        frame(&mut rt, index);
    }
    if let Some(mut trace) = rt.scene.take_trace() {
        trace.flush().unwrap();
    }
    let actions = actions.lock().unwrap().clone();
    (samples, actions)
}

/// What's wrong with a picker clip: the target jumping while it shows, or
/// frames still asked for long after the last beat. A spring leaving rest
/// speeds up, so a jump is a big first step from rest that the next frame
/// doesn't keep up (a grow across the display moves far on its first frame,
/// and further on its second).
fn picker_faults(script: &PickerScript, variant: Variant, samples: &[PickerSample]) -> Vec<String> {
    let mut faults = Vec::new();
    for i in 2..samples.len().saturating_sub(1) {
        let (Some(before), Some(prev), Some(rect), Some(after)) = (
            samples[i - 2].target,
            samples[i - 1].target,
            samples[i].target,
            samples[i + 1].target,
        ) else {
            continue;
        };
        if samples[i].opacity <= 0.05 || variant != Variant::Calm {
            continue;
        }
        for k in 0..4 {
            let step = (rect[k] - prev[k]).abs();
            let was = (prev[k] - before[k]).abs();
            let next = (after[k] - rect[k]).abs();
            if step > 40.0 && was < 0.5 && next < step {
                faults.push(format!(
                    "{} {} at {:.3} s: the target jumps from rest {prev:?} → {rect:?}",
                    script.name,
                    variant.name(),
                    samples[i].t
                ));
            }
        }
    }
    let last = script.beats.last().map_or(0.0, |b| b.0);
    if let Some(late) = samples.iter().find(|s| s.t > last + 1.0 && s.animating) {
        faults.push(format!(
            "{} {}: still asks for frames at {:.3} s",
            script.name,
            variant.name(),
            late.t
        ));
    }
    faults
}

#[test]
fn the_picker_records_what_it_shows_and_comes_to_rest() {
    let mut faults = Vec::new();
    let mut picks = Vec::new();
    for script in picker_scripts() {
        for variant in [Variant::Calm, Variant::Reduced] {
            let (samples, actions) = play_picker(&script, variant, true, None, |_, _| {});
            faults.extend(picker_faults(&script, variant, &samples));
            if variant == Variant::Calm {
                picks.push((script.name, actions));
            }
        }
    }
    assert!(faults.is_empty(), "{faults:#?}");
    use crate::capture_host::CaptureTarget;
    assert_eq!(
        picks,
        vec![
            (
                "onscreen-window",
                vec![Action::Start(CaptureTarget::Window { id: 1 })]
            ),
            (
                "onscreen-area",
                vec![Action::Start(CaptureTarget::Area {
                    rect: [300.0, 180.0, 360.0, 260.0]
                })]
            ),
            ("onscreen-modes", vec![]),
        ]
    );
}

fn picker_runtime(aim: Aim) -> Runtime<PickerState> {
    let mut state = PickerState::still(aim, false);
    state.set_act(Arc::new(|_| {}));
    let mut rt = crate::rune_ui::offscreen::runtime(
        state,
        picker::view,
        (DISPLAY[0], DISPLAY[1]),
        env(true),
        text(),
    );
    rt.poll = Some(picker::poll);
    rt.frame(0.0);
    rt
}

fn run_for(rt: &mut Runtime<PickerState>, secs: f64) {
    for _ in 0..(secs * FPS).round() as usize {
        rt.frame(1.0 / FPS);
    }
}

/// A second display may sit left of or above the primary, at a negative
/// origin. The picker's rect is global, so it targets the primary display;
/// targeting the one under the pointer would add that origin twice.
#[test]
fn the_picker_covers_the_display_under_the_pointer_wherever_it_sits() {
    assert_eq!(picker::DISPLAY, rune::DisplayTarget::Primary);
    let mut rt = picker_runtime(picker_aim(Mode::Window, [100.0, 100.0]));
    for (display, pointer, local) in [
        (
            [-1440.0, -200.0, 1440.0, 900.0],
            [-500.0, 100.0],
            [940.0, 300.0],
        ),
        (
            [0.0, -1080.0, 1920.0, 1080.0],
            [960.0, -40.0],
            [960.0, 1040.0],
        ),
        (
            [1512.0, 0.0, 2560.0, 1440.0],
            [1600.0, 700.0],
            [88.0, 700.0],
        ),
    ] {
        let moved = Arc::new(Mutex::new(Vec::new()));
        let record = moved.clone();
        rt.update(move |s, tx| {
            let mut record = record.lock().unwrap();
            record.push(s.place_on(Mode::Window, display, pointer, tx));
            // Still on it: nothing moves.
            record.push(s.place_on(Mode::Window, display, [pointer[0] + 4.0, pointer[1]], tx));
        });
        assert_eq!(
            *moved.lock().unwrap(),
            [true, false],
            "a hop to {display:?} is a change"
        );
        let aim = rt.state.aim().unwrap();
        assert_eq!(aim.display, display);
        assert_eq!(aim.pointer, Some(local));
        assert_eq!(
            picker::placement(display),
            rune::OverlayPlacement::Rect {
                x: display[0],
                y: display[1],
                width: display[2],
                height: display[3]
            }
        );
    }
}

/// After a pick the picker fades while the capture starts. A start that
/// never comes back brings it back after the retry wait; a new mode chosen
/// meanwhile opens it again at once.
#[test]
fn a_pick_that_goes_nowhere_brings_the_picker_back() {
    let mut rt = picker_runtime(picker_aim(Mode::Window, [480.0, 320.0]));
    rt.update(|s, tx| {
        s.want(Some(Mode::Window));
        tx.wake_after(std::time::Duration::ZERO);
    });
    run_for(&mut rt, 0.5);
    assert!(rt.state.showing());
    rt.update(|s, tx| s.dragged(rune::Phase::Down, [480.0, 320.0], false, tx));
    run_for(&mut rt, 0.5);
    assert!(!rt.state.showing(), "it fades for the capture");
    run_for(&mut rt, 2.0);
    assert!(rt.state.showing(), "the start never came: it's back");

    rt.update(|s, tx| s.dragged(rune::Phase::Down, [480.0, 320.0], false, tx));
    run_for(&mut rt, 0.5);
    assert!(!rt.state.showing());
    rt.update(|s, tx| {
        s.want(Some(Mode::Area));
        tx.wake_after(std::time::Duration::ZERO);
    });
    run_for(&mut rt, 0.2);
    assert!(rt.state.showing(), "a new mode brings it back");
    assert_eq!(rt.state.aim().unwrap().mode, Mode::Area);
}

/// `HEX_CAPSULE_SNAPS=<folder>` writes the picker's stills, light and dark;
/// `HEX_CAPSULE_MOTION=<folder>` its motion-lint traces in
/// `<folder>/runs/<script>-<variant>/`, and with `HEX_CAPSULE_FRAMES=1` its
/// calm frames in `<folder>/frames/<script>-calm-<theme>/`.
#[test]
fn picker_stills_and_motion() {
    let snaps = std::env::var_os("HEX_CAPSULE_SNAPS").map(std::path::PathBuf::from);
    let motion = std::env::var_os("HEX_CAPSULE_MOTION").map(std::path::PathBuf::from);
    if snaps.is_none() && motion.is_none() {
        return;
    }
    let frames = std::env::var_os("HEX_CAPSULE_FRAMES").is_some();
    let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).ok();
    let (w, h) = ((DISPLAY[0] * 2.0) as u32, (DISPLAY[1] * 2.0) as u32);
    let mut parts = gpu.as_ref().map(|gpu| {
        (
            crate::rune_ui::offscreen::renderer(&gpu.device),
            crate::rune_ui::offscreen::target(&gpu.device, w, h),
        )
    });
    let mut draw = |rt: &mut Runtime<PickerState>, path: &std::path::Path| {
        let (Some(gpu), Some((renderer, target))) = (&gpu, &mut parts) else {
            return;
        };
        renderer.prepare(
            &gpu.device,
            &gpu.queue,
            &mut rt.text.atlas,
            &rt.instances,
            (w, h),
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.draw(&mut encoder, &target.view, rt.clear_color());
        target.copy(&mut encoder);
        gpu.queue.submit([encoder.finish()]);
        rune::render::write_png(path, w, h, &target.read(&gpu.device)).unwrap();
    };
    if let Some(folder) = &snaps {
        std::fs::create_dir_all(folder).unwrap();
        for (name, aim) in picker_stills() {
            for dark in [false, true] {
                let theme = if dark { "dark" } else { "light" };
                let state = PickerState::still(aim.clone(), true);
                let mut rt = crate::rune_ui::offscreen::runtime(
                    state,
                    picker::view,
                    (DISPLAY[0], DISPLAY[1]),
                    env(dark),
                    text(),
                );
                rt.frame(0.0);
                draw(&mut rt, &folder.join(format!("{name}-{theme}.png")));
            }
        }
    }
    let Some(folder) = motion else {
        return;
    };
    let mut report = Vec::new();
    for script in picker_scripts() {
        for variant in [Variant::Calm, Variant::Reduced] {
            for dark in [true, false] {
                let theme = if dark { "dark" } else { "light" };
                let framed = frames && variant == Variant::Calm;
                let trace = dark.then(|| {
                    let run =
                        folder
                            .join("runs")
                            .join(format!("{}-{}", script.name, variant.name()));
                    std::fs::create_dir_all(&run).unwrap();
                    let header = rune_motion::TraceHeader {
                        launch: format!("{}-{}", script.name, variant.name()),
                        width: DISPLAY[0],
                        height: DISPLAY[1],
                        hz: FPS as f32,
                        scale: 2.0,
                    };
                    let file = std::fs::File::create(run.join("trace.jsonl")).unwrap();
                    rune_motion::MotionTrace::new(file, &header)
                        .unwrap()
                        .host("picker")
                });
                if trace.is_none() && !framed {
                    continue;
                }
                let out = folder.join("frames").join(format!(
                    "{}-{}-{theme}",
                    script.name,
                    variant.name()
                ));
                if framed {
                    std::fs::create_dir_all(&out).unwrap();
                }
                let (samples, _) = play_picker(&script, variant, dark, trace, |rt, index| {
                    if framed {
                        draw(rt, &out.join(format!("{index:04}.png")));
                    }
                });
                if dark {
                    let found = picker_faults(&script, variant, &samples);
                    report.push(format!(
                        "{} {}: {} frames, {}",
                        script.name,
                        variant.name(),
                        samples.len(),
                        if found.is_empty() {
                            "no faults".to_owned()
                        } else {
                            found.join("; ")
                        }
                    ));
                }
            }
        }
    }
    std::fs::write(folder.join("onscreen-report.txt"), report.join("\n") + "\n").unwrap();
}
