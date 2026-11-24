//! What the tray menu offers: the downloaded models the macOS menu bar and
//! the Linux and Windows trays list, and the tray menu those two share
//! (status, Show Hex, Paste last dictation, the meeting items, the model
//! picker, the update, Enable / Disable Hex, Quit Hex). Each tray maps a picked [`TrayCommand`]
//! onto its own runtime.

use crate::app_settings::AppSettings;
use crate::transcription::models::{
    ModelDefinition, TranscriptionModelId, all_models, language_name,
};

/// One downloaded model in a menu's model picker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModelMenuEntry {
    pub(crate) model: TranscriptionModelId,
    pub(crate) title: String,
    pub(crate) selected: bool,
    pub(crate) enabled: bool,
}

/// The available models `installed` reports downloaded, each with its
/// remembered language. None can be picked while one is `preparing`, and
/// the selected one never can.
pub(crate) fn model_menu_entries(
    settings: &AppSettings,
    preparing: bool,
    installed: impl Fn(&ModelDefinition, &str) -> bool,
) -> Vec<ModelMenuEntry> {
    all_models()
        .into_iter()
        .filter(|model| model.available())
        .filter_map(|model| {
            let selection = settings.transcription_for_model(model.id);
            installed(&model, &selection.language).then(|| ModelMenuEntry {
                model: model.id,
                title: format!("{} · {}", model.name, language_name(&selection.language)),
                selected: model.id == settings.transcription.model,
                enabled: !preparing && model.id != settings.transcription.model,
            })
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub(crate) use tray::*;

#[cfg(not(target_os = "macos"))]
mod tray {
    use super::ModelMenuEntry;
    use crate::desktop::events::VoiceState;
    use crate::desktop::host::DesktopUpdateStatus;
    use crate::desktop::i18n::tr;
    use crate::transcription::models::TranscriptionModelId;

    /// What a tray menu item asks for.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum TrayCommand {
        Show,
        PasteLast,
        /// Labs › Copy dictation audio: the newest kept recording as a file
        /// on the clipboard.
        CopyLastDictationAudio,
        UseModel(TranscriptionModelId),
        /// The update item: restart into it, download it, or switch channel.
        Update,
        ToggleListening,
        Quit,
        RecordMeeting,
        /// Stop & transcribe the recording meeting.
        StopMeeting,
        CopyLastMeeting,
    }

    impl TrayCommand {
        /// The command behind a menu item's id.
        pub(crate) fn from_id(id: &str) -> Option<Self> {
            Some(match id {
                "show" => Self::Show,
                "paste-last" => Self::PasteLast,
                "copy-last-dictation-audio" => Self::CopyLastDictationAudio,
                "update" => Self::Update,
                "toggle" => Self::ToggleListening,
                "quit" => Self::Quit,
                "meeting-record" => Self::RecordMeeting,
                "meeting-stop" => Self::StopMeeting,
                "meeting-copy-last" => Self::CopyLastMeeting,
                _ => Self::UseModel(id.strip_prefix("model:")?.parse().ok()?),
            })
        }
    }

    /// The meeting recorder, as the tray shows it.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum MeetingTray {
        Idle,
        Starting,
        Recording,
        /// Stop was requested; the recorder finishes this meeting before it
        /// takes another, so Record stays disabled.
        Stopping,
        Transcribing,
    }

    /// Everything the tray shows.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub(crate) struct TrayView {
        pub(crate) listening: bool,
        pub(crate) state: Option<VoiceState>,
        pub(crate) update: Option<crate::updater::UpdateOffer>,
        pub(crate) models: Vec<ModelMenuEntry>,
        /// `None` while Meetings is off.
        pub(crate) meetings: Option<MeetingTray>,
        /// Labs › Copy dictation audio: `None` hides its item, `Some`
        /// lists it, enabled while a kept recording can be copied.
        pub(crate) dictation_audio: Option<bool>,
    }

    impl TrayView {
        pub(crate) fn new(
            listening: bool,
            state: Option<VoiceState>,
            update: &DesktopUpdateStatus,
            models: Vec<ModelMenuEntry>,
        ) -> Self {
            Self {
                listening,
                state: state.filter(|_| listening),
                update: crate::updater::offer(update),
                models,
                meetings: None,
                dictation_audio: None,
            }
        }

        fn recording(&self) -> bool {
            self.state == Some(VoiceState::Dictating)
        }

        fn transcribing(&self) -> bool {
            self.state == Some(VoiceState::Transcribing)
        }

        pub(crate) fn tooltip(&self) -> &'static str {
            if self.recording() {
                tr("Hex Dictation: recording")
            } else if self.transcribing() {
                tr("Hex Dictation: transcribing")
            } else if self.update.is_some() {
                tr("Hex Dictation: update available")
            } else {
                tr("Hex Dictation")
            }
        }

        pub(crate) fn menu(&self) -> Vec<rune::TrayItem> {
            use rune::TrayItem;
            let mut menu = Vec::new();
            let status = if self.recording() {
                Some(tr("Recording"))
            } else if self.transcribing() {
                Some(tr("Transcribing…"))
            } else {
                None
            };
            if let Some(status) = status {
                menu.push(TrayItem::action("status", status).enabled(false));
                menu.push(TrayItem::Separator);
            }
            menu.push(TrayItem::action("show", tr("Show Hex")));
            menu.push(
                TrayItem::action("paste-last", tr("Paste last dictation")).enabled(self.listening),
            );
            if let Some(has_audio) = self.dictation_audio {
                // Labs › Copy dictation audio: dimmed while no History entry
                // kept its audio.
                menu.push(
                    TrayItem::action("copy-last-dictation-audio", tr("Copy last dictation audio"))
                        .enabled(has_audio),
                );
            }
            if let Some(meetings) = self.meetings {
                menu.push(TrayItem::Separator);
                // The disabled states take ids of their own: some tray hosts
                // (GNOME's AppIndicators) keep an item's disabled state when
                // the same id comes back enabled (#1287).
                menu.push(match meetings {
                    MeetingTray::Idle => TrayItem::action("meeting-record", tr("Record meeting")),
                    MeetingTray::Starting => {
                        TrayItem::action("meeting-starting", tr("Starting…")).enabled(false)
                    }
                    MeetingTray::Recording => {
                        TrayItem::action("meeting-stop", tr("Stop & transcribe"))
                    }
                    MeetingTray::Stopping => {
                        TrayItem::action("meeting-stopping", tr("Stopping…")).enabled(false)
                    }
                    MeetingTray::Transcribing => {
                        TrayItem::action("meeting-transcribing", tr("Transcribing…")).enabled(false)
                    }
                });
                menu.push(TrayItem::action(
                    "meeting-copy-last",
                    tr("Copy last meeting transcript"),
                ));
            }
            if !self.models.is_empty() {
                menu.push(TrayItem::Separator);
                menu.push(TrayItem::action("models", tr("Speech model")).enabled(false));
                menu.extend(self.models.iter().map(|entry| {
                    let mark = if entry.selected { "✓ " } else { "    " };
                    TrayItem::action(
                        format!("model:{}", entry.model.as_str()),
                        format!("{mark}{}", entry.title),
                    )
                    .enabled(entry.enabled)
                }));
            }
            menu.push(TrayItem::Separator);
            // A stop under way still holds Restart to update: the recorder
            // is finishing that meeting.
            let meeting_recording = matches!(
                self.meetings,
                Some(MeetingTray::Recording | MeetingTray::Stopping)
            );
            menu.extend(self.update.as_ref().map(|offer| {
                // Distinct id while disabled so tray hosts like GNOME's
                // AppIndicators don't keep it stuck disabled (#1287).
                if offer.is_held(meeting_recording) {
                    TrayItem::action("update-held", offer.label.clone()).enabled(false)
                } else {
                    TrayItem::action("update", offer.label.clone())
                }
            }));
            menu.push(TrayItem::action("toggle", tr("Enable / Disable Hex")));
            menu.push(TrayItem::action("quit", tr("Quit Hex")));
            menu
        }
    }

    /// What the update item does for `status`, if it shows.
    pub(crate) fn update_action(
        status: &DesktopUpdateStatus,
    ) -> Option<crate::desktop::host::DesktopAction> {
        crate::updater::offer(status).map(|offer| offer.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::models::TranscriptionSelection;

    #[test]
    fn model_menu_only_lists_downloaded_models_with_their_remembered_language() {
        let mut settings = AppSettings::default();
        settings.remember_transcription(TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "fr".into(),
        });
        settings.remember_transcription(TranscriptionSelection::default());
        let installed = |model: &ModelDefinition, _: &str| {
            matches!(
                model.id,
                TranscriptionModelId::ParakeetUnifiedEnglish
                    | TranscriptionModelId::WhisperLargeV3Turbo
            )
        };
        let entries = model_menu_entries(&settings, false, installed);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Parakeet Unified English · English");
        assert!(entries[0].selected);
        assert!(!entries[0].enabled);
        assert_eq!(entries[1].title, "Whisper large-v3-turbo · French");
        assert!(!entries[1].selected);
        assert!(entries[1].enabled);
        let preparing = model_menu_entries(&settings, true, installed);
        assert!(preparing.iter().all(|entry| !entry.enabled));
        assert!(
            preparing[0].selected,
            "preparing must not mark the candidate selected"
        );
        assert!(model_menu_entries(&settings, false, |_, _| false).is_empty());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn tray_menu_ids_round_trip_and_show_the_state() {
        use crate::desktop::events::VoiceState;
        use crate::desktop::host::DesktopUpdateStatus;
        let settings = AppSettings::default();
        let models = model_menu_entries(&settings, false, |model, _| {
            model.id == settings.transcription.model
        });
        let view = TrayView::new(
            true,
            Some(VoiceState::Dictating),
            &DesktopUpdateStatus::ReadyToRestart,
            models,
        );
        let menu = view.menu();
        let ids: Vec<&str> = menu
            .iter()
            .filter_map(|item| match item {
                rune::TrayItem::Action { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let model_id = format!("model:{}", settings.transcription.model.as_str());
        assert_eq!(
            ids,
            [
                "status",
                "show",
                "paste-last",
                "models",
                model_id.as_str(),
                "update",
                "toggle",
                "quit"
            ]
        );
        assert_eq!(
            TrayCommand::from_id(&model_id),
            Some(TrayCommand::UseModel(settings.transcription.model))
        );
        assert_eq!(
            TrayCommand::from_id("paste-last"),
            Some(TrayCommand::PasteLast)
        );
        assert_eq!(TrayCommand::from_id("status"), None);
        assert_eq!(view.tooltip(), "Hex Dictation: recording");
        // Meetings on adds its items after Paste last dictation.
        let meeting = TrayView {
            meetings: Some(MeetingTray::Recording),
            ..view.clone()
        };
        let menu = meeting.menu();
        let ids: Vec<&str> = menu
            .iter()
            .filter_map(|item| match item {
                rune::TrayItem::Action { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids[2..5],
            ["paste-last", "meeting-stop", "meeting-copy-last"]
        );
        assert_eq!(
            TrayCommand::from_id("meeting-stop"),
            Some(TrayCommand::StopMeeting)
        );
        // Labs › Copy dictation audio lists its item after Paste last
        // dictation, dimmed while no kept recording can be copied.
        let audio = TrayView {
            dictation_audio: Some(false),
            ..view.clone()
        };
        let ids: Vec<String> = audio
            .menu()
            .into_iter()
            .filter_map(|item| match item {
                rune::TrayItem::Action { id, .. } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(ids[2], "paste-last");
        assert_eq!(ids[3], "copy-last-dictation-audio");
        assert_eq!(
            TrayCommand::from_id("copy-last-dictation-audio"),
            Some(TrayCommand::CopyLastDictationAudio)
        );
        // Without a kept recording the item is dimmed rather than hidden.
        assert!(matches!(
            audio.menu().into_iter().find(|item| matches!(
                item,
                rune::TrayItem::Action { id, .. } if id == "copy-last-dictation-audio"
            )),
            Some(rune::TrayItem::Action { enabled: false, .. })
        ));
        // Disabled meeting states never reuse an enabled item's id (#1287).
        let meeting_id = |state| {
            TrayView {
                meetings: Some(state),
                ..view.clone()
            }
            .menu()
            .into_iter()
            .find_map(|item| match item {
                rune::TrayItem::Action { id, .. } if id.starts_with("meeting-") => Some(id),
                _ => None,
            })
            .unwrap()
        };
        let enabled = [
            meeting_id(MeetingTray::Idle),
            meeting_id(MeetingTray::Recording),
        ];
        for disabled in [
            MeetingTray::Starting,
            MeetingTray::Stopping,
            MeetingTray::Transcribing,
        ] {
            assert!(!enabled.contains(&meeting_id(disabled)));
        }
        // Restart to update is dimmed under its own id while a meeting
        // records or stops; Download update stays enabled.
        for held in [MeetingTray::Recording, MeetingTray::Stopping] {
            let held_view = TrayView {
                meetings: Some(held),
                ..view.clone()
            };
            assert!(
                matches!(
                    held_view.menu().into_iter().find(|item| matches!(
                        item,
                        rune::TrayItem::Action { id, .. } if id == "update-held"
                    )),
                    Some(rune::TrayItem::Action { enabled: false, .. })
                ),
                "{held:?} holds Restart to update"
            );
        }
        assert_eq!(TrayCommand::from_id("update-held"), None);
        let download_in_meeting = TrayView {
            meetings: Some(MeetingTray::Recording),
            ..TrayView::new(true, None, &DesktopUpdateStatus::Available, Vec::new())
        };
        assert!(matches!(
            download_in_meeting.menu().into_iter().find(|item| matches!(
                item,
                rune::TrayItem::Action { id, .. } if id == "update"
            )),
            Some(rune::TrayItem::Action { enabled: true, .. })
        ));
        let stopped = TrayView::new(
            false,
            Some(VoiceState::Dictating),
            &DesktopUpdateStatus::Current,
            Vec::new(),
        );
        assert_eq!(stopped.tooltip(), "Hex Dictation");
    }
}
