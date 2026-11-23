//! The macOS recording environment a capture opens for "While dictating":
//! idle-sleep prevention, output suppression, paused media, and the moments
//! suppression says it actually took effect.
use std::sync::mpsc;

use crate::audio::CaptureInstant;
use crate::audio::recording_environment::{
    RecordingEnvironmentController, RecordingEnvironmentSession,
};

use super::Recording;

pub(super) enum RecordingEnvironmentState {
    Disabled,
    Pending(RecordingEnvironmentController),
    Active {
        _session: RecordingEnvironmentSession,
    },
}

impl RecordingEnvironmentState {
    /// Starts the environment now and keeps its report channel, so output
    /// suppression can say when it actually took effect.
    pub(super) fn activate(&mut self, reports: &mut Option<mpsc::Receiver<CaptureInstant>>) {
        let Self::Pending(controller) = self else {
            return;
        };
        let controller = controller.clone();
        let (report, receiver) = mpsc::channel();
        *reports = Some(receiver);
        *self = Self::Active {
            _session: controller.begin_reporting(report),
        };
    }
}

impl Recording {
    /// Raises the head to where output suppression actually took effect, as
    /// the environment reported it, and tells an open stream.
    pub(super) fn apply_suppression_report(&mut self) {
        let Some(at) = self.effective_suppression() else {
            return;
        };
        let Some(head) = self.note_suppression_through(at) else {
            return;
        };
        if let Some(stream) = &mut self.stream {
            stream.raise_ignored_head(head);
        }
    }

    /// The latest moment "While dictating" actually changed the environment.
    fn effective_suppression(&mut self) -> Option<CaptureInstant> {
        self.suppression_reports.as_mut()?.try_iter().last()
    }
}
