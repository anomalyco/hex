//! The `hex settings` key registry: every key, its kind, the retired names,
//! and the capability each key needs.

use crate::app_settings::HotkeySlot;

#[derive(Clone, Copy, Debug)]
pub(super) enum Kind {
    Bool,
    Number {
        min: f64,
        max: f64,
    },
    #[cfg_attr(not(hex_capture), allow(dead_code))]
    Integer {
        min: u64,
        max: u64,
    },
    Choice(&'static [&'static str]),
    Text,
    Shortcut(HotkeySlot),
    Path,
    Json,
}

impl Kind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Number { .. } => "number",
            Self::Integer { .. } => "integer",
            Self::Choice(_) => "choice",
            Self::Text => "text",
            Self::Shortcut(_) => "shortcut",
            Self::Path => "path",
            Self::Json => "json",
        }
    }
}

pub(super) struct Key {
    pub(super) key: &'static str,
    pub(super) kind: Kind,
    /// `none` clears it.
    pub(super) optional: bool,
    /// Why it can't be set here, if it can't.
    pub(super) read_only: Option<&'static str>,
    pub(super) about: &'static str,
}

const fn key(key: &'static str, kind: Kind, about: &'static str) -> Key {
    Key {
        key,
        kind,
        optional: false,
        read_only: None,
        about,
    }
}

const fn optional(key: &'static str, kind: Kind, about: &'static str) -> Key {
    Key {
        key,
        kind,
        optional: true,
        read_only: None,
        about,
    }
}

const fn read_only(key: &'static str, kind: Kind, about: &'static str, why: &'static str) -> Key {
    Key {
        key,
        kind,
        optional: false,
        read_only: Some(why),
        about,
    }
}

const UNIT: Kind = Kind::Number { min: 0.0, max: 1.0 };
const MODEL_CHANGE: &str = "change the transcription model with `hex model use`";

/// Choice keys whose values are numbers in settings.json.
pub(super) const NUMERIC_CHOICES: [&str; 2] =
    ["meetings.calendar.lead_minutes", "labs.unload_idle_minutes"];

pub(super) const KEYS: &[Key] = &[
    key(
        "keyboard_model_switching.enabled",
        Kind::Bool,
        "Labs: switch the model when the keyboard input source changes (macOS, Windows, X11)",
    ),
    key(
        "keyboard_model_switching.mappings",
        Kind::Json,
        "Input-source IDs mapped to a model and language; absent sources don't switch (macOS, Windows, X11)",
    ),
    key(
        "labs.app_model_switching",
        Kind::Bool,
        "Labs: switch the speech model and language with the foreground application (macOS, Windows)",
    ),
    key(
        "release_microphone_while_idle",
        Kind::Bool,
        "Release the microphone between dictations",
    ),
    key(
        "sound_effects",
        Kind::Bool,
        "Play recording and error tones",
    ),
    key(
        "sound_effect_volume",
        UNIT,
        "Tone volume from 0 to 1; 0 turns tones off",
    ),
    key(
        "sound_theme",
        Kind::Choice(&[
            "signature",
            "classic",
            "click",
            "soft",
            "wood",
            "analog",
            "thock",
        ]),
        "Which sounds play; classic, Hex's original tones, is the default",
    ),
    optional(
        "microphone",
        Kind::Text,
        "Microphone by name; system-default (or none) follows the system's default input",
    ),
    key(
        "avoid_bluetooth_microphone",
        Kind::Bool,
        "System default records from the built-in or another wired microphone, not a Bluetooth headset's",
    ),
    key(
        "recording_audio_behavior",
        Kind::Choice(&["mute", "pause_media", "do_nothing", "duck"]),
        "What other audio does while you dictate",
    ),
    key(
        "recording_duck_volume",
        UNIT,
        "Fraction of original output volume while lowering audio, from 0 to 1",
    ),
    key(
        "pause_browser_media",
        Kind::Bool,
        "Labs: also pause a playing browser tab while dictating (macOS, with Pause media)",
    ),
    key(
        "double_tap_lock",
        Kind::Bool,
        "Double-tap the dictation shortcut to keep recording hands-free",
    ),
    key(
        "double_tap_only",
        Kind::Bool,
        "Only a double tap starts dictation (needs double_tap_lock and a key chord)",
    ),
    key(
        "shortcut_mode",
        Kind::Choice(&["hold", "toggle"]),
        "Hold the shortcut while speaking, or press once to start and again to finish",
    ),
    key(
        "dictation_hotkey",
        Kind::Shortcut(HotkeySlot::Dictation),
        "The dictation shortcut",
    ),
    optional(
        "paste_last_hotkey",
        Kind::Shortcut(HotkeySlot::PasteLast),
        "The paste-last-dictation shortcut; none turns it off",
    ),
    key(
        "cancel_hotkey",
        Kind::Shortcut(HotkeySlot::Cancel),
        "Toggle mode's cancel shortcut, Escape by default: discards the recording, \
         nothing pasted or saved. Hex takes the key only while a toggled recording runs",
    ),
    optional(
        "raw_dictation_hotkey",
        Kind::Shortcut(HotkeySlot::RawDictation),
        "Dictate without processing: dictates and pastes the words as heard, skipping \
         the Output steps; none (the default) turns it off",
    ),
    optional(
        "second_dictation_hotkey",
        Kind::Shortcut(HotkeySlot::SecondDictation),
        "Labs: another shortcut that dictates like dictation_hotkey, such as a key on an \
         external keyboard (with labs.second_dictation_shortcut; Linux and Windows need a key); \
         none (the default) turns it off",
    ),
    key(
        "extra_dictation_hotkeys",
        Kind::Json,
        "Labs: up to four more dictation shortcuts as a JSON array of \
         {\"binding\": ..., \"mode\": ...} rows, each with its own activation mode \
         (macOS, with labs.more_dictation_shortcuts); an empty array (the default) has none",
    ),
    optional(
        "typed_text_hotkey",
        Kind::Shortcut(HotkeySlot::TypedText),
        "Labs: the shortcut that runs the focused app's Output steps on the text already \
         typed in the focused field and replaces it (Linux, with labs.typed_text); none \
         (the default) turns it off",
    ),
    optional(
        "terminal_paste_hotkey",
        Kind::Shortcut(HotkeySlot::TerminalPaste),
        "Labs: another shortcut that dictates like dictation_hotkey and always pastes with \
         Ctrl+Shift+V (Windows, Linux, with labs.terminal_paste_shortcut); none (the default) \
         turns it off",
    ),
    key(
        "screenshots.hotkey",
        Kind::Shortcut(HotkeySlot::Screenshot),
        "Labs: the screenshot shortcut, command+shift+4 by default: drag an area, or \
         press space to pick a window (macOS, with labs.screenshots)",
    ),
    optional(
        "screenshots.record_hotkey",
        Kind::Shortcut(HotkeySlot::ScreenshotRecord),
        "Labs: records an area or a window, picked like a screenshot, and press it again \
         to stop; none (the default) turns it off (macOS, with labs.screenshots and \
         capture.enabled)",
    ),
    optional(
        "screenshots.folder",
        Kind::Path,
        "Labs: folder screenshots are saved to; none uses the Desktop (macOS)",
    ),
    key(
        "show_dock_icon",
        Kind::Bool,
        "Keep Hex in the Dock when Settings is closed (macOS)",
    ),
    read_only(
        "transcription.model",
        Kind::Text,
        "The dictation model",
        MODEL_CHANGE,
    ),
    read_only(
        "transcription.language",
        Kind::Text,
        "The dictation language",
        MODEL_CHANGE,
    ),
    read_only(
        "transcription_recents",
        Kind::Json,
        "Recently used models, each with its language",
        "Hex records these as you switch models",
    ),
    key(
        "vocabulary_boost",
        Kind::Choice(&["off", "light", "normal", "strong"]),
        "How readily transcription swaps heard words for a vocabulary term",
    ),
    key(
        "dictation_processing.default_mode",
        Kind::Json,
        "The Global mode: replacements, transformations, and rewriting",
    ),
    key(
        "dictation_processing.modes",
        Kind::Json,
        "Modes for specific apps and websites, as a JSON array",
    ),
    key(
        "history_retention",
        Kind::Choice(&["off", "day", "week", "month", "forever"]),
        "How long dictation history is kept",
    ),
    key(
        "labs.live_text",
        Kind::Bool,
        "Labs: provisional words beside the HUD while dictating",
    ),
    key(
        "labs.failure_notices",
        Kind::Bool,
        "Labs: when a dictation fails, show why for a few seconds beside the HUD",
    ),
    key(
        "labs.live_typing",
        Kind::Bool,
        "Labs: provisional words in the focused text field as an underlined input-method preedit, and the final text typed there instead of pasted (Linux Wayland)",
    ),
    key(
        "labs.settings_search",
        Kind::Bool,
        "Labs: a search field at the top of General that filters its rows",
    ),
    key(
        "labs.release_notes_link",
        Kind::Bool,
        "Labs: a link to this build's release notes in the sidebar footer",
    ),
    key(
        "labs.live_chapters",
        Kind::Bool,
        "Labs: chapter meeting transcripts while recording",
    ),
    key(
        "labs.playground",
        Kind::Bool,
        "Labs: the scene and story browser in the window",
    ),
    key(
        "labs.vocabulary_biasing",
        Kind::Bool,
        "Labs: bias Parakeet transcription toward the vocabulary",
    ),
    key(
        "labs.mouse_shortcuts",
        Kind::Bool,
        "Labs: record mouse buttons as shortcuts (macOS)",
    ),
    key(
        "labs.unload_idle",
        Kind::Bool,
        "Labs: unload the speech model when idle, freeing its memory",
    ),
    key(
        "labs.unload_idle_minutes",
        Kind::Choice(&["5", "15", "30", "60"]),
        "Minutes unused before Labs › Unload model when idle unloads the model",
    ),
    key(
        "labs.restore_shortcut_default",
        Kind::Bool,
        "Labs: offer Restore default beside the Paste last dictation shortcut",
    ),
    key(
        "labs.keep_on_clipboard",
        Kind::Bool,
        "Labs: leave the dictation on the clipboard after pasting instead of restoring what was there",
    ),
    key(
        "labs.copy_only",
        Kind::Bool,
        "Labs: put the dictation on the clipboard instead of pasting it (macOS and Windows)",
    ),
    key(
        "labs.return_to_app",
        Kind::Bool,
        "Labs: switch back to the app a dictation started in before pasting (macOS)",
    ),
    key(
        "labs.spoken_formatting",
        Kind::Bool,
        "Labs: turn spoken \"new line\", \"new paragraph\", and \"open/close quote\" into line breaks and quotation marks",
    ),
    key(
        "labs.spoken_symbols",
        Kind::Bool,
        "Labs: turn spoken \"comma\", \"colon\", \"hashtag\", \"underscore\", and similar words into symbols joined to their words",
    ),
    key(
        "labs.filler_words",
        Kind::Bool,
        "Labs: remove filler words such as um and uh from dictations, without an AI model",
    ),
    key(
        "labs.subtitle_export",
        Kind::Bool,
        "Labs: export meeting transcripts as SRT or VTT subtitles with hex meetings show --srt|--vtt",
    ),
    key(
        "labs.second_dictation_shortcut",
        Kind::Bool,
        "Labs: second_dictation_hotkey dictates like the dictation shortcut",
    ),
    key(
        "labs.more_dictation_shortcuts",
        Kind::Bool,
        "Labs: up to four more dictation shortcuts, each with its own binding and activation \
         mode (extra_dictation_hotkeys, macOS)",
    ),
    key(
        "labs.terminal_paste_shortcut",
        Kind::Bool,
        "Labs: terminal_paste_hotkey dictates like the dictation shortcut and always pastes \
         with Ctrl+Shift+V (Windows, Linux)",
    ),
    key(
        "labs.opencode_in_wsl",
        Kind::Bool,
        "Labs: use an OpenCode service running in the default WSL distribution for Modes \
         rewriting and the Modes model picker; looking for it can start WSL (Windows)",
    ),
    key(
        "labs.smart_spacing",
        Kind::Bool,
        "Labs: add a space before a dictation pasted right after a word (macOS)",
    ),
    key(
        "labs.picker_sort_filter",
        Kind::Bool,
        "Labs: sort the Local transcription picker by recommendation, size, or name, and show only models for the saved language",
    ),
    key(
        "labs.sided_modifier_chords",
        Kind::Bool,
        "Labs: the shortcut recorder keeps which side each modifier of a modifier-only chord is on, such as Right Command + Right Option (macOS)",
    ),
    key(
        "labs.tap_or_hold",
        Kind::Bool,
        "Labs: in Toggle mode, a press held past the hold threshold dictates until it lifts; a short tap still starts and stops (macOS)",
    ),
    key(
        "labs.double_tap_modifiers",
        Kind::Bool,
        "Labs: Double-tap shortcut mode accepts a shortcut without a key, one or more \
         modifiers such as Option, Globe, or ⌃⌥ (macOS)",
    ),
    key(
        "labs.benchmark_details",
        Kind::Bool,
        "Labs: the Local transcription picker shows each model's raw error rate (WER or CER), benchmark, source, and conditions",
    ),
    key(
        "labs.parakeet_ultra",
        Kind::Bool,
        "Labs: Parakeet Ultra, Moondream's post-trained Parakeet v3, in the Local transcription picker; stays on while it is the dictation model",
    ),
    key(
        "labs.standalone_special_keys",
        Kind::Bool,
        "Labs: shortcuts may use Menu, Pause, or Scroll Lock, alone or with modifiers (Linux); off puts such a shortcut back to its default",
    ),
    key(
        "labs.section_key_alone",
        Kind::Bool,
        "Labs: a shortcut may be the key left of 1 on ISO keyboards, § or a dead key such as ^, \
         with no modifier (Linux, macOS); off puts such a shortcut back to its default",
    ),
    key(
        "labs.keep_recordings",
        Kind::Bool,
        "Labs: dictations saved to History keep their audio, so History can transcribe them again with another model; off stops saving, clearing History deletes it",
    ),
    key(
        "labs.history_stats",
        Kind::Bool,
        "Labs: a summary of retained dictations at the top of History, and hex history stats",
    ),
    key(
        "labs.history_multi_select",
        Kind::Bool,
        "Labs: select several dictations in History with Shift-click, Cmd-click (Ctrl on Windows and Linux), or a drag, then copy or delete them together",
    ),
    key(
        "labs.recent_dictations",
        Kind::Bool,
        "Labs: a Copy Recent Dictation submenu in the menu bar that copies one of the last five dictations (macOS)",
    ),
    key(
        "labs.hide_menu_bar_icon",
        Kind::Bool,
        "Labs: hide Hex's menu bar icon while nothing records; shortcuts keep working, and opening Hex again from Finder or Spotlight shows Settings (macOS)",
    ),
    key(
        "labs.quit_closes_window",
        Kind::Bool,
        "Labs: the app menu's Quit Hex (⌘Q) closes the Settings window instead of quitting Hex; dictation keeps running, and the menu bar icon still quits (macOS)",
    ),
    key(
        "labs.caps_lock_dictation",
        Kind::Bool,
        "Labs: a Caps Lock press starts and stops dictation hands-free, and its light shows when Hex records and transcribes; Caps Lock stays off so typing is unchanged (macOS)",
    ),
    key(
        "labs.caps_lock_light",
        Kind::Choice(&["steady", "slow_blink"]),
        "Labs: whether the Caps Lock light stays solid or blinks slowly while recording (macOS)",
    ),
    key(
        "labs.appearance",
        Kind::Bool,
        "Labs: force the main window to light or dark instead of following the system appearance",
    ),
    key(
        "labs.appearance_theme",
        Kind::Choice(&["system", "light", "dark"]),
        "Labs: the appearance the main window uses while Labs › Appearance is on",
    ),
    key(
        "labs.notch_indicator",
        Kind::Bool,
        "Labs: on a screen with a notch, the recording indicator grows out of the notch at either visible position; Hidden still hides it, and flat screens keep the Top or Bottom setting (macOS)",
    ),
    key(
        "labs.skip_pauses",
        Kind::Bool,
        "Labs: a dictation transcribed as a whole clip hears only the start and end of each long pause, so slow computers paste sooner (Linux and Windows)",
    ),
    key(
        "labs.stop_when_call_ends",
        Kind::Bool,
        "Labs: a meeting recording Hex started on its own stops 30 s after its call app leaves the microphone and goes quiet, unless you keep recording (macOS)",
    ),
    key(
        "labs.more_call_apps",
        Kind::Bool,
        "Labs: offer to record calls in Signal, Telegram, WeChat, LINE, Webex, Element and the Phone app, and name calls on Google Voice, Messenger, Teams, FaceTime, GroupMe, and Webex pages",
    ),
    key(
        "labs.movable_meeting_capsules",
        Kind::Bool,
        "Labs: drag meeting capsules such as Meeting transcript ready out of the way; they show in the corner you left them, shared with the recording capsule (macOS)",
    ),
    key(
        "labs.meeting_notes_vocabulary",
        Kind::Bool,
        "Labs: final meeting titles and summaries read the vocabulary, so names are spelled the way you wrote them",
    ),
    key(
        "labs.meeting_finished_event",
        Kind::Bool,
        "Labs: the local API's GET /meetings/events tells one subscriber each finished meeting's id once its final transcript is written; no transcript, audio, path, or title",
    ),
    key(
        "labs.screenshots",
        Kind::Bool,
        "Labs: screenshots.hotkey (command+shift+4) drags out an area or, after space, picks a window; the PNG is copied and saved to screenshots.folder (macOS)",
    ),
    key(
        "labs.annotate_replay",
        Kind::Bool,
        "Labs: keep a rolling 15-second screen buffer so hex capture replay or capture.replay_hotkey saves what just happened; macOS shows the screen-recording dot the whole time it's on (macOS 27+)",
    ),
    key(
        "labs.gnome_clipboard_restore",
        Kind::Bool,
        "Labs: on Wayland desktops without data-control, such as GNOME, a paste restores the previous clipboard through XWayland while it is already running (Linux)",
    ),
    key(
        "labs.typed_text",
        Kind::Bool,
        "Labs: Output shows a Typed text section whose shortcut runs the focused app's Output steps on the text already typed in the focused field and replaces it (Linux)",
    ),
    key(
        "labs.gnome_meeting_capsules",
        Kind::Bool,
        "Labs: on Wayland desktops without layer-shell, such as GNOME, the meeting capsules float over the call through XWayland while it is already running (Linux)",
    ),
    key(
        "labs.gnome_annotate",
        Kind::Bool,
        "Labs: on Wayland desktops without layer-shell, such as GNOME, Annotate's capsule and ink float over the screen through XWayland while it is already running; Esc can't stop drawing there, so use the capsule's pen button or let go of a held Annotate key (Linux)",
    ),
    key(
        "labs.follow_live_transcript",
        Kind::Bool,
        "Labs: while a meeting records, its transcript in Meetings scrolls to the newest line; scroll up to read earlier text, back to the bottom to follow again",
    ),
    key(
        "labs.meeting_import",
        Kind::Bool,
        "Labs: Meetings offers Import audio…, so a local audio file is transcribed and kept as a meeting (macOS)",
    ),
    key(
        "labs.meeting_descriptive_filenames",
        Kind::Bool,
        "Labs: an exported meeting's Markdown follows its title once notes or a rename provide one, keeping the date and time",
    ),
    key(
        "labs.meeting_notes_file",
        Kind::Bool,
        "Labs: a meeting's notes are written to their own file beside the transcript, sharing its name and linking back to it",
    ),
    key(
        "labs.expire_meeting_audio",
        Kind::Bool,
        "Labs: finished meetings drop their WAVs on meetings.audio_retention while their transcripts, notes, summary, and Markdown stay",
    ),
    key(
        "labs.output_fixes",
        Kind::Bool,
        "Labs: Output's Try card offers Vocabulary and Fix this, which copies an agent prompt naming the wrong result",
    ),
    key(
        "labs.picker_active_first",
        Kind::Bool,
        "Labs: the speech model picker lists the model in use first",
    ),
    key(
        "labs.model_in_place",
        Kind::Bool,
        "Labs: the speech model picker's Use model file where it is… adds a GGUF file without copying it, so several accounts can share one file",
    ),
    key(
        "labs.picker_downloaded",
        Kind::Bool,
        "Labs: the speech model picker's All speech models view offers Only downloaded models, with the space they take",
    ),
    key(
        "labs.transcribe_files",
        Kind::Bool,
        "Labs: hex transcribe <file> turns a WAV recording into text with the selected dictation model",
    ),
    key(
        "labs.api_processing",
        Kind::Bool,
        "Labs: apps using Hex's local API can ask for transcripts with Global mode's corrections, \
         rewrite, and transformations applied (process=global)",
    ),
    key(
        "labs.api_timestamps",
        Kind::Bool,
        "Labs: apps using Hex's local API can ask for each segment's or word's start and end times \
         in a transcript, as the speech model heard them (timestamps=segment|word)",
    ),
    key(
        "labs.speakers_only",
        Kind::Bool,
        "Labs: While dictating applies only while the default output is speakers; headphones are \
         left alone, and outputs Hex can't classify count as speakers",
    ),
    key(
        "labs.pause_menu",
        Kind::Bool,
        "Labs: the menu bar offers Pause Hex, which makes every capture shortcut inert until \
         Resume; a restart starts unpaused (macOS)",
    ),
    key(
        "meetings.enabled",
        Kind::Bool,
        "Labs: Meetings (beta), call detection, recording, and transcripts",
    ),
    optional(
        "meetings.output_folder",
        Kind::Path,
        "Folder for meeting Markdown; none uses ~/Documents/Hex Meetings",
    ),
    key(
        "meetings.summaries",
        SUMMARIES,
        "Who titles and summarizes meetings: off, mac (Apple Intelligence, nothing leaves this Mac), or opencode (sends the transcript to the OpenCode model)",
    ),
    optional(
        "meetings.opencode_model",
        Kind::Text,
        "The OpenCode provider/model meeting notes use when meetings.summaries is opencode; none lets Hex pick a fast model",
    ),
    key(
        "meetings.show_recording_indicator",
        Kind::Bool,
        "Show the floating recording indicator during meetings",
    ),
    key(
        "meetings.recording_indicator_corner",
        Kind::Choice(&["top_right", "top_left", "bottom_right", "bottom_left"]),
        "Screen corner of the meeting recording indicator",
    ),
    optional(
        "meetings.retention",
        Kind::Choice(&["day", "week", "month", "forever"]),
        "How long finished meetings are kept before they move to the Trash; none keeps new meetings 24 hours and older ones until you choose",
    ),
    optional(
        "meetings.audio_retention",
        Kind::Choice(&["day", "week", "month", "forever"]),
        "How long finished meetings keep their audio with Labs › Expire meeting audio separately on; none follows meetings.retention",
    ),
    key(
        "meetings.providers",
        Kind::Json,
        "Per-app call choices, as a JSON array of {key, label, policy: ask|always_record|never_offer}",
    ),
    read_only(
        "meetings.calendar.connected",
        Kind::Bool,
        "A calendar is connected for meeting reminders",
        "connect a calendar from Settings › Meetings, which asks macOS for access",
    ),
    key(
        "meetings.calendar.excluded_calendars",
        Kind::Json,
        "Calendars left out of reminders, as a JSON array of EventKit identifiers",
    ),
    key(
        "meetings.calendar.remind",
        Kind::Bool,
        "Remind before calendar meetings",
    ),
    key(
        "meetings.calendar.lead_minutes",
        Kind::Choice(&["0", "1", "2", "5"]),
        "Minutes before a meeting its reminder shows",
    ),
    key(
        "experimental.hex_microphone",
        Kind::Bool,
        "Labs: the Voice Filter and its Hex Microphone virtual input",
    ),
    key(
        "experimental.hex_microphone_mode",
        Kind::Choice(&[
            "normal",
            "muffled",
            "quiet",
            "muted",
            "chipmunk",
            "robot",
            "walkie",
            "underwater",
            "far",
        ]),
        "What Hex Microphone does to your voice while you dictate",
    ),
    key(
        "experimental.hex_microphone_comfort_noise",
        Kind::Bool,
        "Hex Microphone fills silence with soft noise",
    ),
    key(
        "experimental.hex_microphone_strength",
        UNIT,
        "How strongly Hex Microphone applies its filter, from 0 to 1",
    ),
    key(
        "enable_on_launch",
        Kind::Bool,
        "Start listening when Hex opens (Windows and Linux)",
    ),
    key(
        "paste_with_shift",
        Kind::Bool,
        "Paste with Ctrl-Shift-V for terminals (Linux and Windows)",
    ),
    key(
        "gnome_keyboard_shortcuts",
        Kind::Bool,
        "Hex's own GNOME custom keyboard shortcuts, for GNOME without a shortcuts portal (Linux)",
    ),
    key(
        "share_dictation_shortcut",
        Kind::Bool,
        "Labs: let other apps see the dictation shortcut too (Windows)",
    ),
    optional(
        "ui_language",
        Kind::Choice(&[
            "en", "pl", "zh", "zh-Hant", "ja", "de", "es", "uk", "pt", "fr", "ru",
        ]),
        "Interface language; none follows the system",
    ),
    key(
        "transcription_device",
        Kind::Choice(&["auto", "gpu", "cpu"]),
        "Where the speech model runs: auto (a GPU unless it failed before), gpu, or cpu (Linux and Windows)",
    ),
    key(
        "indicator_position",
        Kind::Choice(&["top", "bottom", "hidden"]),
        "Where the dictation HUD appears",
    ),
    optional(
        "updates.channel",
        Kind::Choice(&["stable", "dev"]),
        "Update channel; none follows the build (dev builds use dev, others stable)",
    ),
];

/// Captures exist only in builds with the `capture` build option.
#[cfg(hex_capture)]
pub(super) const CAPTURE_KEYS: &[Key] = &[
    key(
        "capture.enabled",
        Kind::Bool,
        "Annotate: record the screen while you draw and talk (on by default)",
    ),
    key(
        "capture.hotkey",
        Kind::Shortcut(HotkeySlot::Capture),
        "The Annotate shortcut: press to start recording and again to finish",
    ),
    optional(
        "capture.replay_hotkey",
        Kind::Shortcut(HotkeySlot::CaptureReplay),
        "Labs: shortcut to export the last 15 s from Annotate's rolling replay buffer, or hold to narrate over it (macOS 27+, with labs.annotate_replay); none turns it off",
    ),
    key(
        "capture.transcriber",
        Kind::Choice(&["parakeet", "apple"]),
        "What transcribes a capture's narration: Hex's loaded model or Apple's",
    ),
    optional(
        "capture.folder",
        Kind::Path,
        "Folder for new captures; none uses Scrawl's captures folder",
    ),
    optional(
        "capture.media_days",
        RETENTION_DAYS,
        "Days a capture keeps its video and audio; none keeps them",
    ),
    optional(
        "capture.bundle_days",
        RETENTION_DAYS,
        "Days a capture is kept; none keeps it",
    ),
    key(
        "capture.capsule_flow",
        Kind::Choice(&["list", "toast", "menu_bar", "cursor", "notch"]),
        "How the capture capsule offers where to send a capture (prototypes): a list, a \
         one-line toast, the menu bar, a card at the pointer, or a notch",
    ),
    key(
        "capture.routing",
        Kind::Json,
        "Where captures go among OpenCode sessions: Scrawl's router settings (autoSend, sites, apps, smartRouting, …) as a JSON object",
    ),
    key(
        "capture.show_overlays_in_recordings",
        Kind::Bool,
        "Let screen recordings, captures included, show Hex's capsules, ink, and HUD (for demos)",
    ),
];
#[cfg(not(hex_capture))]
pub(super) const CAPTURE_KEYS: &[Key] = &[];
#[cfg(hex_capture)]
const RETENTION_DAYS: Kind = Kind::Integer { min: 1, max: 3650 };

#[cfg(target_os = "macos")]
const SUMMARIES: Kind = Kind::Choice(&["off", "mac", "opencode"]);
#[cfg(not(target_os = "macos"))]
const SUMMARIES: Kind = Kind::Choice(&["off", "opencode"]);

/// Keys that were replaced, and what replaced them.
pub(super) const RETIRED: &[(&str, &str)] = &[
    (
        "meetings.summarize_with_apple_intelligence",
        "meetings.summaries mac",
    ),
    (
        "meetings.summarize_with_opencode",
        "meetings.summaries opencode",
    ),
];

/// Settings that act only where Hex has a capability (by its
/// `rune_ui::capabilities` name), the same one that shows their row in the
/// window. The first matching prefix decides.
pub(super) const NEEDS: &[(&str, &str)] = &[
    ("meetings.calendar.", "calendar"),
    ("meetings.", "meetings"),
    ("labs.live_chapters", "meetings"),
    ("labs.subtitle_export", "meetings"),
    ("labs.stop_when_call_ends", "meetings"),
    ("labs.more_call_apps", "meetings"),
    ("labs.movable_meeting_capsules", "meetings"),
    ("labs.meeting_notes_vocabulary", "meetings"),
    ("labs.meeting_finished_event", "meetings"),
    ("labs.meeting_descriptive_filenames", "meetings"),
    ("labs.meeting_notes_file", "meetings"),
    ("labs.follow_live_transcript", "meetings"),
    ("experimental.hex_microphone", "fun"),
    ("typed_text_hotkey", "typed_text"),
    ("show_dock_icon", "dock_icon"),
    ("labs.hide_menu_bar_icon", "dock_icon"),
    ("labs.quit_closes_window", "dock_icon"),
    ("keyboard_model_switching.", "keyboard_model_switching"),
    ("enable_on_launch", "listener_control"),
    ("share_dictation_shortcut", "share_shortcut"),
];

/// The capability `key` needs, if any.
pub(super) fn needs(key: &str) -> Option<&'static str> {
    NEEDS
        .iter()
        .find(|(prefix, _)| key == *prefix || key.starts_with(prefix))
        .map(|(_, capability)| *capability)
}
