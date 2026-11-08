//! Labs: features we're unsure about, especially ones people asked for,
//! each behind its own switch, off by default. There is no master switch:
//! Settings › Labs lists every feature as a card with a toggle, under its
//! group's collapsible header (`Feature::group`). A feature
//! with its own pane (Meetings, Voice Filter, Playground) joins the sidebar
//! as a normal item while on and is hidden while off; a behaviour-only
//! feature (Live text, Live chapters, Vocabulary biasing, keyboard-language
//! model switching, mouse shortcuts, pausing the browser, restoring Paste
//! last dictation's default shortcut, keeping the dictation on the
//! clipboard or only copying it (macOS), a second dictation shortcut
//! (macOS), Toggle's tap or hold (macOS, Linux), stopping an automatic meeting
//! recording when its call ends (macOS), and Terminal paste, shortcut sharing
//! on Windows, and a terminal-paste dictation shortcut on Windows and Linux)
//! is just its toggle.
//!
//! Each feature has an audience (`Feature::audience`): `Everyone` ships in
//! every build; `Dev` exists only in dev builds (`dev_build::is_dev_build`:
//! debug, local, test, and dev-channel builds). A stable build never lists,
//! accepts, or runs a Dev feature, and keeps its saved setting untouched.
//!
//! Gate a feature where its work starts (the worker, thread, or capture
//! that does it), not only where its UI shows.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::app_settings::{AppSettings, CapsLockLight, LabsSettings};

mod cli;
mod description;
mod group;

pub use self::cli::{LabsCommand, run, run_retired_experiments};
pub use self::group::Group;

/// Where people send feedback on Labs features.
pub const FEEDBACK_URL: &str = "https://github.com/anomalyco/hex/issues/new";

/// Who a Labs feature is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Audience {
    /// Ships in every build.
    Everyone,
    /// Only dev builds have it.
    Dev,
}

impl Audience {
    /// Whether a build of this kind has features for it.
    pub const fn reaches(self, dev_build: bool) -> bool {
        match self {
            Self::Everyone => true,
            Self::Dev => dev_build,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Everyone => "everyone",
            Self::Dev => "dev",
        }
    }
}

/// One Labs feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Feature {
    Meetings,
    VoiceFilter,
    Playground,
    LiveText,
    FailureNotices,
    LiveTyping,
    LiveChapters,
    VocabularyBiasing,
    TerminalPaste,
    KeyboardModelSwitching,
    MouseShortcuts,
    PauseBrowser,
    ShareShortcut,
    UnloadIdle,
    RestoreShortcutDefault,
    KeepOnClipboard,
    CopyOnly,
    SpokenFormatting,
    FillerWords,
    SubtitleExport,
    SecondDictationShortcut,
    TerminalPasteShortcut,
    SmartSpacing,
    SpokenSymbols,
    PickerSortFilter,
    SidedChords,
    TapOrHold,
    BenchmarkDetails,
    ParakeetUltra,
    StandaloneSpecialKeys,
    HistoryStats,
    RecentDictations,
    SkipPauses,
    StopWhenCallEnds,
    OutputFixes,
    PickerActiveFirst,
    ModelInPlace,
    PickerDownloaded,
    SectionKeyAlone,
    KeepRecordings,
    SettingsSearch,
    HideMenuBarIcon,
    MovableMeetingCapsules,
    GnomeClipboardRestore,
    TypedText,
    MeetingNotesVocabulary,
    NotchIndicator,
    MeetingFinishedEvent,
    GnomeMeetingCapsules,
    GnomeAnnotate,
    Screenshots,
    FollowLiveTranscript,
    MoreCallApps,
    AnnotateReplay,
    MeetingImport,
    MeetingDescriptiveFilenames,
    QuitClosesWindow,
    ExpireMeetingAudio,
    CapsLockDictation,
    PersistentInk,
}

impl Feature {
    pub const ALL: [Self; 60] = [
        Self::Meetings,
        Self::VoiceFilter,
        Self::Playground,
        Self::LiveText,
        Self::FailureNotices,
        Self::LiveTyping,
        Self::LiveChapters,
        Self::VocabularyBiasing,
        Self::TerminalPaste,
        Self::KeyboardModelSwitching,
        Self::MouseShortcuts,
        Self::PauseBrowser,
        Self::ShareShortcut,
        Self::UnloadIdle,
        Self::RestoreShortcutDefault,
        Self::KeepOnClipboard,
        Self::CopyOnly,
        Self::SpokenFormatting,
        Self::FillerWords,
        Self::SubtitleExport,
        Self::SecondDictationShortcut,
        Self::TerminalPasteShortcut,
        Self::SmartSpacing,
        Self::SpokenSymbols,
        Self::PickerSortFilter,
        Self::SidedChords,
        Self::TapOrHold,
        Self::BenchmarkDetails,
        Self::ParakeetUltra,
        Self::StandaloneSpecialKeys,
        Self::HistoryStats,
        Self::RecentDictations,
        Self::SkipPauses,
        Self::StopWhenCallEnds,
        Self::OutputFixes,
        Self::PickerActiveFirst,
        Self::ModelInPlace,
        Self::PickerDownloaded,
        Self::SectionKeyAlone,
        Self::KeepRecordings,
        Self::SettingsSearch,
        Self::HideMenuBarIcon,
        Self::MovableMeetingCapsules,
        Self::GnomeClipboardRestore,
        Self::TypedText,
        Self::MeetingNotesVocabulary,
        Self::NotchIndicator,
        Self::MeetingFinishedEvent,
        Self::GnomeMeetingCapsules,
        Self::GnomeAnnotate,
        Self::Screenshots,
        Self::FollowLiveTranscript,
        Self::MoreCallApps,
        Self::AnnotateReplay,
        Self::MeetingImport,
        Self::MeetingDescriptiveFilenames,
        Self::QuitClosesWindow,
        Self::ExpireMeetingAudio,
        Self::CapsLockDictation,
        Self::PersistentInk,
    ];

    /// The features this OS and build have.
    pub fn available() -> impl Iterator<Item = Self> {
        Self::available_where(Self::shipped)
    }

    /// The features on this OS that `reaches` admits.
    fn available_where(reaches: impl Fn(Self) -> bool) -> impl Iterator<Item = Self> {
        Self::ALL
            .into_iter()
            .filter(move |feature| feature.on_this_os() && reaches(*feature))
    }

    /// Whether this build has the feature: its audience reaches a dev or
    /// stable build.
    pub fn shipped(self) -> bool {
        self.audience().reaches(crate::dev_build::is_dev_build())
    }

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
            | Self::TerminalPaste
            | Self::KeyboardModelSwitching
            | Self::MouseShortcuts
            | Self::PauseBrowser
            | Self::ShareShortcut
            | Self::UnloadIdle
            | Self::RestoreShortcutDefault
            | Self::KeepOnClipboard
            | Self::CopyOnly
            | Self::SpokenFormatting
            | Self::FillerWords
            | Self::SubtitleExport
            | Self::SecondDictationShortcut
            | Self::TerminalPasteShortcut
            | Self::SmartSpacing
            | Self::SpokenSymbols
            | Self::PickerSortFilter
            | Self::SidedChords
            | Self::TapOrHold
            | Self::BenchmarkDetails
            | Self::ParakeetUltra
            | Self::StandaloneSpecialKeys
            | Self::HistoryStats
            | Self::RecentDictations
            | Self::SkipPauses
            | Self::StopWhenCallEnds
            | Self::OutputFixes
            | Self::PickerActiveFirst
            | Self::ModelInPlace
            | Self::PickerDownloaded
            | Self::SectionKeyAlone
            | Self::KeepRecordings
            | Self::SettingsSearch
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
            | Self::ExpireMeetingAudio
            | Self::NotchIndicator
            | Self::QuitClosesWindow
            | Self::GnomeAnnotate
            | Self::CapsLockDictation
            | Self::PersistentInk => Audience::Everyone,
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
            | Self::RestoreShortcutDefault
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
            Self::GnomeAnnotate | Self::TerminalPasteShortcut | Self::PersistentInk => 20261108,
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
        if key == "labs.caps_lock_light" {
            return Some(Self::CapsLockDictation);
        }
        if key == "capture.replay_hotkey" {
            return Some(Self::AnnotateReplay);
        }
        Self::ALL
            .into_iter()
            .find(|feature| feature.settings_key() == key)
    }

    /// Whether this OS has the feature at all, in any build.
    pub fn on_this_os(self) -> bool {
        match self {
            Self::Meetings
            | Self::LiveChapters
            | Self::SubtitleExport
            | Self::StopWhenCallEnds
            | Self::MovableMeetingCapsules
            | Self::MeetingImport => cfg!(target_os = "macos"),
            Self::VoiceFilter => cfg!(any(target_os = "macos", target_os = "linux")),
            Self::TerminalPaste | Self::ShareShortcut => cfg!(target_os = "windows"),
            Self::SectionKeyAlone
            | Self::GnomeClipboardRestore
            | Self::TypedText
            | Self::GnomeMeetingCapsules
            | Self::GnomeAnnotate
            | Self::LiveTyping => cfg!(target_os = "linux"),
            Self::StandaloneSpecialKeys | Self::SkipPauses | Self::TerminalPasteShortcut => {
                cfg!(any(target_os = "linux", target_os = "windows"))
            }
            Self::MouseShortcuts
            | Self::PauseBrowser
            | Self::KeepOnClipboard
            | Self::CopyOnly
            | Self::SecondDictationShortcut
            | Self::SmartSpacing
            | Self::SidedChords
            | Self::RecentDictations
            | Self::HideMenuBarIcon
            | Self::QuitClosesWindow
            | Self::NotchIndicator
            | Self::Screenshots
            | Self::CapsLockDictation => cfg!(target_os = "macos"),
            Self::TapOrHold => cfg!(any(target_os = "macos", target_os = "linux")),
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
            Self::PersistentInk => cfg!(hex_capture),
            Self::Playground
            | Self::LiveText
            | Self::FailureNotices
            | Self::VocabularyBiasing
            | Self::UnloadIdle
            | Self::RestoreShortcutDefault
            | Self::SpokenFormatting
            | Self::SpokenSymbols
            | Self::PickerSortFilter
            | Self::BenchmarkDetails
            | Self::ParakeetUltra
            | Self::HistoryStats
            | Self::OutputFixes
            | Self::PickerActiveFirst
            | Self::ModelInPlace
            | Self::PickerDownloaded
            | Self::KeepRecordings
            | Self::MeetingNotesVocabulary
            | Self::MeetingFinishedEvent
            | Self::MeetingDescriptiveFilenames
            | Self::FollowLiveTranscript
            | Self::MoreCallApps
            | Self::ExpireMeetingAudio
            | Self::SettingsSearch
            | Self::FillerWords => true,
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
            Self::TerminalPaste => "terminal-paste",
            Self::KeyboardModelSwitching => "keyboard-model-switching",
            Self::MouseShortcuts => "mouse-shortcuts",
            Self::PauseBrowser => "pause-browser",
            Self::ShareShortcut => "share-shortcut",
            Self::UnloadIdle => "unload-idle",
            Self::RestoreShortcutDefault => "restore-shortcut-default",
            Self::KeepOnClipboard => "keep-on-clipboard",
            Self::CopyOnly => "copy-only",
            Self::SpokenFormatting => "spoken-formatting",
            Self::FillerWords => "filler-words",
            Self::SubtitleExport => "subtitle-export",
            Self::SecondDictationShortcut => "second-dictation-shortcut",
            Self::TerminalPasteShortcut => "terminal-paste-shortcut",
            Self::SmartSpacing => "smart-spacing",
            Self::SpokenSymbols => "spoken-symbols",
            Self::PickerSortFilter => "picker-sort-filter",
            Self::SidedChords => "sided-modifier-chords",
            Self::TapOrHold => "tap-or-hold",
            Self::BenchmarkDetails => "benchmark-details",
            Self::ParakeetUltra => "parakeet-ultra",
            Self::StandaloneSpecialKeys => "special-keys",
            Self::HistoryStats => "history-stats",
            Self::RecentDictations => "recent-dictations",
            Self::SkipPauses => "skip-pauses",
            Self::StopWhenCallEnds => "stop-when-call-ends",
            Self::OutputFixes => "output-fixes",
            Self::PickerActiveFirst => "picker-active-first",
            Self::ModelInPlace => "model-in-place",
            Self::PickerDownloaded => "picker-downloaded",
            Self::SectionKeyAlone => "section-key",
            Self::KeepRecordings => "keep-recordings",
            Self::SettingsSearch => "settings-search",
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
            Self::QuitClosesWindow => "quit-closes-window",
            Self::ExpireMeetingAudio => "expire-meeting-audio",
            Self::CapsLockDictation => "caps-lock-dictation",
            Self::PersistentInk => "persistent-ink",
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
            Self::TerminalPaste => "paste_with_shift",
            Self::KeyboardModelSwitching => "keyboard_model_switching.enabled",
            Self::MouseShortcuts => "labs.mouse_shortcuts",
            Self::PauseBrowser => "pause_browser_media",
            Self::ShareShortcut => "share_dictation_shortcut",
            Self::UnloadIdle => "labs.unload_idle",
            Self::RestoreShortcutDefault => "labs.restore_shortcut_default",
            Self::KeepOnClipboard => "labs.keep_on_clipboard",
            Self::CopyOnly => "labs.copy_only",
            Self::SpokenFormatting => "labs.spoken_formatting",
            Self::FillerWords => "labs.filler_words",
            Self::SubtitleExport => "labs.subtitle_export",
            Self::SecondDictationShortcut => "labs.second_dictation_shortcut",
            Self::TerminalPasteShortcut => "labs.terminal_paste_shortcut",
            Self::SmartSpacing => "labs.smart_spacing",
            Self::SpokenSymbols => "labs.spoken_symbols",
            Self::PickerSortFilter => "labs.picker_sort_filter",
            Self::SidedChords => "labs.sided_modifier_chords",
            Self::TapOrHold => "labs.tap_or_hold",
            Self::BenchmarkDetails => "labs.benchmark_details",
            Self::ParakeetUltra => "labs.parakeet_ultra",
            Self::StandaloneSpecialKeys => "labs.standalone_special_keys",
            Self::HistoryStats => "labs.history_stats",
            Self::RecentDictations => "labs.recent_dictations",
            Self::SkipPauses => "labs.skip_pauses",
            Self::StopWhenCallEnds => "labs.stop_when_call_ends",
            Self::OutputFixes => "labs.output_fixes",
            Self::PickerActiveFirst => "labs.picker_active_first",
            Self::ModelInPlace => "labs.model_in_place",
            Self::PickerDownloaded => "labs.picker_downloaded",
            Self::SectionKeyAlone => "labs.section_key_alone",
            Self::KeepRecordings => "labs.keep_recordings",
            Self::SettingsSearch => "labs.settings_search",
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
            Self::QuitClosesWindow => "labs.quit_closes_window",
            Self::ExpireMeetingAudio => "labs.expire_meeting_audio",
            Self::CapsLockDictation => "labs.caps_lock_dictation",
            Self::PersistentInk => "labs.persistent_ink",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|feature| feature.name() == name)
    }

    /// Whether `settings` turn it on.
    pub fn on(self, settings: &AppSettings) -> bool {
        match self {
            Self::Meetings => settings.meetings.enabled,
            Self::VoiceFilter => settings.experimental.hex_microphone,
            Self::Playground => settings.labs.playground,
            Self::LiveText => settings.labs.live_text,
            Self::FailureNotices => settings.labs.failure_notices,
            Self::LiveTyping => settings.labs.live_typing,
            Self::LiveChapters => settings.labs.live_chapters,
            Self::VocabularyBiasing => settings.labs.vocabulary_biasing,
            Self::TerminalPaste => settings.paste_with_shift,
            Self::KeyboardModelSwitching => settings.keyboard_model_switching.enabled,
            Self::MouseShortcuts => settings.labs.mouse_shortcuts,
            Self::PauseBrowser => settings.pause_browser_media,
            Self::ShareShortcut => settings.share_dictation_shortcut,
            Self::UnloadIdle => settings.labs.unload_idle,
            Self::RestoreShortcutDefault => settings.labs.restore_shortcut_default,
            Self::KeepOnClipboard => settings.labs.keep_on_clipboard,
            Self::CopyOnly => settings.labs.copy_only,
            Self::SpokenFormatting => settings.labs.spoken_formatting,
            Self::FillerWords => settings.labs.filler_words,
            Self::SubtitleExport => settings.labs.subtitle_export,
            Self::SecondDictationShortcut => settings.labs.second_dictation_shortcut,
            Self::TerminalPasteShortcut => settings.labs.terminal_paste_shortcut,
            Self::SmartSpacing => settings.labs.smart_spacing,
            Self::SpokenSymbols => settings.labs.spoken_symbols,
            Self::PickerSortFilter => settings.labs.picker_sort_filter,
            Self::SidedChords => settings.labs.sided_modifier_chords,
            Self::TapOrHold => settings.labs.tap_or_hold,
            Self::BenchmarkDetails => settings.labs.benchmark_details,
            Self::ParakeetUltra => settings.labs.parakeet_ultra,
            Self::StandaloneSpecialKeys => settings.labs.standalone_special_keys,
            Self::HistoryStats => settings.labs.history_stats,
            Self::RecentDictations => settings.labs.recent_dictations,
            Self::SkipPauses => settings.labs.skip_pauses,
            Self::StopWhenCallEnds => settings.labs.stop_when_call_ends,
            Self::OutputFixes => settings.labs.output_fixes,
            Self::PickerActiveFirst => settings.labs.picker_active_first,
            Self::ModelInPlace => settings.labs.model_in_place,
            Self::PickerDownloaded => settings.labs.picker_downloaded,
            Self::SectionKeyAlone => settings.labs.section_key_alone,
            Self::KeepRecordings => settings.labs.keep_recordings,
            Self::SettingsSearch => settings.labs.settings_search,
            Self::HideMenuBarIcon => settings.labs.hide_menu_bar_icon,
            Self::MovableMeetingCapsules => settings.labs.movable_meeting_capsules,
            Self::GnomeClipboardRestore => settings.labs.gnome_clipboard_restore,
            Self::TypedText => settings.labs.typed_text,
            Self::MeetingNotesVocabulary => settings.labs.meeting_notes_vocabulary,
            Self::NotchIndicator => settings.labs.notch_indicator,
            Self::MeetingFinishedEvent => settings.labs.meeting_finished_event,
            Self::GnomeMeetingCapsules => settings.labs.gnome_meeting_capsules,
            Self::GnomeAnnotate => settings.labs.gnome_annotate,
            Self::Screenshots => settings.labs.screenshots,
            Self::FollowLiveTranscript => settings.labs.follow_live_transcript,
            Self::MoreCallApps => settings.labs.more_call_apps,
            Self::AnnotateReplay => settings.labs.annotate_replay,
            Self::MeetingImport => settings.labs.meeting_import,
            Self::MeetingDescriptiveFilenames => settings.labs.meeting_descriptive_filenames,
            Self::QuitClosesWindow => settings.labs.quit_closes_window,
            Self::ExpireMeetingAudio => settings.labs.expire_meeting_audio,
            Self::CapsLockDictation => settings.labs.caps_lock_dictation,
            Self::PersistentInk => settings.labs.persistent_ink,
        }
    }

    /// Every feature's switch in `settings`, for the Labs lock
    /// (`crate::policy`).
    pub fn switches(settings: &mut AppSettings) -> Vec<&mut bool> {
        let labs = &mut settings.labs;
        vec![
            &mut settings.meetings.enabled,
            &mut settings.experimental.hex_microphone,
            &mut settings.paste_with_shift,
            &mut settings.keyboard_model_switching.enabled,
            &mut settings.pause_browser_media,
            &mut settings.share_dictation_shortcut,
            &mut labs.playground,
            &mut labs.live_text,
            &mut labs.failure_notices,
            &mut labs.live_typing,
            &mut labs.live_chapters,
            &mut labs.vocabulary_biasing,
            &mut labs.mouse_shortcuts,
            &mut labs.unload_idle,
            &mut labs.restore_shortcut_default,
            &mut labs.keep_on_clipboard,
            &mut labs.copy_only,
            &mut labs.spoken_formatting,
            &mut labs.filler_words,
            &mut labs.subtitle_export,
            &mut labs.second_dictation_shortcut,
            &mut labs.terminal_paste_shortcut,
            &mut labs.smart_spacing,
            &mut labs.spoken_symbols,
            &mut labs.picker_sort_filter,
            &mut labs.sided_modifier_chords,
            &mut labs.tap_or_hold,
            &mut labs.benchmark_details,
            &mut labs.parakeet_ultra,
            &mut labs.standalone_special_keys,
            &mut labs.history_stats,
            &mut labs.recent_dictations,
            &mut labs.skip_pauses,
            &mut labs.stop_when_call_ends,
            &mut labs.output_fixes,
            &mut labs.picker_active_first,
            &mut labs.model_in_place,
            &mut labs.picker_downloaded,
            &mut labs.section_key_alone,
            &mut labs.keep_recordings,
            &mut labs.settings_search,
            &mut labs.hide_menu_bar_icon,
            &mut labs.movable_meeting_capsules,
            &mut labs.gnome_clipboard_restore,
            &mut labs.typed_text,
            &mut labs.meeting_notes_vocabulary,
            &mut labs.notch_indicator,
            &mut labs.meeting_finished_event,
            &mut labs.gnome_meeting_capsules,
            &mut labs.gnome_annotate,
            &mut labs.screenshots,
            &mut labs.follow_live_transcript,
            &mut labs.more_call_apps,
            &mut labs.annotate_replay,
            &mut labs.meeting_import,
            &mut labs.meeting_descriptive_filenames,
            &mut labs.quit_closes_window,
            &mut labs.expire_meeting_audio,
            &mut labs.caps_lock_dictation,
            &mut labs.persistent_ink,
        ]
    }

    /// The managed lock that holds this feature off, if any.
    pub fn managed(self) -> Option<crate::policy::Lock> {
        crate::policy::Policy::current().labs_lock(self)
    }

    pub fn set(self, settings: &mut AppSettings, on: bool) {
        match self {
            Self::Meetings => settings.meetings.enabled = on,
            Self::VoiceFilter => settings.experimental.hex_microphone = on,
            Self::Playground => settings.labs.playground = on,
            Self::LiveText => settings.labs.live_text = on,
            Self::FailureNotices => settings.labs.failure_notices = on,
            Self::LiveTyping => settings.labs.live_typing = on,
            Self::LiveChapters => settings.labs.live_chapters = on,
            Self::VocabularyBiasing => settings.labs.vocabulary_biasing = on,
            Self::TerminalPaste => settings.paste_with_shift = on,
            Self::KeyboardModelSwitching => settings.keyboard_model_switching.enabled = on,
            Self::MouseShortcuts => settings.labs.mouse_shortcuts = on,
            Self::PauseBrowser => settings.pause_browser_media = on,
            Self::ShareShortcut => settings.share_dictation_shortcut = on,
            Self::UnloadIdle => settings.labs.unload_idle = on,
            Self::RestoreShortcutDefault => settings.labs.restore_shortcut_default = on,
            Self::KeepOnClipboard => settings.labs.keep_on_clipboard = on,
            Self::CopyOnly => settings.labs.copy_only = on,
            Self::SpokenFormatting => settings.labs.spoken_formatting = on,
            Self::FillerWords => settings.labs.filler_words = on,
            Self::SubtitleExport => settings.labs.subtitle_export = on,
            Self::SecondDictationShortcut => settings.labs.second_dictation_shortcut = on,
            Self::TerminalPasteShortcut => settings.labs.terminal_paste_shortcut = on,
            Self::SmartSpacing => settings.labs.smart_spacing = on,
            Self::SpokenSymbols => settings.labs.spoken_symbols = on,
            Self::PickerSortFilter => settings.labs.picker_sort_filter = on,
            Self::SidedChords => settings.labs.sided_modifier_chords = on,
            Self::TapOrHold => settings.labs.tap_or_hold = on,
            Self::BenchmarkDetails => settings.labs.benchmark_details = on,
            Self::ParakeetUltra => settings.labs.parakeet_ultra = on,
            Self::HistoryStats => settings.labs.history_stats = on,
            Self::RecentDictations => settings.labs.recent_dictations = on,
            Self::SkipPauses => settings.labs.skip_pauses = on,
            Self::StopWhenCallEnds => settings.labs.stop_when_call_ends = on,
            Self::OutputFixes => settings.labs.output_fixes = on,
            Self::PickerActiveFirst => settings.labs.picker_active_first = on,
            Self::ModelInPlace => settings.labs.model_in_place = on,
            Self::PickerDownloaded => settings.labs.picker_downloaded = on,
            Self::KeepRecordings => settings.labs.keep_recordings = on,
            Self::SettingsSearch => settings.labs.settings_search = on,
            Self::HideMenuBarIcon => settings.labs.hide_menu_bar_icon = on,
            Self::MovableMeetingCapsules => settings.labs.movable_meeting_capsules = on,
            Self::GnomeClipboardRestore => settings.labs.gnome_clipboard_restore = on,
            Self::TypedText => settings.labs.typed_text = on,
            Self::MeetingNotesVocabulary => settings.labs.meeting_notes_vocabulary = on,
            Self::NotchIndicator => settings.labs.notch_indicator = on,
            Self::MeetingFinishedEvent => settings.labs.meeting_finished_event = on,
            Self::GnomeMeetingCapsules => settings.labs.gnome_meeting_capsules = on,
            Self::GnomeAnnotate => settings.labs.gnome_annotate = on,
            Self::Screenshots => settings.labs.screenshots = on,
            Self::FollowLiveTranscript => settings.labs.follow_live_transcript = on,
            Self::MoreCallApps => settings.labs.more_call_apps = on,
            Self::AnnotateReplay => settings.labs.annotate_replay = on,
            Self::MeetingImport => settings.labs.meeting_import = on,
            Self::MeetingDescriptiveFilenames => settings.labs.meeting_descriptive_filenames = on,
            Self::QuitClosesWindow => settings.labs.quit_closes_window = on,
            Self::ExpireMeetingAudio => settings.labs.expire_meeting_audio = on,
            Self::CapsLockDictation => settings.labs.caps_lock_dictation = on,
            Self::PersistentInk => settings.labs.persistent_ink = on,
            // Off, no shortcut keeps a key only the feature allows.
            Self::StandaloneSpecialKeys => {
                settings.labs.standalone_special_keys = on;
                settings.repair_special_keys();
            }
            Self::SectionKeyAlone => {
                settings.labs.section_key_alone = on;
                settings.repair_section_key();
            }
        }
    }
}

/// Set once this process knows its settings.
const KNOWN: u64 = 1;
const LIVE_TEXT: u64 = 1 << 1;
const LIVE_CHAPTERS: u64 = 1 << 2;
const PLAYGROUND: u64 = 1 << 3;
const VOCABULARY_BIASING: u64 = 1 << 4;
const MOUSE_SHORTCUTS: u64 = 1 << 5;
const UNLOAD_IDLE: u64 = 1 << 6;
const KEEP_ON_CLIPBOARD: u64 = 1 << 7;
const COPY_ONLY: u64 = 1 << 8;
const SPOKEN_FORMATTING: u64 = 1 << 9;
const FILLER_WORDS: u64 = 1 << 10;
const SUBTITLE_EXPORT: u64 = 1 << 11;
const SMART_SPACING: u64 = 1 << 12;
const SPOKEN_SYMBOLS: u64 = 1 << 13;
const SIDED_CHORDS: u64 = 1 << 14;
const TAP_OR_HOLD: u64 = 1 << 15;
const SKIP_PAUSES: u64 = 1 << 16;
const PARAKEET_ULTRA: u64 = 1 << 17;
const HISTORY_STATS: u64 = 1 << 18;
const RECENT_DICTATIONS: u64 = 1 << 19;
const STOP_WHEN_CALL_ENDS: u64 = 1 << 20;
const KEEP_RECORDINGS: u64 = 1 << 21;
const HIDE_MENU_BAR_ICON: u64 = 1 << 22;
const MOVABLE_MEETING_CAPSULES: u64 = 1 << 23;
// Reuses 1 << 24, Labs › Choose meeting summary model before it became
// Meetings › Summaries.
const MORE_CALL_APPS: u64 = 1 << 24;
const GNOME_CLIPBOARD_RESTORE: u64 = 1 << 25;
const MEETING_NOTES_VOCABULARY: u64 = 1 << 26;
const NOTCH_INDICATOR: u64 = 1 << 27;
const MEETING_FINISHED_EVENT: u64 = 1 << 28;
const GNOME_MEETING_CAPSULES: u64 = 1 << 29;
const SCREENSHOTS: u64 = 1 << 30;
const ANNOTATE_REPLAY: u64 = 1 << 31;
/// Unload-idle minutes (at most 1,440, 11 bits) sit above the switches.
const MINUTES_SHIFT: u64 = 32;
const MINUTES_BITS: u32 = 11;
const MINUTES_MASK: u64 = (1 << MINUTES_BITS) - 1;
/// Bits 1..31 are full, so this switch sits above the minutes.
const MEETING_IMPORT: u64 = 1 << (MINUTES_SHIFT + MINUTES_BITS as u64);
/// The next bit above Import audio's switch, clear of the minutes below it.
const QUIT_CLOSES_WINDOW: u64 = MEETING_IMPORT << 1;
/// The next bit above Quit closes Settings, also clear of the minutes below.
const EXPIRE_MEETING_AUDIO: u64 = QUIT_CLOSES_WINDOW << 1;
const LIVE_TYPING: u64 = EXPIRE_MEETING_AUDIO << 1;
/// The next bit above Live typing's switch.
const MEETING_DESCRIPTIVE_FILENAMES: u64 = LIVE_TYPING << 1;
/// The next bit above descriptive meeting file names.
const FAILURE_NOTICES: u64 = MEETING_DESCRIPTIVE_FILENAMES << 1;
/// The next bit above Failure notices' switch.
const CAPS_LOCK_DICTATION: u64 = FAILURE_NOTICES << 1;
/// The next bit above Caps Lock dictation's switch.
const GNOME_ANNOTATE: u64 = CAPS_LOCK_DICTATION << 1;
/// The next bit above Annotate on GNOME's switch.
const TYPED_TEXT: u64 = GNOME_ANNOTATE << 1;
/// The next bit above Typed text's switch: Caps Lock dictation's recording
/// light blinks slowly instead of staying solid.
const CAPS_LOCK_SLOW_BLINK: u64 = TYPED_TEXT << 1;
/// The next bit above the Caps Lock light: lifted Annotate ink stays on
/// screen (Labs › Persistent ink).
const PERSISTENT_INK: u64 = CAPS_LOCK_SLOW_BLINK << 1;

/// This process's `labs` switches: unknown until settings load or one is
/// first read.
static STATE: AtomicU64 = AtomicU64::new(0);

fn on(bit: u64) -> bool {
    state() & bit != 0
}

fn state() -> u64 {
    match STATE.load(Ordering::Acquire) {
        0 => {
            let saved = encode(&AppSettings::saved_labs());
            let _ = STATE.compare_exchange(0, saved, Ordering::AcqRel, Ordering::Acquire);
            STATE.load(Ordering::Acquire)
        }
        state => state,
    }
}

/// Live text: provisional words beside the HUD (read as each dictation starts).
pub fn live_text() -> bool {
    on(LIVE_TEXT)
}

/// A failed dictation's explanation beside the HUD (read as each failure
/// reaches the HUD).
pub fn failure_notices() -> bool {
    on(FAILURE_NOTICES) && Feature::FailureNotices.on_this_os()
}

/// Live typing: provisional words in the focused text field as an
/// input-method preedit on Wayland, and the final text typed there instead of
/// pasted (Linux; read as each hotkey capture starts).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn live_typing() -> bool {
    on(LIVE_TYPING) && Feature::LiveTyping.on_this_os()
}

/// Live chapters while a meeting records.
pub fn live_chapters() -> bool {
    on(LIVE_CHAPTERS)
}

/// The playground in the window.
pub fn playground() -> bool {
    on(PLAYGROUND)
}

/// Transcription biased toward the vocabulary (Parakeet).
pub fn vocabulary_biasing() -> bool {
    on(VOCABULARY_BIASING)
}

/// Mouse buttons may be recorded as shortcuts (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn mouse_shortcuts() -> bool {
    on(MOUSE_SHORTCUTS)
}

/// A pasted dictation stays on the clipboard rather than the previous
/// clipboard being restored (macOS; read as each paste commits).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn keep_on_clipboard() -> bool {
    on(KEEP_ON_CLIPBOARD)
}

/// A dictation goes to the clipboard and is never pasted (macOS; read as
/// each dictation's output commits).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn copy_only() -> bool {
    on(COPY_ONLY)
}

/// Spoken formatting phrases become line breaks and quotation marks (read
/// as each dictation's processing starts).
pub fn spoken_formatting() -> bool {
    on(SPOKEN_FORMATTING)
}

/// Spoken punctuation and symbol words become characters joined to their
/// neighbours (read as each dictation's processing starts).
pub fn spoken_symbols() -> bool {
    on(SPOKEN_SYMBOLS)
}

/// Filler words such as "um" and "uh" leave dictations (read as each
/// dictation's processing starts).
pub fn filler_words() -> bool {
    on(FILLER_WORDS)
}

/// `hex meetings show --srt|--vtt` may export subtitles (read when the
/// command starts).
pub fn subtitle_export() -> bool {
    on(SUBTITLE_EXPORT)
}

/// A pasted dictation gets one leading space when the character before the
/// cursor asks for one (macOS; read as each paste commits).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn smart_spacing() -> bool {
    on(SMART_SPACING)
}

/// The shortcut recorder keeps each modifier's side in a modifier-only
/// chord of two or more keys (macOS; read as each recording starts).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn sided_modifier_chords() -> bool {
    on(SIDED_CHORDS)
}

/// In Toggle mode, a press held past the hold threshold finishes its capture
/// when it lifts instead of locking it (macOS reads it at each release;
/// Linux's listener reads the saved switch with its shortcut options).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn tap_or_hold() -> bool {
    on(TAP_OR_HOLD)
}

/// Parakeet Ultra joins the catalog (read wherever a model is listed,
/// chosen, or run).
pub fn parakeet_ultra() -> bool {
    on(PARAKEET_ULTRA)
}

/// History's summary of retained dictations and `hex history stats` (read
/// when the History pane reloads and when the command starts).
pub fn history_stats() -> bool {
    on(HISTORY_STATS)
}

/// The menu bar's Copy Recent Dictation submenu (macOS; read each time the
/// menu opens).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn recent_dictations() -> bool {
    on(RECENT_DICTATIONS)
}

/// A whole-clip dictation transcription hears only the head and tail of
/// each long pause (Linux and Windows; read as each whole-clip run starts).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn skip_pauses() -> bool {
    on(SKIP_PAUSES) && Feature::SkipPauses.on_this_os()
}

/// A meeting recording Hex started on its own stops once its call app has
/// left the microphone and gone quiet for a grace period (macOS; read on
/// each pass of the meeting controller).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn stop_when_call_ends() -> bool {
    on(STOP_WHEN_CALL_ENDS) && Feature::StopWhenCallEnds.on_this_os()
}

/// Calls in more apps and on more websites are named and offered (read by
/// call detection and the window's Detected calls list).
pub fn more_call_apps() -> bool {
    on(MORE_CALL_APPS) && Feature::MoreCallApps.on_this_os()
}

/// A dictation saved to History keeps its 16 kHz audio, and History offers
/// Transcribe again for it (read as each capture is handed to the engine;
/// the History pane reads the saved setting).
pub fn keep_recordings() -> bool {
    on(KEEP_RECORDINGS)
}

/// The menu-bar icon is hidden while nothing records (macOS; read each
/// time the status item refreshes, about twice a second).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn hide_menu_bar_icon() -> bool {
    on(HIDE_MENU_BAR_ICON) && Feature::HideMenuBarIcon.on_this_os()
}

/// ⌘Q closes the Settings window instead of quitting Hex (macOS; read each
/// time the app menu's Quit Hex runs).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn quit_closes_window() -> bool {
    on(QUIT_CLOSES_WINDOW) && Feature::QuitClosesWindow.on_this_os()
}

/// The meeting panel capsules (Meeting transcript ready, Recording
/// automatically, offers, reminders) show in the saved capsule corner and
/// can be dragged to another (macOS; read each time one shows).
pub fn movable_meeting_capsules() -> bool {
    on(MOVABLE_MEETING_CAPSULES) && Feature::MovableMeetingCapsules.on_this_os()
}

/// Final meeting notes (title and summary) put the vocabulary ahead of the
/// transcript (every OS with meetings; read at each generation, so
/// Regenerate summary uses the current list).
pub fn meeting_notes_vocabulary() -> bool {
    on(MEETING_NOTES_VOCABULARY) && Feature::MeetingNotesVocabulary.on_this_os()
}

/// At either visible indicator position, the HUD grows out of the notch on
/// a screen that has one (macOS; read each time the HUD is placed).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn notch_indicator() -> bool {
    on(NOTCH_INDICATOR) && Feature::NotchIndicator.on_this_os()
}

/// A paste on a Wayland desktop without data-control (GNOME) snapshots and
/// restores the clipboard through an already running XWayland (Linux; read
/// as each paste starts).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gnome_clipboard_restore() -> bool {
    on(GNOME_CLIPBOARD_RESTORE) && Feature::GnomeClipboardRestore.on_this_os()
}

/// A shortcut runs the focused app's Output steps on the text already typed
/// there and replaces it (Linux; read as each shortcut press starts and by
/// the Output pane's row).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn typed_text() -> bool {
    on(TYPED_TEXT) && Feature::TypedText.on_this_os()
}

/// On a Wayland desktop without layer-shell (GNOME), the meeting capsules
/// float on an already running XWayland (Linux; read as each capsule shows).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gnome_meeting_capsules() -> bool {
    on(GNOME_MEETING_CAPSULES) && Feature::GnomeMeetingCapsules.on_this_os()
}

/// On a Wayland desktop without layer-shell (GNOME), Annotate's capsule and
/// ink float on an already running XWayland (Linux; read as each overlay
/// shows).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gnome_annotate() -> bool {
    on(GNOME_ANNOTATE) && Feature::GnomeAnnotate.on_this_os()
}

/// The local API's `GET /meetings/events` serves, and a meeting's final
/// publication sends, its id (every OS with meetings; read where the route
/// starts, on each pass of its stream, and at each publication).
pub fn meeting_finished_event() -> bool {
    on(MEETING_FINISHED_EVENT) && Feature::MeetingFinishedEvent.on_this_os()
}

/// The screenshot shortcut takes an area or a window (macOS; read by the
/// session tap on each key press, so nothing runs while it is off).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn screenshots() -> bool {
    on(SCREENSHOTS) && Feature::Screenshots.on_this_os()
}

/// Annotate's rolling 15 s display replay buffer (`SCClipBufferingOutput`,
/// macOS 27+; read where the replay worker starts and on each `hex capture
/// replay`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn annotate_replay() -> bool {
    on(ANNOTATE_REPLAY) && Feature::AnnotateReplay.on_this_os()
}

/// Lifted Annotate ink stays opaque until the recording ends or Escape
/// clears it (read by the ink overlay each frame and by Escape).
pub fn persistent_ink() -> bool {
    on(PERSISTENT_INK) && Feature::PersistentInk.on_this_os()
}

/// Audio files may be imported as meetings (macOS; read where the Meetings
/// pane offers Import audio and where an import starts).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_import() -> bool {
    on(MEETING_IMPORT) && Feature::MeetingImport.on_this_os()
}

/// Notes or a user rename rename an exported meeting's Markdown to the
/// meeting's title, keeping its date and time (read when notes are applied
/// and when a meeting is renamed).
pub fn meeting_descriptive_filenames() -> bool {
    on(MEETING_DESCRIPTIVE_FILENAMES) && Feature::MeetingDescriptiveFilenames.on_this_os()
}

/// A physical Caps Lock press starts and stops dictation, and its light
/// follows Hex's recording and processing (macOS; read by the listener on
/// every pass and by the event tap and HID tracker through their own flag).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn caps_lock_dictation() -> bool {
    on(CAPS_LOCK_DICTATION) && Feature::CapsLockDictation.on_this_os()
}

/// Whether the Caps Lock light blinks slowly while recording instead of
/// staying solid (macOS; read by the light model for each pattern).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn caps_lock_slow_blink() -> bool {
    on(CAPS_LOCK_SLOW_BLINK) && Feature::CapsLockDictation.on_this_os()
}

/// Finished meetings may drop their WAV tracks on `meetings.audio_retention`
/// (read by the meeting controller's expiry pass and the window's Settings
/// row).
pub fn expire_meeting_audio() -> bool {
    on(EXPIRE_MEETING_AUDIO) && Feature::ExpireMeetingAudio.on_this_os()
}

/// How long the speech model may sit unused before the engine unloads it,
/// while Labs › Unload model when idle is on.
pub fn unload_idle() -> Option<std::time::Duration> {
    let state = state();
    (state & UNLOAD_IDLE != 0)
        .then(|| std::time::Duration::from_secs(((state >> MINUTES_SHIFT) & MINUTES_MASK) * 60))
}

/// Records the switches the loaded or saved settings hold.
pub(crate) fn apply(settings: &LabsSettings) {
    STATE.store(encode(settings), Ordering::Release);
}

fn encode(settings: &LabsSettings) -> u64 {
    let policy = crate::policy::Policy::current();
    encode_where(settings, |feature| {
        feature.shipped() && policy.labs_lock(feature).is_none()
    })
}

/// The switches `settings` hold for the features `reaches` admits; the
/// rest read off while their saved setting stays as it is.
fn encode_where(settings: &LabsSettings, reaches: impl Fn(Feature) -> bool) -> u64 {
    let minutes = u64::from(settings.unload_idle_minutes.clamp(1, 24 * 60)) << MINUTES_SHIFT;
    [
        (Feature::LiveText, settings.live_text, LIVE_TEXT),
        (
            Feature::FailureNotices,
            settings.failure_notices,
            FAILURE_NOTICES,
        ),
        (Feature::LiveTyping, settings.live_typing, LIVE_TYPING),
        (Feature::LiveChapters, settings.live_chapters, LIVE_CHAPTERS),
        (Feature::Playground, settings.playground, PLAYGROUND),
        (
            Feature::VocabularyBiasing,
            settings.vocabulary_biasing,
            VOCABULARY_BIASING,
        ),
        (
            Feature::MouseShortcuts,
            settings.mouse_shortcuts,
            MOUSE_SHORTCUTS,
        ),
        (Feature::UnloadIdle, settings.unload_idle, UNLOAD_IDLE),
        (
            Feature::KeepOnClipboard,
            settings.keep_on_clipboard,
            KEEP_ON_CLIPBOARD,
        ),
        (Feature::CopyOnly, settings.copy_only, COPY_ONLY),
        (
            Feature::SpokenFormatting,
            settings.spoken_formatting,
            SPOKEN_FORMATTING,
        ),
        (Feature::FillerWords, settings.filler_words, FILLER_WORDS),
        (
            Feature::SubtitleExport,
            settings.subtitle_export,
            SUBTITLE_EXPORT,
        ),
        (Feature::SmartSpacing, settings.smart_spacing, SMART_SPACING),
        (
            Feature::SpokenSymbols,
            settings.spoken_symbols,
            SPOKEN_SYMBOLS,
        ),
        (
            Feature::SidedChords,
            settings.sided_modifier_chords,
            SIDED_CHORDS,
        ),
        (Feature::TapOrHold, settings.tap_or_hold, TAP_OR_HOLD),
        (
            Feature::ParakeetUltra,
            settings.parakeet_ultra,
            PARAKEET_ULTRA,
        ),
        (Feature::HistoryStats, settings.history_stats, HISTORY_STATS),
        (
            Feature::RecentDictations,
            settings.recent_dictations,
            RECENT_DICTATIONS,
        ),
        (Feature::SkipPauses, settings.skip_pauses, SKIP_PAUSES),
        (
            Feature::StopWhenCallEnds,
            settings.stop_when_call_ends,
            STOP_WHEN_CALL_ENDS,
        ),
        (
            Feature::MoreCallApps,
            settings.more_call_apps,
            MORE_CALL_APPS,
        ),
        (
            Feature::KeepRecordings,
            settings.keep_recordings,
            KEEP_RECORDINGS,
        ),
        (
            Feature::HideMenuBarIcon,
            settings.hide_menu_bar_icon,
            HIDE_MENU_BAR_ICON,
        ),
        (
            Feature::MovableMeetingCapsules,
            settings.movable_meeting_capsules,
            MOVABLE_MEETING_CAPSULES,
        ),
        (
            Feature::GnomeClipboardRestore,
            settings.gnome_clipboard_restore,
            GNOME_CLIPBOARD_RESTORE,
        ),
        (Feature::TypedText, settings.typed_text, TYPED_TEXT),
        (
            Feature::MeetingNotesVocabulary,
            settings.meeting_notes_vocabulary,
            MEETING_NOTES_VOCABULARY,
        ),
        (
            Feature::NotchIndicator,
            settings.notch_indicator,
            NOTCH_INDICATOR,
        ),
        (
            Feature::MeetingFinishedEvent,
            settings.meeting_finished_event,
            MEETING_FINISHED_EVENT,
        ),
        (
            Feature::AnnotateReplay,
            settings.annotate_replay,
            ANNOTATE_REPLAY,
        ),
        (
            Feature::PersistentInk,
            settings.persistent_ink,
            PERSISTENT_INK,
        ),
        (
            Feature::MeetingImport,
            settings.meeting_import,
            MEETING_IMPORT,
        ),
        (
            Feature::MeetingDescriptiveFilenames,
            settings.meeting_descriptive_filenames,
            MEETING_DESCRIPTIVE_FILENAMES,
        ),
        (
            Feature::CapsLockDictation,
            settings.caps_lock_dictation,
            CAPS_LOCK_DICTATION,
        ),
        (
            Feature::CapsLockDictation,
            settings.caps_lock_light == CapsLockLight::SlowBlink,
            CAPS_LOCK_SLOW_BLINK,
        ),
        (
            Feature::QuitClosesWindow,
            settings.quit_closes_window,
            QUIT_CLOSES_WINDOW,
        ),
        (
            Feature::ExpireMeetingAudio,
            settings.expire_meeting_audio,
            EXPIRE_MEETING_AUDIO,
        ),
        (
            Feature::GnomeMeetingCapsules,
            settings.gnome_meeting_capsules,
            GNOME_MEETING_CAPSULES,
        ),
        (
            Feature::GnomeAnnotate,
            settings.gnome_annotate,
            GNOME_ANNOTATE,
        ),
        (Feature::Screenshots, settings.screenshots, SCREENSHOTS),
    ]
    .into_iter()
    .filter(|(feature, on, _)| *on && reaches(*feature))
    .fold(KNOWN | minutes, |state, (_, _, bit)| state | bit)
}

/// The names of the features on, for `hex status`.
pub fn active_names(settings: &AppSettings) -> Vec<String> {
    Feature::available()
        .filter(|feature| feature.on(settings))
        .map(|feature| feature.name().into())
        .collect()
}

/// Logs the switches to process.log; call once logging is up in a long-lived Hex.
pub fn log_launch() {
    tracing::info!(
        live_text = live_text(),
        failure_notices = failure_notices(),
        live_typing = live_typing(),
        live_chapters = live_chapters(),
        typed_text = typed_text(),
        playground = playground(),
        vocabulary_biasing = vocabulary_biasing(),
        "labs"
    );
}

#[cfg(test)]
mod tests {
    use super::cli::resolve;
    use super::*;

    #[test]
    fn graduated_features_answer_by_name() {
        let error = resolve("custom-models", |_| true).unwrap_err().to_string();
        assert!(error.contains("graduated"), "{error}");
        assert_eq!(Feature::from_name("custom-models"), None);
        let error = resolve("meeting-platform-icons", |_| true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("graduated"), "{error}");
        assert_eq!(Feature::from_name("meeting-platform-icons"), None);
    }

    #[test]
    fn every_feature_is_off_by_default_and_round_trips() {
        let mut settings = AppSettings::default();
        for feature in Feature::ALL {
            assert!(
                !feature.on(&settings),
                "{} is on by default",
                feature.name()
            );
            assert_eq!(Feature::from_name(feature.name()), Some(feature));
            feature.set(&mut settings, true);
            assert!(feature.on(&settings));
        }
    }

    #[test]
    fn switches_hold_every_feature() {
        let mut settings = AppSettings::default();
        for switch in Feature::switches(&mut settings) {
            *switch = true;
        }
        for feature in Feature::ALL {
            assert!(feature.on(&settings), "{} has no switch", feature.name());
        }
        assert_eq!(
            Feature::switches(&mut settings).len(),
            Feature::ALL.len(),
            "one switch per feature"
        );
    }

    #[test]
    fn an_audience_reaches_the_builds_it_names() {
        assert!(Audience::Everyone.reaches(false));
        assert!(Audience::Everyone.reaches(true));
        assert!(Audience::Dev.reaches(true));
        assert!(!Audience::Dev.reaches(false));
    }

    #[test]
    fn the_developer_tools_are_dev_only() {
        for feature in [Feature::Playground, Feature::VoiceFilter] {
            assert_eq!(feature.audience(), Audience::Dev, "{}", feature.name());
            assert!(resolve(feature.name(), |f| f.audience().reaches(false)).is_err());
        }
        let labs = LabsSettings {
            playground: true,
            ..LabsSettings::default()
        };
        assert_eq!(
            encode_where(&labs, |f| f.audience().reaches(false)) & PLAYGROUND,
            0
        );
    }

    /// Stands in for a Dev feature in a stable build.
    fn stable(feature: Feature) -> bool {
        feature != Feature::LiveText
    }

    #[test]
    fn a_dev_feature_in_a_stable_build_is_unlisted_refused_and_off() {
        assert!(Feature::available_where(stable).all(|feature| feature != Feature::LiveText));
        assert!(Feature::available_where(|_| true).any(|feature| feature == Feature::LiveText));
        assert!(resolve("live-text", stable).is_err());
        assert!(resolve("filler-words", stable).is_ok());
        let labs = LabsSettings {
            live_text: true,
            filler_words: true,
            ..LabsSettings::default()
        };
        let state = encode_where(&labs, stable);
        assert_eq!(state & LIVE_TEXT, 0);
        assert_ne!(state & FILLER_WORDS, 0);
        assert_ne!(encode_where(&labs, |_| true) & LIVE_TEXT, 0);
        assert!(labs.live_text, "the saved setting is kept");
    }

    #[test]
    fn settings_keys_name_their_feature() {
        for feature in Feature::ALL {
            assert_eq!(
                Feature::for_settings_key(feature.settings_key()),
                Some(feature)
            );
        }
        assert_eq!(
            Feature::for_settings_key("labs.unload_idle_minutes"),
            Some(Feature::UnloadIdle)
        );
        assert_eq!(
            Feature::for_settings_key("labs.caps_lock_light"),
            Some(Feature::CapsLockDictation)
        );
        assert_eq!(Feature::for_settings_key("dictation_hotkey"), None);
    }

    #[test]
    fn unload_idle_carries_its_minutes() {
        let mut labs = LabsSettings::default();
        assert_eq!(labs.unload_idle_minutes, 15);
        let state = encode(&labs);
        assert_eq!(state & UNLOAD_IDLE, 0);
        labs.unload_idle = true;
        labs.unload_idle_minutes = 30;
        let state = encode(&labs);
        assert_ne!(state & UNLOAD_IDLE, 0);
        assert_eq!(state >> MINUTES_SHIFT, 30);
    }

    #[test]
    fn spoken_formatting_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode(&labs) & SPOKEN_FORMATTING, 0);
        labs.spoken_formatting = true;
        labs.unload_idle_minutes = 60;
        let state = encode(&labs);
        assert_ne!(state & SPOKEN_FORMATTING, 0);
        assert_eq!(state >> MINUTES_SHIFT, 60);
    }

    #[test]
    fn filler_words_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode(&labs) & FILLER_WORDS, 0);
        labs.filler_words = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode(&labs);
        assert_ne!(state & FILLER_WORDS, 0);
        assert_eq!(state & SPOKEN_FORMATTING, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn subtitle_export_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode(&labs) & SUBTITLE_EXPORT, 0);
        labs.subtitle_export = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode(&labs);
        assert_ne!(state & SUBTITLE_EXPORT, 0);
        assert_eq!(state & FILLER_WORDS, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn smart_spacing_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode(&labs) & SMART_SPACING, 0);
        labs.smart_spacing = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode(&labs);
        assert_ne!(state & SMART_SPACING, 0);
        assert_eq!(state & SUBTITLE_EXPORT, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn spoken_symbols_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode(&labs) & SPOKEN_SYMBOLS, 0);
        labs.spoken_symbols = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode(&labs);
        assert_ne!(state & SPOKEN_SYMBOLS, 0);
        assert_eq!(state & SPOKEN_FORMATTING, 0);
        assert_eq!(state & SMART_SPACING, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn sided_modifier_chords_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & SIDED_CHORDS, 0);
        labs.sided_modifier_chords = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & SIDED_CHORDS, 0);
        assert_eq!(state & SPOKEN_SYMBOLS, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn tap_or_hold_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & TAP_OR_HOLD, 0);
        labs.tap_or_hold = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & TAP_OR_HOLD, 0);
        assert_eq!(state & SIDED_CHORDS, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn history_stats_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & HISTORY_STATS, 0);
        labs.history_stats = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & HISTORY_STATS, 0);
        assert_eq!(state & PARAKEET_ULTRA, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn recent_dictations_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & RECENT_DICTATIONS, 0);
        labs.recent_dictations = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & RECENT_DICTATIONS, 0);
        assert_eq!(state & HISTORY_STATS, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn skip_pauses_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & SKIP_PAUSES, 0);
        labs.skip_pauses = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & SKIP_PAUSES, 0);
        assert_eq!(state & TAP_OR_HOLD, 0);
        assert_eq!(state & PARAKEET_ULTRA, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn stop_when_call_ends_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & STOP_WHEN_CALL_ENDS, 0);
        labs.stop_when_call_ends = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & STOP_WHEN_CALL_ENDS, 0);
        assert_eq!(state & RECENT_DICTATIONS, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn keep_recordings_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & KEEP_RECORDINGS, 0);
        labs.keep_recordings = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & KEEP_RECORDINGS, 0);
        assert_eq!(state & STOP_WHEN_CALL_ENDS, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn hide_menu_bar_icon_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & HIDE_MENU_BAR_ICON, 0);
        labs.hide_menu_bar_icon = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & HIDE_MENU_BAR_ICON, 0);
        assert_eq!(state & KEEP_RECORDINGS, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn quit_closes_window_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & QUIT_CLOSES_WINDOW, 0);
        labs.quit_closes_window = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & QUIT_CLOSES_WINDOW, 0);
        assert_eq!(state & MEETING_IMPORT, 0);
        assert_eq!(state & HIDE_MENU_BAR_ICON, 0);
        // The switch sits above the minutes, which keep their value.
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
    }

    #[test]
    fn screenshots_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & SCREENSHOTS, 0);
        labs.screenshots = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & SCREENSHOTS, 0);
        assert_eq!(state & MEETING_FINISHED_EVENT, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn notch_indicator_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & NOTCH_INDICATOR, 0);
        labs.notch_indicator = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & NOTCH_INDICATOR, 0);
        assert_eq!(state & MEETING_NOTES_VOCABULARY, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn movable_meeting_capsules_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & MOVABLE_MEETING_CAPSULES, 0);
        labs.movable_meeting_capsules = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & MOVABLE_MEETING_CAPSULES, 0);
        assert_eq!(state & HIDE_MENU_BAR_ICON, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn gnome_clipboard_restore_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & GNOME_CLIPBOARD_RESTORE, 0);
        labs.gnome_clipboard_restore = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & GNOME_CLIPBOARD_RESTORE, 0);
        assert_eq!(state & MEETING_NOTES_VOCABULARY, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn typed_text_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & TYPED_TEXT, 0);
        labs.typed_text = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & TYPED_TEXT, 0);
        assert_eq!(state & GNOME_CLIPBOARD_RESTORE, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(Feature::TypedText.on_this_os(), cfg!(target_os = "linux"));
    }

    #[test]
    fn meeting_finished_event_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & MEETING_FINISHED_EVENT, 0);
        labs.meeting_finished_event = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & MEETING_FINISHED_EVENT, 0);
        assert_eq!(state & NOTCH_INDICATOR, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn annotate_replay_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & ANNOTATE_REPLAY, 0);
        labs.annotate_replay = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & ANNOTATE_REPLAY, 0);
        assert_eq!(state & GNOME_MEETING_CAPSULES, 0);
        assert_eq!(state & SCREENSHOTS, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn persistent_ink_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & PERSISTENT_INK, 0);
        labs.persistent_ink = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & PERSISTENT_INK, 0);
        assert_eq!(state & ANNOTATE_REPLAY, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(Feature::PersistentInk.on_this_os(), cfg!(hex_capture));
    }
    #[test]
    fn gnome_meeting_capsules_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & GNOME_MEETING_CAPSULES, 0);
        labs.gnome_meeting_capsules = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & GNOME_MEETING_CAPSULES, 0);
        assert_eq!(state & GNOME_CLIPBOARD_RESTORE, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn gnome_annotate_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & GNOME_ANNOTATE, 0);
        labs.gnome_annotate = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & GNOME_ANNOTATE, 0);
        assert_eq!(state & GNOME_MEETING_CAPSULES, 0);
        assert_eq!(state & CAPS_LOCK_DICTATION, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
    }

    #[test]
    fn more_call_apps_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & MORE_CALL_APPS, 0);
        labs.more_call_apps = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & MORE_CALL_APPS, 0);
        assert_eq!(state & STOP_WHEN_CALL_ENDS, 0);
        assert_eq!(state >> MINUTES_SHIFT, 24 * 60);
    }

    #[test]
    fn meeting_import_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & MEETING_IMPORT, 0);
        labs.meeting_import = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & MEETING_IMPORT, 0);
        assert_eq!(state & ANNOTATE_REPLAY, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
    }

    #[test]
    fn meeting_descriptive_filenames_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(
            encode_where(&labs, |_| true) & MEETING_DESCRIPTIVE_FILENAMES,
            0
        );
        labs.meeting_descriptive_filenames = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & MEETING_DESCRIPTIVE_FILENAMES, 0);
        assert_eq!(state & LIVE_TYPING, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
    }

    #[test]
    fn failure_notices_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & FAILURE_NOTICES, 0);
        labs.failure_notices = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & FAILURE_NOTICES, 0);
        assert_eq!(state & MEETING_DESCRIPTIVE_FILENAMES, 0);
        assert_eq!(state & LIVE_TYPING, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
    }

    #[test]
    fn expire_meeting_audio_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & EXPIRE_MEETING_AUDIO, 0);
        labs.expire_meeting_audio = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & EXPIRE_MEETING_AUDIO, 0);
        assert_eq!(state & MEETING_IMPORT, 0);
        assert_eq!(state & QUIT_CLOSES_WINDOW, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
    }

    #[test]
    fn caps_lock_dictation_is_its_own_bit_and_macos_only() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & CAPS_LOCK_DICTATION, 0);
        labs.caps_lock_dictation = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & CAPS_LOCK_DICTATION, 0);
        assert_eq!(state & MEETING_DESCRIPTIVE_FILENAMES, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::CapsLockDictation.on_this_os(),
            cfg!(target_os = "macos")
        );
    }

    #[test]
    fn caps_lock_light_carries_its_slow_blink_choice() {
        let mut labs = LabsSettings::default();
        assert_eq!(labs.caps_lock_light, CapsLockLight::Steady);
        labs.caps_lock_dictation = true;
        assert_eq!(encode_where(&labs, |_| true) & CAPS_LOCK_SLOW_BLINK, 0);
        labs.caps_lock_light = CapsLockLight::SlowBlink;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & CAPS_LOCK_SLOW_BLINK, 0);
        assert_ne!(state & CAPS_LOCK_DICTATION, 0);
        assert_eq!(state & GNOME_ANNOTATE, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 15);
    }

    #[test]
    fn terminal_paste_shortcut_is_windows_or_linux_only() {
        assert_eq!(
            Feature::TerminalPasteShortcut.on_this_os(),
            cfg!(any(target_os = "windows", target_os = "linux"))
        );
    }

    #[test]
    fn retired_switches_are_read_and_ignored() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"labs": {"enabled": true, "collapsible_meeting_sections": true, "meeting_platform_icons": true, "live_text": true}}"#,
        )
        .unwrap();
        assert!(settings.labs.live_text);
        assert!(!settings.labs.playground);
        let json = serde_json::to_value(&settings).unwrap();
        assert!(json["labs"].get("enabled").is_none());
        assert!(json["labs"].get("collapsible_meeting_sections").is_none());
        assert!(json["labs"].get("meeting_platform_icons").is_none());
    }
}
