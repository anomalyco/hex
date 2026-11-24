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
            ("OPENCODE IOS", "2m ago"),
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

/// The ready line and the list under it are one layout: the list's header,
/// its names and its reasons keep their 16 pt margins under the capsule's
/// walls; Paste, Search and ✕ are one row of 28 pt boxes 6 pt apart and
/// 6 pt in from the walls; and Paste's target stays the whole chip around
/// its fill.
#[test]
fn the_ready_line_and_its_list_share_their_edges() {
    use super::surface::{CHIP_H, ISLAND_H};
    let still = |name: &str| {
        let pres = stills().into_iter().find(|(n, _)| *n == name).unwrap().1;
        runtime(pres, true)
    };
    let near = |a: f32, b: f32| (a - b).abs() < 0.26;
    let rt = still("guess");
    let elements = rt.elements();
    let keyed = |key: &str| {
        elements
            .iter()
            .find(|e| e.key.as_deref().and_then(|k| k.split('#').next()) == Some(key))
            .unwrap_or_else(|| panic!("no {key}"))
            .rect
    };
    let worded = |text: &str| {
        elements
            .iter()
            .find(|e| e.text.as_deref() == Some(text))
            .unwrap_or_else(|| panic!("no {text:?}"))
            .rect
    };
    let boxed = |chip: &str| {
        elements
            .iter()
            .find(|e| e.key.as_deref() == Some("box") && e.path.contains(chip))
            .unwrap_or_else(|| panic!("no box in {chip}"))
            .rect
    };
    let (shell, list) = (keyed("shell"), keyed("list"));
    assert!(near(shell[0], list[0]) && near(shell[2], list[2]));

    // The margin is for words: the preview nests in the corner instead
    // (`the_preview_nests_in_the_capsules_left_corners`).
    for text in ["Send to", "SEER", "RUNE"] {
        assert!(
            near(worded(text)[0] - shell[0], 16.0),
            "{text} off the list's margin"
        );
    }
    let right = shell[0] + shell[2];
    for text in ["same page", "2 days ago"] {
        let [x, _, w, _] = worded(text);
        assert!(near(right - (x + w), 16.0), "{text} off the right edge");
    }

    let boxes = ["paste", "search", "close"].map(boxed);
    for [_, y, _, h] in boxes {
        assert!(
            near(h, 28.0) && near(y - shell[1], 6.0),
            "a box at {y}, {h}"
        );
        assert!(near(shell[1] + ISLAND_H - (y + h), 6.0));
    }
    for pair in boxes.windows(2) {
        let gap = pair[1][0] - (pair[0][0] + pair[0][2]);
        assert!(near(gap, 6.0), "boxes {gap} apart");
    }
    let [x, _, w, _] = boxes[2];
    assert!(
        near(right - (x + w), 6.0),
        "✕ is {} from the wall",
        right - (x + w)
    );

    // The fill is smaller than what takes the click.
    let [x, y, w, h] = keyed("paste");
    assert!(near(h, CHIP_H) && w >= 44.0);
    assert!(
        rt.hit_regions()
            .iter()
            .any(|r| near(r[0], x) && near(r[1], y) && near(r[2], x + w) && near(r[3], y + h)),
        "Paste's chip isn't a hit region: {:?}",
        rt.hit_regions()
    );

    // Folded, the words, Paste and ✕ keep 16 pt between them.
    let rt = still("saved");
    let elements = rt.elements();
    let mut found = [[0.0; 4]; 4];
    for e in &elements {
        let slot = match (e.key.as_deref(), e.text.as_deref()) {
            (Some("shell"), _) => 0,
            (Some("thumb"), _) => 1,
            (_, Some("Annotation copied")) => 2,
            (Some("box"), _) if e.path.contains("paste") => 3,
            _ => continue,
        };
        found[slot] = e.rect;
    }
    let [shell, thumb, words, fill] = found;
    assert!(near(words[0] - (thumb[0] + thumb[2]), 8.0), "{found:?}");
    assert!(near(fill[0] - (words[0] + words[2]), 16.0), "{found:?}");
    let right = shell[0] + shell[2];
    assert!(
        near(right - (fill[0] + fill[2]), 6.0 + 28.0 + 6.0),
        "{found:?}"
    );
}

/// The preview nests in the capsule's left corners: the 28 × 20 card clears
/// the left, top and bottom walls by the same 10 pt, so its 4 pt corners
/// turn about the centres the shell's 14 pt corners do. Its hairline, and
/// the chip that lights under it when it opens the capture, turn about them
/// too. Read from the drawn elements, with suggestions and folded, in both
/// appearances.
#[test]
fn the_preview_nests_in_the_capsules_left_corners() {
    use super::surface::{
        CHIP_RADIUS, DONE_TEXT_X, ISLAND_INSET, SHELL_RADIUS, THUMB_H, THUMB_RADIUS, THUMB_W,
    };
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    // A rounded rect's top-left and bottom-left corner centres.
    let centres = |[x, y, _, h]: [f32; 4], r: f32| [[x + r, y + r], [x + r, y + h - r]];
    for (name, dark) in [
        ("guess", true),
        ("guess", false),
        ("saved", true),
        ("saved", false),
    ] {
        let pres = stills().into_iter().find(|(n, _)| *n == name).unwrap().1;
        let rt = runtime(pres, dark);
        let elements = rt.elements();
        let drawn = |key: &str| {
            let e = elements
                .iter()
                .find(|e| e.key.as_deref().and_then(|k| k.split('#').next()) == Some(key))
                .unwrap_or_else(|| panic!("{name}: no {key}"));
            (e.rect, e.radius)
        };
        let (shell, shell_r) = drawn("shell");
        let (thumb, thumb_r) = drawn("thumb");
        let (edge, edge_r) = drawn("thumb-edge");
        assert!(near(shell_r, SHELL_RADIUS) && near(thumb_r, THUMB_RADIUS));
        assert!(near(thumb[2], THUMB_W) && near(thumb[3], THUMB_H) && near(thumb_r, 4.0));
        assert!(
            near(edge_r, thumb_r) && edge.iter().zip(thumb).all(|(a, b)| near(*a, b)),
            "{name}: the hairline at {edge:?}, the preview at {thumb:?}"
        );

        let left = thumb[0] - shell[0];
        let top = thumb[1] - shell[1];
        let bottom = shell[1] + shell[3] - (thumb[1] + thumb[3]);
        assert!(
            near(left, top) && near(top, bottom) && near(left, SHELL_RADIUS - THUMB_RADIUS),
            "{name}: the preview clears the walls by {left}, {top}, {bottom}"
        );
        let (outer, inner) = (centres(shell, shell_r), centres(thumb, thumb_r));
        for (outer, inner) in outer.iter().zip(inner) {
            assert!(
                near(outer[0], inner[0]) && near(outer[1], inner[1]),
                "{name}: the shell turns about {outer:?}, the preview about {inner:?}"
            );
        }
        // Folded, the preview opens the capture: its chip is concentric too,
        // and still starts at the row's inset, so its target is whole.
        if name == "saved" {
            let (chip, chip_r) = drawn("done");
            assert!(near(chip_r, CHIP_RADIUS) && near(chip[0] - shell[0], ISLAND_INSET));
            for (outer, inner) in outer.iter().zip(centres(chip, chip_r)) {
                assert!(
                    near(outer[0], inner[0]) && near(outer[1], inner[1]),
                    "{name}: the shell turns about {outer:?}, the chip about {inner:?}"
                );
            }
            assert!(
                rt.hit_regions()
                    .iter()
                    .any(|r| near(r[0], chip[0]) && near(r[2], chip[0] + chip[2])),
                "{name}: the preview's chip isn't a hit region"
            );
        }
    }

    // Without a frame there is no card: the check and its label stay put.
    let pres = stills().into_iter().find(|(n, _)| *n == "audio-only");
    let rt = runtime(pres.unwrap().1, true);
    let elements = rt.elements();
    assert!(!elements.iter().any(|e| e.key.as_deref() == Some("thumb")));
    let shell = elements.iter().find(|e| e.key.as_deref() == Some("shell"));
    // The lead reads its label too; the words themselves are drawn last.
    let words = elements
        .iter()
        .rfind(|e| e.text.as_deref() == Some("Annotation copied"));
    let (shell, words) = (shell.unwrap().rect, words.unwrap().rect);
    assert!(near(words[0] - shell[0], ISLAND_INSET + DONE_TEXT_X));
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

/// `HEX_CAPSULE_RENDERS=<folder>` draws a recording that shows its
/// microphone's level, quiet and speaking, light and dark. No golden checks
/// it; the plain recording's golden covers the capsule without one.
#[test]
#[ignore = "writes stills only when asked"]
fn renders_the_recording_level() {
    let Some(folder) = std::env::var_os("HEX_CAPSULE_RENDERS") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        eprintln!("skipping the level stills: no GPU adapter");
        return;
    };
    let (_, pres) = stills()
        .into_iter()
        .find(|(name, _)| *name == "recording")
        .unwrap();
    for (name, level) in [("quiet", 0.0), ("speaking", 0.8)] {
        for dark in [false, true] {
            let theme = if dark { "dark" } else { "light" };
            let state = CapsuleState::still(pres.clone(), "0:07", None).hearing(level);
            let mut rt = crate::rune_ui::offscreen::runtime(
                state,
                view,
                (PANEL_W, PANEL_H),
                env(dark),
                text(),
            );
            rt.settle_all();
            rt.frame(0.0);
            let (w, h, pixels) = draw(&gpu, &mut rt, TOP + 40.0 + 36.0);
            let path = folder.join(format!("capsule-level-{name}-{theme}.png"));
            rune::render::write_png(&path, w, h, &pixels).unwrap();
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
                false,
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
