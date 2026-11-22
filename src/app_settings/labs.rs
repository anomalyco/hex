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

/// How a normal dictation's text is written (Labs › Dictation style): the
/// iPhone app's four styles. Every choice is punctuation and case rules
/// only, with no model.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationStyle {
    /// Full sentences: fillers trimmed, each sentence capitalized and
    /// ended, and informal English contractions written out.
    #[default]
    Polished,
    /// Texting: sentences may start lowercase, "I" is "i", an opening
    /// interjection loses its comma, and a short message its final period.
    Casual,
    /// Concise lines, with a list when several items are said.
    Notes,
    /// Exactly as the speech model wrote it.
    Verbatim,
}

impl DictationStyle {
    /// Every choice, in the order the window shows them.
    pub const ALL: [Self; 4] = [Self::Polished, Self::Casual, Self::Notes, Self::Verbatim];

    /// The English label the window shows.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Polished => "Polished",
            Self::Casual => "Casual",
            Self::Notes => "Notes",
            Self::Verbatim => "Verbatim",
        }
    }

    /// What the choice does, in one line under the label.
    pub const fn hint(self) -> &'static str {
        match self {
            Self::Polished => "Full sentences, fillers trimmed",
            Self::Casual => "Texting: lowercase start, no final period",
            Self::Notes => "Short lines, a list for several items",
            Self::Verbatim => "Exact words from the model",
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

/// The `labs` switches (`crate::labs`), each off by default; older Labs features keep their own keys, and retired switches (`labs.enabled`, `labs.collapsible_meeting_sections`, `labs.meeting_platform_icons`, `labs.transcribe_files`, `labs.restore_shortcut_default`, `labs.yearly_stats`, and the graduated meeting labs) are ignored.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LabsSettings {
    /// Live text beside the HUD while dictating.
    pub live_text: bool,
    /// Live text with a shorter lookahead, so its words appear sooner at a
    /// little more GPU work (Parakeet buffered streaming).
    pub faster_live_text: bool,
    /// A failed dictation's explanation beside the HUD for a few seconds.
    pub failure_notices: bool,
    /// Provisional words in the focused text field as a Wayland input-method
    /// preedit, with the final text committed there (Linux Wayland).
    pub live_typing: bool,
    pub live_chapters: bool,
    pub playground: bool,
    /// Names and terms shown in the focused window become temporary
    /// vocabulary for each dictation (macOS).
    pub screen_vocabulary: bool,
    /// A name fixed in text Hex just pasted is added to Vocabulary, read
    /// from that field alone for a few minutes after the paste (macOS).
    pub learn_from_corrections: bool,
    /// The mode's OpenCode rewrite reads the text around the cursor as
    /// context (macOS); password fields are never read.
    pub cursor_context: bool,
    /// Mouse buttons may be recorded as shortcuts (macOS, issue 209).
    pub mouse_shortcuts: bool,
    /// The engine unloads the speech model after `unload_idle_minutes` (5, 15 default, 30, 60) unused (issue 442).
    pub unload_idle: bool,
    pub unload_idle_minutes: u32,
    /// With Release microphone while idle on, the microphone stays open for
    /// `keep_microphone_open_seconds` after a capture, so the next one starts
    /// at once.
    pub keep_microphone_open: bool,
    /// One of [`KEEP_MICROPHONE_OPEN_SECONDS`]; another value loads the
    /// default.
    #[serde(deserialize_with = "deserialize_keep_microphone_open_seconds")]
    pub keep_microphone_open_seconds: u32,
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
    pub clean_up_with_apple_intelligence: bool,
    /// A dictation in a mode that opts in is rewritten into plain, neutral
    /// wording on this Mac instead of the OpenCode rewrite and the cleanup
    /// (macOS).
    pub neutral_voice: bool,
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
    pub recent_dictations: bool,
    pub skip_pauses: bool,
    /// Output's Try card offers Vocabulary and Fix this beside its result (issue 989).
    pub output_fixes: bool,
    /// The picker's "Use model file where it is…" adds a GGUF file without copying it (issue 941).
    pub model_in_place: bool,
    /// An import may use Q6_K, Q5_K, or Q4_K weights, which Metal runs and
    /// Hex hasn't verified (macOS).
    pub k_quant_models: bool,
    /// The picker's All speech models view offers "Only downloaded models" (issue 1018).
    pub picker_downloaded: bool,
    /// The ISO section key, left of 1, may be a shortcut on its own (Linux
    /// and macOS, issue 1050).
    pub section_key_alone: bool,
    /// The microphone (dictation) key in a laptop keyboard's top row may be
    /// a shortcut on its own (macOS, issue 181).
    pub microphone_key: bool,
    /// Dictations saved to History keep their audio, for Transcribe again (issue 1032).
    pub keep_recordings: bool,
    /// A kept recording may be copied from History and the menu bar or tray
    /// as a file on the clipboard; it stores no audio itself.
    pub copy_dictation_audio: bool,
    pub hide_menu_bar_icon: bool,
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
    /// A paste restores the clipboard through a running XWayland on Wayland desktops without data-control, such as GNOME (Linux, issue 1111).
    pub gnome_clipboard_restore: bool,
    /// A shortcut runs the focused app's Output steps on the text already
    /// typed there and replaces it (`typed_text_hotkey`, unbound by default).
    pub typed_text: bool,
    /// At either visible indicator position, the HUD grows out of the notch on a screen with one (macOS).
    pub notch_indicator: bool,
    /// With the HUD position Bottom, the plate sits `hud_bottom_gap` points
    /// above the Dock or the screen's bottom edge (macOS, Labs › HUD height).
    pub hud_height: bool,
    /// The gap in points above the Dock while `hud_height` is on; a saved
    /// value above [`HUD_BOTTOM_GAP_MAX`] is clamped at placement.
    #[serde(default = "default_hud_bottom_gap")]
    pub hud_bottom_gap: u16,
    /// The meeting capsules float on a running XWayland on Wayland desktops without layer-shell, such as GNOME (Linux, issue 1274).
    pub gnome_meeting_capsules: bool,
    /// Annotate's capsule and ink float on a running XWayland on Wayland desktops without layer-shell, such as GNOME (Linux).
    pub gnome_annotate: bool,
    /// On Wayland, a screen capture asks the desktop for the monitor under the
    /// pointer, and each monitor keeps its own ScreenCast choice (Linux).
    pub annotate_pointer_screen: bool,
    /// The screenshot shortcut takes an area or a window (macOS); its
    /// shortcut and folder are `screenshots.*`.
    pub screenshots: bool,
    /// A written Annotate capture copies as one light zip file (macOS,
    /// Linux, and Windows).
    pub copy_capture_zip: bool,
    /// Meetings offers Import audio…: a local audio file becomes a meeting (macOS).
    pub meeting_import: bool,
    /// With `labs.annotate_replay` on, keeps a rolling 15 s display buffer via `SCClipBufferingOutput` (macOS 27+).
    pub annotate_replay: bool,
    /// An OpenCode service running in the default WSL distribution is used
    /// for Modes rewriting and the Modes model picker ahead of a Windows one
    /// when it answers; Windows answers when WSL answers nothing (Windows).
    pub opencode_in_wsl: bool,
    /// Press Return after pasting a shortcut dictation.
    pub yolo_mode: bool,
    /// The apps Labs › YOLO mode sends in, by the names
    /// Modes match; empty means every app.
    pub yolo_mode_apps: Vec<String>,
    /// A WAV, MP3, AAC, M4A, MP4, MOV, AIFF, CAF, FLAC, OGG, Opus, WebM, or
    /// MKV file dropped on the window is transcribed and saved as a text file
    /// beside it.
    pub drop_transcribe: bool,
    /// Holding Paste Last opens a picker of recent dictations and
    /// annotations, and History lists them together (macOS).
    pub paste_last_picker: bool,
    /// The Vocabulary sheet imports terms from a CSV file and exports the
    /// list as one (Labs › Vocabulary files).
    pub vocabulary_files: bool,
    /// `hex annotate inspect`, `zoom`, `frames`, and `skill` read a capture
    /// bundle for a coding agent (Labs › Agent tools for captures).
    pub annotate_agent_tools: bool,
    /// The Annotate pane's Agents section writes the hex-annotate skill into
    /// coding agents' own folders and, on macOS, links `hex` onto PATH
    /// (Labs › Install agent skill from Annotate).
    pub annotate_agent_install: bool,
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
    /// The menu bar lists the next calendar meetings, each a local start and
    /// end time and title; choosing one with a call link opens it (macOS,
    /// with Meetings on and a calendar connected).
    pub calendar_menu: bool,
    /// Meetings › Settings › Calendar shows Select all or Deselect all beside
    /// the Calendars row, so every calendar is included or left out at once
    /// (macOS).
    pub calendar_bulk_select: bool,
    /// The menu bar offers Pause Hex, which makes every capture shortcut
    /// inert until Resume (macOS).
    pub pause_menu: bool,
    /// A spoken trigger phrase inside a dictation hands the mode's OpenCode
    /// model a one-off instruction for that dictation.
    pub voice_prompt: bool,
    /// The words that start a voice prompt: at least two words and six
    /// letters, or the default. Another value fails to load, so the field
    /// returns to its default.
    #[serde(deserialize_with = "deserialize_phrase")]
    pub voice_prompt_phrase: String,
    /// Every normal dictation is written in `dictation_style_choice`'s
    /// style, after corrections and any rewrite and before transformations.
    pub dictation_style: bool,
    /// Which of the four styles it is written as while `dictation_style` is
    /// on.
    pub dictation_style_choice: DictationStyle,
    /// A spoken phrase at the end of a hands-free dictation finishes it as if
    /// the shortcut were pressed again, and never reaches the pasted text.
    pub stop_phrase: bool,
    /// The phrase that finishes a hands-free dictation: at least two words
    /// and six letters, or the default. Another value fails to load, so the
    /// field returns to its default.
    #[serde(deserialize_with = "deserialize_phrase")]
    pub stop_phrase_text: String,
    /// Every normal dictation is signed with `signature_text` on its own line
    /// at the end (every OS).
    pub signature: bool,
    /// The line appended while `signature` is on: one line of at most 200
    /// characters, empty appending nothing. Another value fails to load, so
    /// the field returns to its default.
    #[serde(deserialize_with = "deserialize_signature")]
    pub signature_text: String,
    /// A key or click joining a held modifier-only shortcut a second or more
    /// after it went down reaches the app in front and leaves the dictation
    /// recording, which the release still finishes (macOS and Windows).
    pub hold_through_keys: bool,
    /// The Modes editor joins the sidebar: every mode's apps, corrections,
    /// OpenCode rewrite, and transformations.
    pub modes_editor: bool,
    /// Settings › Labs' groups opened or closed by hand, by `labs::Group`
    /// id; a group not here is open while it holds a lab that's on.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub groups: std::collections::BTreeMap<String, bool>,
    /// Retired switches the `meeting-labs-settings` migration reads into
    /// `meetings.*` once: `meetings.stop_when_call_ends`,
    /// `meetings.export.descriptive_filenames`, `meetings.export.notes_file`,
    /// and whether `meetings.audio_retention` stays. Never written.
    #[serde(skip_serializing)]
    pub stop_when_call_ends: Option<bool>,
    #[serde(skip_serializing)]
    pub meeting_descriptive_filenames: Option<bool>,
    #[serde(skip_serializing)]
    pub meeting_notes_file: Option<bool>,
    #[serde(skip_serializing)]
    pub expire_meeting_audio: Option<bool>,
    /// Retired: `labs.yearly_stats`, read into `history_stats` once by the
    /// `yearly-stats-merge` migration, since the daily rollup and the year
    /// heatmap are part of Dictation stats now. Never written.
    #[serde(skip_serializing)]
    pub yearly_stats: Option<bool>,
}

/// The idle times Labs › Free memory when idle offers, in minutes.
pub const UNLOAD_IDLE_MINUTES: [u32; 4] = [5, 15, 30, 60];

/// The gap above the Dock or the screen's bottom edge the dictation HUD
/// keeps while Labs › HUD height is off, in points.
pub const HUD_BOTTOM_GAP_DEFAULT: u16 = 24;

/// The highest gap Labs › HUD height offers, in points. Placement clamps it
/// further so the plate stays inside the visible frame.
pub const HUD_BOTTOM_GAP_MAX: u16 = 400;

/// The times Labs › Keep mic open after dictating offers, in seconds:
/// 30 s, 1, 3, 5, and 30 minutes.
pub const KEEP_MICROPHONE_OPEN_SECONDS: [u32; 5] = [30, 60, 180, 300, 1_800];

/// The retired Yearly stats lab merged into Dictation stats: its daily
/// rollup and year heatmap are part of the one lab now.
pub(super) const YEARLY_STATS_MERGE_MIGRATION: &str = "yearly-stats-merge";

impl AppSettings {
    /// A saved `labs.yearly_stats` reads into `labs.history_stats` once, so
    /// someone who had Yearly stats on keeps their heatmap and all-time
    /// totals without a second switch. A later choice sticks.
    pub(super) fn migrate_yearly_stats_lab(&mut self) {
        if !self.has_migration(YEARLY_STATS_MERGE_MIGRATION) {
            self.labs.history_stats |= self.labs.yearly_stats == Some(true);
            self.migrations.push(YEARLY_STATS_MERGE_MIGRATION.into());
        }
        self.labs.yearly_stats = None;
    }
}

impl LabsSettings {
    /// The default [`KEEP_MICROPHONE_OPEN_SECONDS`] choice.
    pub const DEFAULT_KEEP_MICROPHONE_OPEN_SECONDS: u32 = 60;

    /// How long a capture's microphone keeps open once it ends, while Labs ›
    /// Keep mic open after dictating is on. `None` turns it off, and
    /// a choice outside [`KEEP_MICROPHONE_OPEN_SECONDS`] (a hand-edited file)
    /// also reads as off rather than guessing.
    pub fn keep_microphone_open_for(&self) -> Option<std::time::Duration> {
        let seconds = self.keep_microphone_open_seconds;
        (self.keep_microphone_open && KEEP_MICROPHONE_OPEN_SECONDS.contains(&seconds))
            .then(|| std::time::Duration::from_secs(u64::from(seconds)))
    }
}

/// The default HUD bottom gap, so a settings file without the field keeps
/// the fixed gap Hex always had.
fn default_hud_bottom_gap() -> u16 {
    HUD_BOTTOM_GAP_DEFAULT
}

/// A saved `keep_microphone_open_seconds` must name one of
/// [`KEEP_MICROPHONE_OPEN_SECONDS`]; another value fails to load, so the
/// field returns to its default with the usual launch note.
fn deserialize_keep_microphone_open_seconds<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let seconds = u32::deserialize(deserializer)?;
    if KEEP_MICROPHONE_OPEN_SECONDS.contains(&seconds) {
        Ok(seconds)
    } else {
        Err(serde::de::Error::custom(format!(
            "expected one of {}",
            KEEP_MICROPHONE_OPEN_SECONDS
                .map(|seconds| seconds.to_string())
                .join(", ")
        )))
    }
}

/// A saved phrase (`voice_prompt_phrase`, `stop_phrase_text`) must pass
/// [`crate::dictation::voice_prompt::phrase_problem`]; another value fails to
/// load, so the field returns to the default with the usual launch note.
fn deserialize_phrase<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let phrase = String::deserialize(deserializer)?;
    let phrase = phrase.trim();
    match crate::dictation::voice_prompt::phrase_problem(phrase) {
        Some(problem) => Err(serde::de::Error::custom(problem.message())),
        None => Ok(phrase.to_owned()),
    }
}

/// A saved `signature_text` must pass
/// [`crate::dictation::signature::text_problem`]; another value fails to
/// load, so the field returns to the default with the usual launch note.
fn deserialize_signature<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let signature = String::deserialize(deserializer)?;
    let signature = signature.trim();
    match crate::dictation::signature::text_problem(signature) {
        Some(problem) => Err(serde::de::Error::custom(problem)),
        None => Ok(signature.to_owned()),
    }
}

impl Default for LabsSettings {
    fn default() -> Self {
        Self {
            live_text: false,
            faster_live_text: false,
            failure_notices: false,
            live_typing: false,
            live_chapters: false,
            playground: false,
            screen_vocabulary: false,
            learn_from_corrections: false,
            cursor_context: false,
            mouse_shortcuts: false,
            unload_idle: false,
            unload_idle_minutes: 15,
            keep_microphone_open: false,
            keep_microphone_open_seconds: Self::DEFAULT_KEEP_MICROPHONE_OPEN_SECONDS,
            keep_on_clipboard: false,
            copy_only: false,
            return_to_app: false,
            spoken_formatting: false,
            filler_words: false,
            clean_up_with_apple_intelligence: false,
            neutral_voice: false,
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
            recent_dictations: false,
            skip_pauses: false,
            output_fixes: false,
            model_in_place: false,
            k_quant_models: false,
            picker_downloaded: false,
            section_key_alone: false,
            microphone_key: false,
            keep_recordings: false,
            copy_dictation_audio: false,
            hide_menu_bar_icon: false,
            caps_lock_dictation: false,
            caps_lock_light: CapsLockLight::Steady,
            appearance: false,
            appearance_theme: AppearanceTheme::System,
            gnome_clipboard_restore: false,
            typed_text: false,
            notch_indicator: false,
            hud_height: false,
            hud_bottom_gap: HUD_BOTTOM_GAP_DEFAULT,
            annotate_replay: false,
            opencode_in_wsl: false,
            yolo_mode: false,
            yolo_mode_apps: Vec::new(),
            drop_transcribe: false,
            paste_last_picker: false,
            vocabulary_files: false,
            annotate_agent_tools: false,
            annotate_agent_install: false,
            api_processing: false,
            api_timestamps: false,
            speakers_only: false,
            app_model_switching: false,
            calendar_menu: false,
            calendar_bulk_select: false,
            pause_menu: false,
            voice_prompt: false,
            voice_prompt_phrase: crate::dictation::voice_prompt::DEFAULT_PHRASE.to_owned(),
            dictation_style: false,
            dictation_style_choice: DictationStyle::Polished,
            stop_phrase: false,
            stop_phrase_text: crate::dictation::stop_phrase::DEFAULT_PHRASE.to_owned(),
            signature: false,
            signature_text: crate::dictation::signature::DEFAULT_TEXT.to_owned(),
            hold_through_keys: false,
            modes_editor: false,
            gnome_meeting_capsules: false,
            gnome_annotate: false,
            annotate_pointer_screen: false,
            screenshots: false,
            copy_capture_zip: false,
            meeting_import: false,
            groups: std::collections::BTreeMap::new(),
            stop_when_call_ends: None,
            meeting_descriptive_filenames: None,
            meeting_notes_file: None,
            expire_meeting_audio: None,
            yearly_stats: None,
        }
    }
}
