//! The dictation HUD's lifecycle and animation, the same on every OS: the
//! event vocabulary the listener reports, the phase and job bookkeeping, and
//! Dot scope's simulation. Each step yields the dots to draw; `rune_hud`
//! draws them with `rune_hud.wgsl` through a Rune overlay.
//!
//! Time is passed in, never read, so a recorded schedule replays exactly
//! (`rune_hud`'s frame captures).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use crate::rune_hud::dot_scope::{self, Pose, Scope, Stage, Tuning};
use crate::rune_hud::hud_placement::Island;

/// The loudness before any meter arrives, dBFS.
const SILENT: f32 = -120.0;
/// The shortest processing look: a fast transcription still shows the field
/// turning blue before done clears it.
const PROCESSING_MIN_DURATION: Duration = Duration::from_millis(200);
/// How long done shows before the HUD collapses: a third into done's
/// 0.3 s wipe, so the collapse and the wipe end together.
const COMPLETED_VISIBLE_DURATION: Duration = Duration::from_millis(100);
const FAILED_VISIBLE_DURATION: Duration = Duration::from_millis(600);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub enum DictationIndicatorEvent {
    Reset,
    /// HUD Lab's or a playground story's tuning.
    ConfigureDotScope(dot_scope::Tuning),
    /// HUD Lab: Dot scope follows a synthetic voice instead of the meter
    /// until the next `Reset`.
    DemoVoice,
    Started,
    Meter {
        average: f32,
        // Dot scope follows `average`; the window's meters read
        // `input_level` instead.
        #[allow(dead_code)]
        peak: f32,
    },
    Submitted {
        job_id: u64,
    },
    Transcribing {
        job_id: u64,
    },
    Processing {
        job_id: u64,
    },
    Discarded,
    Cancelled,
    JobCompleted {
        job_id: u64,
    },
    JobCancelled {
        job_id: u64,
    },
    JobFailed {
        job_id: u64,
    },
    Failed,
}

impl DictationIndicatorEvent {
    /// A `Meter` event from captured samples: their RMS and absolute peak;
    /// `None` for no samples.
    pub fn meter(samples: &[f32]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sum_of_squares = 0.0;
        let mut peak = 0.0_f32;
        for sample in samples {
            sum_of_squares += sample * sample;
            peak = peak.max(sample.abs());
        }
        Some(Self::Meter {
            average: (sum_of_squares / samples.len() as f32).sqrt(),
            peak,
        })
    }
}

#[cfg(target_os = "macos")]
pub use macos::*;

/// macOS reports through a channel the main thread's pump drains into the
/// HUD (`rune_hud::macos`).
#[cfg(target_os = "macos")]
mod macos {
    use std::sync::mpsc::{self, Receiver, Sender};

    use super::DictationIndicatorEvent;

    #[derive(Clone)]
    pub struct DictationIndicatorSender(Sender<DictationIndicatorEvent>);

    impl DictationIndicatorSender {
        pub fn send(&self, event: DictationIndicatorEvent) {
            let _ = self.0.send(event);
        }

        pub fn meter(&self, samples: &[f32]) {
            if let Some(event) = DictationIndicatorEvent::meter(samples) {
                self.send(event);
            }
        }
    }

    pub fn channel() -> (DictationIndicatorSender, Receiver<DictationIndicatorEvent>) {
        let (sender, receiver) = mpsc::channel();
        (DictationIndicatorSender(sender), receiver)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn meter_reports_rms_and_peak() {
            let (sender, receiver) = channel();
            sender.meter(&[-0.5, 0.5]);
            let DictationIndicatorEvent::Meter { average, peak } = receiver.recv().unwrap() else {
                panic!("expected meter event");
            };
            assert!((average - 0.5).abs() < f32::EPSILON);
            assert!((peak - 0.5).abs() < f32::EPSILON);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Hidden,
    Recording,
    Transcribing,
    Completed,
    Cancelled,
    Failed,
}

fn active_phase(capturing: bool, pending_jobs: usize) -> Option<Phase> {
    if capturing {
        Some(Phase::Recording)
    } else if pending_jobs > 0 {
        Some(Phase::Transcribing)
    } else {
        None
    }
}

impl Phase {
    fn visible_duration(self) -> Option<Duration> {
        match self {
            Self::Completed => Some(COMPLETED_VISIBLE_DURATION),
            Self::Cancelled => Some(Duration::ZERO),
            Self::Failed => Some(FAILED_VISIBLE_DURATION),
            _ => None,
        }
    }

    /// The look this phase shows; `None` keeps the last one as it fades.
    fn stage(self) -> Option<Stage> {
        match self {
            Self::Recording => Some(Stage::Recording),
            Self::Transcribing => Some(Stage::Processing),
            Self::Completed => Some(Stage::Done),
            Self::Failed => Some(Stage::Failed),
            Self::Hidden | Self::Cancelled => None,
        }
    }
}

/// One Dot scope frame: the plate's pose and every dot's light.
#[derive(Clone, Debug, PartialEq)]
pub struct DotFrame {
    pub pose: Pose,
    pub tuning: Tuning,
    pub columns: usize,
    pub rows: usize,
    /// (brightness, freshness) per dot, row by row, as normalized u16s.
    pub dots: Vec<[u16; 2]>,
}

/// The middle of `rune_hud.wgsl`'s fade of dots near the field's edge
/// (1.4 to 4.6 pt in): a row or column nearer than this barely shows.
const FIELD_EDGE_FADE: f32 = 3.0;

/// How far in from the top and bottom the outermost row that shows sits.
fn field_edge(tuning: &Tuning) -> f32 {
    let outer = (tuning.height - (tuning.rows - 1.0) * tuning.spacing) / 2.0;
    if outer < FIELD_EDGE_FADE && tuning.rows > 2.0 {
        outer + tuning.spacing
    } else {
        outer
    }
}

/// The tuning that fills an island: the field as wide as the island less
/// its padding, recording and processing alike, at the same dot spacing.
/// The width snaps to whole columns, with the outer columns as far in from
/// the ends as the outermost rows that show are from the top and bottom,
/// so the island (which follows the field) is even all round.
fn fill_island(tuning: Tuning, island: Option<Island>) -> Tuning {
    let Some(island) = island else {
        return tuning;
    };
    let pitch = tuning.spacing;
    let edge = field_edge(&tuning);
    let columns = ((island.plate_width() - 2.0 * edge) / pitch).floor() + 1.0;
    let width = (columns - 1.0) * pitch + 2.0 * edge;
    Tuning {
        columns,
        width,
        processing_width: width,
        ..tuning
    }
}

/// The HUD's state: what the listener reported and Dot scope's simulation.
pub struct IndicatorModel {
    phase: Phase,
    capturing: bool,
    jobs: BTreeSet<u64>,
    phase_started: Instant,
    /// When the processing look began, for [`PROCESSING_MIN_DURATION`].
    working_since: Instant,
    /// The last job finished; done shows once processing has had its
    /// minimum.
    completion_pending: bool,
    scope: Scope,
    stage: Stage,
    /// The tuning as set, before [`fill_island`].
    tuning: Tuning,
    /// The notch island the HUD sits in, if any.
    island: Option<Island>,
    /// The microphone's loudness, dBFS.
    loudness: f32,
    /// HUD Lab's synthetic voice and how far into it Dot scope is.
    demo_voice: Option<f32>,
    /// Since an event, until a step finds everything settled out of sight.
    active: bool,
}

impl IndicatorModel {
    pub fn new(tuning: Tuning, now: Instant) -> Self {
        Self {
            phase: Phase::Hidden,
            capturing: false,
            jobs: BTreeSet::new(),
            phase_started: now,
            working_since: now,
            completion_pending: false,
            scope: Scope::new(tuning),
            stage: Stage::Recording,
            tuning,
            island: None,
            loudness: SILENT,
            demo_voice: None,
            active: false,
        }
    }

    /// Whether the HUD wants frames: from an event until a step finds it
    /// settled out of sight.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Dot scope's tuning, as set.
    #[cfg(target_os = "macos")]
    pub fn tuning(&self) -> Tuning {
        self.tuning
    }

    /// Spreads Dot scope's field across the notch island it now sits in.
    #[cfg(any(target_os = "macos", test))]
    pub fn set_island(&mut self, island: Option<Island>) {
        if self.island != island {
            self.island = island;
            self.scope.tune(fill_island(self.tuning, island));
        }
    }

    pub fn handle(&mut self, event: DictationIndicatorEvent, now: Instant, reduce: bool) {
        self.active = true;
        match event {
            DictationIndicatorEvent::Reset => {
                self.capturing = false;
                self.jobs.clear();
                self.phase = Phase::Hidden;
                self.completion_pending = false;
                self.loudness = SILENT;
                self.demo_voice = None;
            }
            DictationIndicatorEvent::ConfigureDotScope(tuning) => {
                self.tuning = tuning;
                self.scope.tune(fill_island(tuning, self.island));
            }
            DictationIndicatorEvent::DemoVoice => {
                self.demo_voice = Some(0.0);
            }
            DictationIndicatorEvent::Started => {
                // HUD Lab and the playground send `DemoVoice` after this.
                self.demo_voice = None;
                self.capturing = true;
                self.phase = Phase::Recording;
                self.phase_started = now;
                self.completion_pending = false;
                self.loudness = SILENT;
                self.stage = Stage::Recording;
                self.scope.restart(reduce);
            }
            DictationIndicatorEvent::Meter { average, .. } => {
                self.loudness = dot_scope::loudness(average);
            }
            DictationIndicatorEvent::Submitted { job_id } => {
                self.capturing = false;
                self.jobs.insert(job_id);
                self.show_pipeline(now);
            }
            DictationIndicatorEvent::Transcribing { job_id }
            | DictationIndicatorEvent::Processing { job_id } => {
                if self.jobs.contains(&job_id) {
                    self.show_pipeline(now);
                }
            }
            DictationIndicatorEvent::Discarded => {
                self.finish_capture(now, Phase::Hidden);
            }
            DictationIndicatorEvent::Cancelled => {
                self.finish_capture(now, Phase::Cancelled);
            }
            DictationIndicatorEvent::JobCompleted { job_id } => {
                self.finish_job(job_id, now, Phase::Completed);
            }
            DictationIndicatorEvent::JobCancelled { job_id } => {
                self.finish_job(job_id, now, Phase::Cancelled);
            }
            DictationIndicatorEvent::JobFailed { job_id } => {
                self.finish_job(job_id, now, Phase::Failed);
            }
            DictationIndicatorEvent::Failed => {
                self.finish_capture(now, Phase::Failed);
            }
        }
    }

    fn finish_capture(&mut self, now: Instant, empty_phase: Phase) {
        self.capturing = false;
        if !matches!(empty_phase, Phase::Hidden) {
            self.phase_started = now;
        }
        self.completion_pending = false;
        if self.jobs.is_empty() {
            self.phase = empty_phase;
        } else {
            self.show_pipeline(now);
        }
    }

    fn show_pipeline(&mut self, now: Instant) {
        let Some(phase) = active_phase(self.capturing, self.jobs.len()) else {
            return;
        };
        if phase == Phase::Transcribing && self.phase != Phase::Transcribing {
            self.working_since = now;
        }
        self.phase = phase;
        if phase == Phase::Recording {
            return;
        }
        self.phase_started = now;
        self.completion_pending = false;
    }

    fn finish_job(&mut self, job_id: u64, now: Instant, terminal: Phase) {
        if !self.jobs.remove(&job_id) {
            return;
        }
        if active_phase(self.capturing, self.jobs.len()).is_some() {
            self.show_pipeline(now);
            return;
        }
        if terminal == Phase::Completed {
            if self.phase != Phase::Transcribing {
                self.phase = Phase::Transcribing;
                self.working_since = now;
            }
            self.completion_pending = true;
        } else {
            self.phase = terminal;
            self.phase_started = now;
            self.completion_pending = false;
        }
    }

    /// Advances `dt` seconds to `now` and returns what to draw. Once
    /// everything has settled out of sight, [`Self::active`] turns false.
    pub fn step(&mut self, now: Instant, dt: f32, reduce: bool) -> DotFrame {
        let dt = dt.clamp(1.0 / 240.0, 1.0 / 20.0);
        if self.completion_pending
            && now.duration_since(self.working_since) >= PROCESSING_MIN_DURATION
        {
            self.phase = Phase::Completed;
            self.phase_started = now;
            self.completion_pending = false;
        }
        let elapsed = now.duration_since(self.phase_started);
        let terminal_finished = self
            .phase
            .visible_duration()
            .is_some_and(|duration| elapsed >= duration);
        let visible = self.phase != Phase::Hidden && !terminal_finished;
        if let Some(stage) = self.phase.stage() {
            self.stage = stage;
        }
        let loudness = match &mut self.demo_voice {
            Some(time) => {
                *time += dt;
                dot_scope::demo_loudness(*time)
            }
            None => self.loudness,
        };
        self.scope.advance(
            dt,
            dot_scope::Frame {
                stage: self.stage,
                visible,
                loudness,
                reduce_motion: reduce,
            },
        );
        self.active = visible || !self.scope.settled();
        DotFrame {
            pose: self.scope.pose(),
            tuning: self.scope.tuning(),
            columns: self.scope.columns(),
            rows: self.scope.rows(),
            dots: self
                .scope
                .dots()
                .iter()
                .map(|value| value.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_island_spreads_the_field_across_it_at_the_same_spacing() {
        let tuning = Tuning::default();
        assert_eq!(fill_island(tuning, None), tuning);
        let island = Island {
            notch: [185.0, 32.0],
            width: 241.0,
            top: -64.0,
        };
        let filled = fill_island(tuning, Some(island)).clamped();
        assert_eq!((filled.columns, filled.rows), (84.0, tuning.rows));
        assert_eq!(filled.width, filled.processing_width);
        // The outer columns sit as far in from the ends as the outermost
        // rows that show from the top and bottom (the faint outer row
        // doesn't), within the island less its padding.
        let side = (filled.width - (filled.columns - 1.0) * filled.spacing) / 2.0;
        assert!((side - field_edge(&filled)).abs() < 1e-3);
        assert!((field_edge(&filled) - 5.2).abs() < 1e-3);
        assert!(
            filled.width <= island.plate_width()
                && island.plate_width() - filled.width < filled.spacing
        );
    }

    #[test]
    fn meter_helper_reports_rms_and_peak() {
        let Some(DictationIndicatorEvent::Meter { average, peak }) =
            DictationIndicatorEvent::meter(&[-0.5, 0.5])
        else {
            panic!("expected meter event");
        };
        assert!((average - 0.5).abs() < f32::EPSILON);
        assert!((peak - 0.5).abs() < f32::EPSILON);
        assert!(DictationIndicatorEvent::meter(&[]).is_none());
    }

    fn at(start: Instant, seconds: f32) -> Instant {
        start + Duration::from_secs_f32(seconds)
    }

    fn run(model: &mut IndicatorModel, start: Instant, from: f32, seconds: f32) -> Vec<DotFrame> {
        (0..(seconds * 60.0) as usize)
            .map(|n| model.step(at(start, from + n as f32 / 60.0), 1.0 / 60.0, false))
            .collect()
    }

    fn lit(frame: &DotFrame) -> u32 {
        frame.dots.iter().map(|[power, _]| u32::from(*power)).sum()
    }

    /// A hot Linux microphone speaks near −18 dBFS RMS, which a fixed gain
    /// pins at full, so the HUD sat still: the auto level keeps it moving.
    #[test]
    fn a_hot_microphone_still_moves_the_scope() {
        let start = Instant::now();
        let mut model = IndicatorModel::new(Tuning::default(), start);
        model.handle(DictationIndicatorEvent::Started, start, false);
        let mut levels = Vec::new();
        for n in 0..360 {
            let t = n as f32 / 60.0;
            let now = at(start, t);
            // Syllables at 4 Hz between −24 and −12 dBFS.
            let db = -18.0 + 6.0 * (t * 4.0 * std::f32::consts::TAU).sin();
            let rms = 10_f32.powf(db / 20.0);
            model.handle(
                DictationIndicatorEvent::Meter {
                    average: rms,
                    peak: (rms * 3.0).min(1.0),
                },
                now,
                false,
            );
            model.step(now, 1.0 / 60.0, false);
            if t > 2.0 {
                levels.push(model.scope.level());
            }
        }
        let low = levels.iter().copied().fold(f32::MAX, f32::min);
        let high = levels.iter().copied().fold(0.0, f32::max);
        assert!(high - low > 0.35, "the level swings {low}..{high}");
    }

    #[test]
    fn a_dictation_records_processes_finishes_and_settles() {
        let start = Instant::now();
        let mut model = IndicatorModel::new(Tuning::default(), start);
        model.handle(DictationIndicatorEvent::Started, start, false);
        model.handle(DictationIndicatorEvent::DemoVoice, start, false);
        let frames = run(&mut model, start, 0.0, 1.0);
        let frame = frames.last().unwrap();
        assert_eq!(frame.dots.len(), frame.columns * frame.rows);
        assert!(frame.dots.iter().any(|[power, _]| *power > 30_000));
        assert!(frame.pose.opacity > 0.98 && frame.pose.cool < 0.01);

        // A fast transcription: the job finishes as soon as it is submitted.
        model.handle(
            DictationIndicatorEvent::Submitted { job_id: 1 },
            at(start, 1.0),
            false,
        );
        model.handle(
            DictationIndicatorEvent::JobCompleted { job_id: 1 },
            at(start, 1.0),
            false,
        );
        run(&mut model, start, 1.0, 0.15);
        assert_eq!(model.stage, Stage::Processing, "processing shows briefly");
        run(&mut model, start, 1.15, 0.1);
        assert_eq!(model.stage, Stage::Done);
        run(&mut model, start, 1.25, 2.0);
        assert!(!model.active(), "settled out of sight");
    }

    #[test]
    fn processing_that_ran_long_enough_finishes_at_once() {
        let start = Instant::now();
        let mut model = IndicatorModel::new(Tuning::default(), start);
        model.handle(DictationIndicatorEvent::Started, start, false);
        model.handle(
            DictationIndicatorEvent::Submitted { job_id: 1 },
            at(start, 0.5),
            false,
        );
        run(&mut model, start, 0.5, 0.5);
        model.handle(
            DictationIndicatorEvent::JobCompleted { job_id: 1 },
            at(start, 1.0),
            false,
        );
        run(&mut model, start, 1.0, 1.0 / 60.0);
        assert_eq!(model.stage, Stage::Done);
    }

    #[test]
    fn a_failure_turns_coral_and_a_cancel_just_fades() {
        let start = Instant::now();
        let mut model = IndicatorModel::new(Tuning::default(), start);
        model.handle(DictationIndicatorEvent::Started, start, false);
        run(&mut model, start, 0.0, 0.5);
        model.handle(DictationIndicatorEvent::Failed, at(start, 0.5), false);
        let failed = run(&mut model, start, 0.5, 0.5);
        assert!(failed.last().unwrap().pose.fail > 0.9);
        run(&mut model, start, 1.0, 1.5);
        assert!(!model.active());

        model.handle(DictationIndicatorEvent::Started, at(start, 3.0), false);
        run(&mut model, start, 3.0, 0.5);
        model.handle(DictationIndicatorEvent::Cancelled, at(start, 3.5), false);
        let cancelled = run(&mut model, start, 3.5, 1.5);
        assert!(cancelled.iter().all(|frame| frame.pose.fail < 0.01));
        assert!(lit(cancelled.last().unwrap()) == 0 || !model.active());
        assert!(!model.active());
    }

    #[test]
    fn pending_completion_cannot_replace_a_new_recording_phase() {
        assert_eq!(active_phase(true, 0), Some(Phase::Recording));
        assert_eq!(active_phase(true, 2), Some(Phase::Recording));
        assert_eq!(active_phase(false, 2), Some(Phase::Transcribing));
        assert_eq!(active_phase(false, 0), None);
    }
}
