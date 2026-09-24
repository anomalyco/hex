use color_eyre::Result;

use crate::desktop_activity::DesktopActivity;
use crate::transcription_models::{
    ModelPreparationStage, TranscriptionModelId, TranscriptionSelection,
};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopSnapshot {
    pub(crate) activity: DesktopActivity,
    pub(crate) dictation_shortcut: Vec<String>,
    pub(crate) double_tap_lock: bool,
    pub(crate) double_tap_only: bool,
    pub(crate) listener: Option<DesktopListenerSnapshot>,
    pub(crate) operation_error: Option<String>,
    pub(crate) transcription: DesktopTranscriptionSnapshot,
    pub(crate) update_status: DesktopUpdateStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopTranscriptionSnapshot {
    pub(crate) downloaded_bytes: u64,
    pub(crate) error: Option<String>,
    pub(crate) preparation_stage: Option<ModelPreparationStage>,
    pub(crate) selection: TranscriptionSelection,
    pub(crate) preparing: Option<TranscriptionModelId>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopListenerSnapshot {
    pub(crate) running: bool,
    pub(crate) status: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum DesktopUpdateStatus {
    Unavailable,
    Checking,
    Current,
    Failed,
    ReadyToRestart,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopShortcut {
    pub(crate) alt: bool,
    pub(crate) control: bool,
    pub(crate) function: bool,
    pub(crate) key: String,
    pub(crate) platform: bool,
    pub(crate) shift: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum DesktopAction {
    ClearError,
    RestartIntoUpdate,
    SetDictationShortcut(DesktopShortcut),
    SetDoubleTapLock(bool),
    SetDoubleTapOnly(bool),
    StartListening,
    StopListening,
}

/// Contract between the Linux Settings client and its service-owned runtime.
pub(crate) trait DesktopHost {
    fn snapshot(&self) -> DesktopSnapshot;
    fn dispatch(&mut self, action: DesktopAction) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owns_host_object(_: Box<dyn DesktopHost>) {}

    struct TestHost;

    impl DesktopHost for TestHost {
        fn snapshot(&self) -> DesktopSnapshot {
            DesktopSnapshot {
                activity: DesktopActivity::default(),
                dictation_shortcut: Vec::new(),
                double_tap_lock: false,
                double_tap_only: false,
                listener: None,
                operation_error: None,
                transcription: DesktopTranscriptionSnapshot {
                    downloaded_bytes: 0,
                    error: None,
                    preparation_stage: None,
                    selection: TranscriptionSelection::default(),
                    preparing: None,
                },
                update_status: DesktopUpdateStatus::Unavailable,
            }
        }

        fn dispatch(&mut self, _: DesktopAction) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn host_can_be_contained_by_the_shared_window() {
        owns_host_object(Box::new(TestHost));
    }
}
