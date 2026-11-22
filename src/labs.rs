//! Labs: features we're unsure about, especially ones people asked for,
//! each behind its own switch, off by default. There is no master switch:
//! Settings › Labs lists every feature as a card with a toggle, under its
//! group's collapsible header (`Feature::group`). A feature
//! with its own pane (Meetings, Voice Filter, Playground, the Modes editor)
//! joins the sidebar as a normal item while on and is hidden while off; a behaviour-only
//! feature (Live text, Live chapters, Vocabulary help, Words on screen,
//! the rewrite reading the text around the cursor (macOS),
//! keyboard-language
//! model switching, mouse shortcuts, pausing the browser, a spoken phrase
//! that finishes a hands-free dictation, restoring Paste
//! last dictation's default shortcut, keeping the dictation on the
//! clipboard, only copying it instead (macOS and Windows), a second
//! dictation shortcut,
//! more dictation shortcuts with their own activation modes
//! (macOS), Toggle's tap or hold (macOS, Linux), stopping an automatic meeting
//! recording when its call ends (macOS), printing a shell completion script
//! from a terminal, and Terminal paste, shortcut sharing
//! on Windows, and a terminal-paste dictation shortcut on Windows and Linux,
//! and applying While dictating only on speakers) is just its toggle.
//!
//! Each feature has an audience (`Feature::audience`): `Everyone` ships in
//! every build; `Dev` exists only in dev builds (`dev_build::is_dev_build`:
//! debug, local, test, and dev-channel builds). A stable build never lists,
//! accepts, or runs a Dev feature, and keeps its saved setting untouched.
//!
//! Gate a feature where its work starts (the worker, thread, or capture
//! that does it), not only where its UI shows.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, RwLock};

use crate::app_settings::{AppSettings, AppearanceTheme, CapsLockLight, LabsSettings};

mod card;
mod catalog;
mod cli;
mod description;
mod feature;
mod gates;
mod group;
#[cfg(test)]
mod web;

pub use self::cli::{LabsCommand, run, run_retired_experiments};
pub use self::feature::Feature;
pub use self::gates::*;
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

/// An OS Hex runs on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

impl Os {
    /// The OS this build runs on.
    pub const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Macos
    } else if cfg!(target_os = "windows") {
        Self::Windows
    } else {
        Self::Linux
    };
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

impl Feature {
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

    /// Whether `settings` turn it on.
    pub fn on(self, settings: &AppSettings) -> bool {
        match self {
            Self::Meetings => settings.meetings.enabled,
            Self::VoiceFilter => settings.experimental.hex_microphone,
            Self::Playground => settings.labs.playground,
            Self::LiveText => settings.labs.live_text,
            Self::FasterLiveText => settings.labs.faster_live_text,
            Self::FailureNotices => settings.labs.failure_notices,
            Self::LiveTyping => settings.labs.live_typing,
            Self::LiveChapters => settings.labs.live_chapters,
            Self::ScreenVocabulary => settings.labs.screen_vocabulary,
            Self::LearnFromCorrections => settings.labs.learn_from_corrections,
            Self::TerminalPaste => settings.paste_with_shift,
            Self::KeyboardModelSwitching => settings.keyboard_model_switching.enabled,
            Self::MouseShortcuts => settings.labs.mouse_shortcuts,
            Self::PauseBrowser => settings.pause_browser_media,
            Self::ShareShortcut => settings.share_dictation_shortcut,
            Self::UnloadIdle => settings.labs.unload_idle,
            Self::KeepMicrophoneOpen => settings.labs.keep_microphone_open,
            Self::KeepOnClipboard => settings.labs.keep_on_clipboard,
            Self::CopyOnly => settings.labs.copy_only,
            Self::ReturnToApp => settings.labs.return_to_app,
            Self::SpokenFormatting => settings.labs.spoken_formatting,
            Self::FillerWords => settings.labs.filler_words,
            Self::CleanUpWithAppleIntelligence => settings.labs.clean_up_with_apple_intelligence,
            Self::NeutralVoice => settings.labs.neutral_voice,
            Self::SecondDictationShortcut => settings.labs.second_dictation_shortcut,
            Self::TerminalPasteShortcut => settings.labs.terminal_paste_shortcut,
            Self::SmartSpacing => settings.labs.smart_spacing,
            Self::SpokenSymbols => settings.labs.spoken_symbols,
            Self::PickerSortFilter => settings.labs.picker_sort_filter,
            Self::SidedChords => settings.labs.sided_modifier_chords,
            Self::TapOrHold => settings.labs.tap_or_hold,
            Self::DoubleTapModifiers => settings.labs.double_tap_modifiers,
            Self::BenchmarkDetails => settings.labs.benchmark_details,
            Self::ParakeetUltra => settings.labs.parakeet_ultra,
            Self::StandaloneSpecialKeys => settings.labs.standalone_special_keys,
            Self::HistoryStats => settings.labs.history_stats,
            Self::RecentDictations => settings.labs.recent_dictations,
            Self::SkipPauses => settings.labs.skip_pauses,
            Self::OutputFixes => settings.labs.output_fixes,
            Self::ModelInPlace => settings.labs.model_in_place,
            Self::KQuantModels => settings.labs.k_quant_models,
            Self::PickerDownloaded => settings.labs.picker_downloaded,
            Self::SectionKeyAlone => settings.labs.section_key_alone,
            Self::MicrophoneKeyAlone => settings.labs.microphone_key,
            Self::KeepRecordings => settings.labs.keep_recordings,
            Self::Appearance => settings.labs.appearance,
            Self::HideMenuBarIcon => settings.labs.hide_menu_bar_icon,
            Self::GnomeClipboardRestore => settings.labs.gnome_clipboard_restore,
            Self::TypedText => settings.labs.typed_text,
            Self::NotchIndicator => settings.labs.notch_indicator,
            Self::GnomeMeetingCapsules => settings.labs.gnome_meeting_capsules,
            Self::GnomeAnnotate => settings.labs.gnome_annotate,
            Self::AnnotatePointerScreen => settings.labs.annotate_pointer_screen,
            Self::Screenshots => settings.labs.screenshots,
            Self::AnnotateReplay => settings.labs.annotate_replay,
            Self::MeetingImport => settings.labs.meeting_import,
            Self::CapsLockDictation => settings.labs.caps_lock_dictation,
            Self::OpenCodeInWsl => settings.labs.opencode_in_wsl,
            Self::ApiProcessing => settings.labs.api_processing,
            Self::ApiTimestamps => settings.labs.api_timestamps,
            Self::SpeakersOnly => settings.labs.speakers_only,
            Self::CalendarMenu => settings.labs.calendar_menu,
            Self::CalendarSelectAll => settings.labs.calendar_bulk_select,
            Self::PauseMenu => settings.labs.pause_menu,
            Self::VoicePrompt => settings.labs.voice_prompt,
            Self::StopPhrase => settings.labs.stop_phrase,
            Self::Signature => settings.labs.signature,
            Self::HoldThroughKeys => settings.labs.hold_through_keys,
            Self::CopyCaptureZip => settings.labs.copy_capture_zip,
            Self::DropTranscribe => settings.labs.drop_transcribe,
            Self::YoloMode => settings.labs.yolo_mode,
            Self::CursorContext => settings.labs.cursor_context,
            Self::MoreDictationShortcuts => settings.labs.more_dictation_shortcuts,
            Self::CopyDictationAudio => settings.labs.copy_dictation_audio,
            Self::ModesEditor => settings.labs.modes_editor,
            Self::PasteLastPicker => settings.labs.paste_last_picker,
            Self::VocabularyFiles => settings.labs.vocabulary_files,
            Self::AnnotateAgentTools => settings.labs.annotate_agent_tools,
            Self::AnnotateAgentInstall => settings.labs.annotate_agent_install,
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
            &mut labs.faster_live_text,
            &mut labs.failure_notices,
            &mut labs.live_typing,
            &mut labs.live_chapters,
            &mut labs.screen_vocabulary,
            &mut labs.learn_from_corrections,
            &mut labs.mouse_shortcuts,
            &mut labs.unload_idle,
            &mut labs.keep_microphone_open,
            &mut labs.keep_on_clipboard,
            &mut labs.copy_only,
            &mut labs.return_to_app,
            &mut labs.spoken_formatting,
            &mut labs.filler_words,
            &mut labs.clean_up_with_apple_intelligence,
            &mut labs.neutral_voice,
            &mut labs.second_dictation_shortcut,
            &mut labs.terminal_paste_shortcut,
            &mut labs.smart_spacing,
            &mut labs.spoken_symbols,
            &mut labs.picker_sort_filter,
            &mut labs.sided_modifier_chords,
            &mut labs.tap_or_hold,
            &mut labs.double_tap_modifiers,
            &mut labs.benchmark_details,
            &mut labs.parakeet_ultra,
            &mut labs.standalone_special_keys,
            &mut labs.history_stats,
            &mut labs.recent_dictations,
            &mut labs.skip_pauses,
            &mut labs.output_fixes,
            &mut labs.model_in_place,
            &mut labs.k_quant_models,
            &mut labs.picker_downloaded,
            &mut labs.section_key_alone,
            &mut labs.microphone_key,
            &mut labs.keep_recordings,
            &mut labs.appearance,
            &mut labs.hide_menu_bar_icon,
            &mut labs.gnome_clipboard_restore,
            &mut labs.typed_text,
            &mut labs.notch_indicator,
            &mut labs.gnome_meeting_capsules,
            &mut labs.gnome_annotate,
            &mut labs.annotate_pointer_screen,
            &mut labs.screenshots,
            &mut labs.annotate_replay,
            &mut labs.meeting_import,
            &mut labs.caps_lock_dictation,
            &mut labs.opencode_in_wsl,
            &mut labs.api_processing,
            &mut labs.api_timestamps,
            &mut labs.speakers_only,
            &mut labs.calendar_menu,
            &mut labs.calendar_bulk_select,
            &mut labs.app_model_switching,
            &mut labs.pause_menu,
            &mut labs.voice_prompt,
            &mut labs.stop_phrase,
            &mut labs.signature,
            &mut labs.hold_through_keys,
            &mut labs.copy_capture_zip,
            &mut labs.drop_transcribe,
            &mut labs.yolo_mode,
            &mut labs.cursor_context,
            &mut labs.more_dictation_shortcuts,
            &mut labs.copy_dictation_audio,
            &mut labs.modes_editor,
            &mut labs.paste_last_picker,
            &mut labs.vocabulary_files,
            &mut labs.annotate_agent_tools,
            &mut labs.annotate_agent_install,
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
            Self::FasterLiveText => settings.labs.faster_live_text = on,
            Self::FailureNotices => settings.labs.failure_notices = on,
            Self::LiveTyping => settings.labs.live_typing = on,
            Self::LiveChapters => settings.labs.live_chapters = on,
            Self::ScreenVocabulary => settings.labs.screen_vocabulary = on,
            Self::LearnFromCorrections => settings.labs.learn_from_corrections = on,
            Self::TerminalPaste => settings.paste_with_shift = on,
            Self::KeyboardModelSwitching => settings.keyboard_model_switching.enabled = on,
            Self::MouseShortcuts => settings.labs.mouse_shortcuts = on,
            Self::PauseBrowser => settings.pause_browser_media = on,
            Self::ShareShortcut => settings.share_dictation_shortcut = on,
            Self::UnloadIdle => settings.labs.unload_idle = on,
            Self::KeepMicrophoneOpen => settings.labs.keep_microphone_open = on,
            Self::KeepOnClipboard => settings.labs.keep_on_clipboard = on,
            Self::CopyOnly => settings.labs.copy_only = on,
            Self::ReturnToApp => settings.labs.return_to_app = on,
            Self::SpokenFormatting => settings.labs.spoken_formatting = on,
            Self::FillerWords => settings.labs.filler_words = on,
            Self::CleanUpWithAppleIntelligence => {
                settings.labs.clean_up_with_apple_intelligence = on;
            }
            Self::NeutralVoice => settings.labs.neutral_voice = on,
            Self::SecondDictationShortcut => settings.labs.second_dictation_shortcut = on,
            Self::TerminalPasteShortcut => settings.labs.terminal_paste_shortcut = on,
            Self::SmartSpacing => settings.labs.smart_spacing = on,
            Self::SpokenSymbols => settings.labs.spoken_symbols = on,
            Self::PickerSortFilter => settings.labs.picker_sort_filter = on,
            Self::SidedChords => settings.labs.sided_modifier_chords = on,
            Self::TapOrHold => settings.labs.tap_or_hold = on,
            Self::DoubleTapModifiers => {
                settings.labs.double_tap_modifiers = on;
                // Off, a modifier-only shortcut can't be double-tap-only, so
                // it falls back as a fresh load would.
                settings.normalize_double_tap_settings();
            }
            Self::BenchmarkDetails => settings.labs.benchmark_details = on,
            Self::ParakeetUltra => settings.labs.parakeet_ultra = on,
            Self::HistoryStats => settings.labs.history_stats = on,
            Self::RecentDictations => settings.labs.recent_dictations = on,
            Self::SkipPauses => settings.labs.skip_pauses = on,
            Self::OutputFixes => settings.labs.output_fixes = on,
            Self::ModelInPlace => settings.labs.model_in_place = on,
            Self::KQuantModels => settings.labs.k_quant_models = on,
            Self::PickerDownloaded => settings.labs.picker_downloaded = on,
            Self::KeepRecordings => settings.labs.keep_recordings = on,
            Self::Appearance => settings.labs.appearance = on,
            Self::HideMenuBarIcon => settings.labs.hide_menu_bar_icon = on,
            Self::GnomeClipboardRestore => settings.labs.gnome_clipboard_restore = on,
            Self::TypedText => settings.labs.typed_text = on,
            Self::NotchIndicator => settings.labs.notch_indicator = on,
            Self::GnomeMeetingCapsules => settings.labs.gnome_meeting_capsules = on,
            Self::GnomeAnnotate => settings.labs.gnome_annotate = on,
            Self::AnnotatePointerScreen => settings.labs.annotate_pointer_screen = on,
            Self::Screenshots => {
                settings.labs.screenshots = on;
                #[cfg(hex_capture)]
                if on {
                    settings.clear_record_shortcut_sharing_capture();
                }
            }
            Self::AnnotateReplay => settings.labs.annotate_replay = on,
            Self::MeetingImport => settings.labs.meeting_import = on,
            Self::CapsLockDictation => settings.labs.caps_lock_dictation = on,
            Self::OpenCodeInWsl => settings.labs.opencode_in_wsl = on,
            Self::ApiProcessing => settings.labs.api_processing = on,
            Self::ApiTimestamps => settings.labs.api_timestamps = on,
            Self::SpeakersOnly => settings.labs.speakers_only = on,
            Self::CalendarMenu => settings.labs.calendar_menu = on,
            Self::CalendarSelectAll => settings.labs.calendar_bulk_select = on,
            Self::PauseMenu => settings.labs.pause_menu = on,
            Self::VoicePrompt => settings.labs.voice_prompt = on,
            Self::StopPhrase => settings.labs.stop_phrase = on,
            Self::Signature => settings.labs.signature = on,
            Self::HoldThroughKeys => settings.labs.hold_through_keys = on,
            Self::CopyCaptureZip => settings.labs.copy_capture_zip = on,
            Self::DropTranscribe => settings.labs.drop_transcribe = on,
            Self::YoloMode => settings.labs.yolo_mode = on,
            Self::CursorContext => settings.labs.cursor_context = on,
            Self::MoreDictationShortcuts => settings.labs.more_dictation_shortcuts = on,
            Self::CopyDictationAudio => settings.labs.copy_dictation_audio = on,
            Self::ModesEditor => settings.labs.modes_editor = on,
            Self::PasteLastPicker => settings.labs.paste_last_picker = on,
            Self::VocabularyFiles => settings.labs.vocabulary_files = on,
            Self::AnnotateAgentTools => settings.labs.annotate_agent_tools = on,
            Self::AnnotateAgentInstall => settings.labs.annotate_agent_install = on,
            // Off, no shortcut keeps a key only the feature allows.
            Self::StandaloneSpecialKeys => {
                settings.labs.standalone_special_keys = on;
                settings.repair_special_keys();
            }
            Self::SectionKeyAlone => {
                settings.labs.section_key_alone = on;
                settings.repair_section_key();
            }
            Self::MicrophoneKeyAlone => {
                settings.labs.microphone_key = on;
                settings.repair_microphone_key();
            }
        }
    }
}

/// Set once this process knows its settings.
const KNOWN: u64 = 1;
const LIVE_TEXT: u64 = 1 << 1;
const LIVE_CHAPTERS: u64 = 1 << 2;
const PLAYGROUND: u64 = 1 << 3;
const MOUSE_SHORTCUTS: u64 = 1 << 5;
const UNLOAD_IDLE: u64 = 1 << 6;
const KEEP_ON_CLIPBOARD: u64 = 1 << 7;
const COPY_ONLY: u64 = 1 << 8;
const SPOKEN_FORMATTING: u64 = 1 << 9;
const FILLER_WORDS: u64 = 1 << 10;
/// A key or click joining a held modifier-only shortcut a second or more in
/// is left to the app in front (macOS and Windows).
const HOLD_THROUGH_KEYS: u64 = 1 << 11;
const SMART_SPACING: u64 = 1 << 12;
const SPOKEN_SYMBOLS: u64 = 1 << 13;
const SIDED_CHORDS: u64 = 1 << 14;
const TAP_OR_HOLD: u64 = 1 << 15;
const SKIP_PAUSES: u64 = 1 << 16;
const PARAKEET_ULTRA: u64 = 1 << 17;
const HISTORY_STATS: u64 = 1 << 18;
const RECENT_DICTATIONS: u64 = 1 << 19;
/// On Wayland, a screen capture follows the pointer to the monitor it is on
/// (Linux). Yearly stats left this bit free when it merged into Dictation
/// stats.
const ANNOTATE_POINTER_SCREEN: u64 = 1 << 20;
const KEEP_RECORDINGS: u64 = 1 << 21;
const HIDE_MENU_BAR_ICON: u64 = 1 << 22;
/// Labs › Faster live text: a shorter stream lookahead, on the bit the
/// graduated meeting labs left free.
const FASTER_LIVE_TEXT: u64 = 1 << 23;
/// Importing and exporting the vocabulary as a CSV file, on the bit More
/// call apps left free.
// Kept only for the state-word encoding; no gate reads it.
const VOCABULARY_FILES: u64 = 1 << 24;
const GNOME_CLIPBOARD_RESTORE: u64 = 1 << 25;
// 1 << 26 is free (Vocabulary in meeting notes graduated).
const NOTCH_INDICATOR: u64 = 1 << 27;
// 1 << 28 is free (Meeting finished event graduated).
const GNOME_MEETING_CAPSULES: u64 = 1 << 29;
const SCREENSHOTS: u64 = 1 << 30;
const ANNOTATE_REPLAY: u64 = 1 << 31;
/// Unload-idle minutes (at most 1,440, 11 bits) sit above the switches.
const MINUTES_SHIFT: u64 = 32;
const MINUTES_BITS: u32 = 11;
const MINUTES_MASK: u64 = (1 << MINUTES_BITS) - 1;
/// This switch sits above the minutes.
const MEETING_IMPORT: u64 = 1 << (MINUTES_SHIFT + MINUTES_BITS as u64);
/// The next bit above Import audio's switch, clear of the minutes below it.
/// The bit above it (Quit closes Settings) is retired and stays unused.
const LIVE_TYPING: u64 = MEETING_IMPORT << 2;
/// The next bit above Live typing's switch.
const FAILURE_NOTICES: u64 = LIVE_TYPING << 1;
/// The next bit above Failure notices' switch.
const CAPS_LOCK_DICTATION: u64 = FAILURE_NOTICES << 1;
/// The next bit above Caps Lock dictation's switch.
const GNOME_ANNOTATE: u64 = CAPS_LOCK_DICTATION << 1;

/// The next bit above Annotate on GNOME's switch.
const TYPED_TEXT: u64 = GNOME_ANNOTATE << 1;
/// The next bit above Typed text's switch: Caps Lock dictation's recording
/// light blinks slowly instead of staying solid.
const CAPS_LOCK_SLOW_BLINK: u64 = TYPED_TEXT << 1;
/// The next bit above Caps Lock's slow blink: an OpenCode service in WSL is
/// used ahead of a Windows one when it answers, with Windows as the fallback
/// (Windows).
const OPENCODE_IN_WSL: u64 = CAPS_LOCK_SLOW_BLINK << 1;
/// The next bit above OpenCode in WSL: the local API's `process=global`
/// returns a transcript after Global mode's output processing.
const API_PROCESSING: u64 = OPENCODE_IN_WSL << 1;
/// The next bit above Process API transcripts' switch: Double-tap accepts a
/// shortcut without a key (macOS).
const DOUBLE_TAP_MODIFIERS: u64 = API_PROCESSING << 1;
/// The next bit above Double-tap Option, Command, or Globe's switch: a dictation
/// pastes into the app its capture started in (macOS).
const RETURN_TO_APP: u64 = DOUBLE_TAP_MODIFIERS << 1;
/// The next bit above Return to app's switch: "While dictating" applies only
/// when the default output is speakers.
const SPEAKERS_ONLY: u64 = RETURN_TO_APP << 1;
/// The next bit above Only on speakers' switch: the menu bar offers Pause
/// Hex, and every capture shortcut is inert while paused (macOS). The bit
/// above Only on speakers (Switch model with app) is retired and stays unused.
const PAUSE_MENU: u64 = SPEAKERS_ONLY << 2;
/// The next bit above Pause from menu bar: the local API's
/// `timestamps=segment|word` returns native timings.
const API_TIMESTAMPS: u64 = PAUSE_MENU << 1;
/// With Release microphone while idle on, the microphone stays open for a
/// while after a capture (Labs › Keep mic open after dictating). Its window
/// does not fit above the minutes, so it lives in
/// [`MICROPHONE_LINGER_SECONDS`]. A new lab can take one of the free bits
/// below the minutes.
const KEEP_MICROPHONE_OPEN: u64 = 1 << 60;

/// This process's `labs` switches: unknown until settings load or one is
/// first read.
static STATE: AtomicU64 = AtomicU64::new(0);

/// The seconds a capture's microphone keeps open after it ends, set beside
/// [`STATE`]: 0 turns it off. The one free bit above holds the switch; the
/// window itself has no room in the word.
static MICROPHONE_LINGER_SECONDS: AtomicU64 = AtomicU64::new(0);

/// Labs › Words on screen's switch, set beside [`STATE`] like [`SCREEN_VOCABULARY`]:
/// that lab reads the focused window once per dictation.
static SCREEN_VOCABULARY: AtomicBool = AtomicBool::new(false);

/// Labs › Learn from corrections' switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: that lab reads the field a dictation pasted into,
/// and the state word has no bit for it.
static LEARN_FROM_CORRECTIONS: AtomicBool = AtomicBool::new(false);

/// Labs › Rewrite reads nearby text's switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: the mode's rewrite reads the focused text element
/// once per dictation, and the state word has no bit for it.
static CURSOR_CONTEXT: AtomicBool = AtomicBool::new(false);

/// Labs › Rewrite by voice's switch in its own atomic, written by [`state`] and
/// [`apply`].
static VOICE_PROMPT: AtomicBool = AtomicBool::new(false);

/// Labs › Stop phrase's switch, set beside [`STATE`] like [`VOICE_PROMPT`]:
/// the state word is full, so this lab has its own atomic, written by
/// [`state`] and [`apply`].
static STOP_PHRASE: AtomicBool = AtomicBool::new(false);

/// Labs › Signature's switch, set beside [`STATE`] like [`STOP_PHRASE`]: the
/// state word is full, so this lab has its own atomic, written by [`state`]
/// and [`apply`].
static SIGNATURE: AtomicBool = AtomicBool::new(false);

/// Labs › Copy a capture as one file's switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: a written capture copies as one zip file.
static COPY_CAPTURE_ZIP: AtomicBool = AtomicBool::new(false);

/// Labs › Copy dictation audio's switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: History and the menu bar or tray put an entry's kept WAV
/// on the clipboard as a file. The state word is full, so this lab has its
/// own atomic, written by [`state`] and [`apply`].
static COPY_DICTATION_AUDIO: AtomicBool = AtomicBool::new(false);

/// Labs › Smaller model files' switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`], written by [`state`] and [`apply`].
static K_QUANT_MODELS: AtomicBool = AtomicBool::new(false);

/// Labs › Transcribe dropped audio's switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: the state word is full, so this lab has its own atomic,
/// written by [`state`] and [`apply`].
static DROP_TRANSCRIBE: AtomicBool = AtomicBool::new(false);

/// Labs › Upcoming meetings in menu bar's switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: the state word is full, so this lab has its own atomic,
/// written by [`state`] and [`apply`].
static CALENDAR_MENU: AtomicBool = AtomicBool::new(false);

/// None while YOLO mode is off; an empty app list sends everywhere.
static YOLO_MODE_APPS: RwLock<Option<Vec<String>>> = RwLock::new(None);

/// Labs › Clean up with Apple Intelligence's switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: the state word is full, so this lab has its own atomic,
/// written by [`state`] and [`apply`].
static CLEAN_UP: AtomicBool = AtomicBool::new(false);

/// Labs › Neutral voice's switch, set beside [`STATE`] like [`SCREEN_VOCABULARY`]:
/// the state word is full, so this lab has its own atomic, written by
/// [`state`] and [`apply`].
static NEUTRAL_VOICE: AtomicBool = AtomicBool::new(false);

/// Labs › Microphone key alone's switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: the state word is full, so this lab has its own atomic,
/// written by [`state`] and [`apply`].
static MICROPHONE_KEY: AtomicBool = AtomicBool::new(false);

/// Labs › Paste Last picker's switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: the state word is full, so this lab has its own atomic,
/// written by [`state`] and [`apply`].
static PASTE_LAST_PICKER: AtomicBool = AtomicBool::new(false);

/// Labs › Agent tools for captures' switch, set beside [`STATE`] like
/// [`SCREEN_VOCABULARY`]: the state word is full, so this lab has its own atomic,
/// written by [`state`] and [`apply`]. `hex annotate inspect|zoom|frames|skill`
/// reads it in a CLI process that never applied settings.
static ANNOTATE_AGENT_TOOLS: AtomicBool = AtomicBool::new(false);

/// The words that start a voice prompt, set beside [`STATE`] by [`state`] and
/// [`apply`]. An invalid saved phrase reads as the default.
static VOICE_PROMPT_PHRASE: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new(crate::dictation::voice_prompt::DEFAULT_PHRASE.to_owned()));

/// The words that finish a hands-free dictation, set beside [`STATE`] by
/// [`state`] and [`apply`]. An invalid saved phrase reads as the default.
static STOP_PHRASE_TEXT: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new(crate::dictation::stop_phrase::DEFAULT_PHRASE.to_owned()));

/// The signature appended to each normal dictation, set beside [`STATE`] by
/// [`state`] and [`apply`]. Empty appends nothing; an invalid saved text
/// reads as the default.
static SIGNATURE_TEXT: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new(crate::dictation::signature::DEFAULT_TEXT.to_owned()));

fn on(bit: u64) -> bool {
    state() & bit != 0
}

fn state() -> u64 {
    match STATE.load(Ordering::Acquire) {
        0 => {
            let saved = AppSettings::saved_labs();
            if STATE
                .compare_exchange(0, encode(&saved), Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // The switch and its window are two stores. A gate read just
                // between them sees the switch on with no window, which reads
                // as off, and the next read is right.
                MICROPHONE_LINGER_SECONDS.store(linger_seconds(&saved), Ordering::Release);
                SCREEN_VOCABULARY.store(screen_vocabulary_enabled(&saved), Ordering::Release);
                LEARN_FROM_CORRECTIONS
                    .store(learn_from_corrections_enabled(&saved), Ordering::Release);
                CURSOR_CONTEXT.store(cursor_context_enabled(&saved), Ordering::Release);
                VOICE_PROMPT.store(voice_prompt_enabled(&saved), Ordering::Release);
                STOP_PHRASE.store(stop_phrase_enabled(&saved), Ordering::Release);
                SIGNATURE.store(signature_enabled(&saved), Ordering::Release);
                COPY_CAPTURE_ZIP.store(copy_capture_zip_enabled(&saved), Ordering::Release);
                COPY_DICTATION_AUDIO.store(copy_dictation_audio_enabled(&saved), Ordering::Release);
                K_QUANT_MODELS.store(k_quant_models_enabled(&saved), Ordering::Release);
                DROP_TRANSCRIBE.store(drop_transcribe_enabled(&saved), Ordering::Release);
                CALENDAR_MENU.store(calendar_menu_enabled(&saved), Ordering::Release);
                store_yolo_mode(&saved);
                CLEAN_UP.store(clean_up_enabled(&saved), Ordering::Release);
                NEUTRAL_VOICE.store(neutral_voice_enabled(&saved), Ordering::Release);
                MICROPHONE_KEY.store(microphone_key_enabled(&saved), Ordering::Release);
                PASTE_LAST_PICKER.store(paste_last_picker_enabled(&saved), Ordering::Release);
                ANNOTATE_AGENT_TOOLS.store(annotate_agent_tools_enabled(&saved), Ordering::Release);
                store_voice_prompt_phrase(&saved.voice_prompt_phrase);
                store_stop_phrase_text(&saved.stop_phrase_text);
                store_signature_text(&saved.signature_text);
            }
            STATE.load(Ordering::Acquire)
        }
        state => state,
    }
}

/// The window a saved setting holds, in seconds; 0 when it's off.
fn linger_seconds(settings: &LabsSettings) -> u64 {
    settings
        .keep_microphone_open_for()
        .map_or(0, |window| window.as_secs())
}

/// Whether `settings` turn on Labs › Words on screen in this build, before
/// its managed lock; the state word has no bit for it.
fn screen_vocabulary_enabled(settings: &LabsSettings) -> bool {
    settings.screen_vocabulary
        && Feature::ScreenVocabulary.shipped()
        && Feature::ScreenVocabulary.managed().is_none()
}

/// Whether `settings` turn on Labs › Learn from corrections in this build,
/// before its managed lock; the state word has no bit for it.
fn learn_from_corrections_enabled(settings: &LabsSettings) -> bool {
    settings.learn_from_corrections
        && Feature::LearnFromCorrections.shipped()
        && Feature::LearnFromCorrections.managed().is_none()
}

/// Whether `settings` turn on Labs › Rewrite reads nearby text in this build,
/// before its managed lock; the state word has no bit for it.
fn cursor_context_enabled(settings: &LabsSettings) -> bool {
    settings.cursor_context
        && Feature::CursorContext.shipped()
        && Feature::CursorContext.managed().is_none()
}

/// Whether `settings` turn on Labs › Rewrite by voice in this build, before its
/// managed lock; the state word has no bit for it.
fn voice_prompt_enabled(settings: &LabsSettings) -> bool {
    settings.voice_prompt
        && Feature::VoicePrompt.shipped()
        && Feature::VoicePrompt.managed().is_none()
}

/// Whether `settings` turn on Labs › Stop phrase in this build, before its
/// managed lock; the state word has no bit for it.
fn stop_phrase_enabled(settings: &LabsSettings) -> bool {
    settings.stop_phrase && Feature::StopPhrase.shipped() && Feature::StopPhrase.managed().is_none()
}

/// Whether `settings` turn on Labs › Signature in this build, before its
/// managed lock; the state word has no bit for it.
fn signature_enabled(settings: &LabsSettings) -> bool {
    settings.signature && Feature::Signature.shipped() && Feature::Signature.managed().is_none()
}

/// Whether `settings` turn on Labs › Copy a capture as one file in this build,
/// before its managed lock; the state word has no bit for it.
fn copy_capture_zip_enabled(settings: &LabsSettings) -> bool {
    settings.copy_capture_zip
        && Feature::CopyCaptureZip.shipped()
        && Feature::CopyCaptureZip.managed().is_none()
}

/// Whether `settings` turn on Labs › Smaller model files in this build,
/// before its managed lock; the state word has no bit for it.
fn k_quant_models_enabled(settings: &LabsSettings) -> bool {
    settings.k_quant_models
        && Feature::KQuantModels.shipped()
        && Feature::KQuantModels.managed().is_none()
}

/// Whether `settings` turn on Labs › Transcribe dropped audio in this build,
/// before its managed lock; the state word has no bit for it.
fn drop_transcribe_enabled(settings: &LabsSettings) -> bool {
    settings.drop_transcribe
        && Feature::DropTranscribe.shipped()
        && Feature::DropTranscribe.managed().is_none()
}

/// Whether `settings` turn on Labs › Copy dictation audio in this build,
/// before its managed lock; the state word has no bit for it.
fn copy_dictation_audio_enabled(settings: &LabsSettings) -> bool {
    settings.copy_dictation_audio
        && Feature::CopyDictationAudio.shipped()
        && Feature::CopyDictationAudio.managed().is_none()
}

/// Whether `settings` turn on Labs › Upcoming meetings in menu bar in this
/// build, before its managed lock; the state word has no bit for it.
fn calendar_menu_enabled(settings: &LabsSettings) -> bool {
    settings.calendar_menu
        && Feature::CalendarMenu.shipped()
        && Feature::CalendarMenu.managed().is_none()
}

/// Whether `settings` turn on Labs › YOLO mode in this build,
/// before its managed lock; the state word has no bit for it.
fn yolo_mode_enabled(settings: &LabsSettings) -> bool {
    settings.yolo_mode && Feature::YoloMode.shipped() && Feature::YoloMode.managed().is_none()
}

/// Publish the switch and its app scope together.
fn store_yolo_mode(settings: &LabsSettings) {
    *YOLO_MODE_APPS
        .write()
        .unwrap_or_else(|error| error.into_inner()) =
        yolo_mode_enabled(settings).then(|| settings.yolo_mode_apps.clone());
}

/// Whether `settings` turn on Labs › Clean up with Apple Intelligence in this
/// build, before its managed lock; the state word has no bit for it.
fn clean_up_enabled(settings: &LabsSettings) -> bool {
    settings.clean_up_with_apple_intelligence
        && Feature::CleanUpWithAppleIntelligence.shipped()
        && Feature::CleanUpWithAppleIntelligence.managed().is_none()
}

/// Whether `settings` turn on Labs › Neutral voice in this build, before its
/// managed lock; the state word has no bit for it.
fn neutral_voice_enabled(settings: &LabsSettings) -> bool {
    settings.neutral_voice
        && Feature::NeutralVoice.shipped()
        && Feature::NeutralVoice.managed().is_none()
}

/// Whether `settings` turn on Labs › Microphone key alone in this build,
/// before its managed lock; the state word has no bit for it.
fn microphone_key_enabled(settings: &LabsSettings) -> bool {
    settings.microphone_key
        && Feature::MicrophoneKeyAlone.shipped()
        && Feature::MicrophoneKeyAlone.managed().is_none()
}

/// Whether `settings` turn on Labs › Paste Last picker in this build, before
/// its managed lock; the state word has no bit for it.
fn paste_last_picker_enabled(settings: &LabsSettings) -> bool {
    settings.paste_last_picker
        && Feature::PasteLastPicker.shipped()
        && Feature::PasteLastPicker.managed().is_none()
}

/// Whether `settings` turn on Labs › Agent tools for captures in this build,
/// before its managed lock; the state word has no bit for it.
fn annotate_agent_tools_enabled(settings: &LabsSettings) -> bool {
    settings.annotate_agent_tools
        && Feature::AnnotateAgentTools.shipped()
        && Feature::AnnotateAgentTools.managed().is_none()
}

/// Records the trigger phrase matching reads; an invalid value never turns
/// matching off.
fn store_voice_prompt_phrase(phrase: &str) {
    *VOICE_PROMPT_PHRASE
        .write()
        .unwrap_or_else(|error| error.into_inner()) =
        crate::dictation::voice_prompt::accepted_phrase(phrase);
}

/// Records the phrase that finishes a hands-free dictation; an invalid value
/// never turns matching off.
fn store_stop_phrase_text(phrase: &str) {
    *STOP_PHRASE_TEXT
        .write()
        .unwrap_or_else(|error| error.into_inner()) =
        crate::dictation::voice_prompt::accepted_phrase_or(
            phrase,
            crate::dictation::stop_phrase::DEFAULT_PHRASE,
        );
}

/// Records the signature each normal dictation gets; invalid text reads as
/// the default, and empty stays empty so it appends nothing.
fn store_signature_text(signature: &str) {
    let signature = signature.trim();
    let accepted = if crate::dictation::signature::text_problem(signature).is_none() {
        signature
    } else {
        crate::dictation::signature::DEFAULT_TEXT
    };
    *SIGNATURE_TEXT
        .write()
        .unwrap_or_else(|error| error.into_inner()) = accepted.to_owned();
}

/// Whether the main window draws dark: the OS's appearance, unless Labs ›
/// Appearance is on and names one. Resolved for each view, so System keeps
/// following the OS live. Overlay windows (the HUD, capsules, live text) keep
/// following the OS themselves.
pub fn window_dark(labs: &LabsSettings, system_dark: bool) -> bool {
    if !Feature::Appearance.shipped() || !labs.appearance {
        return system_dark;
    }
    match labs.appearance_theme {
        AppearanceTheme::System => system_dark,
        AppearanceTheme::Light => false,
        AppearanceTheme::Dark => true,
    }
}

/// Records the switches the loaded or saved settings hold.
pub(crate) fn apply(settings: &LabsSettings) {
    // The switch and its window are two stores, so a gate read between them
    // sees Labs › Keep mic open after dictating off for one pass; the read
    // after both is right.
    STATE.store(encode(settings), Ordering::Release);
    MICROPHONE_LINGER_SECONDS.store(linger_seconds(settings), Ordering::Release);
    SCREEN_VOCABULARY.store(screen_vocabulary_enabled(settings), Ordering::Release);
    LEARN_FROM_CORRECTIONS.store(learn_from_corrections_enabled(settings), Ordering::Release);
    CURSOR_CONTEXT.store(cursor_context_enabled(settings), Ordering::Release);
    VOICE_PROMPT.store(voice_prompt_enabled(settings), Ordering::Release);
    STOP_PHRASE.store(stop_phrase_enabled(settings), Ordering::Release);
    SIGNATURE.store(signature_enabled(settings), Ordering::Release);
    COPY_CAPTURE_ZIP.store(copy_capture_zip_enabled(settings), Ordering::Release);
    COPY_DICTATION_AUDIO.store(copy_dictation_audio_enabled(settings), Ordering::Release);
    K_QUANT_MODELS.store(k_quant_models_enabled(settings), Ordering::Release);
    DROP_TRANSCRIBE.store(drop_transcribe_enabled(settings), Ordering::Release);
    CALENDAR_MENU.store(calendar_menu_enabled(settings), Ordering::Release);
    store_yolo_mode(settings);
    CLEAN_UP.store(clean_up_enabled(settings), Ordering::Release);
    NEUTRAL_VOICE.store(neutral_voice_enabled(settings), Ordering::Release);
    MICROPHONE_KEY.store(microphone_key_enabled(settings), Ordering::Release);
    PASTE_LAST_PICKER.store(paste_last_picker_enabled(settings), Ordering::Release);
    ANNOTATE_AGENT_TOOLS.store(annotate_agent_tools_enabled(settings), Ordering::Release);
    store_voice_prompt_phrase(&settings.voice_prompt_phrase);
    store_stop_phrase_text(&settings.stop_phrase_text);
    store_signature_text(&settings.signature_text);
    // Labs › Pause from menu bar: turning it off resumes at once, without
    // waiting for the menu to refresh; the menu row hides itself there.
    crate::shortcut_pause::reconcile(pause_menu());
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
            Feature::FasterLiveText,
            settings.faster_live_text,
            FASTER_LIVE_TEXT,
        ),
        (
            Feature::FailureNotices,
            settings.failure_notices,
            FAILURE_NOTICES,
        ),
        (Feature::LiveTyping, settings.live_typing, LIVE_TYPING),
        (Feature::LiveChapters, settings.live_chapters, LIVE_CHAPTERS),
        (Feature::Playground, settings.playground, PLAYGROUND),
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
        (Feature::ReturnToApp, settings.return_to_app, RETURN_TO_APP),
        (
            Feature::SpokenFormatting,
            settings.spoken_formatting,
            SPOKEN_FORMATTING,
        ),
        (Feature::FillerWords, settings.filler_words, FILLER_WORDS),
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
            Feature::DoubleTapModifiers,
            settings.double_tap_modifiers,
            DOUBLE_TAP_MODIFIERS,
        ),
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
            Feature::HoldThroughKeys,
            settings.hold_through_keys,
            HOLD_THROUGH_KEYS,
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
            Feature::VocabularyFiles,
            settings.vocabulary_files,
            VOCABULARY_FILES,
        ),
        (
            Feature::GnomeClipboardRestore,
            settings.gnome_clipboard_restore,
            GNOME_CLIPBOARD_RESTORE,
        ),
        (Feature::TypedText, settings.typed_text, TYPED_TEXT),
        (
            Feature::NotchIndicator,
            settings.notch_indicator,
            NOTCH_INDICATOR,
        ),
        (
            Feature::AnnotateReplay,
            settings.annotate_replay,
            ANNOTATE_REPLAY,
        ),
        (
            Feature::MeetingImport,
            settings.meeting_import,
            MEETING_IMPORT,
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
            Feature::OpenCodeInWsl,
            settings.opencode_in_wsl,
            OPENCODE_IN_WSL,
        ),
        (
            Feature::ApiProcessing,
            settings.api_processing,
            API_PROCESSING,
        ),
        (
            Feature::ApiTimestamps,
            settings.api_timestamps,
            API_TIMESTAMPS,
        ),
        (Feature::SpeakersOnly, settings.speakers_only, SPEAKERS_ONLY),
        (Feature::PauseMenu, settings.pause_menu, PAUSE_MENU),
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
        (
            Feature::AnnotatePointerScreen,
            settings.annotate_pointer_screen,
            ANNOTATE_POINTER_SCREEN,
        ),
        (Feature::Screenshots, settings.screenshots, SCREENSHOTS),
        (
            Feature::KeepMicrophoneOpen,
            settings.keep_microphone_open,
            KEEP_MICROPHONE_OPEN,
        ),
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
        screen_vocabulary = screen_vocabulary(),
        learn_from_corrections = learn_from_corrections(),
        cursor_context = cursor_context(),
        copy_dictation_audio = copy_dictation_audio(),
        hold_through_keys = hold_through_keys(),
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
        // Yearly stats is part of Dictation stats now.
        let error = resolve("yearly-stats", |_| true).unwrap_err().to_string();
        assert!(error.contains("history-stats"), "{error}");
        assert_eq!(Feature::from_name("yearly-stats"), None);
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
            Feature::ALL.len() + 1,
            "one switch per feature plus the saved per-app model opt-in"
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
        assert_eq!(
            Feature::for_settings_key("labs.appearance_theme"),
            Some(Feature::Appearance)
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
    fn keep_microphone_open_is_its_own_bit_and_carries_its_window() {
        let mut labs = LabsSettings::default();
        assert_eq!(
            labs.keep_microphone_open_seconds,
            LabsSettings::DEFAULT_KEEP_MICROPHONE_OPEN_SECONDS
        );
        assert_eq!(labs.keep_microphone_open_for(), None);
        let state = encode(&labs);
        assert_eq!(state & KEEP_MICROPHONE_OPEN, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 15);
        labs.keep_microphone_open = true;
        labs.keep_microphone_open_seconds = 180;
        labs.unload_idle_minutes = 30;
        let state = encode(&labs);
        assert_ne!(state & KEEP_MICROPHONE_OPEN, 0);
        assert_eq!(
            (state >> MINUTES_SHIFT) & MINUTES_MASK,
            30,
            "not the window"
        );
        assert_eq!(
            labs.keep_microphone_open_for(),
            Some(std::time::Duration::from_secs(180))
        );
        assert_eq!(linger_seconds(&labs), 180);
        // The lab's own seconds are a key of their own.
        assert_eq!(
            Feature::for_settings_key("labs.keep_microphone_open_seconds"),
            Some(Feature::KeepMicrophoneOpen)
        );
    }

    #[test]
    fn a_window_outside_the_choices_turns_the_linger_off() {
        let labs = LabsSettings {
            keep_microphone_open: true,
            keep_microphone_open_seconds: 45,
            ..LabsSettings::default()
        };
        assert_eq!(labs.keep_microphone_open_for(), None);
        assert_eq!(linger_seconds(&labs), 0);
    }

    #[test]
    fn voice_prompt_is_its_own_atomic_and_carries_its_phrase() {
        let labs = LabsSettings::default();
        assert!(!voice_prompt_enabled(&labs));
        assert_eq!(
            labs.voice_prompt_phrase,
            crate::dictation::voice_prompt::DEFAULT_PHRASE
        );
        let on = LabsSettings {
            voice_prompt: true,
            ..LabsSettings::default()
        };
        assert!(voice_prompt_enabled(&on));
        assert!(Feature::VoicePrompt.on_this_os());
        assert_eq!(Feature::VoicePrompt.added(), 20261112);
        assert_eq!(Feature::VoicePrompt.audience(), Audience::Everyone);
        assert_eq!(Feature::VoicePrompt.group(), Group::Output);
        assert_eq!(Feature::VoicePrompt.settings_key(), "labs.voice_prompt");
        assert_eq!(Feature::VoicePrompt.name(), "voice-prompt");
        assert_eq!(
            Feature::for_settings_key("labs.voice_prompt_phrase"),
            Some(Feature::VoicePrompt)
        );
    }

    #[test]
    fn stop_phrase_is_its_own_atomic_and_carries_its_phrase() {
        let labs = LabsSettings::default();
        assert!(!stop_phrase_enabled(&labs));
        assert_eq!(
            labs.stop_phrase_text,
            crate::dictation::stop_phrase::DEFAULT_PHRASE
        );
        let on = LabsSettings {
            stop_phrase: true,
            ..LabsSettings::default()
        };
        assert!(stop_phrase_enabled(&on));
        assert!(Feature::StopPhrase.on_this_os());
        assert_eq!(Feature::StopPhrase.added(), 20261116);
        assert_eq!(Feature::StopPhrase.audience(), Audience::Everyone);
        assert_eq!(Feature::StopPhrase.group(), Group::Dictation);
        assert_eq!(Feature::StopPhrase.settings_key(), "labs.stop_phrase");
        assert_eq!(Feature::StopPhrase.name(), "stop-phrase");
        assert_eq!(
            Feature::for_settings_key("labs.stop_phrase_text"),
            Some(Feature::StopPhrase)
        );
    }

    #[test]
    fn signature_is_its_own_atomic_and_carries_its_text() {
        let labs = LabsSettings::default();
        assert!(!signature_enabled(&labs));
        assert_eq!(
            labs.signature_text,
            crate::dictation::signature::DEFAULT_TEXT
        );
        let on = LabsSettings {
            signature: true,
            ..LabsSettings::default()
        };
        assert!(signature_enabled(&on));
        assert!(Feature::Signature.on_this_os());
        assert_eq!(Feature::Signature.added(), 20261121);
        assert_eq!(Feature::Signature.audience(), Audience::Everyone);
        assert_eq!(Feature::Signature.group(), Group::Output);
        assert_eq!(Feature::Signature.settings_key(), "labs.signature");
        assert_eq!(Feature::Signature.name(), "signature");
        assert_eq!(
            Feature::for_settings_key("labs.signature_text"),
            Some(Feature::Signature)
        );
    }

    #[test]
    fn the_window_follows_the_os_unless_appearance_forces_it() {
        let labs = LabsSettings::default();
        assert!(window_dark(&labs, true));
        assert!(!window_dark(&labs, false));
        let appearance = |theme| LabsSettings {
            appearance: true,
            appearance_theme: theme,
            ..LabsSettings::default()
        };
        assert!(window_dark(&appearance(AppearanceTheme::System), true));
        assert!(!window_dark(&appearance(AppearanceTheme::System), false));
        assert!(!window_dark(&appearance(AppearanceTheme::Light), true));
        assert!(window_dark(&appearance(AppearanceTheme::Dark), false));
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

    /// The cleanup lab's own atomic, its switch, and its macOS-only card: the
    /// state word is full, so it is not a bit.
    #[test]
    fn clean_up_is_its_own_atomic_and_macos_only() {
        let labs = LabsSettings::default();
        assert!(!clean_up_enabled(&labs));
        let on = LabsSettings {
            clean_up_with_apple_intelligence: true,
            ..LabsSettings::default()
        };
        assert!(clean_up_enabled(&on));
        assert_eq!(
            Feature::CleanUpWithAppleIntelligence.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::CleanUpWithAppleIntelligence.added(), 20261113);
        assert_eq!(
            Feature::CleanUpWithAppleIntelligence.audience(),
            Audience::Everyone
        );
        assert_eq!(Feature::CleanUpWithAppleIntelligence.group(), Group::Output);
        assert_eq!(
            Feature::CleanUpWithAppleIntelligence.settings_key(),
            "labs.clean_up_with_apple_intelligence"
        );
        assert_eq!(
            Feature::CleanUpWithAppleIntelligence.name(),
            "clean-up-apple-intelligence"
        );
    }

    /// The neutral voice lab's own atomic, its switch, and its macOS-only
    /// card: the state word is full, so it is not a bit.
    #[test]
    fn neutral_voice_is_its_own_atomic_and_macos_only() {
        let labs = LabsSettings::default();
        assert!(!neutral_voice_enabled(&labs));
        assert!(!Feature::NeutralVoice.on(&AppSettings::default()));
        let on = LabsSettings {
            neutral_voice: true,
            ..LabsSettings::default()
        };
        assert!(neutral_voice_enabled(&on));
        assert_eq!(
            Feature::NeutralVoice.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::NeutralVoice.added(), 20261120);
        assert_eq!(Feature::NeutralVoice.audience(), Audience::Everyone);
        assert_eq!(Feature::NeutralVoice.group(), Group::Output);
        assert_eq!(Feature::NeutralVoice.settings_key(), "labs.neutral_voice");
        assert_eq!(Feature::NeutralVoice.name(), "neutral-voice");
        assert_eq!(
            Feature::for_settings_key("labs.neutral_voice"),
            Some(Feature::NeutralVoice)
        );
    }

    #[test]
    fn smart_spacing_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode(&labs) & SMART_SPACING, 0);
        labs.smart_spacing = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode(&labs);
        assert_ne!(state & SMART_SPACING, 0);
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
    fn double_tap_modifiers_is_its_own_bit_and_macos_only() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & DOUBLE_TAP_MODIFIERS, 0);
        labs.double_tap_modifiers = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & DOUBLE_TAP_MODIFIERS, 0);
        assert_eq!(state & TAP_OR_HOLD, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::DoubleTapModifiers.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::DoubleTapModifiers.group(), Group::Shortcuts);
    }

    #[test]
    fn hold_through_keys_is_its_own_bit_and_macos_and_windows_only() {
        assert_eq!(HOLD_THROUGH_KEYS, 1 << 11);
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & HOLD_THROUGH_KEYS, 0);
        labs.hold_through_keys = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & HOLD_THROUGH_KEYS, 0);
        assert_eq!(state & SMART_SPACING, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::HoldThroughKeys.on_this_os(),
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
        assert!(!Feature::HoldThroughKeys.on(&AppSettings::default()));
        assert_eq!(Feature::HoldThroughKeys.group(), Group::Shortcuts);
        assert_eq!(Feature::HoldThroughKeys.added(), 20261116);
        assert_eq!(Feature::HoldThroughKeys.audience(), Audience::Everyone);
        assert_eq!(
            Feature::HoldThroughKeys.settings_key(),
            "labs.hold_through_keys"
        );
        assert_eq!(Feature::HoldThroughKeys.name(), "hold-through-keys");
        assert_eq!(
            Feature::for_settings_key("labs.hold_through_keys"),
            Some(Feature::HoldThroughKeys)
        );
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
    fn keep_recordings_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & KEEP_RECORDINGS, 0);
        labs.keep_recordings = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & KEEP_RECORDINGS, 0);
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
    fn screenshots_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & SCREENSHOTS, 0);
        labs.screenshots = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & SCREENSHOTS, 0);
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
    fn failure_notices_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & FAILURE_NOTICES, 0);
        labs.failure_notices = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & FAILURE_NOTICES, 0);
        assert_eq!(state & LIVE_TYPING, 0);
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
    fn opencode_in_wsl_is_its_own_bit_and_windows_only() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & OPENCODE_IN_WSL, 0);
        labs.opencode_in_wsl = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & OPENCODE_IN_WSL, 0);
        assert_eq!(state & CAPS_LOCK_SLOW_BLINK, 0);
        assert_eq!(state & TYPED_TEXT, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::OpenCodeInWsl.on_this_os(),
            cfg!(target_os = "windows")
        );
    }

    #[test]
    fn api_processing_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & API_PROCESSING, 0);
        labs.api_processing = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & API_PROCESSING, 0);
        assert_eq!(state & OPENCODE_IN_WSL, 0);
        assert_eq!(state & TYPED_TEXT, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert!(Feature::ApiProcessing.on_this_os());
    }

    #[test]
    fn api_timestamps_is_its_own_bit_and_off_by_default() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & API_TIMESTAMPS, 0);
        labs.api_timestamps = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & API_TIMESTAMPS, 0);
        assert_eq!(state & API_PROCESSING, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert!(Feature::ApiTimestamps.on_this_os());
        assert_eq!(Feature::ApiTimestamps.added(), 20261110);
        assert_eq!(Feature::ApiTimestamps.audience(), Audience::Everyone);
    }

    #[test]
    fn speakers_only_is_its_own_bit_and_off_by_default() {
        assert_eq!(SPEAKERS_ONLY, 1 << 55);
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & SPEAKERS_ONLY, 0);
        labs.speakers_only = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & SPEAKERS_ONLY, 0);
        assert_eq!(state & API_PROCESSING, 0);
        assert_eq!(state & DOUBLE_TAP_MODIFIERS, 0);
        assert_eq!(state & RETURN_TO_APP, 0);
        assert_eq!(state & TYPED_TEXT, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert!(Feature::SpeakersOnly.on_this_os());
        assert_eq!(Feature::SpeakersOnly.added(), 20261109);
        assert_eq!(Feature::SpeakersOnly.audience(), Audience::Everyone);
    }

    #[test]
    fn pause_menu_is_its_own_bit_and_macos_only() {
        assert_eq!(PAUSE_MENU, 1 << 57);
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & PAUSE_MENU, 0);
        labs.pause_menu = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & PAUSE_MENU, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(Feature::PauseMenu.on_this_os(), cfg!(target_os = "macos"));
        assert_eq!(Feature::PauseMenu.added(), 20261110);
        assert_eq!(Feature::PauseMenu.audience(), Audience::Everyone);
        assert_eq!(Feature::PauseMenu.group(), Group::Shortcuts);
        assert_eq!(Feature::PauseMenu.settings_key(), "labs.pause_menu");
        assert_eq!(Feature::PauseMenu.name(), "pause-menu");
    }

    #[test]
    fn calendar_menu_is_its_own_switch_and_macos_only() {
        let mut labs = LabsSettings::default();
        assert!(!calendar_menu_enabled(&labs));
        labs.calendar_menu = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(calendar_menu_enabled(&labs));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & PAUSE_MENU, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::CalendarMenu.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::CalendarMenu.added(), 20261114);
        assert_eq!(Feature::CalendarMenu.audience(), Audience::Everyone);
        assert_eq!(Feature::CalendarMenu.group(), Group::Meetings);
        assert_eq!(Feature::CalendarMenu.settings_key(), "labs.calendar_menu");
        assert_eq!(Feature::CalendarMenu.name(), "calendar-menu");
    }

    #[test]
    fn calendar_select_all_is_its_own_switch_and_macos_only() {
        let mut settings = AppSettings::default();
        assert!(!Feature::CalendarSelectAll.on(&settings));
        settings.labs.calendar_bulk_select = true;
        settings.labs.unload_idle_minutes = 24 * 60;
        assert!(Feature::CalendarSelectAll.on(&settings));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&settings.labs, |_| true);
        assert_eq!(state & PAUSE_MENU, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::CalendarSelectAll.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::CalendarSelectAll.added(), 20261118);
        assert_eq!(Feature::CalendarSelectAll.audience(), Audience::Everyone);
        assert_eq!(Feature::CalendarSelectAll.group(), Group::Meetings);
        assert_eq!(
            Feature::CalendarSelectAll.settings_key(),
            "labs.calendar_bulk_select"
        );
        assert_eq!(Feature::CalendarSelectAll.name(), "calendar-select-all");
    }

    #[test]
    fn screen_vocabulary_is_its_own_switch_and_macos_only() {
        let mut labs = LabsSettings::default();
        assert!(!screen_vocabulary_enabled(&labs));
        labs.screen_vocabulary = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(screen_vocabulary_enabled(&labs));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & VOCABULARY_FILES, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::ScreenVocabulary.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::ScreenVocabulary.added(), 20261112);
        assert_eq!(Feature::ScreenVocabulary.audience(), Audience::Everyone);
        assert_eq!(Feature::ScreenVocabulary.group(), Group::Dictation);
        assert_eq!(
            Feature::ScreenVocabulary.settings_key(),
            "labs.screen_vocabulary"
        );
        assert_eq!(Feature::ScreenVocabulary.name(), "screen-vocabulary");
    }

    #[test]
    fn learn_from_corrections_is_its_own_switch_and_macos_only() {
        let mut labs = LabsSettings::default();
        assert!(!learn_from_corrections_enabled(&labs));
        labs.learn_from_corrections = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(learn_from_corrections_enabled(&labs));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & SPEAKERS_ONLY, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::LearnFromCorrections.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::LearnFromCorrections.added(), 20261121);
        assert_eq!(Feature::LearnFromCorrections.audience(), Audience::Everyone);
        assert_eq!(Feature::LearnFromCorrections.group(), Group::Dictation);
        assert_eq!(
            Feature::LearnFromCorrections.settings_key(),
            "labs.learn_from_corrections"
        );
        assert_eq!(
            Feature::LearnFromCorrections.name(),
            "learn-from-corrections"
        );
    }

    #[test]
    fn cursor_context_is_its_own_switch_and_macos_only() {
        let mut labs = LabsSettings::default();
        assert!(!cursor_context_enabled(&labs));
        labs.cursor_context = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(cursor_context_enabled(&labs));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & SMART_SPACING, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::CursorContext.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::CursorContext.added(), 20261113);
        assert_eq!(Feature::CursorContext.audience(), Audience::Everyone);
        assert_eq!(Feature::CursorContext.group(), Group::Output);
        assert_eq!(Feature::CursorContext.settings_key(), "labs.cursor_context");
        assert_eq!(Feature::CursorContext.name(), "cursor-context");
    }

    #[test]
    fn copy_capture_zip_is_its_own_switch_and_everywhere() {
        let mut labs = LabsSettings::default();
        assert!(!copy_capture_zip_enabled(&labs));
        labs.copy_capture_zip = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(copy_capture_zip_enabled(&labs));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & SCREENSHOTS, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::CopyCaptureZip.on_this_os(),
            cfg!(any(
                target_os = "macos",
                target_os = "linux",
                target_os = "windows"
            ))
        );
        assert_eq!(Feature::CopyCaptureZip.added(), 20261112);
        assert_eq!(Feature::CopyCaptureZip.audience(), Audience::Everyone);
        assert_eq!(Feature::CopyCaptureZip.group(), Group::Annotate);
        assert_eq!(
            Feature::CopyCaptureZip.settings_key(),
            "labs.copy_capture_zip"
        );
        assert_eq!(Feature::CopyCaptureZip.name(), "copy-capture-zip");
    }

    #[test]
    fn copy_dictation_audio_is_its_own_switch_and_everywhere() {
        let mut labs = LabsSettings::default();
        assert!(!copy_dictation_audio_enabled(&labs));
        labs.copy_dictation_audio = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(copy_dictation_audio_enabled(&labs));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & KEEP_RECORDINGS, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::CopyDictationAudio.on_this_os(),
            cfg!(any(
                target_os = "macos",
                target_os = "linux",
                target_os = "windows"
            ))
        );
        assert_eq!(Feature::CopyDictationAudio.added(), 20261115);
        assert_eq!(Feature::CopyDictationAudio.audience(), Audience::Everyone);
        assert_eq!(Feature::CopyDictationAudio.group(), Group::History);
        assert_eq!(
            Feature::CopyDictationAudio.settings_key(),
            "labs.copy_dictation_audio"
        );
        assert_eq!(Feature::CopyDictationAudio.name(), "copy-dictation-audio");
    }

    #[test]
    fn smaller_model_files_is_its_own_switch_and_macos_only() {
        let mut labs = LabsSettings::default();
        assert!(!k_quant_models_enabled(&labs));
        labs.k_quant_models = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(k_quant_models_enabled(&labs));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & PARAKEET_ULTRA, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::KQuantModels.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::KQuantModels.added(), 20261113);
        assert_eq!(Feature::KQuantModels.audience(), Audience::Everyone);
        assert_eq!(Feature::KQuantModels.group(), Group::Models);
        assert_eq!(Feature::KQuantModels.settings_key(), "labs.k_quant_models");
        assert_eq!(Feature::KQuantModels.name(), "k-quant-models");
    }

    #[test]
    fn paste_last_picker_is_its_own_switch_and_macos_only() {
        let mut labs = LabsSettings::default();
        assert!(!paste_last_picker_enabled(&labs));
        labs.paste_last_picker = true;
        labs.unload_idle_minutes = 24 * 60;
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & KEEP_RECORDINGS, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(
            Feature::PasteLastPicker.on_this_os(),
            cfg!(target_os = "macos")
        );
        assert_eq!(Feature::PasteLastPicker.added(), 20261119);
        assert_eq!(Feature::PasteLastPicker.audience(), Audience::Dev);
        assert_eq!(Feature::PasteLastPicker.group(), Group::History);
        assert_eq!(
            Feature::PasteLastPicker.settings_key(),
            "labs.paste_last_picker"
        );
        assert_eq!(Feature::PasteLastPicker.name(), "paste-last-picker");
    }

    #[test]
    fn drop_transcribe_is_its_own_switch_and_everywhere() {
        let mut labs = LabsSettings::default();
        assert!(!drop_transcribe_enabled(&labs));
        labs.drop_transcribe = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(drop_transcribe_enabled(&labs));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & PARAKEET_ULTRA, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert!(
            Feature::DropTranscribe.on_this_os(),
            "the window exists on every OS"
        );
        assert_eq!(Feature::DropTranscribe.added(), 20261113);
        assert_eq!(Feature::DropTranscribe.audience(), Audience::Everyone);
        assert_eq!(Feature::DropTranscribe.group(), Group::Models);
        assert_eq!(
            Feature::DropTranscribe.settings_key(),
            "labs.drop_transcribe"
        );
        assert_eq!(Feature::DropTranscribe.name(), "drop-transcribe");
    }

    #[test]
    fn yolo_mode_is_its_own_switch_and_everywhere() {
        let mut labs = LabsSettings::default();
        assert!(!yolo_mode_enabled(&labs));
        labs.yolo_mode = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(yolo_mode_enabled(&labs));
        assert_eq!(
            Feature::for_settings_key("labs.yolo_mode_apps"),
            Some(Feature::YoloMode)
        );
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & COPY_ONLY, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert!(Feature::YoloMode.on_this_os());
        assert_eq!(Feature::YoloMode.added(), 20261113);
        assert_eq!(Feature::YoloMode.audience(), Audience::Everyone);
        assert_eq!(Feature::YoloMode.group(), Group::Output);
        assert_eq!(Feature::YoloMode.settings_key(), "labs.yolo_mode");
        assert_eq!(Feature::YoloMode.name(), "yolo-mode");
    }

    #[test]
    fn keeping_the_clipboard_is_everywhere_and_copy_only_is_macos_or_windows() {
        assert!(Feature::KeepOnClipboard.on_this_os());
        assert_eq!(
            Feature::CopyOnly.on_this_os(),
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
    }

    #[test]
    fn return_to_app_is_its_own_bit_and_macos_only() {
        let mut labs = LabsSettings::default();
        assert_eq!(encode_where(&labs, |_| true) & RETURN_TO_APP, 0);
        labs.return_to_app = true;
        labs.unload_idle_minutes = 24 * 60;
        let state = encode_where(&labs, |_| true);
        assert_ne!(state & RETURN_TO_APP, 0);
        assert_eq!(state & DOUBLE_TAP_MODIFIERS, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert_eq!(Feature::ReturnToApp.on_this_os(), cfg!(target_os = "macos"));
        assert_eq!(Feature::ReturnToApp.audience(), Audience::Everyone);
        assert_eq!(Feature::ReturnToApp.group(), Group::Output);
    }

    #[test]
    fn terminal_paste_shortcut_is_windows_or_linux_only() {
        assert_eq!(
            Feature::TerminalPasteShortcut.on_this_os(),
            cfg!(any(target_os = "windows", target_os = "linux"))
        );
    }

    #[test]
    fn vocabulary_files_is_its_own_switch_and_everywhere() {
        let mut labs = LabsSettings::default();
        assert!(!Feature::VocabularyFiles.on(&AppSettings::default()));
        assert_eq!(encode(&labs) & VOCABULARY_FILES, 0);
        labs.vocabulary_files = true;
        let settings = AppSettings {
            labs,
            ..AppSettings::default()
        };
        assert!(Feature::VocabularyFiles.on(&settings));
        assert_ne!(encode(&settings.labs) & VOCABULARY_FILES, 0);
        assert!(Feature::VocabularyFiles.on_this_os());
        assert_eq!(Feature::VocabularyFiles.added(), 20261120);
        assert_eq!(Feature::VocabularyFiles.audience(), Audience::Everyone);
        assert_eq!(Feature::VocabularyFiles.group(), Group::Dictation);
        assert_eq!(
            Feature::VocabularyFiles.settings_key(),
            "labs.vocabulary_files"
        );
        assert_eq!(Feature::VocabularyFiles.name(), "vocabulary-files");
        assert_eq!(
            Feature::VocabularyFiles.oses(),
            &[Os::Macos, Os::Windows, Os::Linux]
        );
    }

    #[test]
    fn agent_tools_for_captures_is_its_own_switch_and_everywhere() {
        let mut labs = LabsSettings::default();
        assert!(!annotate_agent_tools_enabled(&labs));
        assert!(!Feature::AnnotateAgentTools.on(&AppSettings::default()));
        labs.annotate_agent_tools = true;
        labs.unload_idle_minutes = 24 * 60;
        assert!(annotate_agent_tools_enabled(&labs));
        // The state word has no bit for it: the other switches keep theirs.
        let state = encode_where(&labs, |_| true);
        assert_eq!(state & SCREENSHOTS, 0);
        assert_eq!(state & ANNOTATE_REPLAY, 0);
        assert_eq!(state & COPY_ONLY, 0);
        assert_eq!((state >> MINUTES_SHIFT) & MINUTES_MASK, 24 * 60);
        assert!(Feature::AnnotateAgentTools.on_this_os());
        assert_eq!(Feature::AnnotateAgentTools.added(), 20261121);
        assert_eq!(Feature::AnnotateAgentTools.audience(), Audience::Everyone);
        assert_eq!(Feature::AnnotateAgentTools.group(), Group::Annotate);
        assert_eq!(
            Feature::AnnotateAgentTools.settings_key(),
            "labs.annotate_agent_tools"
        );
        assert_eq!(Feature::AnnotateAgentTools.name(), "annotate-agent-tools");
        assert_eq!(
            Feature::AnnotateAgentTools.oses(),
            &[Os::Macos, Os::Windows, Os::Linux]
        );
        assert!(!Feature::AnnotateAgentTools.title().is_empty());
        assert!(!Feature::AnnotateAgentTools.summary().is_empty());
        assert!(!Feature::AnnotateAgentTools.description().is_empty());
    }

    #[test]
    fn retired_switches_are_read_and_ignored() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"labs": {"enabled": true, "collapsible_meeting_sections": true, "meeting_platform_icons": true, "transcribe_files": true, "restore_shortcut_default": true, "follow_live_transcript": true, "more_call_apps": true, "meeting_notes_vocabulary": true, "meeting_finished_event": true, "subtitle_export": true, "movable_meeting_capsules": true, "live_text": true}}"#,
        )
        .unwrap();
        assert!(settings.labs.live_text);
        assert!(!settings.labs.playground);
        let json = serde_json::to_value(&settings).unwrap();
        assert!(json["labs"].get("enabled").is_none());
        assert!(json["labs"].get("collapsible_meeting_sections").is_none());
        assert!(json["labs"].get("meeting_platform_icons").is_none());
        assert!(json["labs"].get("transcribe_files").is_none());
        assert!(json["labs"].get("restore_shortcut_default").is_none());
        for graduated in [
            "follow_live_transcript",
            "more_call_apps",
            "meeting_notes_vocabulary",
            "meeting_finished_event",
            "subtitle_export",
            "movable_meeting_capsules",
        ] {
            assert!(json["labs"].get(graduated).is_none(), "{graduated}");
        }
    }
}
