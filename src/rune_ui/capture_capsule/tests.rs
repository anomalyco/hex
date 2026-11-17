//! The capsule's states at rest (goldens, and `HEX_CAPSULE_SNAPS` stills)
//! and its changes played frame by frame (motion checks, and with
//! `HEX_CAPSULE_MOTION` the frames and motion-lint traces).

use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::Mutex;

use rune::Runtime;
use rune::text::TextSystem;
use scrawl_capture::capture::Stage;

use super::flow::{
    Delivered, Dest, Failure, Flow, Grant, Input, Mode, Pres, Route, Undone, undo_s,
};
use super::surface::{Id, PANEL_H, PANEL_W, TOP};
use super::{CapsuleState, view};
#[cfg(target_os = "macos")]
use super::{flow::Action, poll, trace_channels};
use crate::capture_host::{AskedBy, CaptureNeed};

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
        site: Some("example.com".into()),
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
        site: Some("example.com".into()),
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
    /// The pointer moves onto the open list's row.
    Point(usize),
}

use Beat::{Click, In, Point};

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
            Point(row) => {
                flow.point(*row, true);
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
    let written = |routing| {
        In(Input::Written {
            bundle: "example.scrawl".into(),
            routing,
        })
    };
    let routed = |route: Route| vec![processing(), written(true), In(Input::Routed(route))];
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
        ("saved", after(&[written(false)])),
        ("audio-only", {
            let Pres::Done(mut done) = after(&[written(false)]) else {
                unreachable!()
            };
            done.no_frame = true;
            Pres::Done(done)
        }),
        (
            "hover",
            after(&[written(true), In(Input::Routed(guess())), Point(1)]),
        ),
        (
            "copy-failed",
            after(&[In(Input::CopyFailed), written(false)]),
        ),
        ("failed", after(&[In(failed())])),
        (
            "failed-start",
            after(&[In(Input::Failed(
                Failure::Start,
                Some("Screen Recording is off".into()),
            ))]),
        ),
        ("rule-sent", after(&routed(rule()))),
        (
            "rule-resolving",
            after(&routed(Route::ResolvingRule {
                name: "board".into(),
            })),
        ),
        ("rule-final", {
            let mut flow = Flow::new();
            flow.input(Input::Routed(rule()), 0.0);
            flow.input(Input::Settled, 0.2);
            flow.pres(undo_s() + 1.0)
        }),
        ("guess", after(&routed(guess()))),
        // Shift down with the pointer on HEX: that row is where it goes.
        ("shift", {
            let mut beats = routed(guess());
            beats.push(Point(1));
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
            after(&[In(Input::NeedsPermission(
                CaptureNeed::ScreenRecording,
                AskedBy::Capture,
            ))]),
        ),
        (
            "permission-asked",
            after(&[
                In(Input::NeedsPermission(
                    CaptureNeed::ScreenRecording,
                    AskedBy::Capture,
                )),
                Click(Id::Ask),
            ]),
        ),
        ("permission-restart", permission(Grant::Restart)),
        ("permission-ready", permission(Grant::Ready)),
        (
            "permission-microphone",
            after(&[In(Input::NeedsPermission(
                CaptureNeed::Microphone,
                AskedBy::Capture,
            ))]),
        ),
    ]
}

/// The permission helper for Screen Recording once its grant is `grant`.
fn permission(grant: Grant) -> Pres {
    after(&[
        In(Input::NeedsPermission(
            CaptureNeed::ScreenRecording,
            AskedBy::Capture,
        )),
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

/// A synthetic marked card, independent of any screen or settings.
fn thumb() -> Option<rune::Image> {
    static IMAGES: std::sync::OnceLock<rune::ImageCache> = std::sync::OnceLock::new();
    let images = IMAGES.get_or_init(|| rune::ImageCache::new(Arc::new(|| {})));
    let image = images.load(include_bytes!("../../../tests/fixtures/annotation-card.png").to_vec());
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
#[cfg(target_os = "macos")]
fn keys(rt: &Runtime<CapsuleState>) -> Vec<String> {
    rt.elements()
        .iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.key.as_deref())
        .map(|key| key.split('#').next().unwrap_or(key).to_owned())
        .collect()
}

#[test]
#[cfg(target_os = "macos")] // Includes the Mac's picker modes.
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
        ("saved", &["done", "paste", "close"]),
        ("failed", &["done", "close"]),
        // A rule's send counts down on the Undo chip, with no words.
        ("rule-sent", &["done", "undo", "close"]),
        ("rule-final", &["done", "inspect", "close"]),
        ("guess", &["done", "paste", "search", "list", "row-SEER"]),
        ("guess-five", &["done", "search", "list", "row-SEER"]),
        ("no-answer", &["done", "why", "search", "list"]),
        ("hover", &["shift-hint"]),
        ("shift", &["shift-hint"]),
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
        ("queued", &["done", "why", "close"]),
    ];
    // Paste and Inspect are for a written capture, and the open list's
    // field takes their place. The top line holds one action at most: Undo
    // before Paste and Inspect.
    let without: &[(&str, &[&str])] = &[
        ("recording-discard", &["cancel", "finish"]),
        ("failed", &["paste", "inspect"]),
        ("failed-start", &["paste", "inspect"]),
        ("list-guess", &["paste", "inspect"]),
        (
            "guess",
            &["field", "inspect", "footer", "shift-hint", "ask-keys"],
        ),
        ("queued", &["paste"]),
        ("rule-sent", &["why", "paste", "ask", "inspect"]),
        ("rule-final", &["ask", "paste"]),
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
        after(&[
            In(Input::Written {
                bundle: "long.scrawl".into(),
                routing: true,
            }),
            In(Input::Routed(ask)),
        ]),
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
        if let Pres::Done(done) = &pres
            && done.paste
        {
            assert!(
                shell[2] <= super::surface::LIST_W + 0.1,
                "long rows don't widen the header"
            );
            let paste = items.iter().find(|i| i.id == Id::Paste).unwrap();
            let search = items.iter().find(|i| i.id == Id::Search).unwrap();
            let close = items.iter().find(|i| i.id == Id::Close).unwrap();
            assert!(search.rect[0] + search.rect[2] < close.rect[0]);
            assert!(paste.rect[0] + paste.rect[2] < search.rect[0]);
            assert!(paste.rect[2] >= 44.0);
        }
    }
}

#[test]
fn the_annotation_toolbar_names_the_local_action_and_the_search_binding() {
    super::surface::FIXTURE_SHORTCUT.set(true);
    let pres = after(&[
        In(Input::Written {
            bundle: "example.scrawl".into(),
            routing: true,
        }),
        In(Input::Routed(guess())),
    ]);
    let rt = runtime(pres, true);
    let elements = rt.elements();
    for label in ["Paste", "Search ⌥⇧S", "Close"] {
        assert!(
            elements
                .iter()
                .any(|e| e.role == "button" && e.label.as_deref() == Some(label)),
            "missing {label}"
        );
    }
}

/// Keycaps, as Settings draws a shortcut, are for Search's chip and the
/// hints under the list; a chip on the top line says its shortcut in quiet
/// text ("Next ⇥").
#[test]
fn the_capsule_draws_keycaps_only_on_search_and_hints() {
    for (name, pres) in stills() {
        let rt = runtime(pres, false);
        for e in rt.elements() {
            let keycap = e
                .key
                .as_deref()
                .is_some_and(|k| k == "keys" || k.starts_with("keycap"));
            assert!(
                !keycap
                    || ["ask-keys", "footer", "card-hints"]
                        .iter()
                        .any(|at| e.path.contains(at)),
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
            let (shell, _) = layout_at(
                &pres,
                "0:00",
                anchor_for(&pres, Anchor::Top),
                super::notch::Geometry::default(),
            );
            (shell[1] + shell[3] + 36.0).max(TOP + 40.0 + 36.0)
        }
    };
    draw(gpu, &mut runtime(pres, dark), height)
}

/// Pixels of `rt`'s panel's top `height` points, at 2×.
fn draw(
    gpu: &rune::render::Gpu,
    rt: &mut Runtime<CapsuleState>,
    height: f32,
) -> (u32, u32, Vec<u8>) {
    let (w, h) = ((PANEL_W * 2.0) as u32, (PANEL_H * 2.0) as u32);
    let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
    let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
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
#[cfg(target_os = "macos")]
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
    "copy-failed",
    "rule-sent",
    "rule-resolving",
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

// The two ready surfaces also run on the Linux golden runner. Vulkan's
// glyph coverage differs from Metal's; do not replace the Mac references.
#[cfg(target_os = "linux")]
const GOLDENS: &[&str] = &["guess", "saved"];

/// States against `tests/golden/capture-capsule-*.png`;
/// `HEX_UPDATE_GOLDENS=1` rewrites them. Without a GPU adapter they skip.
#[test]
fn capture_capsule_goldens() {
    super::surface::FIXTURE_SHORTCUT.set(true);
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        eprintln!("skipping capture capsule goldens: no GPU adapter");
        return;
    };
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    #[cfg(target_os = "linux")]
    let folder = folder.join("linux");
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
            let actual = std::env::temp_dir().join(format!(
                "rune-settings-capture-capsule-{name}-{theme}-actual.png"
            ));
            if !path.exists() {
                rune::render::write_png(&actual, w, h, &pixels).unwrap();
                misses.push(format!(
                    "{name}-{theme}: missing golden; actual at {}",
                    actual.display()
                ));
                continue;
            }
            let (golden_w, golden_h, golden) = read_png(&path);
            if (golden_w, golden_h) != (w, h) {
                rune::render::write_png(&actual, w, h, &pixels).unwrap();
                misses.push(format!(
                    "{name}-{theme}: size changed; actual at {}",
                    actual.display()
                ));
                continue;
            }
            // GPUs differ in the last bit of blending and glyph coverage; a
            // layout or colour change moves far more.
            let differing = pixels
                .chunks(4)
                .zip(golden.chunks(4))
                .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 12))
                .count();
            if differing > (w * h) as usize / 500 {
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

/// `HEX_CAPSULE_SNAPS=<folder>` writes every state, light and dark;
/// `HEX_CAPSULE_LANGUAGE=de` (any interface language code) in that language.
#[test]
fn capture_capsule_snapshots() {
    super::surface::FIXTURE_SHORTCUT.set(true);
    let Some(folder) = std::env::var_os("HEX_CAPSULE_SNAPS") else {
        return;
    };
    top_line_from_env();
    if let Ok(language) = std::env::var("HEX_CAPSULE_LANGUAGE") {
        crate::desktop::i18n::apply(Some(&language));
    }
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

#[cfg(target_os = "macos")]
include!("tests/motion.rs");
