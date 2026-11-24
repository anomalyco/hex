//! The Labs registry: every feature this build can have, in the order
//! Settings lists them, with its settings field, CLI name, group, audience,
//! OS availability, and join date in one table so none of them can drift.

use super::{Audience, Group, Os};
use crate::app_settings::{AppSettings, LabsSettings};

const MACOS: &[Os] = &[Os::Macos];
const WINDOWS: &[Os] = &[Os::Windows];
const LINUX: &[Os] = &[Os::Linux];
const MACOS_WINDOWS: &[Os] = &[Os::Macos, Os::Windows];
const MACOS_LINUX: &[Os] = &[Os::Macos, Os::Linux];
const WINDOWS_LINUX: &[Os] = &[Os::Windows, Os::Linux];
const EVERY: &[Os] = &[Os::Macos, Os::Windows, Os::Linux];

struct Descriptor {
    name: &'static str,
    settings_key: &'static str,
    extra_keys: &'static [&'static str],
    group: Group,
    audience: Audience,
    oses: &'static [Os],
    added: u32,
}

macro_rules! field_read {
    ($settings:expr, labs($field:ident)) => {
        $settings.labs.$field
    };
    ($settings:expr, app($($seg:ident).+)) => {
        $settings.$($seg).+
    };
}

macro_rules! field_read_labs {
    ($labs:expr, labs($field:ident)) => {
        $labs.$field
    };
    ($labs:expr, app($($seg:ident).+)) => {
        false
    };
}

macro_rules! field_write {
    ($settings:expr, $on:expr, labs($field:ident)) => {
        $settings.labs.$field = $on
    };
    ($settings:expr, $on:expr, app($($seg:ident).+)) => {
        $settings.$($seg).+ = $on
    };
}

macro_rules! field_key {
    (labs($field:ident)) => {
        concat!("labs.", stringify!($field))
    };
    (app($head:ident . $tail:ident)) => {
        concat!(stringify!($head), ".", stringify!($tail))
    };
    (app($only:ident)) => {
        stringify!($only)
    };
}

macro_rules! define_features {
    (
        $(
            $variant:ident => $storage:ident($($path:ident).+),
            $name:literal,
            $group:ident,
            $audience:ident,
            $oses:ident,
            $added:literal
            $(, extra: [$($extra:literal),* $(,)?])?
            ;
        )*
    ) => {
        /// One Labs feature.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Feature {
            $($variant,)*
        }

        const DESCRIPTORS: &[Descriptor] = &[
            $(
                Descriptor {
                    name: $name,
                    settings_key: field_key!($storage($($path).+)),
                    extra_keys: &[$($($extra),*)?],
                    group: Group::$group,
                    audience: Audience::$audience,
                    oses: $oses,
                    added: $added,
                },
            )*
        ];

        impl Feature {
            pub const ALL: [Self; DESCRIPTORS.len()] = [$(Self::$variant,)*];

            const fn descriptor(self) -> &'static Descriptor {
                &DESCRIPTORS[self as usize]
            }

            /// Whether `settings` turn it on.
            pub fn on(self, settings: &AppSettings) -> bool {
                match self {
                    $(Self::$variant => field_read!(settings, $storage($($path).+)),)*
                }
            }

            /// Whether `labs` turn it on (features whose switch lives outside
            /// `LabsSettings` read false here and do not use the runtime bank).
            pub(super) fn on_labs(self, labs: &LabsSettings) -> bool {
                match self {
                    $(Self::$variant => field_read_labs!(labs, $storage($($path).+)),)*
                }
            }

            /// Sets this feature's raw switch field without running `set`'s
            /// shortcut-repair side effects (for `crate::policy` enforcement).
            pub(crate) fn set_field(self, settings: &mut AppSettings, on: bool) {
                match self {
                    $(Self::$variant => field_write!(settings, on, $storage($($path).+)),)*
                }
            }

            /// The feature-specific managed lock that governs this feature
            /// (in addition to `Lock::Labs`, which locks every Labs feature).
            pub const fn lock(self) -> Option<crate::policy::Lock> {
                match self {
                    Self::Meetings => Some(crate::policy::Lock::Meetings),
                    Self::VoicePrompt => Some(crate::policy::Lock::OpenCodeProcessing),
                    _ if matches!(self.group(), Group::Annotate) => {
                        Some(crate::policy::Lock::Annotate)
                    }
                    _ => None,
                }
            }
        }
    };
}

define_features! {
    Meetings => app(meetings.enabled), "meetings", Meetings, Everyone, EVERY, 20261030;
    VoiceFilter => app(experimental.hex_microphone), "voice-filter", Dictation, Dev, MACOS_LINUX, 20261030;
    Playground => labs(playground), "playground", Dictation, Dev, EVERY, 20261030;
    LiveText => labs(live_text), "live-text", Dictation, Everyone, EVERY, 20261030;
    FasterLiveText => labs(faster_live_text), "faster-live-text", Dictation, Everyone, EVERY, 20261120;
    FailureNotices => labs(failure_notices), "failure-notices", Dictation, Everyone, EVERY, 20261107;
    LiveTyping => labs(live_typing), "live-typing", Dictation, Everyone, LINUX, 20261107;
    LiveChapters => labs(live_chapters), "live-chapters", Meetings, Everyone, EVERY, 20261030;
    ScreenVocabulary => labs(screen_vocabulary), "screen-vocabulary", Dictation, Everyone, MACOS, 20261112;
    LearnFromCorrections => labs(learn_from_corrections), "learn-from-corrections", Dictation, Everyone, MACOS, 20261121;
    TerminalPaste => app(paste_with_shift), "terminal-paste", Output, Everyone, WINDOWS, 20261030;
    KeyboardModelSwitching => app(keyboard_model_switching.enabled), "keyboard-model-switching", Models, Everyone, EVERY, 20261030;
    ModelSwitchShortcut => labs(model_switch_shortcut), "model-switch-shortcut", Models, Everyone, EVERY, 20261124;
    MouseShortcuts => labs(mouse_shortcuts), "mouse-shortcuts", Shortcuts, Everyone, MACOS, 20261030;
    PauseBrowser => app(pause_browser_media), "pause-browser", Dictation, Everyone, MACOS, 20261030;
    ShareShortcut => app(share_dictation_shortcut), "share-shortcut", Shortcuts, Everyone, WINDOWS, 20261030;
    UnloadIdle => labs(unload_idle), "unload-idle", Models, Everyone, EVERY, 20261031, extra: ["labs.unload_idle_minutes"];
    KeepMicrophoneOpen => labs(keep_microphone_open), "keep-microphone-open", Dictation, Everyone, EVERY, 20261111, extra: ["labs.keep_microphone_open_seconds"];
    KeepOnClipboard => labs(keep_on_clipboard), "keep-on-clipboard", Output, Everyone, EVERY, 20261031;
    CopyOnly => labs(copy_only), "copy-only", Output, Everyone, MACOS_WINDOWS, 20261031;
    ReturnToApp => labs(return_to_app), "return-to-app", Output, Everyone, MACOS, 20261109;
    SpokenFormatting => labs(spoken_formatting), "spoken-formatting", Output, Everyone, EVERY, 20261031;
    FillerWords => labs(filler_words), "filler-words", Output, Everyone, EVERY, 20261031;
    CleanUpWithAppleIntelligence => labs(clean_up_with_apple_intelligence), "clean-up-apple-intelligence", Output, Everyone, MACOS, 20261113;
    NeutralVoice => labs(neutral_voice), "neutral-voice", Output, Everyone, MACOS, 20261120;
    SecondDictationShortcut => labs(second_dictation_shortcut), "second-dictation-shortcut", Shortcuts, Everyone, EVERY, 20261031;
    TerminalPasteShortcut => labs(terminal_paste_shortcut), "terminal-paste-shortcut", Shortcuts, Everyone, WINDOWS_LINUX, 20261108;
    SmartSpacing => labs(smart_spacing), "smart-spacing", Output, Everyone, MACOS, 20261031;
    SpokenSymbols => labs(spoken_symbols), "spoken-symbols", Output, Everyone, EVERY, 20261031;
    PickerSortFilter => labs(picker_sort_filter), "picker-sort-filter", Models, Everyone, EVERY, 20261031;
    SidedChords => labs(sided_modifier_chords), "sided-modifier-chords", Shortcuts, Everyone, MACOS, 20261031;
    TapOrHold => labs(tap_or_hold), "tap-or-hold", Shortcuts, Everyone, MACOS_LINUX, 20261031;
    DoubleTapModifiers => labs(double_tap_modifiers), "double-tap-modifiers", Shortcuts, Everyone, MACOS, 20261109;
    BenchmarkDetails => labs(benchmark_details), "benchmark-details", Models, Everyone, EVERY, 20261031;
    ParakeetUltra => labs(parakeet_ultra), "parakeet-ultra", Models, Everyone, EVERY, 20261101;
    StandaloneSpecialKeys => labs(standalone_special_keys), "special-keys", Shortcuts, Everyone, WINDOWS_LINUX, 20261101;
    HistoryStats => labs(history_stats), "history-stats", History, Everyone, EVERY, 20261101;
    HistoryStatsStrip => labs(history_stats_strip), "history-stats-strip", History, Everyone, EVERY, 20261123;
    RecentDictations => labs(recent_dictations), "recent-dictations", History, Everyone, MACOS, 20261101;
    SkipPauses => labs(skip_pauses), "skip-pauses", Dictation, Everyone, WINDOWS_LINUX, 20261101;
    OutputFixes => labs(output_fixes), "output-fixes", Output, Everyone, EVERY, 20261102;
    ModelInPlace => labs(model_in_place), "model-in-place", Models, Everyone, EVERY, 20261102;
    KQuantModels => labs(k_quant_models), "k-quant-models", Models, Everyone, MACOS, 20261113;
    PickerDownloaded => labs(picker_downloaded), "picker-downloaded", Models, Everyone, EVERY, 20261102;
    SectionKeyAlone => labs(section_key_alone), "section-key", Shortcuts, Everyone, MACOS_LINUX, 20261102;
    MicrophoneKeyAlone => labs(microphone_key), "microphone-key", Shortcuts, Everyone, MACOS, 20261115;
    KeepRecordings => labs(keep_recordings), "keep-recordings", History, Everyone, EVERY, 20261102;
    Appearance => labs(appearance), "appearance", Dictation, Everyone, EVERY, 20261109, extra: ["labs.appearance_theme"];
    HideMenuBarIcon => labs(hide_menu_bar_icon), "hide-menu-bar-icon", Dictation, Everyone, MACOS, 20261102;
    GnomeClipboardRestore => labs(gnome_clipboard_restore), "gnome-clipboard-restore", Output, Everyone, LINUX, 20261103;
    TypedText => labs(typed_text), "typed-text", Output, Everyone, LINUX, 20261107;
    NotchIndicator => labs(notch_indicator), "notch-indicator", Dictation, Everyone, MACOS, 20261103;
    GnomeMeetingCapsules => labs(gnome_meeting_capsules), "gnome-meeting-capsules", Meetings, Everyone, LINUX, 20261104;
    GnomeAnnotate => labs(gnome_annotate), "gnome-annotate", Annotate, Everyone, LINUX, 20261108;
    AnnotatePointerScreen => labs(annotate_pointer_screen), "annotate-pointer-screen", Annotate, Everyone, LINUX, 20261120;
    Screenshots => labs(screenshots), "screenshots", Annotate, Everyone, MACOS, 20261104;
    AnnotateReplay => labs(annotate_replay), "annotate-replay", Annotate, Everyone, MACOS, 20261104, extra: ["capture.replay_hotkey"];
    MeetingImport => labs(meeting_import), "meeting-import", Meetings, Everyone, MACOS, 20261106;
    TranscriptEdits => labs(transcript_edits), "transcript-edits", Meetings, Everyone, EVERY, 20261123;
    CapsLockDictation => labs(caps_lock_dictation), "caps-lock-dictation", Shortcuts, Everyone, MACOS, 20261107, extra: ["labs.caps_lock_light"];
    OpenCodeInWsl => labs(opencode_in_wsl), "opencode-in-wsl", Output, Everyone, WINDOWS, 20261108;
    ApiProcessing => labs(api_processing), "api-processing", Output, Everyone, EVERY, 20261109;
    ApiTimestamps => labs(api_timestamps), "api-timestamps", Output, Everyone, EVERY, 20261110;
    SpeakersOnly => labs(speakers_only), "speakers-only", Dictation, Everyone, EVERY, 20261109;
    CalendarMenu => labs(calendar_menu), "calendar-menu", Meetings, Everyone, MACOS, 20261114;
    CalendarSelectAll => labs(calendar_bulk_select), "calendar-select-all", Meetings, Everyone, MACOS, 20261118;
    MoreDictationShortcuts => labs(more_dictation_shortcuts), "more-dictation-shortcuts", Shortcuts, Everyone, MACOS, 20261109;
    PauseMenu => labs(pause_menu), "pause-menu", Shortcuts, Everyone, MACOS, 20261110;
    VoicePrompt => labs(voice_prompt), "voice-prompt", Output, Everyone, EVERY, 20261112, extra: ["labs.voice_prompt_phrase"];
    CopyCaptureZip => labs(copy_capture_zip), "copy-capture-zip", Annotate, Everyone, EVERY, 20261112;
    YoloMode => labs(yolo_mode), "yolo-mode", Output, Everyone, EVERY, 20261113, extra: ["labs.yolo_mode_apps"];
    CursorContext => labs(cursor_context), "cursor-context", Output, Everyone, MACOS, 20261113;
    CopyDictationAudio => labs(copy_dictation_audio), "copy-dictation-audio", History, Everyone, EVERY, 20261115;
    StopPhrase => labs(stop_phrase), "stop-phrase", Dictation, Everyone, EVERY, 20261116, extra: ["labs.stop_phrase_text"];
    HoldThroughKeys => labs(hold_through_keys), "hold-through-keys", Shortcuts, Everyone, MACOS_WINDOWS, 20261116;
    ModesEditor => labs(modes_editor), "modes-editor", Output, Everyone, EVERY, 20261118;
    DropTranscribe => labs(drop_transcribe), "drop-transcribe", Models, Everyone, EVERY, 20261113;
    PasteLastPicker => labs(paste_last_picker), "paste-last-picker", History, Dev, MACOS, 20261119;
    VocabularyFiles => labs(vocabulary_files), "vocabulary-files", Dictation, Everyone, EVERY, 20261120;
    VocabularyMenu => labs(vocabulary_menu), "vocabulary-menu", Dictation, Everyone, MACOS, 20261123;
    AnnotateAgentTools => labs(annotate_agent_tools), "annotate-agent-tools", Annotate, Everyone, EVERY, 20261121;
    AnnotateAgentInstall => labs(annotate_agent_install), "annotate-agent-install", Annotate, Everyone, EVERY, 20261121;
    Signature => labs(signature), "signature", Output, Everyone, EVERY, 20261121, extra: ["labs.signature_text"];
    DictationStyle => labs(dictation_style), "dictation-style", Output, Everyone, EVERY, 20261122, extra: ["labs.dictation_style_choice"];
    HudHeight => labs(hud_height), "hud-height", Dictation, Everyone, MACOS, 20261122, extra: ["labs.hud_bottom_gap"];
    VoiceIsolation => labs(voice_isolation), "voice-isolation", Dictation, Everyone, MACOS, 20261122;
    DictateWhileAnnotating => labs(dictate_while_annotating), "dictate-while-annotating", Annotate, Everyone, EVERY, 20261122;
    StopAnyCallRecording => labs(stop_any_call_recording), "stop-any-call-recording", Meetings, Everyone, EVERY, 20261123;
    ClipboardPhrase => labs(clipboard_phrase), "clipboard-phrase", Output, Everyone, EVERY, 20261124, extra: ["labs.clipboard_phrase_text"];
    MeetingMicrophoneMute => labs(meeting_microphone_mute), "meeting-microphone-mute", Meetings, Everyone, MACOS, 20261124;
    MeetingIndicatorDisplay => labs(meeting_indicator_display), "meeting-indicator-display", Meetings, Everyone, MACOS_WINDOWS, 20261124;
    MeetingAudioSource => labs(meeting_audio_source), "meeting-audio-source", Meetings, Everyone, EVERY, 20261124, extra: ["labs.meeting_audio_source_choice"];
}

impl Feature {
    /// Who the feature is for. Every feature picks one.
    pub const fn audience(self) -> Audience {
        self.descriptor().audience
    }

    /// The group Labs lists it under. Every feature picks one.
    pub const fn group(self) -> Group {
        self.descriptor().group
    }

    /// The day it joined Labs, as `yyyymmdd`. Labs marks the newest as New.
    pub const fn added(self) -> u32 {
        self.descriptor().added
    }

    /// The OSes that have the feature, in the order Hex's website names
    /// them. Linux's keyboard-language switching needs an X11 session.
    pub const fn oses(self) -> &'static [Os] {
        self.descriptor().oses
    }

    /// The name `hex labs on|off` takes.
    pub const fn name(self) -> &'static str {
        self.descriptor().name
    }

    /// Its `hex settings` key.
    pub const fn settings_key(self) -> &'static str {
        self.descriptor().settings_key
    }

    /// Companion `hex settings` keys that belong to this feature.
    #[cfg(test)]
    pub const fn extra_settings_keys(self) -> &'static [&'static str] {
        self.descriptor().extra_keys
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|feature| feature.name() == name)
    }

    /// The feature `hex settings` key `key` switches (its toggle, or a
    /// companion key such as Unload when idle's minutes).
    pub fn for_settings_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|feature| {
            let info = feature.descriptor();
            info.settings_key == key || info.extra_keys.contains(&key)
        })
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

    pub fn set(self, settings: &mut AppSettings, on: bool) {
        self.set_field(settings, on);
        self.apply_side_effects(settings);
    }

    /// Runs any setting normalization or shortcut repair implied by this
    /// feature's current value in `settings`. Shared by [`Self::set`] and
    /// `hex settings set`.
    pub(crate) fn apply_side_effects(self, settings: &mut AppSettings) {
        match self {
            Self::DoubleTapModifiers => {
                // Off, a modifier-only shortcut can't be double-tap-only, so
                // it falls back as a fresh load would.
                settings.normalize_double_tap_settings();
            }
            Self::Screenshots =>
            {
                #[cfg(hex_capture)]
                if self.on(settings) {
                    settings.clear_record_shortcut_sharing_capture();
                }
            }
            // Off, no shortcut keeps a key only the feature allows.
            Self::StandaloneSpecialKeys => settings.repair_special_keys(),
            Self::SectionKeyAlone => settings.repair_section_key(),
            Self::MicrophoneKeyAlone => settings.repair_microphone_key(),
            _ => {}
        }
    }
}
