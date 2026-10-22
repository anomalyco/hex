use crate::desktop_activity::DesktopActivity;
use crate::transcription_models::{
    ModelPreparationStage, TranscriptionModelId, TranscriptionSelection,
};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopSnapshot {
    pub(crate) activity: DesktopActivity,
    pub(crate) listener: DesktopListenerSnapshot,
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
    #[serde(default)]
    pub(crate) warning: Option<String>,
    #[serde(default)]
    pub(crate) shortcuts: DesktopShortcuts,
}

/// How the running listener receives its shortcuts.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum DesktopShortcuts {
    /// Hex registers the shortcuts Settings records (X11).
    #[default]
    Registered,
    /// The desktop's shortcuts portal bound these, in request order.
    Portal(Vec<DesktopGrantedShortcut>),
    /// Nothing registers shortcuts: the desktop's keybindings run `hex` commands.
    Commands,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopGrantedShortcut {
    pub(crate) name: String,
    /// The trigger the desktop granted; `None` when it refused.
    pub(crate) trigger: Option<String>,
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
    StartListening,
    StopListening,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_listener_snapshots_have_no_warning() {
        let snapshot: DesktopListenerSnapshot =
            serde_json::from_str(r#"{"running":true,"status":"Listening"}"#).unwrap();
        assert!(snapshot.running);
        assert_eq!(snapshot.status, "Listening");
        assert!(snapshot.warning.is_none());
        assert_eq!(snapshot.shortcuts, DesktopShortcuts::Registered);
    }
}
