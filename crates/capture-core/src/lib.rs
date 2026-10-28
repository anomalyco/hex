//! Screen recording for macOS on `ScreenCaptureKit`.
//!
//! A host describes a recording with a [`Plan`] and starts it with [`Recorder::start`]. Each
//! display or window source records to its own H.264 MP4 through an `SCRecordingOutput`.
//! [`Recording::stop`] returns at once; the files are finished on the recording's own thread and
//! come back from [`Finishing::wait`] along with a versioned [`Manifest`] that places every file
//! on one host clock.
//!
//! The crate holds no global state and starts nothing that outlives a recording.
#![cfg(target_os = "macos")]

mod av_writer;
mod clicks;
mod cursor;
mod error;
mod external_audio;
mod final_frame_hold;
mod frames;
mod input;
mod manifest;
mod native;
mod plan;
mod probe;
mod session;
mod system_audio;
#[cfg(test)]
mod test_dir;
mod time;

pub use cursor::{
    CursorButton, CursorButtonEvent, CursorButtonState, CursorCapture, CursorCaptureDiagnostics,
    CursorClickCaptureStatus, CursorPositionSample, CursorTelemetry, Millis,
    NORMALIZED_COORDINATE_MAX, NormalizedCoordinate,
};
pub use error::Error;
pub use external_audio::ExternalAudioSink;
pub use final_frame_hold::{FinalFrameHold, FinalFrameHoldError, hold_final_video_frame};
pub use frames::{Frame, FrameTap};
pub use input::{
    HostEvent, HostInputReceiver, HostInputSender, InputEvent, KeyPolicy, Modifiers, RecordedInput,
    filter_key, host_channel, secure_input_enabled,
};
pub use manifest::{
    AudioGap, Clock, FileEntry, FileKind, Frame as SourceFrame, MANIFEST_VERSION, Manifest,
    Permission, SourceEntry, TrackKind,
};
pub use native::Recorder;
pub use plan::{
    AudioPlan, CursorDrawing, Exclusions, ExternalMic, Input, MANIFEST_NAME, Mic, Names, Plan,
    Source,
};
pub use session::{Finished, Finishing, Recording, SourceCursor};
pub use time::{HostTime, MediaTime, Timebase};
