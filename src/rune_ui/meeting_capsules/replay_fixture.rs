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
    /// A press on the element with this key, where it is drawn.
    Press(&'static str),
    /// The offer's 20 s ran out.
    Timeout,
    /// The controller offers the call again, in the same window.
    Offered,
    /// The finished meeting's notes arrive, or arrive again, with this title.
    Retitled(&'static str),
}

struct Replay {
    name: &'static str,
    reduced: bool,
    frames: usize,
    /// Starts as a live recording rather than an offer.
    live: bool,
    /// The surface is hidden before the first frame, as it is when the
    /// controller first shows it.
    enters: bool,
    cues: &'static [(usize, Cue)],
}

const REPLAYS: &[Replay] = &[
    // The draft replay's schedule, frame for frame.
    Replay {
        name: "whole",
        reduced: false,
        frames: 555,
        live: false,
        enters: false,
        cues: &[
            (40, Cue::Over("record")),
            (44, Cue::Record),
            (60, Cue::Move(417., 82.)),
            (96, Cue::Started),
            (172, Cue::Move(432., 23.)),
            (228, Cue::Over("stop")),
            (276, Cue::Over("copy-prompt")),
            (312, Cue::Over("copy-all")),
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
        enters: false,
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
        enters: false,
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
        enters: false,
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
        enters: false,
        cues: &[
            (20, Cue::Over("record")),
            (24, Cue::Record),
            (54, Cue::Refused),
        ],
    },
    // The offer arrives; Not now turns it down.
    Replay {
        name: "not-now",
        reduced: false,
        frames: 90,
        live: false,
        enters: true,
        cues: &[(36, Cue::Over("not-now")), (40, Cue::Press("not-now"))],
    },
    Replay {
        name: "not-now-reduced",
        reduced: true,
        frames: 90,
        live: false,
        enters: true,
        cues: &[(36, Cue::Over("not-now")), (40, Cue::Press("not-now"))],
    },
    // The other capsules arrive and leave the same way.
    Replay {
        name: "reminder",
        reduced: false,
        frames: 80,
        live: false,
        enters: true,
        cues: &[(36, Cue::Over("not-now")), (40, Cue::Press("not-now"))],
    },
    Replay {
        name: "call-ended",
        reduced: false,
        frames: 80,
        live: false,
        enters: true,
        cues: &[
            (36, Cue::Over("keep-recording")),
            (40, Cue::Press("keep-recording")),
        ],
    },
    Replay {
        name: "auto-started",
        reduced: false,
        frames: 80,
        live: false,
        enters: true,
        cues: &[(36, Cue::Over("dismiss")), (40, Cue::Press("dismiss"))],
    },
    // Nobody answers the offer.
    Replay {
        name: "timeout",
        reduced: false,
        frames: 70,
        live: false,
        enters: false,
        cues: &[(24, Cue::Timeout)],
    },
    // Refused six frames after Record: the offer is still contracting and
    // "Starting…" has not been said.
    Replay {
        name: "refused-early",
        reduced: false,
        frames: 80,
        live: false,
        enters: false,
        cues: &[
            (20, Cue::Over("record")),
            (24, Cue::Record),
            (30, Cue::Refused),
        ],
    },
    // ✕ on the finished meeting before its notes arrive; they arrive four
    // frames into its exit.
    Replay {
        name: "close",
        reduced: false,
        frames: 150,
        live: true,
        enters: false,
        cues: &[
            (10, Cue::Move(432., 23.)),
            (30, Cue::PressStop),
            (40, Cue::Stopped),
            (60, Cue::Published),
            (86, Cue::Over("dismiss")),
            (90, Cue::Press("dismiss")),
            (94, Cue::Notes),
        ],
    },
    // A worker that reports every phase within frames, then a late retitle.
    Replay {
        name: "titles",
        reduced: false,
        frames: 190,
        live: true,
        enters: false,
        cues: &[
            (10, Cue::Move(432., 23.)),
            (30, Cue::PressStop),
            (40, Cue::Stopped),
            (46, Cue::Published),
            (50, Cue::Notes),
            (54, Cue::Retitled("Lantern soak test and festival demo")),
            (120, Cue::Retitled("Lantern design sync")),
        ],
    },
    // Not now, and the call is offered again six frames into the exit.
    Replay {
        name: "not-now-then-offer",
        reduced: false,
        frames: 130,
        live: false,
        enters: false,
        cues: &[
            (20, Cue::Over("not-now")),
            (24, Cue::Press("not-now")),
            (30, Cue::Offered),
            (70, Cue::Over("record")),
            (74, Cue::Record),
            (80, Cue::Started),
        ],
    },
    // A refused start, and the call offered again four frames into its exit.
    Replay {
        name: "refused-then-offer",
        reduced: false,
        frames: 110,
        live: false,
        enters: false,
        cues: &[
            (20, Cue::Over("record")),
            (24, Cue::Record),
            (30, Cue::Refused),
            (34, Cue::Offered),
        ],
    },
    // A hundred minutes in: the time is a digit wider.
    Replay {
        name: "hundred",
        reduced: false,
        frames: 150,
        live: true,
        enters: false,
        cues: &[(20, Cue::Move(432., 23.)), (90, Cue::Leave)],
    },
    // The microphone is silent: the resting pill is already the panel's
    // width and says so beside the meter.
    Replay {
        name: "notice",
        reduced: false,
        frames: 150,
        live: true,
        enters: false,
        cues: &[(20, Cue::Move(432., 23.)), (90, Cue::Leave)],
    },
    // A replay named "left…" is played in the top-left corner. The pointer
    // opens and leaves the pill, then capture ends with the pill at rest.
    Replay {
        name: "left",
        reduced: false,
        frames: 220,
        live: true,
        enters: false,
        cues: &[
            (20, Cue::Move(68., 23.)),
            (90, Cue::Leave),
            (130, Cue::Stopped),
            (170, Cue::Published),
        ],
    },
    Replay {
        name: "left-stop",
        reduced: false,
        frames: 160,
        live: true,
        enters: false,
        cues: &[
            (20, Cue::Move(68., 23.)),
            (80, Cue::PressStop),
            (90, Cue::Stopped),
            (110, Cue::Published),
            (120, Cue::Notes),
        ],
    },
    Replay {
        name: "left-notice",
        reduced: false,
        frames: 150,
        live: true,
        enters: false,
        cues: &[(20, Cue::Move(68., 23.)), (90, Cue::Leave)],
    },
    Replay {
        name: "reduced",
        reduced: true,
        frames: 330,
        live: false,
        enters: false,
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

thread_local! {
    /// The levels the next frame's poll hears, and when.
    static FEED: Cell<Option<(Instant, [f32; 2])>> = const { Cell::new(None) };
}

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
    /// The window's width; an offer's follows its words.
    width: f32,
    /// The surface: x, y, width, height.
    pill: Option<[f32; 4]>,
    velocity: (f32, f32),
    veil: (f32, f32),
    /// Offer, "Starting…", meter, then Stop and the three copies.
    presence: [f32; 6],
    pending: Option<[f32; 4]>,
    elapsed: Option<[f32; 4]>,
    glyph: Option<[f32; 4]>,
    label: Option<([f32; 4], String)>,
    title: Option<String>,
    title_at: Option<[f32; 4]>,
    field: bool,
    /// How strongly the time, the field and Stop were drawn, ancestors included.
    shown: (f32, f32, f32),
    pointer: Option<(f32, f32)>,
    /// How strongly the whole surface and its title were drawn.
    surface: f32,
    title_shown: f32,
    /// The time as it reads, and the field's brightest dot as it is painted.
    clock: Option<String>,
    light: f32,
    /// The field's place, its columns, how far their middle sits from its
    /// own, and each column's light from the left.
    field_at: Option<[f32; 4]>,
    columns: (usize, f32, Vec<f32>),
    /// The header's notice: the place it shows through, and its words'
    /// place and strength.
    notice: Option<([f32; 4], [f32; 4], f32)>,
    /// How many times the capsule has asked for its window to hide.
    hidden: u64,
}

/// Plays `replay`, handing every frame to `each` once it is laid out.
fn play(
    replay: &Replay,
    mut each: impl FnMut(usize, &mut Runtime<Capsule>, &Shot),
) -> Vec<Shot> {
    use stage::Layer;
    let size = match replay.name {
        "reminder" => REMINDER,
        "call-ended" => CALL_ENDED,
        "auto-started" => PANEL,
        _ => (500., 328.5),
    };
    let kind = if replay.live {
        live_recording("replay")
    } else if replay.name == "reminder" {
        reminder(true, false)
    } else if replay.name == "call-ended" {
        call_ended()
    } else if replay.name == "auto-started" {
        auto_started()
    } else {
        offer(
            provider_cases()
                .into_iter()
                .find(|(s, _)| *s == "meet")
                .unwrap()
                .1,
        )
    };
    // The recording is dark in either appearance; the offer follows the host.
    let dark = std::env::var("HEX_HUD_THEME").as_deref() != Ok("light");
    let (mut rt, commands, _) = runtime(kind, false, dark, size);
    let width = if matches!(rt.state.kind, Some(Kind::Offer(..))) {
        rt.state.presentation.window().0
    } else {
        size.0
    };
    rt.transparent = true;
    rt.set_system_reduced_motion(replay.reduced);
    rt.poll = Some(|c: &mut Capsule, now, tx: &mut Tx| {
        let moved = c.step_stage(now, tx);
        if let Some((at, levels)) = FEED.take() {
            waveform::feed(c, at, || levels);
        }
        moved
    });
    if let Some(Kind::Recording(shown)) = &mut rt.state.kind {
        match replay.name {
            "notice" | "left-notice" => {
                shown.warning = Some(meeting::TrackWarning::DigitalSilence)
            }
            "hundred" => shown.started = Instant::now() - Duration::from_secs(6_005),
            _ => (),
        }
        settle_stage(&mut rt);
    }
    let corner = if replay.name.starts_with("left") {
        MeetingIndicatorCorner::TopLeft
    } else {
        MeetingIndicatorCorner::TopRight
    };
    rt.update(move |c, tx| {
        c.corner = corner;
        tx.instant();
    });
    rt.frame(0.);
    if replay.enters {
        rt.state.surface = Default::default();
    }
    let hides = Arc::new(AtomicU64::new(0));
    let hidden = hides.clone();
    rt.state.hide = Some(Arc::new(move || {
        hidden.fetch_add(1, Ordering::SeqCst);
    }));
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
    let mut pointer = None;
    let mut shots = Vec::new();
    for frame in 0..replay.frames {
        for (_, cue) in replay.cues.iter().filter(|(at, _)| *at == frame) {
            match *cue {
                Cue::Move(x, y) => {
                    pointer = Some((x, y));
                    rt.pointer_move(x, y);
                }
                Cue::Over(key) => {
                    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key} at {frame}"));
                    pointer = Some((x, y));
                    rt.pointer_move(x, y);
                }
                Cue::Leave => {
                    pointer = None;
                    rt.pointer_leave();
                }
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
                    // As `close_panel` does.
                    rt.update(|c, tx| c.dismiss(tx));
                }
                Cue::Press(key) => {
                    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key} at {frame}"));
                    pointer = Some((x, y));
                    rt.pointer_move(x, y);
                    rt.pointer_down(x, y);
                    rt.pointer_up(x, y);
                    pressed = Some(*cue);
                }
                Cue::Timeout => {
                    // As the offer's timer delivers it.
                    rt.update(|c, _| c.expire_offer());
                    pressed = Some(*cue);
                }
                Cue::Offered => {
                    let kind = offer(
                        provider_cases()
                            .into_iter()
                            .find(|(s, _)| *s == "meet")
                            .unwrap()
                            .1,
                    );
                    if let Kind::Offer(id, _) = &kind {
                        host = offer::Host::Offered(*id);
                    }
                    let presentation = offer_surface::Presentation::for_kind(&kind, false);
                    // As `show_panel` does.
                    rt.update(move |c, tx| {
                        c.show(kind, presentation, MeetingIndicatorCorner::TopRight, None, tx)
                    });
                }
                Cue::Retitled(title) => rt.update(move |c, _| {
                    let Kind::Ready(mut m) = ready_with(serde_json::json!({
                        "title": title,
                        "summary": "March 26 depends on passing the soak test by March 5. Friday's playtest decides the festival demo.",
                    })) else {
                        unreachable!()
                    };
                    m.duration_ms = c.length();
                    c.refresh_ready(m);
                }),
                Cue::PressStop => {
                    // Wherever the pointer is, Stop is pressed where it is drawn.
                    let (x, y) = rt.find("stop").unwrap_or_else(|| panic!("no Stop at {frame}"));
                    pointer = Some((x, y));
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
                    // The finished meeting is as long as its recording was.
                    m.duration_ms = Some(match c.kind {
                        Some(Kind::Finalizing { duration_ms }) => duration_ms,
                        _ => 5_000,
                    });
                    c.published(m);
                    c.stir(tx);
                }),
                Cue::Notes => rt.update(|c, _| {
                    let Kind::Ready(mut m) = ready_summarized() else {
                        unreachable!()
                    };
                    m.duration_ms = c.length();
                    c.refresh_ready(m);
                }),
            }
        }
        if let Some(since) = live_since {
            // The app's clock tick: the time redraws without any other cause.
            let run = match (replay.name, replay.live) {
                ("hundred", _) => 6_005_000,
                (_, true) => 258_000,
                _ => 0,
            } + (frame - since) as u64 * 1000 / 60;
            rt.update(move |c, _| {
                if let Some(Kind::Recording(shown)) = &mut c.kind {
                    shown.started = Instant::now() - Duration::from_millis(run);
                }
            });
        }
        // As the overlay polls: the surface steps, then the field samples.
        FEED.set(Some((at(frame), replay_levels(replay, frame))));
        rt.frame(1. / 60.);
        // As the compositor does before it paints.
        waveform::fit(&mut rt.state);
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
            Some(Cue::Press(key)) => {
                // Not now tells the controller; ✕ and Keep recording only close.
                let sent = commands.try_recv();
                match (replay.name, key) {
                    ("reminder", "not-now") => {
                        assert!(matches!(sent, Ok(ControllerCommand::DismissReminder(_))))
                    }
                    (_, "not-now") => assert!(matches!(sent, Ok(ControllerCommand::Dismiss))),
                    _ => assert!(sent.is_err(), "{key} sent a command"),
                }
                assert!(rt.state.kind.is_none(), "{key} is true the frame it is pressed");
            }
            Some(Cue::Timeout) => {
                assert!(matches!(
                    commands.try_recv(),
                    Ok(ControllerCommand::OfferTimedOut(_))
                ));
                assert!(rt.state.kind.is_none());
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
            width,
            phase: match &c.kind {
                None if c.surface.leaving() => "leaving",
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
            title_at: rect("title"),
            field: rt.find("meeting-listening-field").is_some(),
            shown: (
                rt.presented("elapsed").map_or(0., |(_, opacity)| opacity),
                rt.presented("meeting-listening-field")
                    .map_or(0., |(_, opacity)| opacity),
                rt.presented("stop").map_or(0., |(_, opacity)| opacity),
            ),
            pointer,
            surface: rt.presented("pill").map_or(0., |(_, opacity)| opacity),
            title_shown: rt.presented("title").map_or(0., |(_, opacity)| opacity),
            clock: rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("elapsed"))
                .and_then(|e| e.text),
            light: c.waveform.as_ref().map_or(0., |wave| wave.light()),
            field_at: rect("meeting-listening-field"),
            notice: rt.presented("notice-lane").map(|(lane, shown)| {
                let words = rt
                    .elements()
                    .into_iter()
                    .find(|e| e.key.as_deref() == Some("notice") && e.rect[1] < lane[1] + lane[3])
                    .unwrap();
                (lane, words.rect, shown)
            }),
            columns: c
                .waveform
                .as_ref()
                .map_or((0, 0., Vec::new()), |wave| wave.columns()),
            hidden: hides.load(Ordering::SeqCst),
        };
        each(frame, &mut rt, &shot);
        shots.push(shot);
    }
    shots
}

impl Capsule {
    /// The shown finished meeting's length, for notes that arrive for it.
    fn length(&self) -> Option<u64> {
        match &self.kind {
            Some(Kind::Ready(m)) => m.duration_ms,
            _ => Some(5_000),
        }
    }
}

impl Shot {
    /// Where the field's leftmost dot is centred.
    fn first_dot(&self) -> Option<f32> {
        let [x, _, w, _] = self.field_at?;
        (self.columns.0 > 0).then(|| {
            x + w / 2. + self.columns.1
                - (self.columns.0 - 1) as f32 * crate::rune_hud::dot_scope::Tuning::default().spacing
                    / 2.
        })
    }
}

fn replay(name: &str) -> &'static Replay {
    REPLAYS.iter().find(|r| r.name == name).unwrap()
}

/// A replay run for its trace's claims alone, with nothing watching live.
fn replayed(name: &str) -> Vec<Shot> {
    play(replay(name), |_, _, _| {})
}

/// The right and top edges of the surface never leave the saved corner.
fn anchored(shots: &[Shot]) {
    for (frame, shot) in shots.iter().enumerate() {
        let Some([x, y, w, _]) = shot.pill else {
            continue;
        };
        assert!(
            (x + w - (shot.width - CAPSULE_INSET)).abs() < 0.03 && (y - CAPSULE_INSET).abs() < 0.03,
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
    let shots = replayed("delayed");
    anchored(&shots);
    // Its centre: it arrives a little small, about that point.
    let seen: Vec<(f32, f32)> = shots
        .iter()
        .filter_map(|s| s.pending)
        .map(|r| (r[0] + r[2] / 2., r[1] + r[3] / 2.))
        .collect();
    assert!(seen.len() > 60, "a slow start says Starting…");
    let rest = (shots[0].width - CAPSULE_INSET - REST_WIDTH / 2., CAPSULE_INSET + REST_HEIGHT / 2.);
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

    let shots = replayed("fast");
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
    let shots = replayed("whole");
    anchored(&shots);
    let open = &shots[172..232];
    let (mut led, mut ordered) = (0, 0);
    for shot in open {
        let [_, _, w, h] = shot.pill.unwrap();
        let (dw, dh) = (
            (w - REST_WIDTH) / (PANEL_WIDTH - REST_WIDTH),
            (h - REST_HEIGHT) / (panel::open_height_without_notice() - REST_HEIGHT),
        );
        if dh > 0.02 && dh < 0.98 {
            assert!(dw > dh, "the width trails at {w}×{h}");
            led += 1;
        }
        let rows = &shot.presence[3..];
        assert!(rows.windows(2).all(|p| p[0] >= p[1]), "{rows:?}");
        ordered += usize::from(rows[0] > rows[2] && rows[2] > 0.0);
        // A moving edge is soft; a resting one cuts nothing.
        assert_eq!(shot.veil.0 > 0.0, shot.velocity.0 != 0.0 || shot.veil.0 > 0.0);
    }
    assert!(led > 8 && ordered > 4, "{led} {ordered}");
    assert!(open[1].presence[5] == 0.0, "the last row waits its turn");
    let rest = &shots[240];
    assert_eq!((rest.veil, rest.velocity), ((0., 0.), (0., 0.)));
    assert_eq!(rest.presence[3..], [1.0; 3]);
}

/// The drafts dropped Stop's glyph in the frame of the press and moved its
/// label 17 pt to restyle it. Then the field was cut the frame capture ended.
#[test]
fn stop_says_stopping_in_place_and_the_field_cools() {
    let shots = replayed("whole");
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
    // Stop leaves over several frames, drawn where it was pressed, while
    // the title arrives.
    assert!(shots[424].shown.2 > 0.999);
    assert!(shots[426].shown.2 > 0.2 && shots[426].shown.2 < 1.0, "{}", shots[426].shown.2);
    assert!(shots[427].shown.2 < shots[426].shown.2);
    assert!(shots[426].presence[3] > 0.2 && shots[426].presence[3] < 1.0);
    assert_eq!(shots[460].presence[3], 0.0);
    // The meter is one thing from its first frame to the finished meeting:
    // the drafts mounted it again at each change of state, and it dipped.
    for (frame, shot) in shots.iter().enumerate().skip(130) {
        assert!(shot.shown.0 > 0.999, "f{frame}: the time at {}", shot.shown.0);
        if frame < 425 {
            assert!(shot.shown.1 > 0.999, "f{frame}: the field at {}", shot.shown.1);
        }
    }
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
    let shots = replayed("interrupted");
    anchored(&shots);
    let sizes: Vec<(f32, f32)> = shots
        .iter()
        .map(|s| s.pill.map(|r| (r[2], r[3])).unwrap())
        .collect();
    // No frame moves the edge by more than a frame of its peak speed, and
    // no frame's travel differs from the one before by more than a launch
    // from rest does (6.8 pt), reversals included.
    for (frame, w) in sizes.windows(3).enumerate() {
        let (a, b) = (w[1].1 - w[0].1, w[2].1 - w[1].1);
        assert!(b.abs() < 20.0, "f{frame}: a {b} pt step");
        assert!((b - a).abs() < 7.5, "f{frame}: speed {a} → {b}");
    }
    // The first candidate dropped from 651 to -57 pt/s in the frame after
    // the pointer left at 20. The speed now bends round: in that frame the
    // edge still travels the way it was going, a little slower, and no frame
    // anywhere changes its speed by more than a launch from rest (532 pt/s).
    for at in [21, 28, 34] {
        let (before, after) = (shots[at - 1].velocity.1, shots[at].velocity.1);
        assert!(
            before * after > 0.0 && (after - before).abs() < 350.0,
            "f{at}: {before} → {after} pt/s"
        );
    }
    for (frame, pair) in shots.windows(2).enumerate() {
        let step = (pair[1].velocity.1 - pair[0].velocity.1).abs();
        assert!(step < 540.0, "f{}: the speed stepped {step} pt/s", frame + 1);
    }
    assert!(sizes[21].1 > sizes[20].1 && sizes[20].1 > sizes[19].1);
    assert!(sizes[26].1 < sizes[22].1);
    let last = shots.last().unwrap();
    assert_eq!(last.phase, "summary");
    assert_eq!((last.veil, last.velocity), ((0., 0.), (0., 0.)));
}

#[test]
fn reduced_motion_lands_the_surface_and_only_fades() {
    let shots = replayed("reduced");
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
            (PANEL_WIDTH, panel::open_height_without_notice()),
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
        if only
            .as_deref()
            .is_some_and(|names| !names.split(',').any(|name| name == replay.name))
        {
            continue;
        }
        let dir = root.join(replay.name);
        std::fs::create_dir_all(&dir).unwrap();
        // Two pixels per point; a panel capsule is painted at its window's size.
        let (w, h) = match replay.name {
            "reminder" | "call-ended" => (1040, 128),
            "auto-started" => (840, 128),
            _ => (1000, 657),
        };
        let mut renderer = offscreen::renderer(&gpu.device);
        let target = offscreen::target(&gpu.device, w, h);
        let mut ink = waveform::Ink::default();
        let shots = play(replay, |frame, rt, _| {
            // A closed capsule is painted until its window is asked to hide.
            let pixels = if rt.state.drawn().is_some() {
                paint_recording(&gpu, &mut renderer, &target, rt, &mut ink)
            } else {
                vec![0; (w * h * 4) as usize]
            };
            rune::render::write_png(&dir.join(format!("{frame:03}.png")), w, h, &pixels).unwrap();
        });
        let mut trace = String::from(
            "frame,phase,x,y,w,h,vw,vh,veil_x,veil_y,offer,pending,meter,stop,copy_prompt,copy_all,pending_x,elapsed_x,elapsed_y,label_x,title,field,elapsed_opacity,field_opacity,stop_opacity,pointer_x,pointer_y,surface,title_opacity,clock,light,field_x,field_w,columns,shift,first_dot_x,light_resting_columns,light_new_columns,title_x,notice_right,notice_opacity\n",
        );
        for (frame, s) in shots.iter().enumerate() {
            let [x, y, pw, ph] = s.pill.unwrap_or_default();
            let p = s.presence;
            let at = |r: Option<[f32; 4]>, i: usize| r.map_or(String::new(), |r| r[i].to_string());
            writeln!(
                trace,
                "{frame},{},{x},{y},{pw},{ph},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
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
                at(s.pending, 0),
                at(s.elapsed, 0),
                at(s.elapsed, 1),
                s.label.as_ref().map_or(String::new(), |l| l.0[0].to_string()),
                s.title.clone().unwrap_or_default().replace(',', " "),
                u8::from(s.field),
                s.shown.0,
                s.shown.1,
                s.shown.2,
                s.pointer.map_or(String::new(), |p| p.0.to_string()),
                s.pointer.map_or(String::new(), |p| p.1.to_string()),
                s.surface,
                s.title_shown,
                s.clock.clone().unwrap_or_default(),
                s.light,
                at(s.field_at, 0),
                at(s.field_at, 2),
                s.columns.0,
                s.columns.1,
                s.first_dot().map_or(String::new(), |x| x.to_string()),
                s.columns.2.iter().rev().take(31).sum::<f32>(),
                s.columns.2.iter().rev().skip(31).sum::<f32>(),
                at(s.title_at, 0),
                s.notice
                    .map_or(String::new(), |(lane, _, _)| (lane[0] + lane[2]).to_string()),
                s.notice
                    .map_or(String::new(), |(_, _, shown)| shown.to_string()),
            )
            .unwrap();
        }
        std::fs::write(dir.join("trace.csv"), trace).unwrap();
    }
}
