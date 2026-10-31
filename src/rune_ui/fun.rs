//! The Voice Filter pane (macOS, under Experiments while the `voice-filter`
//! experiment is on or the Hex Microphone is already in use): the
//! virtual microphone's driver, the filter mode and its key sliders, comfort
//! noise, and "Try it": record five seconds or use the example clip, then
//! compare your voice with what the call hears on two waveforms drawn with
//! Rune shapes. The driver, the live filter, and the demo audio stay with the
//! host (`PlatformState::{hex_microphone, tuning, demo}`, `PlatformRequest::
//! {SetTuning, HexMicrophoneDemo}`).

use rune::{El, LineCap, Shape, canvas, col, row, text};

use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, METRICS, Paint, button, card, page, pulsing_dot, segmented,
    setting_row, status_dot, toggle_row,
};
use super::lists::{Tone, slider, small_button, tone_tag};
use super::platform::{DemoRequest, PlatformRequest};
use crate::desktop_i18n::{tr, tr_fill};
use crate::hex_microphone::{
    DemoPlaybackTrack, DemoSnapshot, DemoSourceKind, DriverActionState, TunedFilterConfig,
};
use crate::voice_filter::{DictationSpan, VoiceFilterMode};

const SLIDER_WIDTH: f32 = 300.0;
const WAVE_HEIGHT: f32 = 28.0;

/// A tunable of the live filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tunable {
    Strength,
    MuffledWall,
    MuffledRoom,
    MuffledLevel,
    QuietLevel,
    MutedRoomTone,
    ChipmunkPitch,
    RobotCarrier,
    WalkieCrunch,
    UnderwaterDepth,
    FarRoom,
}

impl Tunable {
    fn key(self) -> &'static str {
        match self {
            Self::Strength => "fun-slider-strength",
            Self::MuffledWall => "fun-slider-wall",
            Self::MuffledRoom => "fun-slider-room",
            Self::MuffledLevel => "fun-slider-level",
            Self::QuietLevel => "fun-slider-quiet",
            Self::MutedRoomTone => "fun-slider-room-tone",
            Self::ChipmunkPitch => "fun-slider-pitch",
            Self::RobotCarrier => "fun-slider-carrier",
            Self::WalkieCrunch => "fun-slider-crunch",
            Self::UnderwaterDepth => "fun-slider-depth",
            Self::FarRoom => "fun-slider-far-room",
        }
    }

    /// Where the value sits on the slider, and how it reads.
    fn position(self, cfg: &TunedFilterConfig) -> (f32, String) {
        let norm = |v: f64, min: f64, max: f64| ((v - min) / (max - min)).clamp(0.0, 1.0) as f32;
        match self {
            Self::Strength => (
                cfg.strength.clamp(0.0, 1.0),
                format!("{:.0}%", cfg.strength * 100.0),
            ),
            Self::MuffledWall => (
                norm(cfg.muffled_wall_hz, 150.0, 3000.0),
                format!("{:.0} Hz", cfg.muffled_wall_hz),
            ),
            Self::MuffledRoom => (
                norm(cfg.muffled_room_mix, 0.0, 0.8),
                format!("{:.0}%", cfg.muffled_room_mix * 100.0),
            ),
            Self::MuffledLevel => (
                norm(cfg.muffled_level_db, -24.0, 0.0),
                format!("{:.0} dB", cfg.muffled_level_db),
            ),
            Self::QuietLevel => (
                norm(cfg.quiet_level_db, -36.0, -6.0),
                format!("{:.0} dB", cfg.quiet_level_db),
            ),
            Self::MutedRoomTone => (
                norm(cfg.muted_room_tone_db, -80.0, -42.0),
                format!("{:.0} dB", cfg.muted_room_tone_db),
            ),
            Self::ChipmunkPitch => (
                norm(cfg.chipmunk_semitones, 2.0, 12.0),
                format!("+{:.1} st", cfg.chipmunk_semitones),
            ),
            Self::RobotCarrier => (
                norm(cfg.robot_carrier_hz, 30.0, 180.0),
                format!("{:.0} Hz", cfg.robot_carrier_hz),
            ),
            Self::WalkieCrunch => (
                norm(cfg.walkie_crunch, 1.0, 8.0),
                format!("{:.1}x", cfg.walkie_crunch),
            ),
            Self::UnderwaterDepth => (
                norm(cfg.underwater_depth_hz, 160.0, 900.0),
                format!("{:.0} Hz", cfg.underwater_depth_hz),
            ),
            Self::FarRoom => (
                norm(cfg.far_distance_mix, 0.2, 0.95),
                format!("{:.0}%", cfg.far_distance_mix * 100.0),
            ),
        }
    }

    /// Sets the value from a slider position, rounded to the tunable's step.
    fn apply(self, cfg: &mut TunedFilterConfig, position: f32) {
        let t = f64::from(position.clamp(0.0, 1.0));
        let lerp = |min: f64, max: f64| min + (max - min) * t;
        match self {
            Self::Strength => cfg.strength = t as f32,
            Self::MuffledWall => cfg.muffled_wall_hz = lerp(150.0, 3000.0).round(),
            Self::MuffledRoom => cfg.muffled_room_mix = (lerp(0.0, 0.8) * 100.0).round() / 100.0,
            Self::MuffledLevel => cfg.muffled_level_db = lerp(-24.0, 0.0).round(),
            Self::QuietLevel => cfg.quiet_level_db = lerp(-36.0, -6.0).round(),
            Self::MutedRoomTone => cfg.muted_room_tone_db = lerp(-80.0, -42.0).round(),
            Self::ChipmunkPitch => cfg.chipmunk_semitones = (lerp(2.0, 12.0) * 2.0).round() / 2.0,
            Self::RobotCarrier => cfg.robot_carrier_hz = lerp(30.0, 180.0).round(),
            Self::WalkieCrunch => cfg.walkie_crunch = (lerp(1.0, 8.0) * 10.0).round() / 10.0,
            Self::UnderwaterDepth => cfg.underwater_depth_hz = lerp(160.0, 900.0).round(),
            Self::FarRoom => cfg.far_distance_mix = (lerp(0.2, 0.95) * 100.0).round() / 100.0,
        }
    }
}

pub(super) fn mode_label(mode: VoiceFilterMode) -> &'static str {
    match mode {
        VoiceFilterMode::Muffled => tr("Muffled"),
        VoiceFilterMode::Muted => tr("Muted"),
        VoiceFilterMode::Quiet => tr("Quiet"),
        VoiceFilterMode::Normal => tr("Normal"),
        VoiceFilterMode::Chipmunk => tr("Chipmunk"),
        VoiceFilterMode::Robot => tr("Robot"),
        VoiceFilterMode::Walkie => tr("Walkie"),
        VoiceFilterMode::Underwater => tr("Underwater"),
        VoiceFilterMode::Far => tr("Far"),
    }
}

pub(super) fn mode_description(mode: VoiceFilterMode) -> &'static str {
    match mode {
        VoiceFilterMode::Muffled => tr("Sounds like a hand over the microphone while you dictate"),
        VoiceFilterMode::Muted => tr("Silences your voice with soft room tone while you dictate"),
        VoiceFilterMode::Quiet => tr("Turns your voice down to a whisper while you dictate"),
        VoiceFilterMode::Normal => tr("Passes your voice through unchanged while you dictate"),
        VoiceFilterMode::Chipmunk => tr("Sounds pitched up while you dictate"),
        VoiceFilterMode::Robot => tr("Sounds robotic while you dictate"),
        VoiceFilterMode::Walkie => tr("Sounds like a handheld radio while you dictate"),
        VoiceFilterMode::Underwater => tr("Sounds submerged while you dictate"),
        VoiceFilterMode::Far => tr("Sounds like you are across the room while you dictate"),
    }
}

/// The pane's own state: the last mode other than Normal, whose sliders
/// stay in place while Normal is chosen.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FunPane {
    last_mode: VoiceFilterMode,
}

/// Saves a mode, comfort noise, or strength and applies the tuning live.
pub(super) fn set_tuning(ui: &mut Ui, change: impl FnOnce(&mut TunedFilterConfig), save: bool) {
    let mut cfg = ui.platform.tuning.clone();
    change(&mut cfg);
    if cfg.mode != VoiceFilterMode::Normal {
        ui.fun.last_mode = cfg.mode;
    }
    if save {
        let (mode, comfort, strength) = (cfg.mode, cfg.comfort_noise, cfg.strength);
        ui.update(|settings| {
            settings.experimental.hex_microphone_mode = mode;
            settings.experimental.hex_microphone_comfort_noise = comfort;
            settings.experimental.hex_microphone_strength = strength;
        });
    }
    ui.platform.tuning = cfg.clone();
    ui.ask(PlatformRequest::SetTuning(cfg));
}

fn ask_demo(ui: &mut Ui, request: DemoRequest) {
    ui.ask(PlatformRequest::HexMicrophoneDemo(request));
}

/// The driver's state in the card header.
fn header_status(ui: &Ui, p: Paint) -> El {
    let Some(status) = &ui.platform.hex_microphone else {
        return tone_tag(p, "driver-status", tr("Checking…"), Tone::Neutral);
    };
    let (label, tone) = match &status.action_state {
        DriverActionState::Installing => (tr("Installing…").to_string(), Tone::Neutral),
        DriverActionState::Uninstalling => (tr("Uninstalling…").to_string(), Tone::Neutral),
        _ if !status.driver_loaded => (tr("Driver not installed").to_string(), Tone::Neutral),
        _ if status.needs_update() => {
            (tr_fill("Update v{}", &status.bundled_version), Tone::Accent)
        }
        _ if !status.active_apps.is_empty() => (
            tr_fill("In use by {}", &status.active_apps.join(", ")),
            Tone::Success,
        ),
        _ => (
            tr_fill(
                "Installed v{}",
                status
                    .installed_version
                    .as_deref()
                    .unwrap_or(&status.bundled_version),
            ),
            Tone::Success,
        ),
    };
    tone_tag(p, "driver-status", &label, tone)
}

fn tunable(ui: &Ui, p: Paint, which: Tunable, label: &str) -> El {
    let (position, display) = which.position(&ui.platform.tuning);
    slider(
        p,
        which.key(),
        label,
        position,
        display,
        SLIDER_WIDTH,
        move |ui, position, done| {
            // Strength is a setting; the per-mode values live with the filter.
            set_tuning(
                ui,
                |cfg| which.apply(cfg, position),
                done && which == Tunable::Strength,
            );
        },
    )
}

/// Whether the Voice Filter runs (`app_settings::voice_filter_enabled`): its
/// switch is on, or the host found its driver installed.
pub(super) fn running(ui: &Ui) -> bool {
    ui.settings.experimental.hex_microphone
        || ui
            .platform
            .hex_microphone
            .as_ref()
            .is_some_and(|status| status.driver_bundle_installed)
}

/// Turns the Voice Filter on or off. Off, Hex doesn't watch the driver or
/// feed the Hex Microphone (unless the driver is installed).
fn switch(ui: &Ui, p: Paint) -> El {
    card(
        p,
        "fun-switch",
        vec![toggle_row(
            p,
            "fun-enabled",
            tr("Turn on Voice Filter"),
            tr("Feeds the Hex Microphone virtual input through a voice filter while you dictate"),
            ui.settings.experimental.hex_microphone,
            |ui: &mut Ui| {
                let on = !ui.settings.experimental.hex_microphone;
                ui.update(|settings| settings.experimental.hex_microphone = on);
            },
        )],
    )
}

pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    if !running(ui) {
        return super::scroll_body("fun-scroll", page("content").child(switch(ui, p)));
    }
    let cfg = &ui.platform.tuning;
    let mode = cfg.mode;
    let labels: Vec<&str> = VoiceFilterMode::ALL
        .iter()
        .map(|m| mode_label(*m))
        .collect();
    let selected = VoiceFilterMode::ALL
        .iter()
        .position(|m| *m == mode)
        .unwrap_or(0);
    let shown = if mode == VoiceFilterMode::Normal {
        ui.fun.last_mode
    } else {
        mode
    };
    let (primary, primary_label) = match shown {
        VoiceFilterMode::Muffled | VoiceFilterMode::Normal => (Tunable::MuffledWall, tr("Wall")),
        VoiceFilterMode::Muted => (Tunable::MutedRoomTone, tr("Room tone")),
        VoiceFilterMode::Quiet => (Tunable::QuietLevel, tr("Level")),
        VoiceFilterMode::Chipmunk => (Tunable::ChipmunkPitch, tr("Pitch")),
        VoiceFilterMode::Robot => (Tunable::RobotCarrier, tr("Carrier")),
        VoiceFilterMode::Walkie => (Tunable::WalkieCrunch, tr("Crunch")),
        VoiceFilterMode::Underwater => (Tunable::UnderwaterDepth, tr("Depth")),
        VoiceFilterMode::Far => (Tunable::FarRoom, tr("Room")),
    };
    let mut tuning = col()
        .key("tuning")
        .px(METRICS.row_px)
        .py(10.0)
        .gap(12.0)
        .child(
            row()
                .key("mode-heading")
                .gap(16.0)
                .center_y()
                .justify_between()
                .child(
                    col()
                        .key("copy")
                        .grow(1.0)
                        .shrink()
                        .gap(2.0)
                        .child(
                            text(tr("Mode while dictating"))
                                .key("label")
                                .font(p.k.ty.ui)
                                .color(p.text()),
                        )
                        .child(
                            text(mode_description(mode))
                                .key("description")
                                .font(DESCRIPTION)
                                .color(p.muted())
                                .wrap(),
                        ),
                )
                .child(small_button(
                    p,
                    "fun-reset-tuning",
                    tr("Reset"),
                    |ui: &mut Ui, _| {
                        set_tuning(
                            ui,
                            |cfg| {
                                let mode = cfg.mode;
                                *cfg = TunedFilterConfig::default();
                                cfg.mode = mode;
                                cfg.comfort_noise = true;
                            },
                            true,
                        );
                    },
                )),
        )
        // Nine modes share the card's width rather than sizing to the
        // longest label.
        .child(
            rune_kit::SegmentedControl::new(&labels, Some(selected))
                .width(None)
                .on_pick(|ui: &mut Ui, index: usize, _: &mut rune::Tx| {
                    let mode = VoiceFilterMode::ALL[index];
                    set_tuning(ui, |cfg| cfg.mode = mode, true);
                })
                .build(p.k)
                .key("fun-mode")
                .role("segmented"),
        );
    if mode != VoiceFilterMode::Normal {
        tuning = tuning.child(
            row()
                .key("sliders")
                .gap(16.0)
                .flex_wrap()
                .child(tunable(ui, p, primary, primary_label))
                .child(tunable(ui, p, Tunable::Strength, tr("Effect strength"))),
        );
    }
    if mode == VoiceFilterMode::Muffled {
        tuning = tuning.child(
            row()
                .key("muffled-sliders")
                .gap(16.0)
                .flex_wrap()
                .child(tunable(ui, p, Tunable::MuffledRoom, tr("Room")))
                .child(tunable(ui, p, Tunable::MuffledLevel, tr("Level"))),
        );
    }
    let mut rows = vec![super::platform_settings::driver_row(ui, p), tuning];
    if mode == VoiceFilterMode::Muted {
        rows.push(toggle_row(
            p,
            "fun-comfort-noise",
            tr("Comfort noise"),
            tr("Keeps a quiet room tone while muted so callers know the line is live"),
            cfg.comfort_noise,
            |ui: &mut Ui| {
                let enabled = !ui.platform.tuning.comfort_noise;
                set_tuning(ui, |cfg| cfg.comfort_noise = enabled, true);
            },
        ));
    }
    rows.push(try_it(ui, p));
    let section = col()
        .key("fun-hex-microphone")
        .gap(METRICS.label_gap)
        .child(
            row()
                .key("header-row")
                .mx(METRICS.row_px)
                .gap(8.0)
                .center_y()
                .justify_between()
                .child(p.section_label(tr("Hex Microphone")))
                .child(header_status(ui, p)),
        )
        .child(card(p, "card", rows));
    let content = page("content").child(switch(ui, p)).child(section);
    super::scroll_body("fun-scroll", content)
}

/// Record or pick a clip, see both tracks, and play either one.
fn try_it(ui: &Ui, p: Paint) -> El {
    let Some(demo) = &ui.platform.demo else {
        return setting_row(
            p,
            "fun-try-it",
            tr("Try it"),
            Some(tr(
                "Compare your raw microphone with what the call hears during dictation",
            )),
            text(tr("Checking…"))
                .key("loading")
                .font(DESCRIPTION)
                .color(p.muted()),
        );
    };
    let mut controls = row().key("demo-controls").gap(8.0).center_y();
    if demo.has_recording {
        controls = controls.child(segmented(
            p,
            "fun-demo-source",
            &[tr("Example"), tr("Your voice")],
            usize::from(demo.source == DemoSourceKind::Recording),
            |ui, index| {
                let source = if index == 1 {
                    DemoSourceKind::Recording
                } else {
                    DemoSourceKind::Example
                };
                ask_demo(ui, DemoRequest::SelectSource(source));
            },
        ));
    }
    controls = controls.child(
        row()
            .key("record")
            .gap(8.0)
            .center_y()
            .child(if demo.is_recording {
                pulsing_dot(p.danger())
            } else {
                status_dot(p.danger())
            })
            .child(if demo.is_recording {
                button(
                    p,
                    "fun-demo-record",
                    tr("Stop recording"),
                    ButtonKind::Danger,
                    |ui: &mut Ui, _| ask_demo(ui, DemoRequest::StopRecording),
                )
            } else {
                button(
                    p,
                    "fun-demo-record",
                    tr("Record 5s"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| ask_demo(ui, DemoRequest::Record),
                )
            }),
    );
    let play_button = |key: &str, label: &str, kind: ButtonKind, track: DemoPlaybackTrack| {
        let playing = demo.playing == Some(track);
        button(
            p,
            key,
            if playing { tr("Stop") } else { label },
            kind,
            move |ui: &mut Ui, _| {
                ask_demo(
                    ui,
                    if playing {
                        DemoRequest::StopPlayback
                    } else {
                        DemoRequest::Play(track)
                    },
                );
            },
        )
    };
    let play = row()
        .key("play")
        .gap(8.0)
        .center_y()
        .justify_between()
        .child(
            row()
                .key("buttons")
                .gap(8.0)
                .child(play_button(
                    "fun-play-call-hears",
                    tr("Play what call hears"),
                    ButtonKind::Primary,
                    DemoPlaybackTrack::CallHears,
                ))
                .child(play_button(
                    "fun-play-you",
                    tr("Play your voice"),
                    ButtonKind::Secondary,
                    DemoPlaybackTrack::You,
                )),
        )
        .child(
            text(tr("Stays in memory"))
                .key("memory")
                .font(p.k.ty.small)
                .color(p.faint()),
        );
    col()
        .key("fun-try-it")
        .px(METRICS.row_px)
        .py(10.0)
        .gap(12.0)
        .child(
            row()
                .key("heading")
                .gap(16.0)
                .center_y()
                .justify_between()
                .flex_wrap()
                .child(
                    col()
                        .key("copy")
                        .grow(1.0)
                        .shrink()
                        .min_w(200.0)
                        .gap(2.0)
                        .child(text(tr("Try it")).key("label").font(p.k.ty.ui).color(p.text()))
                        .child(
                            text(tr("Compare your raw microphone with what the call hears during dictation"))
                                .key("description")
                                .font(DESCRIPTION)
                                .color(p.muted())
                                .wrap(),
                        ),
                )
                .child(controls),
        )
        .child(waveforms(p, demo))
        .child(play)
}

/// The clip's two tracks as bars, the dictated span highlighted, and the
/// recording or playback progress under them.
fn waveforms(p: Paint, demo: &DemoSnapshot) -> El {
    let c = &p.k.c;
    let clip = match demo.source {
        DemoSourceKind::Example => {
            "“Yeah, I’m looking at the new layout now — Reply to Olive: pick up dinner…”"
        }
        DemoSourceKind::Recording => tr("Your 5s sample"),
    };
    let span = format!(
        "{:.1}s – {:.1}s",
        demo.span.start_ms / 1000.0,
        demo.span.end_ms / 1000.0
    );
    let track = |key: &str, label: &str, accent: bool, peaks: &[f32]| {
        row()
            .key(key)
            .gap(12.0)
            .center_y()
            .child(
                text(label)
                    .key("label")
                    .font(if accent {
                        p.k.ty.small_medium
                    } else {
                        p.k.ty.small
                    })
                    .color(if accent { p.accent() } else { p.faint() })
                    .w(80.0),
            )
            .child(
                canvas(bars(p, peaks, &demo.span, demo.duration_ms, accent))
                    .key("wave")
                    .grow(1.0)
                    .shrink()
                    .h(WAVE_HEIGHT)
                    .radius(p.k.radius.sm)
                    .fill(c.layer_01),
            )
    };
    let active = demo.is_recording || demo.playing.is_some();
    let progress = if demo.is_recording {
        demo.recording_progress
    } else {
        demo.playback_progress
    }
    .clamp(0.0, 1.0);
    let mut card = col()
        .key("fun-waveform")
        .padding(12.0)
        .gap(10.0)
        .radius(p.k.radius.md)
        .border(p.hair, c.border_weak)
        .fill(c.bg_base)
        .child(
            row()
                .key("clip")
                .gap(8.0)
                .center_y()
                .justify_between()
                .child(
                    row()
                        .key("quote")
                        .gap(8.0)
                        .center_y()
                        .shrink()
                        .child(
                            text(clip)
                                .key("text")
                                .font(DESCRIPTION)
                                .color(p.muted())
                                .truncate(),
                        )
                        .child(tone_tag(p, "span", &span, Tone::Accent)),
                )
                .child(
                    text(format!("{:.1}s", demo.duration_ms as f64 / 1000.0))
                        .key("duration")
                        .font(p.k.ty.mono)
                        .color(p.faint()),
                ),
        )
        .child(track("you", tr("You"), false, &demo.peaks_you))
        .child(track("call", tr("Call hears"), true, &demo.peaks_call));
    if active {
        card = card.child(
            rune_kit::Progress::new(Some(progress))
                .build(p.k)
                .key("progress"),
        );
    }
    card
}

/// One bar per peak, `WAVE_HEIGHT` tall at full scale, in a view box the
/// canvas stretches to its width.
fn bars(p: Paint, peaks: &[f32], span: &DictationSpan, duration_ms: u32, accent: bool) -> Shape {
    let c = &p.k.c;
    let (inside, outside) = if accent {
        (p.accent(), c.text_muted)
    } else {
        (c.text_base, c.text_faint)
    };
    let count = peaks.len().max(1);
    let width = count as f32 * 4.0;
    let mid = WAVE_HEIGHT / 2.0;
    let duration = f64::from(duration_ms.max(1));
    let mut shape = Shape::new();
    for (index, peak) in peaks.iter().enumerate() {
        let at = (index as f64 + 0.5) / count as f64 * duration;
        let color = if at >= span.start_ms && at <= span.end_ms {
            inside
        } else {
            outside
        };
        let half = peak.clamp(0.06, 1.0) * (mid - 4.0);
        let x = index as f32 * 4.0 + 2.0;
        shape = shape.line([x, mid - half], [x, mid + half], 2.0, LineCap::Round, color);
    }
    shape.view_box(width, WAVE_HEIGHT)
}

/// Deterministic demo state for scenes: the example clip with both tracks.
pub(super) fn preview_demo(recording: bool, playing: Option<DemoPlaybackTrack>) -> DemoSnapshot {
    let peaks = |seed: f32, damp: f32| -> Vec<f32> {
        (0..96)
            .map(|i| {
                let x = i as f32;
                let voice = ((x * 0.37 + seed).sin() * 0.5 + (x * 0.11).sin() * 0.35).abs();
                let speaking = (x > 18.0 && x < 70.0) as u8 as f32;
                (0.08
                    + voice
                        * (0.3 + 0.7 * speaking)
                        * if (24.0..64.0).contains(&x) { damp } else { 1.0 })
                .min(1.0)
            })
            .collect()
    };
    DemoSnapshot {
        source: DemoSourceKind::Example,
        has_recording: true,
        is_recording: recording,
        recording_progress: 0.4,
        playing,
        playback_progress: 0.55,
        duration_ms: 5_200,
        span: DictationSpan {
            start_ms: 1_300.0,
            end_ms: 3_450.0,
        },
        peaks_you: peaks(0.0, 1.0),
        peaks_call: peaks(0.0, 0.25),
    }
}
