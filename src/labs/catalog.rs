//! Each Labs feature's catalog entry: who it is for, the OSes that have it,
//! the day it joined Labs, and the names Settings, `hex settings`, and
//! `hex labs` take. Split from [`super::Feature`] to keep the registry under
//! the source cap.

use super::{Audience, Feature, Os};
use crate::app_settings::AppSettings;

impl Feature {
    /// Who the feature is for. Every feature picks one.
    pub const fn audience(self) -> Audience {
        match self {
            // Developer tools: the story browser, and the virtual microphone
            // (a stable build keeps only its Uninstall while the driver is
            // installed).
            Self::VoiceFilter | Self::Playground => Audience::Dev,
            Self::Meetings
            | Self::LiveText
            | Self::FailureNotices
            | Self::LiveTyping
            | Self::LiveChapters
            | Self::VocabularyBiasing
            | Self::ScreenVocabulary
            | Self::TerminalPaste
            | Self::KeyboardModelSwitching
            | Self::AppModelSwitching
            | Self::MouseShortcuts
            | Self::PauseBrowser
            | Self::ShareShortcut
            | Self::UnloadIdle
            | Self::KeepMicrophoneOpen
            | Self::KeepOnClipboard
            | Self::CopyOnly
            | Self::ReturnToApp
            | Self::SpokenFormatting
            | Self::FillerWords
            | Self::CleanUpWithAppleIntelligence
            | Self::SubtitleExport
            | Self::SecondDictationShortcut
            | Self::TerminalPasteShortcut
            | Self::SmartSpacing
            | Self::SpokenSymbols
            | Self::PickerSortFilter
            | Self::SidedChords
            | Self::TapOrHold
            | Self::DoubleTapModifiers
            | Self::BenchmarkDetails
            | Self::ParakeetUltra
            | Self::StandaloneSpecialKeys
            | Self::HistoryStats
            | Self::HistoryMultiSelect
            | Self::RecentDictations
            | Self::SkipPauses
            | Self::StopWhenCallEnds
            | Self::OutputFixes
            | Self::PickerActiveFirst
            | Self::ModelInPlace
            | Self::KQuantModels
            | Self::PickerDownloaded
            | Self::SectionKeyAlone
            | Self::KeepRecordings
            | Self::SettingsSearch
            | Self::Appearance
            | Self::ReleaseNotesLink
            | Self::HideMenuBarIcon
            | Self::MovableMeetingCapsules
            | Self::GnomeClipboardRestore
            | Self::TypedText
            | Self::MeetingNotesVocabulary
            | Self::MeetingFinishedEvent
            | Self::GnomeMeetingCapsules
            | Self::Screenshots
            | Self::FollowLiveTranscript
            | Self::MoreCallApps
            | Self::AnnotateReplay
            | Self::MeetingImport
            | Self::MeetingDescriptiveFilenames
            | Self::MeetingNotesFile
            | Self::ExpireMeetingAudio
            | Self::NotchIndicator
            | Self::QuitClosesWindow
            | Self::GnomeAnnotate
            | Self::CapsLockDictation
            | Self::OpenCodeInWsl
            | Self::ApiProcessing
            | Self::ApiTimestamps
            | Self::ApiBrowserAccess
            | Self::SpeakersOnly
            | Self::PauseMenu
            | Self::PrintStart
            | Self::VoicePrompt
            | Self::CopyCaptureZip
            | Self::ShellCompletions
            | Self::YoloMode
            | Self::CursorContext
            | Self::MoreDictationShortcuts => Audience::Everyone,
        }
    }

    /// The day it joined Labs, as `yyyymmdd`. Labs marks the newest as New.
    pub const fn added(self) -> u32 {
        match self {
            Self::Meetings
            | Self::VoiceFilter
            | Self::Playground
            | Self::LiveText
            | Self::LiveChapters
            | Self::VocabularyBiasing
            | Self::TerminalPaste
            | Self::KeyboardModelSwitching
            | Self::MouseShortcuts
            | Self::PauseBrowser
            | Self::ShareShortcut => 20261030,
            Self::UnloadIdle
            | Self::KeepOnClipboard
            | Self::CopyOnly
            | Self::SpokenFormatting
            | Self::FillerWords
            | Self::SubtitleExport
            | Self::SecondDictationShortcut
            | Self::SmartSpacing
            | Self::SpokenSymbols
            | Self::PickerSortFilter
            | Self::SidedChords
            | Self::TapOrHold
            | Self::BenchmarkDetails => 20261031,
            Self::ParakeetUltra
            | Self::StandaloneSpecialKeys
            | Self::HistoryStats
            | Self::RecentDictations
            | Self::SkipPauses => 20261101,
            Self::StopWhenCallEnds
            | Self::OutputFixes
            | Self::PickerActiveFirst
            | Self::ModelInPlace
            | Self::PickerDownloaded
            | Self::SectionKeyAlone
            | Self::KeepRecordings
            | Self::HideMenuBarIcon => 20261102,
            Self::MovableMeetingCapsules
            | Self::GnomeClipboardRestore
            | Self::MeetingNotesVocabulary
            | Self::NotchIndicator => 20261103,
            Self::MeetingFinishedEvent
            | Self::GnomeMeetingCapsules
            | Self::Screenshots
            | Self::FollowLiveTranscript
            | Self::AnnotateReplay => 20261104,
            Self::MoreCallApps | Self::SettingsSearch | Self::MeetingImport => 20261106,
            Self::LiveTyping
            | Self::MeetingDescriptiveFilenames
            | Self::QuitClosesWindow
            | Self::CapsLockDictation
            | Self::ExpireMeetingAudio
            | Self::FailureNotices
            | Self::TypedText => 20261107,
            Self::GnomeAnnotate
            | Self::TerminalPasteShortcut
            | Self::ReleaseNotesLink
            | Self::OpenCodeInWsl => 20261108,
            Self::ApiProcessing
            | Self::Appearance
            | Self::DoubleTapModifiers
            | Self::ReturnToApp
            | Self::SpeakersOnly
            | Self::MeetingNotesFile
            | Self::MoreDictationShortcuts => 20261109,
            Self::AppModelSwitching
            | Self::ApiTimestamps
            | Self::PauseMenu
            | Self::HistoryMultiSelect => 20261110,
            Self::KeepMicrophoneOpen | Self::PrintStart => 20261111,
            Self::ScreenVocabulary
            | Self::VoicePrompt
            | Self::CopyCaptureZip
            | Self::ApiBrowserAccess => 20261112,
            Self::KQuantModels => 20261113,
            Self::ShellCompletions
            | Self::YoloMode
            | Self::CursorContext
            | Self::CleanUpWithAppleIntelligence => 20261113,
        }
    }

    /// Why turning this feature off is refused now: Parakeet Ultra stays on
    /// while it is the dictation model, until a switch to another succeeds.
    pub fn off_refusal(self, settings: &AppSettings) -> Option<&'static str> {
        (self == Self::ParakeetUltra
            && settings.transcription.model
                == crate::transcription::models::TranscriptionModelId::ParakeetUltra)
            .then_some(
                "Parakeet Ultra is the dictation model; choose another model before turning it off",
            )
    }

    /// The feature `hex settings` key `key` switches (its toggle, or
    /// Unload when idle's minutes).
    pub fn for_settings_key(key: &str) -> Option<Self> {
        if key == "labs.unload_idle_minutes" {
            return Some(Self::UnloadIdle);
        }
        if key == "labs.keep_microphone_open_seconds" {
            return Some(Self::KeepMicrophoneOpen);
        }
        if key == "labs.yolo_mode_seconds" || key == "labs.yolo_mode_apps" {
            return Some(Self::YoloMode);
        }
        if key == "labs.voice_prompt_phrase" {
            return Some(Self::VoicePrompt);
        }
        if key == "labs.caps_lock_light" {
            return Some(Self::CapsLockDictation);
        }
        if key == "labs.appearance_theme" {
            return Some(Self::Appearance);
        }
        if key == "labs.api_browser_origins" {
            return Some(Self::ApiBrowserAccess);
        }
        if key == "capture.replay_hotkey" {
            return Some(Self::AnnotateReplay);
        }
        Self::ALL
            .into_iter()
            .find(|feature| feature.settings_key() == key)
    }

    /// Whether this OS has the feature at all, in any build: one of its
    /// [`Self::oses`], unless this session or build leaves it out.
    pub fn on_this_os(self) -> bool {
        match self {
            Self::KeyboardModelSwitching => crate::keyboard_input_sources::available(),
            Self::AnnotateReplay => {
                #[cfg(hex_capture)]
                {
                    crate::capture_host::replay_available()
                }
                #[cfg(not(hex_capture))]
                {
                    false
                }
            }
            _ => self.oses().contains(&Os::CURRENT),
        }
    }

    /// The OSes that have the feature, in the order Hex's website names
    /// them. Linux's keyboard-language switching needs an X11 session.
    pub const fn oses(self) -> &'static [Os] {
        const MACOS: &[Os] = &[Os::Macos];
        const WINDOWS: &[Os] = &[Os::Windows];
        const LINUX: &[Os] = &[Os::Linux];
        const MACOS_WINDOWS: &[Os] = &[Os::Macos, Os::Windows];
        const MACOS_LINUX: &[Os] = &[Os::Macos, Os::Linux];
        const WINDOWS_LINUX: &[Os] = &[Os::Windows, Os::Linux];
        const EVERY: &[Os] = &[Os::Macos, Os::Windows, Os::Linux];
        match self {
            Self::Meetings
            | Self::LiveChapters
            | Self::SubtitleExport
            | Self::StopWhenCallEnds
            | Self::MovableMeetingCapsules
            | Self::KQuantModels
            | Self::MeetingImport => MACOS,
            Self::VoiceFilter => MACOS_LINUX,
            Self::TerminalPaste | Self::ShareShortcut | Self::OpenCodeInWsl => WINDOWS,
            Self::SectionKeyAlone => MACOS_LINUX,
            Self::GnomeClipboardRestore
            | Self::TypedText
            | Self::GnomeMeetingCapsules
            | Self::GnomeAnnotate
            | Self::LiveTyping => LINUX,
            Self::StandaloneSpecialKeys | Self::SkipPauses | Self::TerminalPasteShortcut => {
                WINDOWS_LINUX
            }
            Self::MouseShortcuts
            | Self::PauseBrowser
            | Self::SmartSpacing
            | Self::SidedChords
            | Self::RecentDictations
            | Self::HideMenuBarIcon
            | Self::QuitClosesWindow
            | Self::NotchIndicator
            | Self::Screenshots
            | Self::CapsLockDictation
            | Self::DoubleTapModifiers
            | Self::PauseMenu
            | Self::PrintStart
            | Self::ScreenVocabulary
            | Self::CursorContext
            | Self::MoreDictationShortcuts => MACOS,
            Self::CleanUpWithAppleIntelligence => MACOS,
            Self::CopyOnly => MACOS_WINDOWS,
            Self::ReturnToApp => MACOS,
            Self::TapOrHold => MACOS_LINUX,
            Self::AppModelSwitching => MACOS_WINDOWS,
            // Only builds with the capture build option have it.
            Self::AnnotateReplay => MACOS,
            Self::Playground
            | Self::KeyboardModelSwitching
            | Self::LiveText
            | Self::FailureNotices
            | Self::SecondDictationShortcut
            | Self::VocabularyBiasing
            | Self::UnloadIdle
            | Self::KeepMicrophoneOpen
            | Self::KeepOnClipboard
            | Self::SpokenFormatting
            | Self::SpokenSymbols
            | Self::PickerSortFilter
            | Self::BenchmarkDetails
            | Self::ParakeetUltra
            | Self::HistoryStats
            | Self::HistoryMultiSelect
            | Self::OutputFixes
            | Self::PickerActiveFirst
            | Self::ModelInPlace
            | Self::PickerDownloaded
            | Self::KeepRecordings
            | Self::MeetingNotesVocabulary
            | Self::MeetingFinishedEvent
            | Self::MeetingDescriptiveFilenames
            | Self::MeetingNotesFile
            | Self::FollowLiveTranscript
            | Self::MoreCallApps
            | Self::ExpireMeetingAudio
            | Self::SettingsSearch
            | Self::Appearance
            | Self::ReleaseNotesLink
            | Self::FillerWords
            | Self::ApiProcessing
            | Self::ApiTimestamps
            | Self::ApiBrowserAccess
            | Self::VoicePrompt
            | Self::CopyCaptureZip
            | Self::SpeakersOnly
            | Self::ShellCompletions
            | Self::YoloMode => EVERY,
        }
    }

    /// The name `hex labs on|off` takes.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Meetings => "meetings",
            Self::VoiceFilter => "voice-filter",
            Self::Playground => "playground",
            Self::LiveText => "live-text",
            Self::FailureNotices => "failure-notices",
            Self::LiveTyping => "live-typing",
            Self::LiveChapters => "live-chapters",
            Self::VocabularyBiasing => "vocabulary-biasing",
            Self::ScreenVocabulary => "screen-vocabulary",
            Self::TerminalPaste => "terminal-paste",
            Self::KeyboardModelSwitching => "keyboard-model-switching",
            Self::AppModelSwitching => "app-model-switching",
            Self::MouseShortcuts => "mouse-shortcuts",
            Self::PauseBrowser => "pause-browser",
            Self::ShareShortcut => "share-shortcut",
            Self::UnloadIdle => "unload-idle",
            Self::KeepMicrophoneOpen => "keep-microphone-open",
            Self::KeepOnClipboard => "keep-on-clipboard",
            Self::CopyOnly => "copy-only",
            Self::ReturnToApp => "return-to-app",
            Self::SpokenFormatting => "spoken-formatting",
            Self::FillerWords => "filler-words",
            Self::CleanUpWithAppleIntelligence => "clean-up-apple-intelligence",
            Self::SubtitleExport => "subtitle-export",
            Self::SecondDictationShortcut => "second-dictation-shortcut",
            Self::TerminalPasteShortcut => "terminal-paste-shortcut",
            Self::SmartSpacing => "smart-spacing",
            Self::SpokenSymbols => "spoken-symbols",
            Self::PickerSortFilter => "picker-sort-filter",
            Self::SidedChords => "sided-modifier-chords",
            Self::TapOrHold => "tap-or-hold",
            Self::DoubleTapModifiers => "double-tap-modifiers",
            Self::BenchmarkDetails => "benchmark-details",
            Self::ParakeetUltra => "parakeet-ultra",
            Self::StandaloneSpecialKeys => "special-keys",
            Self::HistoryStats => "history-stats",
            Self::HistoryMultiSelect => "history-multi-select",
            Self::RecentDictations => "recent-dictations",
            Self::SkipPauses => "skip-pauses",
            Self::StopWhenCallEnds => "stop-when-call-ends",
            Self::OutputFixes => "output-fixes",
            Self::PickerActiveFirst => "picker-active-first",
            Self::ModelInPlace => "model-in-place",
            Self::KQuantModels => "k-quant-models",
            Self::PickerDownloaded => "picker-downloaded",
            Self::SectionKeyAlone => "section-key",
            Self::KeepRecordings => "keep-recordings",
            Self::SettingsSearch => "settings-search",
            Self::Appearance => "appearance",
            Self::ReleaseNotesLink => "release-notes-link",
            Self::HideMenuBarIcon => "hide-menu-bar-icon",
            Self::MovableMeetingCapsules => "movable-meeting-capsules",
            Self::GnomeClipboardRestore => "gnome-clipboard-restore",
            Self::TypedText => "typed-text",
            Self::MeetingNotesVocabulary => "meeting-notes-vocabulary",
            Self::NotchIndicator => "notch-indicator",
            Self::MeetingFinishedEvent => "meeting-finished-event",
            Self::GnomeMeetingCapsules => "gnome-meeting-capsules",
            Self::GnomeAnnotate => "gnome-annotate",
            Self::Screenshots => "screenshots",
            Self::FollowLiveTranscript => "follow-live-transcript",
            Self::MoreCallApps => "more-call-apps",
            Self::AnnotateReplay => "annotate-replay",
            Self::MeetingImport => "meeting-import",
            Self::MeetingDescriptiveFilenames => "meeting-descriptive-filenames",
            Self::MeetingNotesFile => "meeting-notes-file",
            Self::QuitClosesWindow => "quit-closes-window",
            Self::ExpireMeetingAudio => "expire-meeting-audio",
            Self::CapsLockDictation => "caps-lock-dictation",
            Self::OpenCodeInWsl => "opencode-in-wsl",
            Self::ApiProcessing => "api-processing",
            Self::ApiTimestamps => "api-timestamps",
            Self::ApiBrowserAccess => "api-browser-access",
            Self::SpeakersOnly => "speakers-only",
            Self::PauseMenu => "pause-menu",
            Self::PrintStart => "print-start",
            Self::VoicePrompt => "voice-prompt",
            Self::CopyCaptureZip => "copy-capture-zip",
            Self::ShellCompletions => "shell-completions",
            Self::YoloMode => "yolo-mode",
            Self::CursorContext => "cursor-context",
            Self::MoreDictationShortcuts => "more-dictation-shortcuts",
        }
    }

    /// Its `hex settings` key.
    pub const fn settings_key(self) -> &'static str {
        match self {
            Self::Meetings => "meetings.enabled",
            Self::VoiceFilter => "experimental.hex_microphone",
            Self::Playground => "labs.playground",
            Self::LiveText => "labs.live_text",
            Self::FailureNotices => "labs.failure_notices",
            Self::LiveTyping => "labs.live_typing",
            Self::LiveChapters => "labs.live_chapters",
            Self::VocabularyBiasing => "labs.vocabulary_biasing",
            Self::ScreenVocabulary => "labs.screen_vocabulary",
            Self::TerminalPaste => "paste_with_shift",
            Self::KeyboardModelSwitching => "keyboard_model_switching.enabled",
            Self::AppModelSwitching => "labs.app_model_switching",
            Self::MouseShortcuts => "labs.mouse_shortcuts",
            Self::PauseBrowser => "pause_browser_media",
            Self::ShareShortcut => "share_dictation_shortcut",
            Self::UnloadIdle => "labs.unload_idle",
            Self::KeepMicrophoneOpen => "labs.keep_microphone_open",
            Self::KeepOnClipboard => "labs.keep_on_clipboard",
            Self::CopyOnly => "labs.copy_only",
            Self::ReturnToApp => "labs.return_to_app",
            Self::SpokenFormatting => "labs.spoken_formatting",
            Self::FillerWords => "labs.filler_words",
            Self::CleanUpWithAppleIntelligence => "labs.clean_up_with_apple_intelligence",
            Self::SubtitleExport => "labs.subtitle_export",
            Self::SecondDictationShortcut => "labs.second_dictation_shortcut",
            Self::TerminalPasteShortcut => "labs.terminal_paste_shortcut",
            Self::SmartSpacing => "labs.smart_spacing",
            Self::SpokenSymbols => "labs.spoken_symbols",
            Self::PickerSortFilter => "labs.picker_sort_filter",
            Self::SidedChords => "labs.sided_modifier_chords",
            Self::TapOrHold => "labs.tap_or_hold",
            Self::DoubleTapModifiers => "labs.double_tap_modifiers",
            Self::BenchmarkDetails => "labs.benchmark_details",
            Self::ParakeetUltra => "labs.parakeet_ultra",
            Self::StandaloneSpecialKeys => "labs.standalone_special_keys",
            Self::HistoryStats => "labs.history_stats",
            Self::HistoryMultiSelect => "labs.history_multi_select",
            Self::RecentDictations => "labs.recent_dictations",
            Self::SkipPauses => "labs.skip_pauses",
            Self::StopWhenCallEnds => "labs.stop_when_call_ends",
            Self::OutputFixes => "labs.output_fixes",
            Self::PickerActiveFirst => "labs.picker_active_first",
            Self::ModelInPlace => "labs.model_in_place",
            Self::KQuantModels => "labs.k_quant_models",
            Self::PickerDownloaded => "labs.picker_downloaded",
            Self::SectionKeyAlone => "labs.section_key_alone",
            Self::KeepRecordings => "labs.keep_recordings",
            Self::SettingsSearch => "labs.settings_search",
            Self::Appearance => "labs.appearance",
            Self::ReleaseNotesLink => "labs.release_notes_link",
            Self::HideMenuBarIcon => "labs.hide_menu_bar_icon",
            Self::MovableMeetingCapsules => "labs.movable_meeting_capsules",
            Self::GnomeClipboardRestore => "labs.gnome_clipboard_restore",
            Self::TypedText => "labs.typed_text",
            Self::MeetingNotesVocabulary => "labs.meeting_notes_vocabulary",
            Self::NotchIndicator => "labs.notch_indicator",
            Self::MeetingFinishedEvent => "labs.meeting_finished_event",
            Self::GnomeMeetingCapsules => "labs.gnome_meeting_capsules",
            Self::GnomeAnnotate => "labs.gnome_annotate",
            Self::Screenshots => "labs.screenshots",
            Self::FollowLiveTranscript => "labs.follow_live_transcript",
            Self::MoreCallApps => "labs.more_call_apps",
            Self::AnnotateReplay => "labs.annotate_replay",
            Self::MeetingImport => "labs.meeting_import",
            Self::MeetingDescriptiveFilenames => "labs.meeting_descriptive_filenames",
            Self::MeetingNotesFile => "labs.meeting_notes_file",
            Self::QuitClosesWindow => "labs.quit_closes_window",
            Self::ExpireMeetingAudio => "labs.expire_meeting_audio",
            Self::CapsLockDictation => "labs.caps_lock_dictation",
            Self::OpenCodeInWsl => "labs.opencode_in_wsl",
            Self::ApiProcessing => "labs.api_processing",
            Self::ApiTimestamps => "labs.api_timestamps",
            Self::ApiBrowserAccess => "labs.api_browser_access",
            Self::SpeakersOnly => "labs.speakers_only",
            Self::PauseMenu => "labs.pause_menu",
            Self::PrintStart => "labs.print_start",
            Self::VoicePrompt => "labs.voice_prompt",
            Self::CopyCaptureZip => "labs.copy_capture_zip",
            Self::ShellCompletions => "labs.shell_completions",
            Self::YoloMode => "labs.yolo_mode",
            Self::CursorContext => "labs.cursor_context",
            Self::MoreDictationShortcuts => "labs.more_dictation_shortcuts",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|feature| feature.name() == name)
    }
}
