use crate::desktop::activity::DesktopActivity;
use crate::transcription::models::{
    ModelPreparationStage, TranscriptionModelId, TranscriptionSelection,
};

/// The Linux service's snapshot; Windows and macOS publish their own
/// in-process.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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
    /// The desktop can show its own dialog for Hex's shortcuts.
    #[serde(default)]
    pub(crate) shortcuts_configurable: bool,
}

/// How the running listener receives its shortcuts.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum DesktopShortcuts {
    /// No listener has registered its shortcuts yet: it is starting or stopped.
    #[default]
    Unregistered,
    /// Hex registers the shortcuts Settings records (X11).
    Registered,
    /// The desktop's shortcuts portal bound these, in request order.
    Portal(Vec<DesktopGrantedShortcut>),
    /// Nothing registers shortcuts: the desktop's keybindings run `hex` commands.
    Commands,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopGrantedShortcut {
    pub(crate) name: String,
    /// The trigger the desktop granted; `None` when it refused, or, with
    /// `unconfirmed`, when it names none.
    pub(crate) trigger: Option<String>,
    /// The desktop assigns the key in its own configuration and its portal
    /// describes no trigger (Hyprland), so a missing trigger is not a
    /// refusal: Hex cannot confirm which key, if any, runs it.
    #[serde(default)]
    pub(crate) unconfirmed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum DesktopUpdateStatus {
    /// A test build, which never updates.
    Unavailable,
    /// An administrator turned updates off (`DisableUpdates`).
    Managed,
    Checking,
    Current,
    Failed,
    Downloading,
    /// Another release exists that this install cannot apply itself; the
    /// window links to its download.
    Available,
    ReadyToRestart,
    /// The Update channel setting names another channel; its current
    /// release installs only once the user confirms ("Switch to Stable
    /// 2.3.1").
    SwitchAvailable {
        channel: String,
        version: String,
    },
    /// Installed from a source checkout, which updates it; the release
    /// service never replaces it.
    SourceBuild,
    /// Installed by a package manager (pacman, Nix), which updates it.
    PackageManager,
    /// The package manager that installed Hex serves another version.
    PackageUpdate {
        command: String,
    },
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
    /// Checks for an update now ("Check for updates").
    CheckForUpdates,
    ClearError,
    /// Opens the download an install cannot apply itself.
    OpenUpdate,
    RestartIntoUpdate,
    /// Installs the other channel's release the Update channel setting chose.
    ConfirmUpdateSwitch,
    /// The window opened or gained focus; the Dev channel checks then.
    WindowFocused,
    SetDictationShortcut(DesktopShortcut),
    /// Opens the desktop's dialog for the shortcuts its portal bound.
    ConfigureShortcuts,
    StartListening,
    StopListening,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_listener_snapshots_have_no_warning_or_shortcuts() {
        let snapshot: DesktopListenerSnapshot =
            serde_json::from_str(r#"{"running":true,"status":"Listening"}"#).unwrap();
        assert!(snapshot.running);
        assert_eq!(snapshot.status, "Listening");
        assert!(snapshot.warning.is_none());
        assert_eq!(snapshot.shortcuts, DesktopShortcuts::Unregistered);
    }
}
