//! What a meeting's system track records: all computer audio (the default),
//! one output device, or one application (Labs › Choose meeting audio).
//!
//! The listing functions never fail: a sound server, COM apartment, or Core
//! Audio read that cannot answer lists nothing, which keeps the Meetings pane
//! usable. The capture backends read [`effective`] as they open, and again on
//! each route check: another choice reopens the source mid-recording, so a
//! choice is honored only while the lab is on and applies as soon as it is
//! made. With the lab off it is always [`SourceChoice::All`], exactly today's
//! behavior.

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
#[path = "audio_source/linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "audio_source/macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "audio_source/windows.rs"]
mod platform;

/// One source the Meetings pane offers: an output device or an application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceEntry {
    /// The platform's stable identity. macOS: a canonical bundle id;
    /// Windows: a lowercase executable file name such as `ms-teams.exe`;
    /// Linux: PipeWire's `application.process.binary` (else
    /// `application.name`) for an app, or a sink name for an output.
    pub(crate) id: String,
    /// What the window shows.
    pub(crate) name: String,
}

/// Which audio a meeting's system track records.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceChoice {
    /// Every output, as meetings have always recorded (the default).
    #[default]
    All,
    /// One application's output only.
    App {
        /// The platform's stable identity for the app.
        id: String,
        /// The app's display name, kept so a saved choice stays readable
        /// while the app isn't playing.
        name: String,
    },
    /// One output device only.
    Output {
        /// The platform's stable identity for the device.
        id: String,
        /// The device's display name, kept so a saved choice stays readable
        /// while it isn't connected.
        name: String,
    },
}

/// The choice a meeting's system track records now: the saved one while
/// Labs › Choose meeting audio is on, else all computer audio. A backend
/// reads it as it opens and on each route check, so another choice reopens
/// the source mid-recording.
pub(crate) fn effective() -> SourceChoice {
    crate::labs::meeting_audio_source().unwrap_or_default()
}

impl SourceChoice {
    pub(crate) fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// The app this choice records, while it names one.
    pub(crate) fn app_id(&self) -> Option<&str> {
        match self {
            Self::App { id, .. } => Some(id),
            _ => None,
        }
    }

    /// The output this choice records, while it names one.
    pub(crate) fn output_id(&self) -> Option<&str> {
        match self {
            Self::Output { id, .. } => Some(id),
            _ => None,
        }
    }

    /// Whether both choices name the same source, whatever display name
    /// each carries. A saved app or device whose name changed is still the
    /// listed one, not a second "not playing now" entry.
    pub(crate) fn same_source(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::All, Self::All) => true,
            (Self::App { id: left, .. }, Self::App { id: right, .. })
            | (Self::Output { id: left, .. }, Self::Output { id: right, .. }) => left == right,
            _ => false,
        }
    }

    /// The English label the window shows.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::All => "All computer audio",
            Self::App { name, .. } | Self::Output { name, .. } => name,
        }
    }

    /// Why a saved choice isn't in a fresh list, in one line under its name.
    pub(crate) fn missing_hint(&self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Output { .. } => Some("not connected"),
            Self::App { .. } => Some("not playing now"),
        }
    }
}

/// The applications playing audio right now, each once. Off the UI thread:
/// listing reads process metadata and may start a PipeWire watcher.
pub(crate) fn playing_apps() -> Vec<SourceEntry> {
    platform::playing_apps()
}

/// The output devices a meeting may record right now. Off the UI thread:
/// listing runs `pactl` on Linux and opens Core Audio or COM objects
/// elsewhere.
pub(crate) fn output_devices() -> Vec<SourceEntry> {
    platform::output_devices()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_round_trips_through_json() {
        for choice in [
            SourceChoice::All,
            SourceChoice::App {
                id: "com.apple.Music".into(),
                name: "Music".into(),
            },
            SourceChoice::Output {
                id: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
                name: "Built-in Audio".into(),
            },
        ] {
            let json = serde_json::to_string(&choice).unwrap();
            let decoded: SourceChoice = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, choice, "{json}");
        }
        assert_eq!(
            serde_json::to_string(&SourceChoice::All).unwrap(),
            "\"all\"",
            "All is the plain default a settings file writes"
        );
    }

    #[test]
    fn a_malformed_saved_choice_decodes_to_all() {
        for saved in [
            "null",
            "\"future\"",
            "{}",
            "{\"app\":{\"id\":7,\"name\":\"Music\"}}",
            "{\"output\":{\"id\":\"speakers\"}}",
            "[\"all\"]",
        ] {
            let decoded: SourceChoice = serde_json::from_str(saved).unwrap_or_default();
            assert_eq!(decoded, SourceChoice::All, "{saved}");
        }
    }

    /// With the lab off the system track records every output exactly as it
    /// always has, whatever choice is saved; on, it records the choice.
    #[test]
    fn the_lab_gate_keeps_all_computer_audio_while_off() {
        let chosen = SourceChoice::App {
            id: "com.apple.Music".into(),
            name: "Music".into(),
        };
        crate::labs::apply(&crate::app_settings::LabsSettings {
            meeting_audio_source: false,
            meeting_audio_source_choice: chosen.clone(),
            ..Default::default()
        });
        assert_eq!(effective(), SourceChoice::All);
        crate::labs::apply(&crate::app_settings::LabsSettings {
            meeting_audio_source: true,
            meeting_audio_source_choice: chosen.clone(),
            ..Default::default()
        });
        assert_eq!(effective(), chosen);
        crate::labs::apply(&crate::app_settings::LabsSettings::default());
    }

    /// A choice matches the listing by its id alone: an app that renamed
    /// itself is still the listed app, never a second missing entry.
    #[test]
    fn a_saved_choice_matches_a_listed_source_by_id() {
        let listed = SourceChoice::App {
            id: "google-chrome".into(),
            name: "Google Chrome".into(),
        };
        let renamed = SourceChoice::App {
            id: "google-chrome".into(),
            name: "Chrome".into(),
        };
        assert!(renamed.same_source(&listed));
        assert!(
            !SourceChoice::App {
                id: "safari".into(),
                name: "Google Chrome".into(),
            }
            .same_source(&listed)
        );
        let speakers = SourceChoice::Output {
            id: "speakers".into(),
            name: "Speakers".into(),
        };
        let renamed_speakers = SourceChoice::Output {
            id: "speakers".into(),
            name: "Built-in audio".into(),
        };
        assert!(renamed_speakers.same_source(&speakers));
        assert!(!SourceChoice::All.same_source(&listed));
        assert!(!speakers.same_source(&SourceChoice::All));
        assert!(SourceChoice::All.same_source(&SourceChoice::All));
    }

    /// A choice a person saved stays readable while the app or device is
    /// missing, with a line naming why it isn't listed.
    #[test]
    fn a_missing_choice_keeps_its_name_and_says_why() {
        let app = SourceChoice::App {
            id: "com.apple.Music".into(),
            name: "Music".into(),
        };
        assert_eq!(app.name(), "Music");
        assert_eq!(app.app_id(), Some("com.apple.Music"));
        assert_eq!(app.output_id(), None);
        assert_eq!(app.missing_hint(), Some("not playing now"));
        let output = SourceChoice::Output {
            id: "speakers".into(),
            name: "Speakers".into(),
        };
        assert_eq!(output.missing_hint(), Some("not connected"));
        assert_eq!(SourceChoice::All.missing_hint(), None);
        assert_eq!(SourceChoice::All.name(), "All computer audio");
    }
}
