//! Shared dictation-HUD state machine, extracted from the macOS indicator.
//!
//! This module is an adapted copy of the non-Metal parts of
//! `src/platform/macos/dictation_indicator.rs`: the event vocabulary, the
//! phase/job bookkeeping, the critically-damped springs, and the per-frame
//! uniform assembly. macOS keeps its original implementation until its shell
//! port unifies on this model; until then the two MUST be kept in lockstep —
//! any change to the macOS state machine has to be mirrored here (and vice
//! versa). `HudTuning` was also copied from that file, where it is defined.
//!
//! The model is pure `std` plus [`IndicatorUniforms`]: where the macOS draw
//! path reads the Metal drawable for its resolution, this model multiplies
//! the logical window size by a backing scale supplied via
//! [`IndicatorModel::set_backing_scale`].

// TODO: remove once the Windows/Linux shells render the HUD through this
// model.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::desktop::indicator_shader::IndicatorUniforms;

pub const WINDOW_WIDTH: f32 = 112.0;
pub const WINDOW_HEIGHT: f32 = 64.0;
const CAPSULE_WIDTH: f32 = 56.0;
const CAPSULE_HEIGHT: f32 = 16.0;
pub const TOP_OFFSET: f64 = 12.0;
const ENTRANCE_ANGULAR_FREQUENCY: f32 = 24.0;
const EXIT_ANGULAR_FREQUENCY: f32 = 24.0;
const GEOMETRY_ANGULAR_FREQUENCY: f32 = 24.0;
const HIDDEN_SCALE: f32 = 0.82;
const HIDDEN_SOFTNESS: f32 = 4.0;
const PROCESSING_MORPH_DURATION: Duration = Duration::from_millis(250);
const RECORDING_FLASH_HALF_LIFE: Duration = Duration::from_millis(280);

#[derive(Clone, Copy, Debug)]
pub enum DictationIndicatorEvent {
    Reset,
    Configure(HudTuning),
    Started,
    EditingStarted,
    PromotedToVoiceAction,
    Meter { average: f32, peak: f32 },
    Submitted { job_id: u64 },
    Transcribing { job_id: u64 },
    Processing { job_id: u64 },
    Discarded,
    Cancelled,
    JobCompleted { job_id: u64 },
    JobCancelled { job_id: u64 },
    JobFailed { job_id: u64 },
    Failed,
}

impl DictationIndicatorEvent {
    /// Builds a `Meter` event from raw audio samples, mirroring
    /// `DictationIndicatorSender::meter` on macOS: `average` is the RMS of
    /// the slice and `peak` the maximum absolute sample. Returns `None` for
    /// an empty slice (the macOS sender sends nothing in that case).
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

#[derive(Clone, Copy, Debug)]
pub struct HudTuning {
    pub style: f32,
    pub line_count: f32,
    pub curvature: f32,
    pub speed: f32,
    pub sharpness: f32,
    pub glow: f32,
    pub depth: f32,
    pub light_angle: f32,
    pub outline: f32,
}

impl Default for HudTuning {
    fn default() -> Self {
        Self {
            style: 1.0,
            line_count: 2.0,
            curvature: 0.47,
            speed: 17.08,
            sharpness: 0.29,
            glow: 0.77,
            depth: 0.65,
            light_angle: 0.35,
            outline: 1.0,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JobPhase {
    Queued,
    Transcribing,
    Processing,
}

impl Phase {
    fn visible_duration(self) -> Option<Duration> {
        match self {
            Self::Completed => Some(Duration::from_millis(240)),
            Self::Cancelled => Some(Duration::ZERO),
            Self::Failed => Some(Duration::from_millis(600)),
            _ => None,
        }
    }
}

fn recording_flash(elapsed: Duration) -> f32 {
    2.0_f32.powf(-elapsed.as_secs_f32() / RECORDING_FLASH_HALF_LIFE.as_secs_f32())
}

fn recording_flash_for(phase: Phase, editing: bool, elapsed: Duration) -> f32 {
    if matches!(phase, Phase::Recording) && !editing {
        recording_flash(elapsed)
    } else {
        0.0
    }
}

/// One stepped frame of the HUD: the shader uniforms to render with, and
/// whether the HUD window should still be on screen. `visible` mirrors the
/// return value of the macOS `MetalRenderer::draw`: it stays `true` through
/// the exit animation and only flips to `false` once the opacity, scale, and
/// softness springs have settled at their hidden targets, at which point the
/// shell should hide/close the HUD window.
pub struct IndicatorFrame {
    pub uniforms: IndicatorUniforms,
    pub visible: bool,
}

/// The dictation-HUD state machine: everything the macOS `MetalRenderer`
/// holds except the Metal plumbing (layer, command queue, pipeline).
pub struct IndicatorModel {
    phase: Phase,
    capturing: bool,
    editing: bool,
    jobs: BTreeMap<u64, JobPhase>,
    phase_started: Instant,
    render_started: Instant,
    last_frame: Instant,
    exiting: bool,
    completion_pending: bool,
    scale: f32,
    target_average: f32,
    target_peak: f32,
    average: Spring,
    peak: Spring,
    width: Spring,
    height: Spring,
    opacity: Spring,
    visual_scale: Spring,
    softness: Spring,
    processing: Spring,
    post_processing: Spring,
    tuning: HudTuning,
}

impl Default for IndicatorModel {
    fn default() -> Self {
        Self::new()
    }
}

impl IndicatorModel {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            phase: Phase::Hidden,
            capturing: false,
            editing: false,
            jobs: BTreeMap::new(),
            phase_started: now,
            render_started: now,
            last_frame: now,
            exiting: true,
            completion_pending: false,
            scale: 2.0,
            target_average: 0.0,
            target_peak: 0.0,
            average: Spring::new(0.0),
            peak: Spring::new(0.0),
            width: Spring::new(CAPSULE_WIDTH),
            height: Spring::new(CAPSULE_HEIGHT),
            opacity: Spring::new(0.0),
            visual_scale: Spring::new(HIDDEN_SCALE),
            softness: Spring::new(HIDDEN_SOFTNESS),
            processing: Spring::new(0.0),
            post_processing: Spring::new(0.0),
            tuning: HudTuning::default(),
        }
    }

    /// The backing scale factor of the screen the HUD renders on. Stands in
    /// for the macOS `set_scale`, which also resized the Metal drawable; here
    /// it only feeds `uniforms.resolution`. Defaults to 2.0 like the macOS
    /// layer's initial contents scale.
    pub fn set_backing_scale(&mut self, scale: f32) {
        self.scale = scale;
    }

    pub fn handle(&mut self, event: DictationIndicatorEvent) {
        let now = Instant::now();
        let editing_started = matches!(event, DictationIndicatorEvent::EditingStarted);
        match event {
            DictationIndicatorEvent::Reset => {
                self.capturing = false;
                self.editing = false;
                self.jobs.clear();
                self.phase = Phase::Hidden;
                self.completion_pending = false;
                self.freeze_visual_state();
            }
            DictationIndicatorEvent::Configure(tuning) => {
                self.tuning = tuning;
            }
            DictationIndicatorEvent::Started | DictationIndicatorEvent::EditingStarted => {
                self.capturing = true;
                self.editing = editing_started;
                self.phase = Phase::Recording;
                self.phase_started = now;
                self.render_started = now;
                self.last_frame = now;
                self.exiting = false;
                self.completion_pending = false;
                self.target_average = 0.0;
                self.target_peak = 0.0;
                self.average.reset(0.0);
                self.peak.reset(0.0);
                self.width.reset(CAPSULE_WIDTH);
                self.height.reset(CAPSULE_HEIGHT);
                self.opacity.reset(0.0);
                self.visual_scale.reset(HIDDEN_SCALE);
                self.softness.reset(HIDDEN_SOFTNESS);
                self.processing.reset(0.0);
                self.post_processing.reset(0.0);
            }
            DictationIndicatorEvent::PromotedToVoiceAction => {
                self.editing = true;
            }
            DictationIndicatorEvent::Meter { average, peak } => {
                self.target_average = (average * 9.0).clamp(0.0, 1.0);
                self.target_peak = (peak * 3.0).clamp(0.0, 1.0);
            }
            DictationIndicatorEvent::Submitted { job_id } => {
                self.capturing = false;
                self.editing = false;
                self.jobs.insert(job_id, JobPhase::Queued);
                self.show_pipeline(now);
            }
            DictationIndicatorEvent::Transcribing { job_id } => {
                if let Some(phase) = self.jobs.get_mut(&job_id) {
                    *phase = JobPhase::Transcribing;
                    self.show_pipeline(now);
                }
            }
            DictationIndicatorEvent::Processing { job_id } => {
                if let Some(phase) = self.jobs.get_mut(&job_id) {
                    *phase = JobPhase::Processing;
                    self.show_pipeline(now);
                }
            }
            DictationIndicatorEvent::Discarded => {
                self.capturing = false;
                self.editing = false;
                self.completion_pending = false;
                self.freeze_visual_state();
                if self.jobs.is_empty() {
                    self.phase = Phase::Hidden;
                } else {
                    self.show_pipeline(now);
                }
            }
            DictationIndicatorEvent::Cancelled => {
                self.capturing = false;
                self.editing = false;
                self.phase_started = now;
                self.completion_pending = false;
                self.freeze_visual_state();
                if self.jobs.is_empty() {
                    self.phase = Phase::Cancelled;
                } else {
                    self.show_pipeline(now);
                }
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
                self.capturing = false;
                self.editing = false;
                self.phase_started = now;
                self.completion_pending = false;
                self.freeze_visual_state();
                if self.jobs.is_empty() {
                    self.phase = Phase::Failed;
                } else {
                    self.show_pipeline(now);
                }
            }
        }
    }

    fn show_pipeline(&mut self, now: Instant) {
        let Some(phase) = active_phase(self.capturing, self.jobs.len()) else {
            return;
        };
        self.phase = phase;
        if matches!(phase, Phase::Recording) {
            return;
        }
        self.phase_started = now;
        self.completion_pending = false;
        self.target_average = 0.0;
        self.target_peak = 0.0;
    }

    fn finish_job(&mut self, job_id: u64, now: Instant, terminal: Phase) {
        if self.jobs.remove(&job_id).is_none() {
            return;
        }
        if active_phase(self.capturing, self.jobs.len()).is_some() {
            self.show_pipeline(now);
            return;
        }
        if matches!(terminal, Phase::Completed) {
            self.phase = Phase::Transcribing;
            self.phase_started = now;
            self.completion_pending = true;
        } else {
            self.phase = terminal;
            self.phase_started = now;
            self.completion_pending = false;
            self.freeze_visual_state();
        }
    }

    fn freeze_visual_state(&mut self) {
        self.target_average = self.average.value;
        self.target_peak = self.peak.value;
        self.average.velocity = 0.0;
        self.peak.velocity = 0.0;
        self.width.velocity = 0.0;
        self.height.velocity = 0.0;
        self.processing.velocity = 0.0;
        self.post_processing.velocity = 0.0;
    }

    /// Advances every spring by the time since the previous frame and
    /// assembles the shader uniforms — the state-machine half of the macOS
    /// `MetalRenderer::draw`, with the Metal encoding removed.
    pub fn frame(&mut self, now: Instant) -> IndicatorFrame {
        if self.completion_pending
            && now.duration_since(self.phase_started) >= PROCESSING_MORPH_DURATION
        {
            self.width.reset(CAPSULE_HEIGHT);
            self.height.reset(CAPSULE_HEIGHT);
            self.processing.reset(1.0);
            self.phase = Phase::Completed;
            self.phase_started = now;
            self.completion_pending = false;
            self.freeze_visual_state();
        }
        let dt = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .min(1.0 / 20.0);
        self.last_frame = now;
        let elapsed = now.duration_since(self.phase_started);
        let terminal_finished = self
            .phase
            .visible_duration()
            .is_some_and(|duration| elapsed >= duration);
        let visible = !matches!(self.phase, Phase::Hidden) && !terminal_finished;
        if !visible && !self.exiting {
            // Reversing an in-flight entrance velocity would briefly grow or sharpen the HUD.
            self.opacity.velocity = 0.0;
            self.visual_scale.velocity = 0.0;
            self.softness.velocity = 0.0;
            self.exiting = true;
        }
        let target_width = match self.phase {
            Phase::Recording => CAPSULE_WIDTH,
            Phase::Transcribing => CAPSULE_HEIGHT,
            Phase::Hidden | Phase::Completed | Phase::Cancelled | Phase::Failed => self.width.value,
        };
        let target_height = CAPSULE_HEIGHT;
        let target_processing = match self.phase {
            Phase::Recording => 0.0,
            Phase::Transcribing => 1.0,
            Phase::Hidden | Phase::Completed | Phase::Cancelled | Phase::Failed => {
                self.processing.value
            }
        };
        let target_post_processing = if self
            .jobs
            .first_key_value()
            .is_some_and(|(_, phase)| matches!(phase, JobPhase::Processing))
        {
            1.0
        } else {
            0.0
        };

        self.average.step_critical(self.target_average, dt, 22.0);
        self.peak.step_critical(self.target_peak, dt, 26.0);
        self.width
            .step_critical(target_width, dt, GEOMETRY_ANGULAR_FREQUENCY);
        self.height
            .step_critical(target_height, dt, GEOMETRY_ANGULAR_FREQUENCY);
        self.processing
            .step_critical(target_processing, dt, GEOMETRY_ANGULAR_FREQUENCY);
        self.post_processing
            .step_critical(target_post_processing, dt, GEOMETRY_ANGULAR_FREQUENCY);
        let visibility_frequency = if visible {
            ENTRANCE_ANGULAR_FREQUENCY
        } else {
            EXIT_ANGULAR_FREQUENCY
        };
        self.opacity
            .step_critical(if visible { 1.0 } else { 0.0 }, dt, visibility_frequency);
        self.visual_scale.step_critical(
            if visible { 1.0 } else { HIDDEN_SCALE },
            dt,
            visibility_frequency,
        );
        self.softness.step_critical(
            if visible { 0.0 } else { HIDDEN_SOFTNESS },
            dt,
            visibility_frequency,
        );
        self.target_peak *= 0.91_f32.powf(dt * 60.0);

        let uniforms = IndicatorUniforms {
            resolution: [WINDOW_WIDTH * self.scale, WINDOW_HEIGHT * self.scale],
            time: now.duration_since(self.render_started).as_secs_f32(),
            width: self.width.value,
            height: self.height.value,
            opacity: self.opacity.value.clamp(0.0, 1.0),
            scale: self.visual_scale.value.max(0.01),
            softness: self.softness.value.clamp(0.0, HIDDEN_SOFTNESS),
            average: self.average.value.clamp(0.0, 1.0),
            peak: self.peak.value.clamp(0.0, 1.0),
            processing: self.processing.value.clamp(0.0, 1.0),
            post_processing: self.post_processing.value.clamp(0.0, 1.0),
            capturing: if self.capturing { 1.0 } else { 0.0 },
            editing: if self.editing { 1.0 } else { 0.0 },
            queued_count: if self.capturing {
                self.jobs.len() as f32
            } else {
                self.jobs.len().saturating_sub(1) as f32
            },
            line_style: self.tuning.style,
            line_count: self.tuning.line_count,
            line_curvature: self.tuning.curvature,
            line_speed: self.tuning.speed,
            line_sharpness: self.tuning.sharpness,
            line_glow: self.tuning.glow,
            sphere_depth: self.tuning.depth,
            light_angle: self.tuning.light_angle,
            sphere_outline: self.tuning.outline,
            completion: if matches!(self.phase, Phase::Completed) {
                (elapsed.as_secs_f32() / 0.24).clamp(0.0, 1.0)
            } else {
                0.0
            },
            recording_flash: recording_flash_for(self.phase, self.editing, elapsed),
        };

        IndicatorFrame {
            uniforms,
            visible: visible
                || !self.opacity.is_settled(0.0, 0.002, EXIT_ANGULAR_FREQUENCY)
                || !self
                    .visual_scale
                    .is_settled(HIDDEN_SCALE, 0.002, EXIT_ANGULAR_FREQUENCY)
                || !self
                    .softness
                    .is_settled(HIDDEN_SOFTNESS, 0.02, EXIT_ANGULAR_FREQUENCY),
        }
    }
}

struct Spring {
    value: f32,
    velocity: f32,
}

impl Spring {
    fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
        }
    }

    fn reset(&mut self, value: f32) {
        self.value = value;
        self.velocity = 0.0;
    }

    fn step_critical(&mut self, target: f32, dt: f32, angular_frequency: f32) {
        let displacement = self.value - target;
        let coefficient = self.velocity + angular_frequency * displacement;
        let decay = (-angular_frequency * dt).exp();
        self.value = target + (displacement + coefficient * dt) * decay;
        self.velocity = (self.velocity - angular_frequency * coefficient * dt) * decay;
    }

    fn is_settled(&self, target: f32, value_epsilon: f32, angular_frequency: f32) -> bool {
        (self.value - target).abs() <= value_epsilon
            && self.velocity.abs() <= value_epsilon * angular_frequency
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn started_hud_fades_in_as_a_recording_capsule() {
        let mut model = IndicatorModel::new();
        model.handle(DictationIndicatorEvent::Started);
        let frame = model.frame(model.last_frame + Duration::from_millis(50));
        assert!(frame.visible);
        assert!(frame.uniforms.opacity > 0.0);
        assert_eq!(frame.uniforms.processing, 0.0);
        assert_eq!(frame.uniforms.capturing, 1.0);
    }

    #[test]
    fn meter_events_drive_the_average_spring_upward() {
        let mut model = IndicatorModel::new();
        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Meter {
            average: 0.1,
            peak: 0.3,
        });
        let frame = model.frame(model.last_frame + Duration::from_millis(50));
        assert!(frame.uniforms.average > 0.0);
        assert!(frame.uniforms.peak > 0.0);
    }

    #[test]
    fn completed_job_blooms_and_then_hides() {
        let mut model = IndicatorModel::new();
        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Submitted { job_id: 1 });
        model.handle(DictationIndicatorEvent::JobCompleted { job_id: 1 });
        let mut now = model.last_frame;
        let mut saw_completion = false;
        let mut hidden_after = None;
        for step in 0..250 {
            now += Duration::from_millis(16);
            let frame = model.frame(now);
            saw_completion |= frame.uniforms.completion > 0.0;
            if !frame.visible {
                hidden_after = Some(step);
                break;
            }
        }
        assert!(saw_completion, "expected a completion bloom before exit");
        assert!(
            hidden_after.is_some(),
            "expected the HUD to hide within four simulated seconds"
        );
    }

    #[test]
    fn reset_makes_the_next_frame_invisible() {
        let mut model = IndicatorModel::new();
        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Reset);
        let frame = model.frame(model.last_frame + Duration::from_millis(16));
        assert!(!frame.visible);
        assert!(frame.uniforms.opacity <= f32::EPSILON);
    }
}
