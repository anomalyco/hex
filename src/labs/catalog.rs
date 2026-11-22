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
            Self::VoiceFilter | Self::Playground | Self::PasteLastPicker => Audience::Dev,
            Self::Meetings
            | Self::LiveText
            | Self::FasterLiveText
            | Self::FailureNotices
            | Self::LiveTyping
            | Self::LiveChapters
            | Self::ScreenVocabulary
            | Self::LearnFromCorrections
            | Self::TerminalPaste
            | Self::KeyboardModelSwitching
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
            | Self::NeutralVoice
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
            | Self::RecentDictations
            | Self::SkipPauses
            | Self::OutputFixes
            | Self::ModelInPlace
            | Self::KQuantModels
            | Self::PickerDownloaded
            | Self::SectionKeyAlone
            | Self::MicrophoneKeyAlone
            | Self::KeepRecordings
            | Self::Appearance
            | Self::HideMenuBarIcon
            | Self::GnomeClipboardRestore
            | Self::TypedText
            | Self::GnomeMeetingCapsules
            | Self::AnnotatePointerScreen
            | Self::Screenshots
            | Self::AnnotateReplay
            | Self::MeetingImport
            | Self::NotchIndicator
            | Self::GnomeAnnotate
            | Self::CapsLockDictation
            | Self::OpenCodeInWsl
            | Self::ApiProcessing
            | Self::ApiTimestamps
            | Self::SpeakersOnly
            | Self::PauseMenu
            | Self::CalendarMenu
            | Self::CalendarSelectAll
            | Self::VoicePrompt
            | Self::CopyCaptureZip
            | Self::DropTranscribe
            | Self::YoloMode
            | Self::CursorContext
            | Self::MoreDictationShortcuts
            | Self::CopyDictationAudio
            | Self::StopPhrase
            | Self::HoldThroughKeys
            | Self::ModesEditor
            | Self::VocabularyFiles
            | Self::AnnotateAgentTools
            | Self::AnnotateAgentInstall
            | Self::Signature
            | Self::DictationStyle
            | Self::HudHeight
            | Self::VoiceIsolation
            | Self::DictateWhileAnnotating => Audience::Everyone,
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
            Self::OutputFixes
            | Self::ModelInPlace
            | Self::PickerDownloaded
            | Self::SectionKeyAlone
            | Self::KeepRecordings
            | Self::HideMenuBarIcon => 20261102,
            Self::GnomeClipboardRestore | Self::NotchIndicator => 20261103,
            Self::GnomeMeetingCapsules | Self::Screenshots | Self::AnnotateReplay => 20261104,
            Self::MeetingImport => 20261106,
            Self::LiveTyping | Self::CapsLockDictation | Self::FailureNotices | Self::TypedText => {
                20261107
            }
            Self::GnomeAnnotate | Self::TerminalPasteShortcut | Self::OpenCodeInWsl => 20261108,
            Self::ApiProcessing
            | Self::Appearance
            | Self::DoubleTapModifiers
            | Self::ReturnToApp
            | Self::SpeakersOnly
            | Self::MoreDictationShortcuts => 20261109,
            Self::ApiTimestamps | Self::PauseMenu => 20261110,
            Self::KeepMicrophoneOpen => 20261111,
            Self::ScreenVocabulary | Self::VoicePrompt | Self::CopyCaptureZip => 20261112,
            Self::KQuantModels | Self::DropTranscribe => 20261113,
            Self::YoloMode | Self::CursorContext | Self::CleanUpWithAppleIntelligence => 20261113,
            Self::CalendarMenu => 20261114,
            Self::CopyDictationAudio | Self::MicrophoneKeyAlone => 20261115,
            Self::StopPhrase | Self::HoldThroughKeys => 20261116,
            Self::ModesEditor => 20261118,
            Self::CalendarSelectAll => 20261118,
            Self::PasteLastPicker => 20261119,
            Self::NeutralVoice
            | Self::AnnotatePointerScreen
            | Self::FasterLiveText
            | Self::VocabularyFiles => 20261120,
            Self::LearnFromCorrections
            | Self::AnnotateAgentTools
            | Self::AnnotateAgentInstall
            | Self::Signature => 20261121,
            Self::DictationStyle => 20261122,
            Self::HudHeight => 20261122,
            Self::VoiceIsolation => 20261122,
            Self::DictateWhileAnnotating => 20261122,
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
        if key == "labs.yolo_mode_apps" {
            return Some(Self::YoloMode);
        }
        if key == "labs.voice_prompt_phrase" {
            return Some(Self::VoicePrompt);
        }
        if key == "labs.stop_phrase_text" {
            return Some(Self::StopPhrase);
        }
        if key == "labs.signature_text" {
            return Some(Self::Signature);
        }
        if key == "labs.dictation_style_choice" {
            return Some(Self::DictationStyle);
        }
        if key == "labs.hud_bottom_gap" {
            return Some(Self::HudHeight);
        }
        if key == "labs.caps_lock_light" {
            return Some(Self::CapsLockDictation);
        }
        if key == "labs.appearance_theme" {
            return Some(Self::Appearance);
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
            Self::Meetings | Self::LiveChapters | Self::KQuantModels | Self::MeetingImport => MACOS,
            Self::VoiceFilter => MACOS_LINUX,
            Self::TerminalPaste | Self::ShareShortcut | Self::OpenCodeInWsl => WINDOWS,
            Self::SectionKeyAlone => MACOS_LINUX,
            Self::MicrophoneKeyAlone => MACOS,
            Self::GnomeClipboardRestore
            | Self::TypedText
            | Self::GnomeMeetingCapsules
            | Self::GnomeAnnotate
            | Self::AnnotatePointerScreen
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
            | Self::NotchIndicator
            | Self::HudHeight
            | Self::Screenshots
            | Self::CapsLockDictation
            | Self::DoubleTapModifiers
            | Self::PauseMenu
            | Self::ScreenVocabulary
            | Self::LearnFromCorrections
            | Self::CursorContext
            | Self::MoreDictationShortcuts
            | Self::CalendarMenu
            | Self::CalendarSelectAll
            | Self::PasteLastPicker
            | Self::VoiceIsolation => MACOS,
            Self::CleanUpWithAppleIntelligence => MACOS,
            Self::NeutralVoice => MACOS,
            Self::CopyOnly => MACOS_WINDOWS,
            Self::HoldThroughKeys => MACOS_WINDOWS,
            Self::ReturnToApp => MACOS,
            Self::TapOrHold => MACOS_LINUX,
            // Only builds with the capture build option have it.
            Self::AnnotateReplay => MACOS,
            Self::Playground
            | Self::KeyboardModelSwitching
            | Self::LiveText
            | Self::FasterLiveText
            | Self::FailureNotices
            | Self::SecondDictationShortcut
            | Self::UnloadIdle
            | Self::KeepMicrophoneOpen
            | Self::KeepOnClipboard
            | Self::SpokenFormatting
            | Self::SpokenSymbols
            | Self::PickerSortFilter
            | Self::BenchmarkDetails
            | Self::ParakeetUltra
            | Self::HistoryStats
            | Self::OutputFixes
            | Self::ModelInPlace
            | Self::PickerDownloaded
            | Self::KeepRecordings
            | Self::Appearance
            | Self::FillerWords
            | Self::ApiProcessing
            | Self::ApiTimestamps
            | Self::VoicePrompt
            | Self::CopyCaptureZip
            | Self::DropTranscribe
            | Self::SpeakersOnly
            | Self::YoloMode
            | Self::CopyDictationAudio
            | Self::StopPhrase
            | Self::ModesEditor
            | Self::VocabularyFiles
            | Self::AnnotateAgentTools
            | Self::AnnotateAgentInstall
            | Self::Signature
            | Self::DictationStyle
            | Self::DictateWhileAnnotating => EVERY,
        }
    }

    /// The name `hex labs on|off` takes.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Meetings => "meetings",
            Self::VoiceFilter => "voice-filter",
            Self::Playground => "playground",
            Self::LiveText => "live-text",
            Self::FasterLiveText => "faster-live-text",
            Self::FailureNotices => "failure-notices",
            Self::LiveTyping => "live-typing",
            Self::LiveChapters => "live-chapters",
            Self::ScreenVocabulary => "screen-vocabulary",
            Self::LearnFromCorrections => "learn-from-corrections",
            Self::TerminalPaste => "terminal-paste",
            Self::KeyboardModelSwitching => "keyboard-model-switching",
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
            Self::NeutralVoice => "neutral-voice",
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
            Self::RecentDictations => "recent-dictations",
            Self::SkipPauses => "skip-pauses",
            Self::OutputFixes => "output-fixes",
            Self::ModelInPlace => "model-in-place",
            Self::KQuantModels => "k-quant-models",
            Self::PickerDownloaded => "picker-downloaded",
            Self::SectionKeyAlone => "section-key",
            Self::MicrophoneKeyAlone => "microphone-key",
            Self::KeepRecordings => "keep-recordings",
            Self::Appearance => "appearance",
            Self::HideMenuBarIcon => "hide-menu-bar-icon",
            Self::GnomeClipboardRestore => "gnome-clipboard-restore",
            Self::TypedText => "typed-text",
            Self::NotchIndicator => "notch-indicator",
            Self::GnomeMeetingCapsules => "gnome-meeting-capsules",
            Self::GnomeAnnotate => "gnome-annotate",
            Self::AnnotatePointerScreen => "annotate-pointer-screen",
            Self::Screenshots => "screenshots",
            Self::AnnotateReplay => "annotate-replay",
            Self::MeetingImport => "meeting-import",
            Self::CapsLockDictation => "caps-lock-dictation",
            Self::OpenCodeInWsl => "opencode-in-wsl",
            Self::ApiProcessing => "api-processing",
            Self::ApiTimestamps => "api-timestamps",
            Self::SpeakersOnly => "speakers-only",
            Self::PauseMenu => "pause-menu",
            Self::VoicePrompt => "voice-prompt",
            Self::CopyCaptureZip => "copy-capture-zip",
            Self::DropTranscribe => "drop-transcribe",
            Self::YoloMode => "yolo-mode",
            Self::CursorContext => "cursor-context",
            Self::CalendarMenu => "calendar-menu",
            Self::CalendarSelectAll => "calendar-select-all",
            Self::MoreDictationShortcuts => "more-dictation-shortcuts",
            Self::CopyDictationAudio => "copy-dictation-audio",
            Self::StopPhrase => "stop-phrase",
            Self::HoldThroughKeys => "hold-through-keys",
            Self::ModesEditor => "modes-editor",
            Self::PasteLastPicker => "paste-last-picker",
            Self::VocabularyFiles => "vocabulary-files",
            Self::AnnotateAgentTools => "annotate-agent-tools",
            Self::AnnotateAgentInstall => "annotate-agent-install",
            Self::Signature => "signature",
            Self::DictationStyle => "dictation-style",
            Self::HudHeight => "hud-height",
            Self::VoiceIsolation => "voice-isolation",
            Self::DictateWhileAnnotating => "dictate-while-annotating",
        }
    }

    /// Its `hex settings` key.
    pub const fn settings_key(self) -> &'static str {
        match self {
            Self::Meetings => "meetings.enabled",
            Self::VoiceFilter => "experimental.hex_microphone",
            Self::Playground => "labs.playground",
            Self::LiveText => "labs.live_text",
            Self::FasterLiveText => "labs.faster_live_text",
            Self::FailureNotices => "labs.failure_notices",
            Self::LiveTyping => "labs.live_typing",
            Self::LiveChapters => "labs.live_chapters",
            Self::ScreenVocabulary => "labs.screen_vocabulary",
            Self::LearnFromCorrections => "labs.learn_from_corrections",
            Self::TerminalPaste => "paste_with_shift",
            Self::KeyboardModelSwitching => "keyboard_model_switching.enabled",
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
            Self::NeutralVoice => "labs.neutral_voice",
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
            Self::RecentDictations => "labs.recent_dictations",
            Self::SkipPauses => "labs.skip_pauses",
            Self::OutputFixes => "labs.output_fixes",
            Self::ModelInPlace => "labs.model_in_place",
            Self::KQuantModels => "labs.k_quant_models",
            Self::PickerDownloaded => "labs.picker_downloaded",
            Self::SectionKeyAlone => "labs.section_key_alone",
            Self::MicrophoneKeyAlone => "labs.microphone_key",
            Self::KeepRecordings => "labs.keep_recordings",
            Self::Appearance => "labs.appearance",
            Self::HideMenuBarIcon => "labs.hide_menu_bar_icon",
            Self::GnomeClipboardRestore => "labs.gnome_clipboard_restore",
            Self::TypedText => "labs.typed_text",
            Self::NotchIndicator => "labs.notch_indicator",
            Self::GnomeMeetingCapsules => "labs.gnome_meeting_capsules",
            Self::GnomeAnnotate => "labs.gnome_annotate",
            Self::AnnotatePointerScreen => "labs.annotate_pointer_screen",
            Self::Screenshots => "labs.screenshots",
            Self::AnnotateReplay => "labs.annotate_replay",
            Self::MeetingImport => "labs.meeting_import",
            Self::CapsLockDictation => "labs.caps_lock_dictation",
            Self::OpenCodeInWsl => "labs.opencode_in_wsl",
            Self::ApiProcessing => "labs.api_processing",
            Self::ApiTimestamps => "labs.api_timestamps",
            Self::SpeakersOnly => "labs.speakers_only",
            Self::PauseMenu => "labs.pause_menu",
            Self::VoicePrompt => "labs.voice_prompt",
            Self::CopyCaptureZip => "labs.copy_capture_zip",
            Self::DropTranscribe => "labs.drop_transcribe",
            Self::PasteLastPicker => "labs.paste_last_picker",
            Self::YoloMode => "labs.yolo_mode",
            Self::CursorContext => "labs.cursor_context",
            Self::CalendarMenu => "labs.calendar_menu",
            Self::CalendarSelectAll => "labs.calendar_bulk_select",
            Self::MoreDictationShortcuts => "labs.more_dictation_shortcuts",
            Self::CopyDictationAudio => "labs.copy_dictation_audio",
            Self::StopPhrase => "labs.stop_phrase",
            Self::HoldThroughKeys => "labs.hold_through_keys",
            Self::ModesEditor => "labs.modes_editor",
            Self::VocabularyFiles => "labs.vocabulary_files",
            Self::AnnotateAgentTools => "labs.annotate_agent_tools",
            Self::AnnotateAgentInstall => "labs.annotate_agent_install",
            Self::Signature => "labs.signature",
            Self::DictationStyle => "labs.dictation_style",
            Self::HudHeight => "labs.hud_height",
            Self::VoiceIsolation => "labs.voice_isolation",
            Self::DictateWhileAnnotating => "labs.dictate_while_annotating",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|feature| feature.name() == name)
    }
}
