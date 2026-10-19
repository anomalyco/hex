//! Composable bundle pipeline and stage functions for `.scrawl` bundles.
//!
//! Inputs are grouped into [`Recording`], [`Interactions`], [`TranscriptData`], and
//! [`OutputOptions`], and flow through four pure, swappable stages:
//!
//! 1. [`plan_keyframes`] — groups strokes/flags into marks, selects keyframes within the duration
//!    budget, and optionally upgrades candidate frames via full-resolution video decode.
//! 2. [`solve_crop`] — annotates frames (ink, click markers, ripples) and solves the
//!    activity-weighted auto-crop when configured.
//! 3. [`render_frames`] — packs the contact sheet (and optional uncropped backup sheet) and stamps
//!    uniform corner badges.
//! 4. [`assemble_bundle`] — produces an in-memory [`BuiltBundle`] ready for inspection or writing
//!    to disk via [`BuiltBundle::write_to`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::autocrop;
use crate::keyframes::select_keyframes;
use crate::model::{
    AutocropInfo, CaptureMode, ContextChange, DeviceInfo, InteractionEvent, Keyframe, Mark,
    RgbaFrame, SCALE, ScrawlBundle, ScreenSpan, Shape, Stroke, TargetFrame, TargetInfo,
    TranscriptData, TranscriptSegment, WordTiming, keyframe_budget,
};
use crate::render::{
    render_contact_sheet, render_contact_sheet_for_duration, render_synthetic_demo_frame,
};
use crate::sketch::{marks_of, summary_markdown};
use crate::traits::ScrawlInputBatch;
use crate::windows::{ActivityLog, CursorSample, WindowSnapshot};

pub const FRAME_SAMPLE_INTERVAL: Duration = Duration::from_millis(220);
/// Candidate frames only feed scene diffs and keyframe selection; keyframes can be decoded from
/// `recording.mp4` at full size afterwards.
pub const CANDIDATE_EDGE: f64 = 480.0;
/// Without a video recording to decode from, the candidates are the keyframes, so keep them sharper.
pub const CANDIDATE_EDGE_NO_VIDEO: f64 = 1280.0;
/// Memory ceiling for candidate frames regardless of capture length.
pub const CANDIDATE_BUDGET_BYTES: usize = 384 << 20;

/// Bounded-memory candidate frame buffer. When full, drops every other regular frame (preserving
/// the first frame and urgent click/stroke/shortcut frames) and doubles the sampling interval.
#[derive(Clone, Debug)]
pub struct CandidateBuffer {
    frames: Vec<(RgbaFrame, bool)>,
    cap: usize,
    budget: usize,
    interval: Duration,
}

impl CandidateBuffer {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            frames: Vec::new(),
            cap: 0,
            budget: budget_bytes,
            interval: FRAME_SAMPLE_INTERVAL,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn push(&mut self, frame: RgbaFrame, urgent: bool) {
        if self.cap == 0 {
            self.cap = (self.budget / frame.rgba.len().max(1)).clamp(60, 1_200);
        }
        self.frames.push((frame, urgent));
        if self.frames.len() >= self.cap {
            self.thin();
        }
    }

    fn thin(&mut self) {
        let mut regular = 0_usize;
        let mut kept: Vec<(RgbaFrame, bool)> = std::mem::take(&mut self.frames)
            .into_iter()
            .enumerate()
            .filter(|(i, (_, urgent))| {
                let keep = *i == 0 || *urgent || regular.is_multiple_of(2);
                if !urgent {
                    regular += 1;
                }
                keep
            })
            .map(|(_, f)| f)
            .collect();
        if kept.len() > self.cap * 3 / 4 {
            kept = kept
                .into_iter()
                .enumerate()
                .filter(|(i, _)| i.is_multiple_of(2))
                .map(|(_, f)| f)
                .collect();
        }
        self.frames = kept;
        self.interval *= 2;
    }

    pub fn into_frames(self) -> Vec<RgbaFrame> {
        self.frames.into_iter().map(|(f, _)| f).collect()
    }
}

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn make_bundle_id() -> String {
    // Two IDs in the same millisecond (parallel tests, overlapping ingests) must still differ.
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let ms = now_epoch_ms();
    let pid = std::process::id();
    match COUNT.fetch_add(1, Ordering::Relaxed) {
        0 => format!("scrawl-{ms}-{pid:x}"),
        n => format!("scrawl-{ms}-{pid:x}-{n}"),
    }
}

/// Matches full-resolution decoded frames back to selected keyframes by timestamp (within 250ms),
/// so a single frame decode failure only leaves that one keyframe at candidate resolution.
pub fn merge_full_keyframes(
    selected: Vec<(Keyframe, RgbaFrame)>,
    full: Vec<RgbaFrame>,
) -> (Vec<(Keyframe, RgbaFrame)>, usize) {
    let mut pool: Vec<Option<RgbaFrame>> = full.into_iter().map(Some).collect();
    let mut matched = 0;
    let merged = selected
        .into_iter()
        .map(|(key, fallback)| {
            let best = pool
                .iter()
                .enumerate()
                .filter_map(|(idx, f)| f.as_ref().map(|frame| (idx, frame.t_ms.abs_diff(key.t))))
                .filter(|&(_, diff)| diff <= 250)
                .min_by_key(|&(_, diff)| diff)
                .map(|(idx, _)| idx);
            if let Some(idx) = best
                && let Some(mut full_frame) = pool[idx].take()
            {
                full_frame.t_ms = key.t;
                matched += 1;
                (key, full_frame)
            } else {
                (key, fallback)
            }
        })
        .collect();
    (merged, matched)
}

/// Captured media, timing, and target context for a Scrawl session.
#[derive(Clone, Debug, Default)]
pub struct Recording {
    pub target: TargetInfo,
    pub created_at_ms: Option<u64>,
    pub duration_ms: u64,
    pub frames: Vec<RgbaFrame>,
    pub video: Option<String>,
    pub video_offset_ms: u64,
    pub audio: Option<String>,
    pub source: Option<String>,
    pub device: Option<DeviceInfo>,
    pub context: BTreeMap<String, String>,
    pub context_changes: Vec<ContextChange>,
    pub screens: Vec<ScreenSpan>,
}

impl Recording {
    pub fn new(target: TargetInfo, duration_ms: u64, frames: Vec<RgbaFrame>) -> Self {
        Self {
            target,
            duration_ms,
            frames,
            ..Default::default()
        }
    }
}

/// User interactions, pointer trajectory, window snapshots, and time flags observed during a capture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interactions {
    pub strokes: Vec<Stroke>,
    pub events: Vec<InteractionEvent>,
    pub cursor: Vec<CursorSample>,
    pub windows: Vec<WindowSnapshot>,
    pub flags: Vec<u64>,
}

impl Interactions {
    pub fn from_batch(batch: ScrawlInputBatch) -> Self {
        Self {
            strokes: batch.strokes,
            events: batch.events,
            cursor: batch.cursor,
            windows: batch.windows,
            flags: Vec::new(),
        }
    }

    pub fn extend_batch(&mut self, batch: ScrawlInputBatch) {
        self.strokes.extend(batch.strokes);
        self.events.extend(batch.events);
        self.cursor.extend(batch.cursor);
        self.windows.extend(batch.windows);
    }
}

/// Function signature for decoding frames at specific capture-relative timestamps at full quality.
pub type FullFramesFn = dyn Fn(&[u64]) -> Result<Vec<RgbaFrame>>;

/// Owned decoder closure for full-quality keyframes.
pub type FullFrames = Box<FullFramesFn>;

/// Options controlling auto-crop, full-resolution frame replacement, and user notes.
#[derive(Default)]
pub struct OutputOptions {
    /// `Some(settings)` enables activity-weighted auto-crop; `None` disables it.
    pub auto_crop: Option<autocrop::Settings>,
    /// Optional decoder that upgrades selected keyframe timestamps to full-resolution frames.
    pub full_frames: Option<FullFrames>,
    /// Optional user note attached to the bundle.
    pub note: Option<String>,
}

impl OutputOptions {
    /// Default options for `target`: enables default [`autocrop::Settings`] for full-display captures.
    pub fn for_target(target: &TargetInfo) -> Self {
        Self {
            auto_crop: (target.mode == CaptureMode::Display).then(autocrop::Settings::default),
            full_frames: None,
            note: None,
        }
    }
}

/// Stage 1 output: grouped marks, selected keyframes + frames, and baseline frame for diffing.
#[derive(Clone, Debug)]
pub struct KeyframePlan {
    pub marks: Vec<Mark>,
    pub selected: Vec<(Keyframe, RgbaFrame)>,
    pub baseline: Option<RgbaFrame>,
}

/// Stage 2 output: annotated keyframe images and optional auto-crop solution + uncropped backups.
#[derive(Clone, Debug)]
pub struct CroppedFrames {
    pub marks: Vec<Mark>,
    pub annotated_frames: Vec<(Keyframe, RgbaFrame)>,
    pub autocrop: Option<AutocropInfo>,
    pub uncropped_frames: Option<Vec<(Keyframe, RgbaFrame)>>,
}

/// Uncropped annotated frames and contact sheet saved beside an auto-cropped bundle.
pub type FullFrameBundle = (AutocropInfo, Vec<(Keyframe, RgbaFrame)>, RgbaFrame);

/// Stage 3 output: badged keyframe images, packed contact sheet, and optional uncropped backup bundle.
#[derive(Clone, Debug)]
pub struct RenderedFrames {
    pub marks: Vec<Mark>,
    pub keyframes: Vec<Keyframe>,
    pub annotated_frames: Vec<(Keyframe, RgbaFrame)>,
    pub contact_sheet: RgbaFrame,
    pub full_frames: Option<FullFrameBundle>,
}

/// Stage 1: Groups strokes and flags into [`Mark`]s, selects keyframes within the duration budget,
/// and optionally replaces candidate frames with full-resolution decoded frames.
pub fn plan_keyframes(
    recording: &Recording,
    interactions: &Interactions,
    transcript: &TranscriptData,
    full_frames: Option<&FullFramesFn>,
) -> KeyframePlan {
    let mut marks = marks_of(&interactions.strokes);
    for (i, &t) in interactions.flags.iter().enumerate() {
        marks.push(Mark {
            id: format!("f{}", i + 1),
            strokes: Vec::new(),
            t0: t,
            t1: t,
            box_norm: [0.0, 0.0, 1.0, 1.0],
            shape: Shape::Flag,
            from: None,
            to: None,
            pin: None,
            on: None,
            description: "flagged this moment".into(),
            frame: None,
        });
    }
    let selected = select_keyframes(
        &recording.frames,
        &interactions.strokes,
        &mut marks,
        &interactions.events,
        transcript,
        keyframe_budget(recording.duration_ms),
    );
    let baseline = recording.frames.first().cloned();
    let selected = match full_frames {
        Some(decode) if !selected.is_empty() => {
            let times: Vec<u64> = selected.iter().map(|(k, _)| k.t).collect();
            match decode(&times) {
                Ok(full) => merge_full_keyframes(selected, full).0,
                Err(_) => selected,
            }
        }
        _ => selected,
    };
    KeyframePlan {
        marks,
        selected,
        baseline,
    }
}

/// Stage 2: Annotates selected frames with ink/click/laser overlays and solves the activity-weighted
/// auto-crop when `crop_settings` is `Some`.
pub fn solve_crop(
    plan: KeyframePlan,
    recording: &Recording,
    interactions: &Interactions,
    crop_settings: Option<&autocrop::Settings>,
) -> CroppedFrames {
    let (annotated_frames, cropped) = autocrop::annotate_and_crop(
        plan.selected,
        plan.baseline.as_ref(),
        crop_settings,
        &recording.target,
        recording.duration_ms,
        &interactions.strokes,
        &plan.marks,
        &interactions.events,
        &interactions.windows,
        &interactions.cursor,
    );
    let (autocrop, uncropped_frames) = match cropped {
        Some((info, full)) => (Some(info), Some(full)),
        None => (None, None),
    };
    CroppedFrames {
        marks: plan.marks,
        annotated_frames,
        autocrop,
        uncropped_frames,
    }
}

/// Stage 3: Renders the justified contact sheet (plus uncropped contact sheet when auto-cropped)
/// and stamps uniform corner badges onto the keyframe images.
pub fn render_frames(duration_ms: u64, mut cropped: CroppedFrames) -> RenderedFrames {
    let keyframes: Vec<Keyframe> = cropped
        .annotated_frames
        .iter()
        .map(|(kf, _)| kf.clone())
        .collect();
    let (full_sheet, contact_sheet) = thread::scope(|scope| {
        let full_handle = cropped
            .uncropped_frames
            .as_ref()
            .map(|full| scope.spawn(|| render_contact_sheet_for_duration(duration_ms, full)));
        let sheet = render_contact_sheet_for_duration(duration_ms, &cropped.annotated_frames);
        (full_handle.map(|h| h.join().unwrap()), sheet)
    });
    crate::render::stamp_keyframe_badges(&mut cropped.annotated_frames);
    if let Some(full) = &mut cropped.uncropped_frames {
        crate::render::stamp_keyframe_badges(full);
    }
    let full_frames = match (cropped.autocrop, cropped.uncropped_frames, full_sheet) {
        (Some(info), Some(full), Some(sheet)) => Some((info, full, sheet)),
        _ => None,
    };
    RenderedFrames {
        marks: cropped.marks,
        keyframes,
        annotated_frames: cropped.annotated_frames,
        contact_sheet,
        full_frames,
    }
}

/// Stage 4: Assembles the final [`BuiltBundle`] from the recording, interactions, transcript, and
/// rendered frame artifacts.
pub fn assemble_bundle(
    id: impl Into<String>,
    recording: Recording,
    interactions: Interactions,
    transcript: TranscriptData,
    note: Option<String>,
    rendered: RenderedFrames,
) -> BuiltBundle {
    let autocrop = rendered
        .full_frames
        .as_ref()
        .map(|(info, _, _)| info.clone());
    let bundle = ScrawlBundle {
        version: 1,
        id: id.into(),
        created_at_ms: recording.created_at_ms.unwrap_or_else(now_epoch_ms),
        duration_ms: recording.duration_ms,
        target: recording.target,
        transcript,
        note,
        strokes: interactions.strokes,
        marks: rendered.marks,
        events: interactions.events,
        keyframes: rendered.keyframes,
        contact_sheet: "contact-sheet.png".into(),
        video: recording.video,
        video_offset_ms: recording.video_offset_ms,
        audio: recording.audio,
        source: recording.source,
        device: recording.device,
        context: recording.context,
        context_changes: recording.context_changes,
        screens: recording.screens,
        windows: interactions.windows,
        cursor: interactions.cursor,
        autocrop,
        destination: None,
    };
    BuiltBundle {
        bundle,
        annotated_frames: rendered.annotated_frames,
        contact_sheet: rendered.contact_sheet,
        full_frames: rendered.full_frames,
    }
}

/// Runs all four stages ([`plan_keyframes`] → [`solve_crop`] → [`render_frames`] → [`assemble_bundle`])
/// over grouped inputs.
pub fn build_bundle(
    id: impl Into<String>,
    recording: Recording,
    interactions: Interactions,
    transcript: TranscriptData,
    options: OutputOptions,
) -> BuiltBundle {
    let plan = plan_keyframes(
        &recording,
        &interactions,
        &transcript,
        options.full_frames.as_deref(),
    );
    let cropped = solve_crop(plan, &recording, &interactions, options.auto_crop.as_ref());
    let rendered = render_frames(recording.duration_ms, cropped);
    assemble_bundle(
        id,
        recording,
        interactions,
        transcript,
        options.note,
        rendered,
    )
}

/// Per-stage timing breakdown.
#[derive(Clone, Copy, Debug, Default)]
pub struct StageTimings {
    pub started_at: Option<std::time::Instant>,
    pub stop_ms: Option<u64>,
    pub audio_ms: Option<u64>,
    pub candidates_ms: Option<u64>,
    pub transcribe_ms: Option<u64>,
}

/// Pre-rendered keyframe visuals and encoded PNGs computed while transcription is running.
pub struct PreparedVisuals {
    pub keyframes: Vec<Keyframe>,
    pub autocrop: Option<AutocropInfo>,
    pub contact_sheet_png: Vec<u8>,
    pub frame_pngs: Vec<(String, Vec<u8>)>,
    pub full_sheet_png: Option<Vec<u8>>,
    pub full_frame_pngs: Vec<(String, Vec<u8>)>,
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_visuals(
    selected: Vec<(Keyframe, RgbaFrame)>,
    baseline: Option<&RgbaFrame>,
    crop_settings: Option<&autocrop::Settings>,
    target: &TargetInfo,
    duration_ms: u64,
    strokes: &[Stroke],
    marks: &[Mark],
    events: &[InteractionEvent],
    activity: &ActivityLog,
) -> Option<PreparedVisuals> {
    if selected.is_empty() {
        return None;
    }
    let (mut annotated_frames, mut cropped) = autocrop::annotate_and_crop(
        selected,
        baseline,
        crop_settings,
        target,
        duration_ms,
        strokes,
        marks,
        events,
        &activity.windows,
        &activity.cursor,
    );
    let keyframes: Vec<Keyframe> = annotated_frames.iter().map(|(kf, _)| kf.clone()).collect();
    let (full_sheet, contact_sheet) = thread::scope(|scope| {
        let full_handle = cropped.as_ref().map(|(_, full)| {
            scope.spawn(|| crate::render::render_contact_sheet_for_duration(duration_ms, full))
        });
        let sheet =
            crate::render::render_contact_sheet_for_duration(duration_ms, &annotated_frames);
        (full_handle.and_then(|h| h.join().ok()), sheet)
    });
    crate::render::stamp_keyframe_badges(&mut annotated_frames);
    if let Some((_, full)) = &mut cropped {
        crate::render::stamp_keyframe_badges(full);
    }
    let contact_sheet_png = crate::render::encode_png(&contact_sheet).ok()?;
    let frame_pngs: Vec<(String, Vec<u8>)> = thread::scope(|scope| {
        let workers: Vec<_> = annotated_frames
            .iter()
            .map(|(kf, frame)| {
                scope.spawn(move || {
                    crate::render::encode_png(frame)
                        .ok()
                        .map(|b| (kf.file.clone(), b))
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().ok().flatten())
            .collect::<Option<Vec<_>>>()
    })?;
    let (autocrop, full_sheet_png, full_frame_pngs) = match (cropped, full_sheet) {
        (Some((info, full)), Some(sheet)) => {
            let sheet_png = crate::render::encode_png(&sheet).ok()?;
            let full_pngs: Vec<(String, Vec<u8>)> = thread::scope(|scope| {
                let workers: Vec<_> = full
                    .iter()
                    .map(|(kf, frame)| {
                        scope.spawn(move || {
                            let name = Path::new(&kf.file)
                                .file_name()
                                .and_then(|n| n.to_str())?
                                .to_owned();
                            crate::render::encode_png(frame).ok().map(|b| (name, b))
                        })
                    })
                    .collect();
                workers
                    .into_iter()
                    .map(|w| w.join().ok().flatten())
                    .collect::<Option<Vec<_>>>()
            })?;
            (Some(info), Some(sheet_png), full_pngs)
        }
        _ => (None, None, Vec::new()),
    };
    Some(PreparedVisuals {
        keyframes,
        autocrop,
        contact_sheet_png,
        frame_pngs,
        full_sheet_png,
        full_frame_pngs,
    })
}

/// What a source other than this Mac's screen adds to a bundle.
#[derive(Default)]
pub struct SourceExtras {
    pub source: Option<String>,
    pub device: Option<DeviceInfo>,
    /// What the recorded app said about itself: values at the start, their changes, and screens.
    pub context: BTreeMap<String, String>,
    pub context_changes: Vec<ContextChange>,
    pub screens: Vec<ScreenSpan>,
    /// Screen mode's window and cursor log.
    pub activity: ActivityLog,
    /// Time-only markers (ms); each becomes a `Flag` mark with keyframe priority.
    pub flags: Vec<u64>,
    /// When the video starts on the bundle's clock (ms).
    pub video_offset_ms: u64,
    /// Screen-capture auto-crop: `None` uses default `autocrop::Settings` for Display captures, `Some(None)` is off.
    pub autocrop: Option<Option<autocrop::Settings>>,
    /// Decodes the chosen keyframes again at full quality, for sources that select on small
    /// candidate frames.
    pub full_frames: Option<FullFrames>,
    pub prepared: Option<PreparedVisuals>,
    pub timings: StageTimings,
}

/// In-memory result of [`build_bundle`] or [`BundleBuilder::build`], ready for inspection or
/// writing to disk via [`BuiltBundle::write_to`].
pub struct BuiltBundle {
    pub bundle: ScrawlBundle,
    pub annotated_frames: Vec<(Keyframe, RgbaFrame)>,
    pub contact_sheet: RgbaFrame,
    pub full_frames: Option<FullFrameBundle>,
}

impl BuiltBundle {
    /// Renders `summary.md` with paths rooted at `bundle_dir`.
    pub fn summary_markdown(&self, bundle_dir: &Path) -> String {
        summary_markdown(&self.bundle, bundle_dir)
    }

    /// Writes `scrawl.json`, `summary.md`, `contact-sheet.png`, `frames/*.png`, and any
    /// full-frame auto-crop backups into `bundle_dir`.
    pub fn write_to(&self, bundle_dir: &Path) -> Result<(PathBuf, ScrawlBundle, String)> {
        let summary = self.summary_markdown(bundle_dir);
        thread::scope(|scope| -> Result<()> {
            let full_write = self.full_frames.as_ref().map(|(info, full, sheet)| {
                scope.spawn(|| autocrop::write_full_frames(bundle_dir, info, full, sheet))
            });
            write_bundle_artifacts(
                bundle_dir,
                &self.bundle,
                &self.annotated_frames,
                &self.contact_sheet,
                &summary,
            )?;
            if let Some(handle) = full_write {
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("Full-frame writer panicked"))??;
            }
            Ok(())
        })?;
        Ok((bundle_dir.to_path_buf(), self.bundle.clone(), summary))
    }
}

/// Composable builder over grouped [`Recording`], [`Interactions`], [`TranscriptData`], and [`OutputOptions`].
pub struct BundleBuilder {
    id: String,
    recording: Recording,
    interactions: Interactions,
    transcript: TranscriptData,
    options: OutputOptions,
}

impl BundleBuilder {
    pub fn new(id: impl Into<String>, recording: Recording) -> Self {
        let options = OutputOptions::for_target(&recording.target);
        Self {
            id: id.into(),
            recording,
            interactions: Interactions::default(),
            transcript: TranscriptData::default(),
            options,
        }
    }

    pub fn interactions(mut self, interactions: Interactions) -> Self {
        self.interactions = interactions;
        self
    }

    pub fn transcript(mut self, transcript: TranscriptData) -> Self {
        self.transcript = transcript;
        self
    }

    pub fn options(mut self, options: OutputOptions) -> Self {
        self.options = options;
        self
    }

    /// Runs [`plan_keyframes`] → [`solve_crop`] → [`render_frames`] → [`assemble_bundle`] in memory.
    pub fn build(self) -> BuiltBundle {
        build_bundle(
            self.id,
            self.recording,
            self.interactions,
            self.transcript,
            self.options,
        )
    }
}

/// Writes all bundle artifacts (`scrawl.json`, `summary.md`, `contact-sheet.png`, `frames/frame-XX.png`)
/// into a new or specified `.scrawl` bundle directory.
pub fn write_bundle_artifacts(
    out_dir: &Path,
    bundle: &ScrawlBundle,
    annotated_frames: &[(Keyframe, RgbaFrame)],
    contact_sheet: &RgbaFrame,
    summary_md: &str,
) -> Result<()> {
    std::fs::create_dir_all(out_dir.join("frames"))
        .with_context(|| format!("could not create {}", out_dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(out_dir, std::fs::Permissions::from_mode(0o700));
    }

    thread::scope(|scope| -> Result<()> {
        let workers: Vec<_> = annotated_frames
            .iter()
            .map(|(kf, frame)| {
                scope.spawn(move || -> Result<()> {
                    crate::render::save_png(frame, &out_dir.join(&kf.file))
                })
            })
            .collect();
        crate::render::save_png(contact_sheet, &out_dir.join(&bundle.contact_sheet))?;
        for worker in workers {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("PNG encoding worker panicked"))??;
        }
        Ok(())
    })?;

    let manifest_bytes = serde_json::to_vec_pretty(bundle)?;
    std::fs::write(out_dir.join("scrawl.json"), manifest_bytes)?;
    std::fs::write(out_dir.join("summary.md"), summary_md.as_bytes())?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn build_bundle_from_parts(
    bundle_dir: &Path,
    target: TargetInfo,
    duration_ms: u64,
    frames: Vec<RgbaFrame>,
    strokes: Vec<Stroke>,
    events: Vec<InteractionEvent>,
    transcript: TranscriptData,
    video_path: Option<PathBuf>,
    note: Option<String>,
    extras: SourceExtras,
) -> Result<(PathBuf, ScrawlBundle, String)> {
    let mut marks = marks_of(&strokes);
    for (i, &t) in extras.flags.iter().enumerate() {
        marks.push(Mark {
            id: format!("f{}", i + 1),
            strokes: Vec::new(),
            t0: t,
            t1: t,
            box_norm: [0.0, 0.0, 1.0, 1.0],
            shape: Shape::Flag,
            from: None,
            to: None,
            pin: None,
            on: None,
            description: "flagged this moment".into(),
            frame: None,
        });
    }
    let selected = select_keyframes(
        &frames,
        &strokes,
        &mut marks,
        &events,
        &transcript,
        keyframe_budget(duration_ms),
    );
    let matches_prepared = extras.prepared.as_ref().is_some_and(|prep| {
        prep.keyframes.len() == selected.len()
            && prep.keyframes.iter().zip(&selected).all(|(a, (b, _))| {
                a.index == b.index
                    && a.t == b.t
                    && a.mark_id == b.mark_id
                    && a.cursor == b.cursor
                    && a.file == b.file
            })
    });
    let id = bundle_dir
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("scrawl-bundle")
        .to_string();
    let video_rel = video_path.and_then(|p| {
        p.exists()
            .then(|| p.file_name()?.to_str().map(str::to_owned))
            .flatten()
    });
    let audio_rel = bundle_dir
        .join("audio.wav")
        .exists()
        .then(|| "audio.wav".to_string());

    if matches_prepared {
        drop(frames);
        let prep = extras.prepared.unwrap();
        let keyframes: Vec<Keyframe> = selected.into_iter().map(|(kf, _)| kf).collect();
        let bundle = ScrawlBundle {
            version: 1,
            id,
            created_at_ms: now_epoch_ms(),
            duration_ms,
            target,
            transcript,
            note,
            strokes,
            marks,
            events,
            keyframes,
            contact_sheet: "contact-sheet.png".into(),
            video: video_rel,
            video_offset_ms: extras.video_offset_ms,
            audio: audio_rel,
            source: extras.source,
            device: extras.device,
            context: extras.context,
            context_changes: extras.context_changes,
            screens: extras.screens,
            windows: extras.activity.windows,
            cursor: extras.activity.cursor,
            autocrop: prep.autocrop,
            destination: None,
        };
        let summary = summary_markdown(&bundle, bundle_dir);
        std::fs::create_dir_all(bundle_dir.join("frames"))
            .with_context(|| format!("could not create {}", bundle_dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(bundle_dir, std::fs::Permissions::from_mode(0o700));
        }
        std::fs::write(
            bundle_dir.join(&bundle.contact_sheet),
            &prep.contact_sheet_png,
        )?;
        for (rel, bytes) in &prep.frame_pngs {
            std::fs::write(bundle_dir.join(rel), bytes)?;
        }
        if let (Some(info), Some(full_sheet_png)) = (&bundle.autocrop, &prep.full_sheet_png) {
            let full_dir = bundle_dir.join(&info.full_frames);
            std::fs::create_dir_all(&full_dir)?;
            std::fs::write(bundle_dir.join(&info.full_contact_sheet), full_sheet_png)?;
            for (name, bytes) in &prep.full_frame_pngs {
                std::fs::write(full_dir.join(name), bytes)?;
            }
        }
        let manifest_bytes = serde_json::to_vec_pretty(&bundle)?;
        std::fs::write(bundle_dir.join("scrawl.json"), manifest_bytes)?;
        std::fs::write(bundle_dir.join("summary.md"), summary.as_bytes())?;
        return Ok((bundle_dir.to_path_buf(), bundle, summary));
    }

    let auto_crop = match extras.autocrop {
        Some(settings) => settings,
        None => (target.mode == CaptureMode::Display).then(autocrop::Settings::default),
    };
    let recording = Recording {
        target,
        created_at_ms: None,
        duration_ms,
        frames,
        video: video_rel,
        video_offset_ms: extras.video_offset_ms,
        audio: audio_rel,
        source: extras.source,
        device: extras.device,
        context: extras.context,
        context_changes: extras.context_changes,
        screens: extras.screens,
    };
    let interactions = Interactions {
        strokes,
        events,
        cursor: extras.activity.cursor,
        windows: extras.activity.windows,
        flags: extras.flags,
    };
    let options = OutputOptions {
        auto_crop,
        full_frames: extras.full_frames,
        note,
    };
    let _ = render_contact_sheet;
    build_bundle(id, recording, interactions, transcript, options).write_to(bundle_dir)
}

/// Generates a rich, deterministic multi-frame demo bundle showcasing clicks, circles, arrows, pins, and aligned speech.
pub fn generate_demo_bundle(out: Option<PathBuf>) -> Result<(PathBuf, ScrawlBundle, String)> {
    let bundle_dir =
        out.unwrap_or_else(|| std::env::temp_dir().join(format!("{}.scrawl", make_bundle_id())));
    let (fw, fh) = (1280_u32, 800_u32);
    let target = TargetInfo {
        mode: CaptureMode::Window,
        app: Some("OpenCode".into()),
        bundle_id: Some("ai.opencode.desktop".into()),
        window_title: Some("session/tool-running — OpenCode".into()),
        window_id: Some(4210),
        display_id: 1,
        frame: TargetFrame {
            x: 100.0,
            y: 60.0,
            width: f64::from(fw),
            height: f64::from(fh),
            scale: 2.0,
        },
    };

    let frames = vec![
        render_synthetic_demo_frame(fw, fh, 400, 0),
        render_synthetic_demo_frame(fw, fh, 1_500, 1),
        render_synthetic_demo_frame(fw, fh, 3_600, 1),
        render_synthetic_demo_frame(fw, fh, 6_400, 2),
        render_synthetic_demo_frame(fw, fh, 9_200, 2),
    ];

    // 1. Circle around the shifted header band (cx=0.52, cy=0.14, r_x=0.14, r_y=0.06)
    let mut circle_pts = Vec::new();
    for i in 0..=36 {
        let angle = (i as f32 / 36.0) * std::f32::consts::TAU;
        let x = ((0.52 + 0.13 * angle.cos()) * SCALE as f32).round() as i32;
        let y = ((0.15 + 0.055 * angle.sin()) * SCALE as f32).round() as i32;
        circle_pts.extend_from_slice(&[x, y, i * 18]);
    }
    let circle_stroke = Stroke {
        t0: 2_900,
        t1: 2_900 + 36 * 18,
        fw,
        fh,
        on: Some("Band header".into()),
        pin: None,
        laser: false,
        pts: circle_pts,
    };

    // 2. Arrow pointing from sidebar item to the tool card (shaft + arrowhead)
    let mut shaft_pts = Vec::new();
    for i in 0..=12 {
        let t = i as f32 / 12.0;
        let x = ((0.22 + (0.44 - 0.22) * t) * SCALE as f32).round() as i32;
        let y = ((0.48 + (0.36 - 0.48) * t) * SCALE as f32).round() as i32;
        shaft_pts.extend_from_slice(&[x, y, i * 16]);
    }
    let arrow_shaft = Stroke {
        t0: 5_800,
        t1: 5_800 + 12 * 16,
        fw,
        fh,
        on: Some("Used 12 tools".into()),
        pin: None,
        laser: false,
        pts: shaft_pts,
    };
    let arrow_head = Stroke {
        t0: 6_100,
        t1: 6_160,
        fw,
        fh,
        on: Some("Used 12 tools".into()),
        pin: None,
        laser: false,
        pts: vec![
            (0.41 * SCALE as f32) as i32,
            (0.35 * SCALE as f32) as i32,
            0,
            (0.44 * SCALE as f32) as i32,
            (0.36 * SCALE as f32) as i32,
            30,
            (0.42 * SCALE as f32) as i32,
            (0.40 * SCALE as f32) as i32,
            60,
        ],
    };

    // 3. Numbered pin on the composer send button
    let pin_stroke = Stroke {
        t0: 8_900,
        t1: 8_900,
        fw,
        fh,
        on: Some("Send button".into()),
        pin: Some(1),
        laser: false,
        pts: vec![
            (0.88 * SCALE as f32) as i32,
            (0.88 * SCALE as f32) as i32,
            0,
        ],
    };

    let strokes = vec![circle_stroke, arrow_shaft, arrow_head, pin_stroke];
    let events = vec![
        InteractionEvent::Click {
            t: 1_250,
            x: 0.46,
            y: 0.36,
            button: Some("left".into()),
            label: Some("Used 12 tools".into()),
            role: Some("AXButton".into()),
        },
        InteractionEvent::Scroll {
            t: 4_800,
            x: 0.55,
            y: 0.55,
            dx: 0.0,
            dy: -240.0,
            label: Some("Transcript".into()),
        },
    ];

    let transcript = TranscriptData {
        source: "speech-analyzer".into(),
        model: Some("SpeechTranscriber en_US".into()),
        text: "When I click Used 12 tools this band header jumps down by four pixels, and the row spacing under the card feels too tight.".into(),
        segments: vec![
            TranscriptSegment {
                t0: 600,
                t1: 4_100,
                text: "When I click Used 12 tools this band header jumps down by four pixels,".into(),
            },
            TranscriptSegment {
                t0: 5_400,
                t1: 9_400,
                text: "and the row spacing under the card feels too tight.".into(),
            },
        ],
        words: vec![
            WordTiming { t0: 600, t1: 850, text: "When".into() },
            WordTiming { t0: 850, t1: 980, text: "I".into() },
            WordTiming { t0: 980, t1: 1_320, text: "click".into() },
            WordTiming { t0: 1_320, t1: 1_650, text: "Used".into() },
            WordTiming { t0: 1_650, t1: 1_900, text: "12".into() },
            WordTiming { t0: 1_900, t1: 2_300, text: "tools".into() },
            WordTiming { t0: 2_450, t1: 2_700, text: "this".into() },
            WordTiming { t0: 2_700, t1: 3_050, text: "band".into() },
            WordTiming { t0: 3_050, t1: 3_400, text: "header".into() },
            WordTiming { t0: 3_400, t1: 3_700, text: "jumps".into() },
            WordTiming { t0: 3_700, t1: 3_900, text: "down".into() },
            WordTiming { t0: 3_900, t1: 4_100, text: "by four pixels,".into() },
            WordTiming { t0: 5_400, t1: 5_800, text: "and the".into() },
            WordTiming { t0: 5_800, t1: 6_300, text: "row spacing".into() },
            WordTiming { t0: 6_300, t1: 7_200, text: "under the card".into() },
            WordTiming { t0: 7_800, t1: 9_400, text: "feels too tight.".into() },
        ],
        estimated: false,
    };

    build_bundle_from_parts(
        &bundle_dir,
        target,
        9_800,
        frames,
        strokes,
        events,
        transcript,
        None,
        Some("Header inset shifts on expand".into()),
        SourceExtras::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_failures_do_not_publish_a_manifest() {
        let root = std::env::temp_dir().join(make_bundle_id());
        let (_, mut bundle, _) = generate_demo_bundle(Some(root.join("demo.scrawl"))).unwrap();
        let frame = RgbaFrame::new(0, 1, 1, vec![20, 30, 40, 255]);
        let mut key = bundle.keyframes[0].clone();
        key.file = "frames/missing/frame.png".into();
        let frame_failure = root.join("frame-failure.scrawl");
        assert!(
            write_bundle_artifacts(
                &frame_failure,
                &bundle,
                &[(key, frame.clone())],
                &frame,
                "summary"
            )
            .is_err()
        );
        assert!(!frame_failure.join("scrawl.json").exists());

        bundle.contact_sheet = "missing/contact-sheet.png".into();
        let sheet_failure = root.join("sheet-failure.scrawl");
        assert!(write_bundle_artifacts(&sheet_failure, &bundle, &[], &frame, "summary").is_err());
        assert!(!sheet_failure.join("scrawl.json").exists());

        bundle.contact_sheet = "contact-sheet.png".into();
        let empty_frames = root.join("empty-frames.scrawl");
        write_bundle_artifacts(&empty_frames, &bundle, &[], &frame, "summary").unwrap();
        assert!(empty_frames.join("scrawl.json").exists());
        assert!(empty_frames.join("contact-sheet.png").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn full_keyframes_match_by_time_and_preserve_partial_decodes() {
        let kf = |index: usize, t: u64| Keyframe {
            index,
            t,
            reason: crate::model::KeyframeReason::Click,
            file: format!("frames/frame-{index:02}.png"),
            event: "tap".into(),
            said: None,
            mark_id: None,
            cursor: None,
        };
        let small = |t: u64| RgbaFrame::new(t, 32, 32, vec![10; 32 * 32 * 4]);
        let large = |t: u64| RgbaFrame::new(t, 160, 160, vec![200; 160 * 160 * 4]);

        let selected = vec![
            (kf(1, 1_000), small(1_000)),
            (kf(2, 2_000), small(2_000)),
            (kf(3, 3_000), small(3_000)),
        ];
        // Frame at 2_000 failed to decode and frames arrived out of order (3_000 clamped to 2_999).
        let full = vec![large(2_999), large(1_000)];
        let (merged, matched) = merge_full_keyframes(selected, full);
        assert_eq!(matched, 2);
        assert_eq!(merged[0].1.width, 160);
        assert_eq!(merged[0].1.t_ms, 1_000);
        assert_eq!(merged[1].1.width, 32);
        assert_eq!(merged[1].1.t_ms, 2_000);
        assert_eq!(merged[2].1.width, 160);
        assert_eq!(merged[2].1.t_ms, 3_000);
    }

    #[test]
    fn stages_compose_and_allow_intermediate_inspection_or_override() {
        let target = TargetInfo {
            mode: CaptureMode::Display,
            app: Some("OpenCode".into()),
            bundle_id: Some("ai.opencode.desktop".into()),
            window_title: Some("Workspace".into()),
            window_id: None,
            display_id: 1,
            frame: TargetFrame {
                x: 0.0,
                y: 0.0,
                width: 640.0,
                height: 400.0,
                scale: 2.0,
            },
        };
        let recording = Recording {
            target: target.clone(),
            created_at_ms: Some(1_700_000_000_000),
            duration_ms: 4_000,
            frames: vec![
                render_synthetic_demo_frame(320, 200, 500, 0),
                render_synthetic_demo_frame(320, 200, 2_500, 1),
            ],
            video: Some("recording.mp4".into()),
            video_offset_ms: 40,
            audio: Some("audio.wav".into()),
            ..Default::default()
        };
        let interactions = Interactions {
            events: vec![InteractionEvent::Click {
                t: 2_400,
                x: 0.45,
                y: 0.35,
                button: Some("left".into()),
                label: Some("Run".into()),
                role: None,
            }],
            flags: vec![500],
            ..Default::default()
        };
        let transcript = TranscriptData {
            source: "hex".into(),
            model: Some("parakeet-tdt-0.6b-v3".into()),
            text: "Click Run here.".into(),
            segments: Vec::new(),
            words: vec![
                WordTiming {
                    t0: 2_100,
                    t1: 2_350,
                    text: "Click".into(),
                },
                WordTiming {
                    t0: 2_350,
                    t1: 2_600,
                    text: "Run".into(),
                },
                WordTiming {
                    t0: 2_600,
                    t1: 2_900,
                    text: "here.".into(),
                },
            ],
            estimated: false,
        };
        let options = OutputOptions::for_target(&target);

        // Run stages individually and verify intermediate seam outputs:
        let plan = plan_keyframes(
            &recording,
            &interactions,
            &transcript,
            options.full_frames.as_deref(),
        );
        assert_eq!(plan.marks.len(), 1);
        assert_eq!(plan.marks[0].shape, Shape::Flag);
        assert!(!plan.selected.is_empty());

        // Swap crop stage to disable auto-crop even on Display mode:
        let uncropped = solve_crop(plan.clone(), &recording, &interactions, None);
        assert!(uncropped.autocrop.is_none());
        let rendered = render_frames(recording.duration_ms, uncropped);
        let built_manual = assemble_bundle(
            "manual-stages",
            recording.clone(),
            interactions.clone(),
            transcript.clone(),
            Some("Stage override".into()),
            rendered,
        );
        assert!(built_manual.bundle.autocrop.is_none());
        assert_eq!(built_manual.bundle.note.as_deref(), Some("Stage override"));

        // Builder with default options runs auto-crop on Display:
        let built_default = BundleBuilder::new("builder-test", recording)
            .interactions(interactions)
            .transcript(transcript)
            .options(options)
            .build();
        assert_eq!(built_default.bundle.id, "builder-test");
        assert!(!built_default.bundle.keyframes.is_empty());
        assert_eq!(
            built_default.annotated_frames.len(),
            built_default.bundle.keyframes.len()
        );
        let summary = built_default.summary_markdown(Path::new("/tmp/builder-test.scrawl"));
        assert!(summary.contains("Click Run here."));
    }

    #[test]
    fn candidate_buffer_thins_regular_frames_and_preserves_urgent_ones() {
        // Each 1x1 RGBA frame is 4 bytes; cap clamps to at least 60 frames.
        let mut buf = CandidateBuffer::new(240);
        assert_eq!(buf.interval(), FRAME_SAMPLE_INTERVAL);
        for i in 0..60_u64 {
            let urgent = i == 17 || i == 41;
            buf.push(RgbaFrame::new(i * 100, 1, 1, vec![1, 2, 3, 255]), urgent);
        }
        assert_eq!(buf.interval(), FRAME_SAMPLE_INTERVAL * 2);
        let kept = buf.into_frames();
        assert!(kept.len() < 60);
        assert_eq!(kept[0].t_ms, 0);
        assert!(kept.iter().any(|f| f.t_ms == 1_700));
        assert!(kept.iter().any(|f| f.t_ms == 4_100));
    }
}
