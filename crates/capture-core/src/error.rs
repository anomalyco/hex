use std::fmt;

use crate::plan::{Rect, Source};

/// Why a recording could not start, or how a source failed to finish.
#[derive(Debug)]
pub enum Error {
    /// `ScreenCaptureKit` could not list shareable content (usually missing permission).
    ShareableContent(String),
    /// A source in the plan is not on screen.
    SourceUnavailable(Source),
    InvalidDimensions,
    /// A region with no area on its display.
    InvalidRegion(Rect),
    InvalidFrameRate,
    NoSources,
    DuplicateSources,
    /// File names must be plain, distinct names inside the bundle directory.
    InvalidNames(String),
    /// Window exclusions can be combined with whole-app exclusions only when the windows belong to
    /// those apps: `ScreenCaptureKit` filters a display by windows or by apps, not both.
    Exclusion(String),
    MicrophoneUnavailable(String),
    Output(std::io::Error),
    CreateRecordingOutput,
    ConfigureRecording(String),
    ConfigureStream(String),
    Start(String),
    /// The recording output reported a failure.
    Recording(String),
    /// The system stopped the stream. Distinct from [`Error::Recording`]: the output may still
    /// finish a complete file.
    StreamStopped(String),
    Stop(String),
    FinishTimeout,
    /// `SCScreenshotManager` could not take or read a screenshot.
    Screenshot(String),
    /// The finishing thread ended without a result.
    FinisherLost,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShareableContent(error) => write!(f, "screen capture access failed: {error}"),
            Self::SourceUnavailable(source) => {
                write!(f, "capture source is no longer available: {source:?}")
            }
            Self::InvalidDimensions => f.write_str("capture source has invalid dimensions"),
            Self::InvalidRegion(rect) => {
                write!(f, "capture region is not on its display: {rect:?}")
            }
            Self::InvalidFrameRate => f.write_str("frame rate must be greater than zero"),
            Self::NoSources => f.write_str("at least one capture source is required"),
            Self::DuplicateSources => f.write_str("capture sources must be unique"),
            Self::InvalidNames(error) => write!(f, "invalid file names: {error}"),
            Self::Exclusion(error) => write!(f, "cannot build the exclusion filter: {error}"),
            Self::MicrophoneUnavailable(id) => write!(f, "microphone is not available: {id}"),
            Self::Output(error) => write!(f, "failed to prepare recording output: {error}"),
            Self::CreateRecordingOutput => {
                f.write_str("failed to create a ScreenCaptureKit recording output")
            }
            Self::ConfigureRecording(error) => {
                write!(f, "failed to configure recording output: {error}")
            }
            Self::ConfigureStream(error) => {
                write!(f, "failed to configure capture stream: {error}")
            }
            Self::Start(error) => write!(f, "failed to start recording: {error}"),
            Self::Recording(error) => write!(f, "recording failed: {error}"),
            Self::StreamStopped(error) => write!(f, "capture stream stopped: {error}"),
            Self::Stop(error) => write!(f, "failed to stop recording: {error}"),
            Self::FinishTimeout => f.write_str("timed out waiting for recording finalization"),
            Self::Screenshot(error) => write!(f, "screenshot failed: {error}"),
            Self::FinisherLost => f.write_str("the finishing thread ended without a result"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Output(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Output(error)
    }
}
