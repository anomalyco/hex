//! The persisted `labs` switches (`crate::labs`).

use super::*;

/// The `labs` switches (`crate::labs`), each off by default; older Labs features keep their own keys, and retired switches (`labs.enabled`, `labs.collapsible_meeting_sections`, `labs.meeting_platform_icons`) are ignored.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LabsSettings {
    /// Live text beside the HUD while dictating.
    pub live_text: bool,
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
    /// A pasted dictation stays on the clipboard, not the previous one (macOS, issue 200).
    pub keep_on_clipboard: bool,
    /// A dictation is put on the clipboard and never pasted (macOS, issue 629).
    pub copy_only: bool,
    /// Spoken "new line", "new paragraph", and "open/close quote" become line breaks and quotes (issue 630).
    pub spoken_formatting: bool,
    /// Filler words such as "um" and "uh" are removed from dictations (issue 631).
    pub filler_words: bool,
    /// `hex meetings show --srt|--vtt` exports a transcript as timed subtitles (macOS, issue 633).
    pub subtitle_export: bool,
    /// `second_dictation_hotkey` dictates like the dictation shortcut (macOS, issue 627).
    pub second_dictation_shortcut: bool,
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
    /// The picker shows each model's raw error rate, benchmark, source, and conditions (issue 647).
    pub benchmark_details: bool,
    /// Parakeet Ultra joins the Local transcription picker (issue 648).
    pub parakeet_ultra: bool,
    /// Menu, Pause, and Scroll Lock may be shortcut keys (Linux, issue 767).
    pub standalone_special_keys: bool,
    pub history_stats: bool,
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
    pub hide_menu_bar_icon: bool,
    pub movable_meeting_capsules: bool,
    /// A paste restores the clipboard through a running XWayland on Wayland desktops without data-control, such as GNOME (Linux, issue 1111).
    pub gnome_clipboard_restore: bool,
    /// Meeting titles and summaries read the vocabulary (issue 1154).
    pub meeting_notes_vocabulary: bool,
    /// With the indicator at the top, the HUD grows out of the notch on a screen with one (macOS).
    pub notch_indicator: bool,
    /// The local API's `GET /meetings/events` tells a subscriber each finished meeting's id (issue 964).
    pub meeting_finished_event: bool,
    /// The meeting capsules float on a running XWayland on Wayland desktops without layer-shell, such as GNOME (Linux, issue 1274).
    pub gnome_meeting_capsules: bool,
    /// The screenshot shortcut takes an area or a window (macOS); its
    /// shortcut and folder are `screenshots.*`.
    pub screenshots: bool,
    /// A recording meeting's transcript scrolls to its newest line, until scrolled up (issue 1332).
    pub follow_live_transcript: bool,
    /// With `labs.annotate_replay` on, keeps a rolling 15 s display buffer via `SCClipBufferingOutput` (macOS 27+).
    pub annotate_replay: bool,
    /// Calls in more apps (Signal, Telegram, WeChat, LINE, Webex, Element, Phone) and on more websites are offered and named.
    pub more_call_apps: bool,
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
            live_chapters: false,
            playground: false,
            vocabulary_biasing: false,
            mouse_shortcuts: false,
            unload_idle: false,
            unload_idle_minutes: 15,
            restore_shortcut_default: false,
            keep_on_clipboard: false,
            copy_only: false,
            spoken_formatting: false,
            filler_words: false,
            subtitle_export: false,
            second_dictation_shortcut: false,
            smart_spacing: false,
            spoken_symbols: false,
            picker_sort_filter: false,
            sided_modifier_chords: false,
            tap_or_hold: false,
            benchmark_details: false,
            parakeet_ultra: false,
            standalone_special_keys: false,
            history_stats: false,
            recent_dictations: false,
            skip_pauses: false,
            stop_when_call_ends: false,
            output_fixes: false,
            picker_active_first: false,
            model_in_place: false,
            picker_downloaded: false,
            section_key_alone: false,
            keep_recordings: false,
            hide_menu_bar_icon: false,
            movable_meeting_capsules: false,
            gnome_clipboard_restore: false,
            meeting_notes_vocabulary: false,
            notch_indicator: false,
            meeting_finished_event: false,
            annotate_replay: false,
            more_call_apps: false,
            gnome_meeting_capsules: false,
            screenshots: false,
            follow_live_transcript: false,
            groups: std::collections::BTreeMap::new(),
        }
    }
}
