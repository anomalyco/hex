use color_eyre::Result;

use crate::desktop_activity::DesktopActivity;
use crate::transcription_models::{
    ModelPreparationStage, TranscriptionModelId, TranscriptionSelection,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) struct DesktopCapabilities {
    pub(crate) activity: bool,
    pub(crate) commands: bool,
    pub(crate) history: bool,
    pub(crate) hud_lab: bool,
    pub(crate) meetings: bool,
    pub(crate) modes: bool,
    pub(crate) replacements: bool,
    pub(crate) listener_control: bool,
    pub(crate) update_restart: bool,
    pub(crate) voice_action: bool,
}

impl DesktopCapabilities {
    #[cfg(target_os = "macos")]
    pub(crate) const fn macos(developer_features: bool) -> Self {
        Self {
            activity: developer_features,
            commands: true,
            history: true,
            hud_lab: developer_features,
            meetings: developer_features,
            modes: true,
            replacements: true,
            listener_control: false,
            update_restart: false,
            voice_action: true,
        }
    }

    #[cfg(any(target_os = "linux", test))]
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub(crate) const fn linux_x11() -> Self {
        Self {
            activity: true,
            commands: false,
            history: true,
            hud_lab: crate::DEVELOPER_FEATURES_ENABLED,
            meetings: false,
            modes: true,
            replacements: true,
            listener_control: true,
            update_restart: true,
            voice_action: true,
        }
    }

    #[cfg(target_os = "windows")]
    pub(crate) const fn windows() -> Self {
        Self {
            activity: true,
            commands: false,
            history: true,
            hud_lab: crate::DEVELOPER_FEATURES_ENABLED,
            meetings: false,
            modes: true,
            replacements: true,
            listener_control: true,
            update_restart: true,
            voice_action: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSnapshot {
    pub(crate) activity: DesktopActivity,
    pub(crate) dictation_shortcut: Vec<String>,
    pub(crate) dictation_shortcut_label: String,
    pub(crate) double_tap_lock: bool,
    pub(crate) double_tap_only: bool,
    pub(crate) microphone: DesktopMicrophoneSnapshot,
    pub(crate) paste_last_shortcut: Option<Vec<String>>,
    pub(crate) listener: Option<DesktopListenerSnapshot>,
    pub(crate) operation_error: Option<String>,
    pub(crate) observations_path: String,
    pub(crate) transcription: DesktopTranscriptionSnapshot,
    pub(crate) update_status: DesktopUpdateStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopMicrophoneSnapshot {
    pub(crate) devices: Vec<String>,
    pub(crate) error: Option<String>,
    pub(crate) selected: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopTranscriptionSnapshot {
    pub(crate) downloaded_bytes: u64,
    pub(crate) error: Option<String>,
    pub(crate) preparation_stage: Option<ModelPreparationStage>,
    pub(crate) selection: TranscriptionSelection,
    pub(crate) preparing: Option<TranscriptionModelId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopListenerSnapshot {
    pub(crate) running: bool,
    pub(crate) status: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
pub(crate) enum DesktopUpdateStatus {
    Unavailable,
    Checking,
    Current,
    Failed,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Available,
    ReadyToRestart,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopShortcut {
    pub(crate) alt: bool,
    pub(crate) control: bool,
    pub(crate) function: bool,
    pub(crate) key: String,
    pub(crate) platform: bool,
    pub(crate) shift: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum DesktopAction {
    CheckForUpdates,
    ClearError,
    RefreshMicrophones,
    RestartIntoUpdate,
    SetDictationShortcut(DesktopShortcut),
    SetDoubleTapLock(bool),
    SetDoubleTapOnly(bool),
    SetMicrophone(Option<String>),
    StartListening,
    StopListening,
}

pub(crate) trait DesktopHost {
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    fn capabilities(&self) -> DesktopCapabilities;
    fn snapshot(&self) -> DesktopSnapshot;
    fn dispatch(&mut self, action: DesktopAction) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owns_host_object(_: Box<dyn DesktopHost>) {}

    struct TestHost;

    impl DesktopHost for TestHost {
        fn capabilities(&self) -> DesktopCapabilities {
            DesktopCapabilities::linux_x11()
        }

        fn snapshot(&self) -> DesktopSnapshot {
            DesktopSnapshot {
                activity: DesktopActivity::default(),
                dictation_shortcut: Vec::new(),
                dictation_shortcut_label: String::new(),
                double_tap_lock: false,
                double_tap_only: false,
                microphone: DesktopMicrophoneSnapshot {
                    devices: Vec::new(),
                    error: None,
                    selected: None,
                },
                paste_last_shortcut: None,
                listener: None,
                operation_error: None,
                observations_path: String::new(),
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

    #[test]
    fn linux_exposes_only_implemented_product_capabilities() {
        // Activity shipped on Linux in the 2026-08 port wave. HUD Lab is
        // visible only in developer builds. The command runtime prototype
        // remains a Settings opt-in; it does not advertise the catalog pane.
        // EWMH application context, mode processing, transformations, and
        // PRIMARY-selection Voice Action are source-complete; physical Arch/i3
        // validation remains separate from capability availability.
        assert_eq!(
            DesktopCapabilities::linux_x11(),
            DesktopCapabilities {
                activity: true,
                commands: false,
                history: true,
                hud_lab: crate::DEVELOPER_FEATURES_ENABLED,
                meetings: false,
                modes: true,
                replacements: true,
                listener_control: true,
                update_restart: true,
                voice_action: true,
            }
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_exposes_the_managed_update_restart_action() {
        assert!(DesktopCapabilities::windows().update_restart);
    }
}
