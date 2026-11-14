//! The area each Labs feature belongs to: Settings › Labs lists them under
//! one collapsible header per group.

use super::Feature;

/// One area of Settings › Labs. Every feature picks one (`Feature::group`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Group {
    Dictation,
    /// Paste, formatting, and spoken punctuation.
    Output,
    History,
    Meetings,
    Annotate,
    Shortcuts,
    Models,
}

impl Group {
    /// In the order Labs lists them.
    pub const ALL: [Self; 7] = [
        Self::Dictation,
        Self::Output,
        Self::History,
        Self::Meetings,
        Self::Annotate,
        Self::Shortcuts,
        Self::Models,
    ];

    /// Its stable id: `hex://labs?category=<id>` and `labs.groups` in
    /// settings.json.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Output => "output",
            Self::History => "history",
            Self::Meetings => "meetings",
            Self::Annotate => "annotate",
            Self::Shortcuts => "shortcuts",
            Self::Models => "models",
        }
    }

    /// Its English header in Settings › Labs.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Dictation => "Dictation",
            Self::Output => "Output",
            Self::History => "History",
            Self::Meetings => "Meetings",
            Self::Annotate => "Annotate",
            Self::Shortcuts => "Shortcuts",
            Self::Models => "Models",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.id() == id)
    }
}

impl Feature {
    /// The group Labs lists it under. Every feature picks one.
    pub const fn group(self) -> Group {
        match self {
            Self::LiveText
            | Self::FailureNotices
            | Self::LiveTyping
            | Self::NotchIndicator
            | Self::VocabularyBiasing
            | Self::ScreenVocabulary
            | Self::PauseBrowser
            | Self::KeepMicrophoneOpen
            | Self::SkipPauses
            | Self::VoiceFilter
            | Self::HideMenuBarIcon
            | Self::QuitClosesWindow
            | Self::SettingsSearch
            | Self::Appearance
            | Self::ReleaseNotesLink
            | Self::SpeakersOnly
            | Self::Playground => Group::Dictation,
            Self::SmartSpacing
            | Self::SpokenFormatting
            | Self::SpokenSymbols
            | Self::FillerWords
            | Self::CleanUpWithAppleIntelligence
            | Self::KeepOnClipboard
            | Self::CopyOnly
            | Self::ReturnToApp
            | Self::GnomeClipboardRestore
            | Self::TypedText
            | Self::TerminalPaste
            | Self::OutputFixes
            | Self::OpenCodeInWsl
            | Self::ApiProcessing
            | Self::ApiTimestamps
            | Self::ApiBrowserAccess
            | Self::PrintStart
            | Self::ShellCompletions
            | Self::YoloMode
            | Self::CursorContext
            | Self::VoicePrompt => Group::Output,
            Self::KeepRecordings
            | Self::HistoryStats
            | Self::HistoryMultiSelect
            | Self::RecentDictations => Group::History,
            Self::Meetings
            | Self::LiveChapters
            | Self::SubtitleExport
            | Self::StopWhenCallEnds
            | Self::MovableMeetingCapsules
            | Self::MeetingNotesVocabulary
            | Self::MeetingFinishedEvent
            | Self::FollowLiveTranscript
            | Self::MoreCallApps
            | Self::GnomeMeetingCapsules
            | Self::MeetingImport
            | Self::MeetingDescriptiveFilenames
            | Self::MeetingNotesFile
            | Self::ExpireMeetingAudio => Group::Meetings,
            Self::Screenshots
            | Self::AnnotateReplay
            | Self::GnomeAnnotate
            | Self::CopyCaptureZip => Group::Annotate,
            Self::MouseShortcuts
            | Self::SecondDictationShortcut
            | Self::MoreDictationShortcuts
            | Self::TerminalPasteShortcut
            | Self::SidedChords
            | Self::TapOrHold
            | Self::DoubleTapModifiers
            | Self::StandaloneSpecialKeys
            | Self::SectionKeyAlone
            | Self::ShareShortcut
            | Self::CapsLockDictation
            | Self::PauseMenu => Group::Shortcuts,
            Self::KeyboardModelSwitching
            | Self::AppModelSwitching
            | Self::UnloadIdle
            | Self::PickerSortFilter
            | Self::BenchmarkDetails
            | Self::PickerActiveFirst
            | Self::ModelInPlace
            | Self::KQuantModels
            | Self::PickerDownloaded
            | Self::ParakeetUltra => Group::Models,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_has_a_feature_and_round_trips_its_id() {
        for group in Group::ALL {
            assert_eq!(Group::from_id(group.id()), Some(group));
            assert!(
                Feature::ALL.iter().any(|feature| feature.group() == group),
                "{} holds no feature",
                group.id()
            );
        }
    }
}
