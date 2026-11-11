//! The persisted `labs` switches (`crate::labs`).

use super::*;

/// How the Caps Lock light shows while a capture records (Labs › Caps Lock
/// dictation, macOS).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsLockLight {
    /// On for the whole recording.
    #[default]
    Steady,
    /// On and off every 700 ms while recording, slower than the
    /// transcribing blink.
    SlowBlink,
}

impl CapsLockLight {
    /// Every choice, in the order the window shows them.
    pub const ALL: [Self; 2] = [Self::Steady, Self::SlowBlink];

    /// The English label the window shows.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Steady => "Steady",
            Self::SlowBlink => "Slow blink",
        }
    }
}

/// The main window's appearance: the OS's, or one forced on it (Labs ›
/// Appearance).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceTheme {
    /// Follow the OS.
    #[default]
    System,
    /// Always light, whatever the OS shows.
    Light,
    /// Always dark, whatever the OS shows.
    Dark,
}

impl AppearanceTheme {
    /// Every choice, in the order the window shows them.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// The English label the window shows.
    pub const fn name(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light mode",
            Self::Dark => "Dark mode",
        }
    }
}

/// The `labs` switches (`crate::labs`), each off by default; older Labs features keep their own keys, and retired switches (`labs.enabled`, `labs.collapsible_meeting_sections`, `labs.meeting_platform_icons`) are ignored.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LabsSettings {
    /// Live text beside the HUD while dictating.
    pub live_text: bool,
    /// A failed dictation's explanation beside the HUD for a few seconds.
    pub failure_notices: bool,
    /// Provisional words in the focused text field as a Wayland input-method
    /// preedit, with the final text committed there (Linux Wayland).
    pub live_typing: bool,
    pub live_chapters: bool,
    pub playground: bool,
    /// Parakeet transcription biased toward the vocabulary.
    pub vocabulary_biasing: bool,
    /// Mouse buttons may be recorded as shortcuts (macOS, issue 209).
    pub mouse_shortcuts: bool,
    /// The engine unloads the speech model after `unload_idle_minutes` (5, 15 default, 30, 60) unused (issue 442).
    pub unload_idle: bool,
    pub unload_idle_minutes: u32,
    /// Paste last dictation's row offers its default shortcut back (issue 613).
    pub restore_shortcut_default: bool,
    /// A pasted dictation stays on the clipboard, not the previous one (macOS and Windows, issue 200).
    pub keep_on_clipboard: bool,
    /// A dictation is put on the clipboard and never pasted (macOS and Windows, issue 629).
    pub copy_only: bool,
    /// A dictation pastes into the app its capture started in, bringing it
    /// back to the front if another app came forward meanwhile (macOS).
    pub return_to_app: bool,
    /// Spoken "new line", "new paragraph", and "open/close quote" become line breaks and quotes (issue 630).
    pub spoken_formatting: bool,
    /// Filler words such as "um" and "uh" are removed from dictations (issue 631).
    pub filler_words: bool,
    /// `hex meetings show --srt|--vtt` exports a transcript as timed subtitles (macOS, issue 633).
    pub subtitle_export: bool,
    /// `second_dictation_hotkey` dictates like the dictation shortcut (issue 627).
    pub second_dictation_shortcut: bool,
    /// Up to four more dictation shortcuts, each with its own activation
    /// mode (`extra_dictation_hotkeys`, macOS).
    pub more_dictation_shortcuts: bool,
    /// `terminal_paste_hotkey` dictates like the dictation shortcut and always pastes
    /// with Ctrl+Shift+V (Windows, Linux).
    pub terminal_paste_shortcut: bool,
    /// A paste gets a leading space when the character before the cursor wants one (macOS, issue 632).
    pub smart_spacing: bool,
    /// Spoken symbol words ("comma", "hashtag", "underscore") become characters joined to their words (issue 678).
    pub spoken_symbols: bool,
    /// The Local transcription picker offers a sort and a "Supports the saved language" toggle (issue 645).
    pub picker_sort_filter: bool,
    /// The recorder keeps each modifier's side in a modifier-only chord, such as Right ⌘ + Right ⌥ (macOS, issue 710).
    pub sided_modifier_chords: bool,
    /// In Toggle mode, a press held past the hold threshold dictates until it lifts (macOS, issue 711).
    pub tap_or_hold: bool,
    /// Double-tap-only accepts a dictation shortcut without a key: one or
    /// more modifiers, such as Option, Globe, or ⌃⌥ (macOS).
    pub double_tap_modifiers: bool,
    /// The picker shows each model's raw error rate, benchmark, source, and conditions (issue 647).
    pub benchmark_details: bool,
    /// Parakeet Ultra joins the Local transcription picker (issue 648).
    pub parakeet_ultra: bool,
    /// Menu, Pause, and Scroll Lock may be shortcut keys (Linux, issue 767).
    pub standalone_special_keys: bool,
    pub history_stats: bool,
    /// History selects several dictations at once, with Shift, Command (Ctrl
    /// on Windows and Linux), or a drag, to copy or delete them together.
    pub history_multi_select: bool,
    pub recent_dictations: bool,
    pub skip_pauses: bool,
    /// An automatic meeting recording stops after its call ends (macOS, issue 835).
    pub stop_when_call_ends: bool,
    /// Output's Try card offers Vocabulary and Fix this beside its result (issue 989).
    pub output_fixes: bool,
    /// The speech model picker lists the model in use first (issue 1019).
    pub picker_active_first: bool,
    /// The picker's "Use model file where it is…" adds a GGUF file without copying it (issue 941).
    pub model_in_place: bool,
    /// The picker's All speech models view offers "Only downloaded models" (issue 1018).
    pub picker_downloaded: bool,
    /// The ISO § key may be a shortcut on its own (Linux, issue 1050).
    pub section_key_alone: bool,
    /// Dictations saved to History keep their audio, for Transcribe again (issue 1032).
    pub keep_recordings: bool,
    /// General's pane starts with a search field that filters its rows.
    pub settings_search: bool,
    /// The sidebar footer links this build's release notes.
    pub release_notes_link: bool,
    pub hide_menu_bar_icon: bool,
    /// ⌘Q closes the Settings window instead of quitting Hex (macOS).
    pub quit_closes_window: bool,
    /// A Caps Lock press starts and stops dictation, and its light follows
    /// Hex's recording and processing (macOS).
    pub caps_lock_dictation: bool,
    /// Whether that light stays solid or blinks slowly while recording
    /// (macOS).
    pub caps_lock_light: CapsLockLight,
    /// The main window is forced light or dark instead of following the OS
    /// (Labs › Appearance).
    pub appearance: bool,
    /// Which appearance the window shows while `appearance` is on.
    pub appearance_theme: AppearanceTheme,
    pub movable_meeting_capsules: bool,
    /// A paste restores the clipboard through a running XWayland on Wayland desktops without data-control, such as GNOME (Linux, issue 1111).
    pub gnome_clipboard_restore: bool,
    /// A shortcut runs the focused app's Output steps on the text already
    /// typed there and replaces it (`typed_text_hotkey`, unbound by default).
    pub typed_text: bool,
    /// Meeting titles and summaries read the vocabulary (issue 1154).
    pub meeting_notes_vocabulary: bool,
    /// At either visible indicator position, the HUD grows out of the notch on a screen with one (macOS).
    pub notch_indicator: bool,
    /// The local API's `GET /meetings/events` tells a subscriber each finished meeting's id (issue 964).
    pub meeting_finished_event: bool,
    /// The meeting capsules float on a running XWayland on Wayland desktops without layer-shell, such as GNOME (Linux, issue 1274).
    pub gnome_meeting_capsules: bool,
    /// Annotate's capsule and ink float on a running XWayland on Wayland desktops without layer-shell, such as GNOME (Linux).
    pub gnome_annotate: bool,
    /// The screenshot shortcut takes an area or a window (macOS); its
    /// shortcut and folder are `screenshots.*`.
    pub screenshots: bool,
    /// A recording meeting's transcript scrolls to its newest line, until scrolled up (issue 1332).
    pub follow_live_transcript: bool,
    /// Meetings offers Import audio…: a local audio file becomes a meeting (macOS).
    pub meeting_import: bool,
    /// Notes or a rename rename an exported meeting's Markdown to the meeting's title, keeping its date and time.
    pub meeting_descriptive_filenames: bool,
    /// A meeting's notes are written to their own file beside the transcript,
    /// sharing its name and linking back to it.
    pub meeting_notes_file: bool,
    /// Finished meetings drop their WAV tracks on `meetings.audio_retention`,
    /// keeping their transcripts, notes, and summary.
    pub expire_meeting_audio: bool,
    /// With `labs.annotate_replay` on, keeps a rolling 15 s display buffer via `SCClipBufferingOutput` (macOS 27+).
    pub annotate_replay: bool,
    /// Calls in more apps (Signal, Telegram, WeChat, LINE, Webex, Element, Phone) and on more websites are offered and named.
    pub more_call_apps: bool,
    /// An OpenCode service running in the default WSL distribution is used
    /// for Modes rewriting and the Modes model picker when no Windows
    /// service answers (Windows).
    pub opencode_in_wsl: bool,
    /// `hex transcribe <file>` turns a WAV recording into text with the
    /// dictation model.
    pub transcribe_files: bool,
    /// The local API's `process=global` returns transcripts after Global
    /// mode's output processing.
    pub api_processing: bool,
    /// The local API's `timestamps=segment|word` returns the speech model's
    /// own segment and word times.
    pub api_timestamps: bool,
    /// "While dictating" applies only while the default output is speakers;
    /// headphones are left alone.
    pub speakers_only: bool,
    /// A dictation mode's speech model and language follow the foreground
    /// application (macOS and Windows).
    pub app_model_switching: bool,
    /// The menu bar offers Pause Hex, which makes every capture shortcut
    /// inert until Resume (macOS).
    pub pause_menu: bool,
    /// Settings › Labs' groups opened or closed by hand, by `labs::Group`
    /// id; a group not here is open while it holds a lab that's on.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub groups: std::collections::BTreeMap<String, bool>,
}

/// The idle times Labs › Unload model when idle offers, in minutes.
pub const UNLOAD_IDLE_MINUTES: [u32; 4] = [5, 15, 30, 60];

impl Default for LabsSettings {
    fn default() -> Self {
        Self {
            live_text: false,
            failure_notices: false,
            live_typing: false,
            live_chapters: false,
            playground: false,
            vocabulary_biasing: false,
            mouse_shortcuts: false,
            unload_idle: false,
            unload_idle_minutes: 15,
            restore_shortcut_default: false,
            keep_on_clipboard: false,
            copy_only: false,
            return_to_app: false,
            spoken_formatting: false,
            filler_words: false,
            subtitle_export: false,
            second_dictation_shortcut: false,
            more_dictation_shortcuts: false,
            terminal_paste_shortcut: false,
            smart_spacing: false,
            spoken_symbols: false,
            picker_sort_filter: false,
            sided_modifier_chords: false,
            tap_or_hold: false,
            double_tap_modifiers: false,
            benchmark_details: false,
            parakeet_ultra: false,
            standalone_special_keys: false,
            history_stats: false,
            history_multi_select: false,
            recent_dictations: false,
            skip_pauses: false,
            stop_when_call_ends: false,
            output_fixes: false,
            picker_active_first: false,
            model_in_place: false,
            picker_downloaded: false,
            section_key_alone: false,
            keep_recordings: false,
            settings_search: false,
            release_notes_link: false,
            hide_menu_bar_icon: false,
            quit_closes_window: false,
            caps_lock_dictation: false,
            caps_lock_light: CapsLockLight::Steady,
            appearance: false,
            appearance_theme: AppearanceTheme::System,
            movable_meeting_capsules: false,
            gnome_clipboard_restore: false,
            typed_text: false,
            meeting_notes_vocabulary: false,
            notch_indicator: false,
            meeting_finished_event: false,
            annotate_replay: false,
            more_call_apps: false,
            opencode_in_wsl: false,
            transcribe_files: false,
            api_processing: false,
            api_timestamps: false,
            speakers_only: false,
            app_model_switching: false,
            pause_menu: false,
            gnome_meeting_capsules: false,
            gnome_annotate: false,
            screenshots: false,
            follow_live_transcript: false,
            meeting_import: false,
            meeting_descriptive_filenames: false,
            meeting_notes_file: false,
            expire_meeting_audio: false,
            groups: std::collections::BTreeMap::new(),
        }
    }
}
