// The whole recording host in one replay: an offer, Record, the live pill,
// its panel under the pointer, Stop and the finished meeting, on one runtime
// and one surface. `HEX_HUD_REPLAY=<dir>` writes every frame and a trace of
// the surface; without it only the trace's claims are checked.

/// One step of a replay's script.
#[derive(Clone, Copy)]
enum Cue {
    Move(f32, f32),
    Over(&'static str),
    Leave,
    Record,
    Started,
    Refused,
    PressStop,
    Stopped,
    Published,
    Notes,
}

struct Replay {
    name: &'static str,
    reduced: bool,
    frames: usize,
    /// Starts as a live recording rather than an offer.
    live: bool,
    cues: &'static [(usize, Cue)],
}

const REPLAYS: &[Replay] = &[
    // The draft replay's schedule, frame for frame.
    Replay {
        name: "whole",
        reduced: false,
        frames: 555,
        live: false,
        cues: &[
            (40, Cue::Over("record")),
            (44, Cue::Record),
            (60, Cue::Move(417., 82.)),
            (96, Cue::Started),
            (172, Cue::Move(432., 23.)),
            (228, Cue::Over("stop")),
            (276, Cue::Over("copy-new")),
            (312, Cue::Over("copy-all")),
            (336, Cue::Over("copy-prompt")),
            (396, Cue::Over("stop")),
            (404, Cue::PressStop),
            (415, Cue::Move(214., 91.)),
            (425, Cue::Stopped),
            (470, Cue::Published),
            (493, Cue::Notes),
        ],
    },
    Replay {
        name: "fast",
        reduced: false,
        frames: 120,
        live: false,
        cues: &[
            (20, Cue::Over("record")),
            (24, Cue::Record),
            (27, Cue::Started),
        ],
    },
    Replay {
        name: "delayed",
        reduced: false,
        frames: 210,
        live: false,
        cues: &[
            (20, Cue::Over("record")),
            (24, Cue::Record),
            (114, Cue::Started),
        ],
    },
    // The pointer comes and goes before anything has landed, Stop is pressed
    // while the panel is still opening and the worker answers within frames.
    Replay {
        name: "interrupted",
        reduced: false,
        frames: 240,
        live: true,
        cues: &[
            (12, Cue::Move(432., 23.)),
            (20, Cue::Leave),
            (27, Cue::Move(432., 23.)),
            (33, Cue::Leave),
            (60, Cue::Move(432., 23.)),
            (74, Cue::PressStop),
            (78, Cue::Leave),
            (80, Cue::Stopped),
            (84, Cue::Published),
            (90, Cue::Notes),
        ],
    },
    Replay {
        name: "failure",
        reduced: false,
        frames: 110,
        live: false,
        cues: &[
            (20, Cue::Over("record")),
            (24, Cue::Record),
            (54, Cue::Refused),
        ],
    },
    Replay {
        name: "reduced",
        reduced: true,
        frames: 330,
        live: false,
        cues: &[
            (20, Cue::Over("record")),
            (24, Cue::Record),
            (40, Cue::Move(417., 82.)),
            (60, Cue::Started),
            (100, Cue::Move(432., 23.)),
            (150, Cue::Over("stop")),
            (170, Cue::PressStop),
            (180, Cue::Move(214., 91.)),
            (190, Cue::Stopped),
            (230, Cue::Published),
            (250, Cue::Notes),
        ],
    },
];

/// Two people talking, with one pause in the long replay so the field is
/// seen going quiet and coming back.
fn replay_levels(replay: &Replay, frame: usize) -> [f32; 2] {
    if replay.name == "whole" && (340..372).contains(&frame) {
        return [0.0; 2];
    }
    let t = frame as f32 / 60.0;
    let phrase = |speed: f32, beat: f32| ((t * speed).sin().abs() * (t * beat).cos().abs()).sqrt();
    [
        0.12 * phrase(13.0, 2.8).powi(3),
        0.09 * phrase(6.3, 4.1).powi(3),
    ]
}

/// One frame of a replay as the surface presented it.
#[derive(Clone, Debug, Default)]
struct Shot {
    phase: &'static str,
    /// The surface: x, y, width, height.
    pill: Option<[f32; 4]>,
    velocity: (f32, f32),
    veil: (f32, f32),
    /// Offer, "Starting…", meter, then Stop and the three copies.
    presence: [f32; 7],
    pending: Option<[f32; 4]>,
    elapsed: Option<[f32; 4]>,
    glyph: Option<[f32; 4]>,
    label: Option<([f32; 4], String)>,
    title: Option<String>,
    field: bool,
}

/// Plays `replay`, handing every frame to `each` once it is laid out.
fn play(
    replay: &Replay,
    mut each: impl FnMut(usize, &mut Runtime<Capsule>, &Shot),
) -> Vec<Shot> {
    use stage::Layer;
    let size = (500., 328.5);
    let kind = if replay.live {
        live_recording("replay")
    } else {
        offer(
            provider_cases()
                .into_iter()
                .find(|(s, _)| *s == "meet")
                .unwrap()
                .1,
        )
    };
    let (mut rt, commands, _) = runtime(kind, false, true, size);
    rt.transparent = true;
    rt.set_system_reduced_motion(replay.reduced);
    rt.update(|c, tx| {
        c.corner = MeetingIndicatorCorner::TopRight;
        tx.instant();
    });
    rt.frame(0.);
    let clock = Instant::now();
    let at = |frame: usize| clock + Duration::from_secs_f64(frame as f64 / 60.);
    if replay.live {
        rt.state.waveform = Some(Box::new(waveform::Listening::new(clock)));
        rt.state.live.entries = call();
    }
    let mut host = match rt.state.kind.as_ref() {
        Some(Kind::Offer(id, _)) => offer::Host::Offered(*id),
        _ => offer::Host::default(),
    };
    let mut live_since = replay.live.then_some(0usize);
    let mut pressed = None;
    let mut shots = Vec::new();
    for frame in 0..replay.frames {
        for (_, cue) in replay.cues.iter().filter(|(at, _)| *at == frame) {
            match *cue {
                Cue::Move(x, y) => rt.pointer_move(x, y),
                Cue::Over(key) => {
                    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key} at {frame}"));
                    rt.pointer_move(x, y);
                }
                Cue::Leave => rt.pointer_leave(),
                Cue::Record => {
                    let (x, y) = rt.find("record").unwrap();
                    rt.pointer_down(x, y);
                    rt.pointer_up(x, y);
                    pressed = Some(*cue);
                }
                Cue::Started => {
                    let event = started("replay");
                    assert_eq!(host.route(&event), offer::Route::Inherit);
                    let mut model = PillModel::default();
                    model.apply(&event, Instant::now());
                    let shown = model.pill(true).unwrap().clone();
                    rt.update(move |c, tx| {
                        c.start_recording(shown, tx);
                        c.live.entries = call();
                    });
                    rt.state.waveform = Some(Box::new(waveform::Listening::new(at(frame))));
                    live_since = Some(frame);
                }
                Cue::Refused => {
                    let event = ControllerEvent::Failed {
                        title: "Fixture".into(),
                        error: "Permission denied".into(),
                    };
                    assert_eq!(host.route(&event), offer::Route::Close);
                    rt.update(|c, _| c.close());
                }
                Cue::PressStop => {
                    // Wherever the pointer is, Stop is pressed where it is drawn.
                    let (x, y) = rt.find("stop").unwrap_or_else(|| panic!("no Stop at {frame}"));
                    rt.pointer_move(x, y);
                    rt.pointer_down(x, y);
                    rt.pointer_up(x, y);
                    pressed = Some(*cue);
                }
                Cue::Stopped => {
                    rt.update(|c, tx| {
                        c.capture_stopped();
                        c.stir(tx);
                    });
                }
                Cue::Published => rt.update(|c, tx| {
                    let Kind::Ready(mut m) = ready_transcript() else {
                        unreachable!()
                    };
                    m.duration_ms = Some(5_000);
                    c.published(m);
                    c.stir(tx);
                }),
                Cue::Notes => rt.update(|c, _| {
                    let Kind::Ready(mut m) = ready_summarized() else {
                        unreachable!()
                    };
                    m.duration_ms = Some(5_000);
                    c.refresh_ready(m);
                }),
            }
        }
        if let Some(since) = live_since {
            // The app's clock tick: the time redraws without any other cause.
            let run = if replay.live { 258_000 } else { 0 } + (frame - since) as u64 * 1000 / 60;
            rt.update(move |c, _| {
                if let Some(Kind::Recording(shown)) = &mut c.kind {
                    shown.started = Instant::now() - Duration::from_millis(run);
                }
            });
        }
        waveform::feed(&mut rt.state, at(frame), || replay_levels(replay, frame));
        rt.frame(1. / 60.);
        // A press is carried out in its own frame; nothing waits for motion.
        match pressed.take() {
            Some(Cue::Record) => {
                let Ok(ControllerCommand::Record(_, Some(id))) = commands.try_recv() else {
                    panic!("Record not sent")
                };
                assert_eq!(
                    host.route(&ControllerEvent::Starting(Some(id))),
                    offer::Route::Keep
                );
            }
            Some(_) => {
                assert!(matches!(commands.try_recv(), Ok(ControllerCommand::Stop)));
                assert!(rt.state.stopping, "Stop is true the frame it is pressed");
            }
            None => (),
        }
        assert!(commands.try_recv().is_err(), "an unasked command at {frame}");
        let c = &rt.state;
        let rect = |key: &str| rt.presented(key).map(|(rect, _)| rect);
        let shot = Shot {
            phase: match &c.kind {
                None => "closed",
                Some(Kind::Offer(..)) if c.presentation.departing() => "starting",
                Some(Kind::Offer(..)) => "offer",
                Some(Kind::Recording(_)) if c.stopping => "stopping",
                Some(Kind::Recording(_)) => "recording",
                Some(Kind::Finalizing { .. }) => "transcribing",
                Some(Kind::Ready(m)) if m.notes.is_some() => "summary",
                Some(Kind::Ready(_)) => "published",
                _ => "other",
            },
            pill: rect("pill"),
            velocity: c.stage.velocity(),
            veil: c.stage.veil(),
            presence: [
                Layer::Offer,
                Layer::Pending,
                Layer::Meter,
                Layer::Action(0),
                Layer::Action(1),
                Layer::Action(2),
                Layer::Action(3),
            ]
            .map(|layer| c.stage.presence(layer).opacity),
            pending: rect("pending"),
            elapsed: rect("elapsed"),
            glyph: rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("stop"))
                .and(rect("glyph")),
            label: rt.find("stop").and_then(|_| {
                let stop = rt.presented("stop")?.0;
                rt.elements()
                    .into_iter()
                    .filter(|e| e.key.as_deref() == Some("label"))
                    .find(|e| e.rect[1] >= stop[1] && e.rect[1] < stop[1] + stop[3])
                    .map(|e| (e.rect, e.text.unwrap_or_default()))
            }),
            title: rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("title"))
                .and_then(|e| e.text),
            field: rt.find("meeting-listening-field").is_some(),
        };
        each(frame, &mut rt, &shot);
        shots.push(shot);
    }
    shots
}

fn replay(name: &str) -> &'static Replay {
    REPLAYS.iter().find(|r| r.name == name).unwrap()
}

/// The right and top edges of the surface never leave the saved corner.
fn anchored(shots: &[Shot]) {
    for (frame, shot) in shots.iter().enumerate() {
        let Some([x, y, w, _]) = shot.pill else {
            continue;
        };
        assert!(
            (x + w - (500. - CAPSULE_INSET)).abs() < 0.03 && (y - CAPSULE_INSET).abs() < 0.03,
            "f{frame}: {:?}",
            shot.pill
        );
    }
}

/// The drafts entered "Starting…" at the centre of a shell that was still
/// contracting, so the words rode 90 pt across the screen; and they showed it
/// for a start that took three frames.
#[test]
fn starting_keeps_its_place_and_a_quick_start_never_says_it() {
    let shots = play(replay("delayed"), |_, _, _| {});
    anchored(&shots);
    // Its centre: it arrives a little small, about that point.
    let seen: Vec<(f32, f32)> = shots
        .iter()
        .filter_map(|s| s.pending)
        .map(|r| (r[0] + r[2] / 2., r[1] + r[3] / 2.))
        .collect();
    assert!(seen.len() > 60, "a slow start says Starting…");
    let rest = (500. - CAPSULE_INSET - REST_WIDTH / 2., CAPSULE_INSET + REST_HEIGHT / 2.);
    assert!(
        seen.iter()
            .all(|at| (at.0 - rest.0).abs() < 0.05 && (at.1 - rest.1).abs() < 0.05),
        "Starting… moved: {:?} from {rest:?}",
        seen.iter().find(|at| (at.0 - rest.0).abs() >= 0.05)
    );
    // It waits until the start has been slow, and leaves as the time arrives.
    assert!(shots[24..34].iter().all(|s| s.presence[1] == 0.0));
    let last = shots.last().unwrap();
    assert_eq!((last.presence[1], last.presence[2]), (0.0, 1.0));
    assert_eq!(last.pill.map(|r| (r[2], r[3])), Some((REST_WIDTH, REST_HEIGHT)));

    let shots = play(replay("fast"), |_, _, _| {});
    anchored(&shots);
    assert!(shots.iter().all(|s| s.presence[1] == 0.0 && s.pending.is_none()));
    // The offer contracts straight into the live pill, never growing back.
    let widths: Vec<f32> = shots[24..].iter().filter_map(|s| s.pill).map(|r| r[2]).collect();
    assert!(widths.windows(2).all(|w| w[1] <= w[0] + 0.02));
    assert_eq!(*widths.last().unwrap(), REST_WIDTH);
}

/// The drafts opened the width and height on one spring and mounted every
/// row whole in the first frame, cut by the moving edges.
#[test]
fn the_panel_opens_width_first_and_its_rows_follow_in_order() {
    let shots = play(replay("whole"), |_, _, _| {});
    anchored(&shots);
    let open = &shots[172..232];
    let (mut led, mut ordered) = (0, 0);
    for shot in open {
        let [_, _, w, h] = shot.pill.unwrap();
        let (dw, dh) = (
            (w - REST_WIDTH) / (PANEL_WIDTH - REST_WIDTH),
            (h - REST_HEIGHT) / (219. - REST_HEIGHT),
        );
        if dh > 0.02 && dh < 0.98 {
            assert!(dw > dh, "the width trails at {w}×{h}");
            led += 1;
        }
        let rows = &shot.presence[3..];
        assert!(rows.windows(2).all(|p| p[0] >= p[1]), "{rows:?}");
        ordered += usize::from(rows[0] > rows[3] && rows[3] > 0.0);
        // A moving edge is soft; a resting one cuts nothing.
        assert_eq!(shot.veil.0 > 0.0, shot.velocity.0 != 0.0 || shot.veil.0 > 0.0);
    }
    assert!(led > 8 && ordered > 4, "{led} {ordered}");
    assert!(open[1].presence[6] == 0.0, "the last row waits its turn");
    let rest = &shots[240];
    assert_eq!((rest.veil, rest.velocity), ((0., 0.), (0., 0.)));
    assert_eq!(rest.presence[3..], [1.0; 4]);
}

/// The drafts dropped Stop's glyph in the frame of the press and moved its
/// label 17 pt to restyle it. Then the field was cut the frame capture ended.
#[test]
fn stop_says_stopping_in_place_and_the_field_cools() {
    let shots = play(replay("whole"), |_, _, _| {});
    let (before, after) = (&shots[403], &shots[404]);
    assert_eq!((before.phase, after.phase), ("recording", "stopping"));
    assert_eq!(before.glyph, after.glyph);
    assert!(after.glyph.is_some());
    let (was, now) = (before.label.clone().unwrap(), after.label.clone().unwrap());
    assert_eq!((was.1.as_str(), now.1.as_str()), ("Stop", "Stopping…"));
    assert_eq!(was.0[..2], now.0[..2], "the label's first letters moved");
    // Capture ends at 425: the field is still drawn, quieter each frame.
    assert_eq!(shots[425].phase, "transcribing");
    assert!(shots[425].field && shots[440].field);
    assert!(shots[460].phase == "transcribing" && !shots[460].field);
    // Stop leaves over several frames while the title arrives.
    assert!(shots[426].presence[3] > 0.2 && shots[426].presence[3] < 1.0);
    assert_eq!(shots[460].presence[3], 0.0);
    // The time never moves through any of it.
    let clock = shots[400].elapsed.unwrap();
    for (frame, shot) in shots.iter().enumerate().skip(400) {
        let now = shot.elapsed.unwrap();
        assert!(
            (now[0] - clock[0]).abs() < 0.03 && (now[1] - clock[1]).abs() < 0.03,
            "f{frame}: {now:?}"
        );
    }
}

/// A pointer that comes and goes mid-flight turns the surface round from
/// where it is, at the speed it has; nothing restarts from rest.
#[test]
fn an_interrupted_panel_turns_round_without_a_stop_or_a_jump() {
    let shots = play(replay("interrupted"), |_, _, _| {});
    anchored(&shots);
    let sizes: Vec<(f32, f32)> = shots
        .iter()
        .map(|s| s.pill.map(|r| (r[2], r[3])).unwrap())
        .collect();
    // No frame moves the edge by more than a frame of its peak speed, and
    // its speed never changes by more than the spring can turn it.
    for (frame, w) in sizes.windows(3).enumerate() {
        let (a, b) = (w[1].1 - w[0].1, w[2].1 - w[1].1);
        assert!(b.abs() < 20.0, "f{frame}: a {b} pt step");
        assert!((b - a).abs() < 15.0, "f{frame}: speed {a} → {b}");
    }
    // Leaving at 20 reverses an opening panel: it carries on a little first.
    assert!(sizes[20].1 > sizes[19].1 && sizes[24].1 < sizes[21].1);
    let last = shots.last().unwrap();
    assert_eq!(last.phase, "summary");
    assert_eq!((last.veil, last.velocity), ((0., 0.), (0., 0.)));
}

#[test]
fn reduced_motion_lands_the_surface_and_only_fades() {
    let shots = play(replay("reduced"), |_, _, _| {});
    anchored(&shots);
    for (frame, pair) in shots.windows(2).enumerate() {
        assert_eq!(pair[1].veil, (0., 0.));
        let (Some(a), Some(b)) = (pair[0].pill, pair[1].pill) else {
            continue;
        };
        if pair[1].phase == "summary" || pair[1].phase == "offer" {
            continue;
        }
        // The outline is either where it was or where it is going.
        let rests = [
            (REST_WIDTH, REST_HEIGHT),
            (PANEL_WIDTH, 219.),
            (a[2], a[3]),
        ];
        assert!(
            rests
                .iter()
                .any(|(w, h)| (b[2] - w).abs() < 0.03 && (b[3] - h).abs() < 0.03),
            "f{frame}: {a:?} → {b:?}"
        );
    }
}

#[test]
fn hud_replay_frames() {
    use crate::rune_ui::offscreen;
    use std::fmt::Write;
    let Some(root) = std::env::var_os("HEX_HUD_REPLAY") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let only = std::env::var("HEX_HUD_CASE").ok();
    let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).unwrap();
    for replay in REPLAYS {
        if only.as_deref().is_some_and(|name| name != replay.name) {
            continue;
        }
        let dir = root.join(replay.name);
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (1000, 657);
        let mut renderer = offscreen::renderer(&gpu.device);
        let target = offscreen::target(&gpu.device, w, h);
        let mut ink = waveform::Ink::default();
        let shots = play(replay, |frame, rt, _| {
            let pixels = if rt.state.kind.is_some() {
                paint_recording(&gpu, &mut renderer, &target, rt, &mut ink)
            } else {
                vec![0; (w * h * 4) as usize]
            };
            rune::render::write_png(&dir.join(format!("{frame:03}.png")), w, h, &pixels).unwrap();
        });
        let mut trace = String::from(
            "frame,phase,x,y,w,h,vw,vh,veil_x,veil_y,offer,pending,meter,stop,copy_new,copy_all,copy_prompt,pending_x,elapsed_x,elapsed_y,label_x,title,field\n",
        );
        for (frame, s) in shots.iter().enumerate() {
            let [x, y, pw, ph] = s.pill.unwrap_or_default();
            let p = s.presence;
            let at = |r: Option<[f32; 4]>, i: usize| r.map_or(String::new(), |r| r[i].to_string());
            writeln!(
                trace,
                "{frame},{},{x},{y},{pw},{ph},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                s.phase,
                s.velocity.0,
                s.velocity.1,
                s.veil.0,
                s.veil.1,
                p[0],
                p[1],
                p[2],
                p[3],
                p[4],
                p[5],
                p[6],
                at(s.pending, 0),
                at(s.elapsed, 0),
                at(s.elapsed, 1),
                s.label.as_ref().map_or(String::new(), |l| l.0[0].to_string()),
                s.title.clone().unwrap_or_default().replace(',', " "),
                u8::from(s.field),
            )
            .unwrap();
        }
        std::fs::write(dir.join("trace.csv"), trace).unwrap();
    }
}
