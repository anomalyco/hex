//! Pure, GPUI-free core library for Scrawl screen-capture bundles inside Hex.
//!
//! `scrawl-core` owns the `.scrawl` bundle data model, keyframe selection, stroke/shape
//! classification, activity-weighted auto-crop, CPU RGBA frame and contact-sheet rendering,
//! zoom-sheet layout, video export planning, the `⌥⇧S` tap/hold gesture state machine, and
//! the composable [`pipeline`] stages ([`plan_keyframes`] → [`solve_crop`] → [`render_frames`] →
//! [`assemble_bundle`]).
//!
//! Platform capture (ScreenCaptureKit, EventTap, CoreAudio, AVFoundation, Metal overlays) and
//! UI shells (GPUI, AppKit) live outside this crate and interact through [`traits`] and
//! [`pipeline`].

pub mod autocrop;
pub mod export;
pub mod hold;
pub mod keyframes;
pub mod model;
pub mod pipeline;
pub mod render;
pub mod sketch;
pub mod traits;
pub mod windows;
pub mod zoom;

pub use model::{
    AutocropInfo, CaptureMode, ContextChange, DeviceInfo, InteractionEvent, Keyframe, KeyframeCrop,
    KeyframeReason, Mark, RgbaFrame, SCALE, ScrawlBundle, ScreenSpan, Shape, Stroke, TargetFrame,
    TargetInfo, TranscriptData, TranscriptSegment, WordTiming, format_clock, keyframe_budget,
    parse_clock_ms,
};
pub use pipeline::{
    BuiltBundle, BundleBuilder, CANDIDATE_BUDGET_BYTES, CANDIDATE_EDGE, CANDIDATE_EDGE_NO_VIDEO,
    CandidateBuffer, CroppedFrames, FRAME_SAMPLE_INTERVAL, FullFrameBundle, FullFrames,
    FullFramesFn, Interactions, KeyframePlan, OutputOptions, PreparedVisuals, Recording,
    RenderedFrames, SourceExtras, StageTimings, assemble_bundle, build_bundle,
    build_bundle_from_parts, generate_demo_bundle, make_bundle_id, merge_full_keyframes,
    now_epoch_ms, plan_keyframes, prepare_visuals, render_frames, solve_crop,
    write_bundle_artifacts,
};
pub use traits::{
    Clock, FrameSource, InputSource, ScrawlInputBatch, SystemClock, WindowProbe, WordTranscriber,
};
pub use windows::{ActivityLog, AppMeta, CursorSample, RawWindow, WindowEntry, WindowSnapshot};
