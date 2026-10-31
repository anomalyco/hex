//! Labs: features we're unsure about, especially ones people asked for,
//! each behind its own switch, off by default. There is no master switch:
//! Settings › Labs lists every feature as a row with a toggle. A feature
//! with its own pane (Meetings, Voice Filter, Playground) joins the sidebar
//! as a normal item while on and is hidden while off; a behaviour-only
//! feature (Live text, Live chapters, Vocabulary biasing, keyboard-language
//! model switching, mouse shortcuts, pausing the browser, restoring Paste
//! last dictation's default shortcut, keeping the dictation on the
//! clipboard or only copying it (macOS), a second dictation shortcut
//! (macOS), and Terminal paste
//! and shortcut sharing on Windows) is just its toggle.
//!
//! Each feature has an audience (`Feature::audience`): `Everyone` ships in
//! every build; `Dev` exists only in dev builds (`dev_build::is_dev_build`:
//! debug, local, test, and dev-channel builds). A stable build never lists,
//! accepts, or runs a Dev feature, and keeps its saved setting untouched.
//!
//! Gate a feature where its work starts (the worker, thread, or capture
//! that does it), not only where its UI shows.

use std::sync::atomic::{AtomicU32, Ordering};

use clap::Subcommand;
use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::app_settings::{AppSettings, LabsSettings};

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
    SmartSpacing,
    SpokenSymbols,
}

impl Feature {
    pub const ALL: [Self; 21] = [
        Self::Meetings,
        Self::VoiceFilter,
        Self::Playground,
        Self::LiveText,
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
        Self::SmartSpacing,
        Self::SpokenSymbols,
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
            | Self::ShareShortcut
            | Self::UnloadIdle
            | Self::RestoreShortcutDefault
            | Self::KeepOnClipboard
            | Self::CopyOnly
            | Self::SpokenFormatting
            | Self::FillerWords
            | Self::SubtitleExport
            | Self::SecondDictationShortcut
            | Self::SmartSpacing
            | Self::SpokenSymbols => Audience::Everyone,
        }
    }

    /// The feature `hex settings` key `key` switches (its toggle, or
    /// Unload when idle's minutes).
    pub fn for_settings_key(key: &str) -> Option<Self> {
        if key == "labs.unload_idle_minutes" {
            return Some(Self::UnloadIdle);
        }
        Self::ALL
            .into_iter()
            .find(|feature| feature.settings_key() == key)
    }

    fn on_this_os(self) -> bool {
        match self {
            Self::Meetings | Self::LiveChapters | Self::SubtitleExport => {
                cfg!(target_os = "macos")
            }
            Self::VoiceFilter => cfg!(any(target_os = "macos", target_os = "linux")),
            Self::TerminalPaste | Self::ShareShortcut => cfg!(target_os = "windows"),
            Self::MouseShortcuts
            | Self::PauseBrowser
            | Self::KeepOnClipboard
            | Self::CopyOnly
            | Self::SecondDictationShortcut
            | Self::SmartSpacing => cfg!(target_os = "macos"),
            Self::KeyboardModelSwitching => crate::keyboard_input_sources::available(),
            Self::Playground
            | Self::LiveText
            | Self::VocabularyBiasing
            | Self::UnloadIdle
            | Self::RestoreShortcutDefault
            | Self::SpokenFormatting
            | Self::SpokenSymbols
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
            Self::SmartSpacing => "smart-spacing",
            Self::SpokenSymbols => "spoken-symbols",
        }
    }

    /// Its `hex settings` key.
    pub const fn settings_key(self) -> &'static str {
        match self {
            Self::Meetings => "meetings.enabled",
            Self::VoiceFilter => "experimental.hex_microphone",
            Self::Playground => "labs.playground",
            Self::LiveText => "labs.live_text",
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
            Self::SmartSpacing => "labs.smart_spacing",
            Self::SpokenSymbols => "labs.spoken_symbols",
        }
    }

    /// The English one-line description the CLI prints; the window shows
    /// its translated row.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Meetings => "Record calls and keep transcripts (its own pane)",
            Self::VoiceFilter => {
                "Hex Microphone virtual input and real-time voice filters (its own pane)"
            }
            Self::Playground => "Scene and story browser in the window",
            Self::LiveText => "Show provisional words beside the HUD while dictating",
            Self::LiveChapters => "Break meeting transcripts into chapters while recording",
            Self::VocabularyBiasing => {
                "Bias Parakeet transcription toward your vocabulary; rewriting and search use it either way"
            }
            Self::TerminalPaste => "Paste with Ctrl+Shift+V, for apps that ignore Ctrl+V",
            Self::KeyboardModelSwitching => {
                "Changing the keyboard language switches the speech model and language"
            }
            Self::MouseShortcuts => "Record mouse buttons as shortcuts",
            Self::PauseBrowser => "Also pause the browser while dictating, with Pause media",
            Self::ShareShortcut => "Let apps such as Discord see the dictation shortcut too",
            Self::UnloadIdle => {
                "Free the speech model's memory after a while unused (labs.unload_idle_minutes); the next dictation reloads it"
            }
            Self::RestoreShortcutDefault => {
                "Offer Restore default next to Paste last dictation's shortcut"
            }
            Self::KeepOnClipboard => {
                "Leave the dictation on the clipboard after pasting instead of restoring what was there"
            }
            Self::CopyOnly => "Put the dictation on the clipboard instead of pasting it",
            Self::SpokenFormatting => {
                "Turn spoken \"new line\", \"new paragraph\", and \"open/close quote\" into line breaks and quotation marks"
            }
            Self::FillerWords => {
                "Remove filler words such as um and uh from dictations, without an AI model"
            }
            Self::SubtitleExport => {
                "Export meeting transcripts as SRT or VTT subtitles with hex meetings show"
            }
            Self::SecondDictationShortcut => {
                "Another shortcut that dictates like the main one (second_dictation_hotkey), such as a key on an external keyboard"
            }
            Self::SmartSpacing => "Add a space before a dictation pasted right after a word",
            Self::SpokenSymbols => {
                "Turn spoken \"comma\", \"colon\", \"hashtag\", \"underscore\", and similar words into symbols joined to their words"
            }
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
            Self::SmartSpacing => settings.labs.smart_spacing,
            Self::SpokenSymbols => settings.labs.spoken_symbols,
        }
    }

    pub fn set(self, settings: &mut AppSettings, on: bool) {
        match self {
            Self::Meetings => settings.meetings.enabled = on,
            Self::VoiceFilter => settings.experimental.hex_microphone = on,
            Self::Playground => settings.labs.playground = on,
            Self::LiveText => settings.labs.live_text = on,
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
            Self::SmartSpacing => settings.labs.smart_spacing = on,
            Self::SpokenSymbols => settings.labs.spoken_symbols = on,
        }
    }
}

/// Set once this process knows its settings.
const KNOWN: u32 = 1;
const LIVE_TEXT: u32 = 1 << 1;
const LIVE_CHAPTERS: u32 = 1 << 2;
const PLAYGROUND: u32 = 1 << 3;
const VOCABULARY_BIASING: u32 = 1 << 4;
const MOUSE_SHORTCUTS: u32 = 1 << 5;
const UNLOAD_IDLE: u32 = 1 << 6;
const KEEP_ON_CLIPBOARD: u32 = 1 << 7;
const COPY_ONLY: u32 = 1 << 8;
const SPOKEN_FORMATTING: u32 = 1 << 9;
const FILLER_WORDS: u32 = 1 << 10;
const SUBTITLE_EXPORT: u32 = 1 << 11;
const SMART_SPACING: u32 = 1 << 12;
const SPOKEN_SYMBOLS: u32 = 1 << 13;
/// Unload-idle minutes sit above the switches.
const MINUTES_SHIFT: u32 = 14;

/// This process's `labs` switches: unknown until settings load or one is
/// first read.
static STATE: AtomicU32 = AtomicU32::new(0);

fn on(bit: u32) -> bool {
    state() & bit != 0
}

fn state() -> u32 {
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

/// How long the speech model may sit unused before the engine unloads it,
/// while Labs › Unload model when idle is on.
pub fn unload_idle() -> Option<std::time::Duration> {
    let state = state();
    (state & UNLOAD_IDLE != 0)
        .then(|| std::time::Duration::from_secs(u64::from(state >> MINUTES_SHIFT) * 60))
}

/// Records the switches the loaded or saved settings hold.
pub(crate) fn apply(settings: &LabsSettings) {
    STATE.store(encode(settings), Ordering::Release);
}

fn encode(settings: &LabsSettings) -> u32 {
    encode_where(settings, Feature::shipped)
}

/// The switches `settings` hold for the features `reaches` admits; the
/// rest read off while their saved setting stays as it is.
fn encode_where(settings: &LabsSettings, reaches: impl Fn(Feature) -> bool) -> u32 {
    let minutes = settings.unload_idle_minutes.clamp(1, 24 * 60) << MINUTES_SHIFT;
    [
        (Feature::LiveText, settings.live_text, LIVE_TEXT),
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
        live_chapters = live_chapters(),
        playground = playground(),
        vocabulary_biasing = vocabulary_biasing(),
        "labs"
    );
}

#[derive(Debug, Subcommand)]
pub enum LabsCommand {
    /// List every Labs feature and whether it is on (the default).
    List,
    /// Turn a Labs feature on.
    On { feature: String },
    /// Turn a Labs feature off.
    Off { feature: String },
    /// Print the link that opens Hex's Labs page at a feature's row (macOS).
    #[cfg(target_os = "macos")]
    Link { feature: String },
}

/// `hex labs`: edits `settings.json`; a running Hex applies most changes
/// when it next starts.
pub fn run(command: Option<&LabsCommand>) -> Result<()> {
    #[cfg(target_os = "macos")]
    if let Some(LabsCommand::Link { feature }) = command {
        println!(
            "{}",
            crate::rune_ui::deep_link::labs_link(
                crate::rune_ui::story_link::scheme(),
                resolve(feature, Feature::shipped)?
            )
        );
        return Ok(());
    }
    let mut settings = AppSettings::load()?;
    let (name, on) = match command {
        None | Some(LabsCommand::List) => {
            print!("{}", list(&settings));
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        Some(LabsCommand::Link { .. }) => unreachable!("handled above"),
        Some(LabsCommand::On { feature }) => (feature, true),
        Some(LabsCommand::Off { feature }) => (feature, false),
    };
    let feature = resolve(name, Feature::shipped)?;
    if feature == Feature::VoiceFilter
        && !on
        && crate::audio::hex_microphone::driver_bundle_installed()
    {
        return Err(eyre!(
            "voice-filter stays on while the Hex Microphone driver is installed; uninstall the driver in its pane to turn it off"
        ));
    }
    let state = if on { "on" } else { "off" };
    // A running Hex saves and applies it at once, as its Labs row would.
    if let Some(reply) = crate::control::cli::set_in_running_hex(
        feature.settings_key(),
        if on { "true" } else { "false" },
    ) {
        return match reply {
            crate::control::ControlReply::Error { message, .. } => Err(eyre!("{message}")),
            _ => {
                println!("Turned {} {state}.", feature.name());
                Ok(())
            }
        };
    }
    if feature.on(&settings) == on {
        println!("{} is already {state}.", feature.name());
        return Ok(());
    }
    feature.set(&mut settings, on);
    settings.save()?;
    println!("Turned {} {state}.", feature.name());
    Ok(())
}

/// The feature `name` names, if this OS has it and `reaches` admits it.
fn resolve(name: &str, reaches: impl Fn(Feature) -> bool + Copy) -> Result<Feature> {
    Feature::from_name(name)
        .filter(|feature| feature.on_this_os() && reaches(*feature))
        .ok_or_else(|| {
            eyre!(
                "unknown Labs feature {name:?}; this Hex has: {}",
                Feature::available_where(reaches)
                    .map(Feature::name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn list(settings: &AppSettings) -> String {
    let width = Feature::available()
        .map(|feature| feature.name().len())
        .max()
        .unwrap_or_default();
    let mut out = String::new();
    for feature in Feature::available() {
        let on = feature.on(settings)
            || (feature == Feature::VoiceFilter
                && crate::audio::hex_microphone::driver_bundle_installed());
        out.push_str(&format!(
            "{:<width$}  {:<3}  {:<8}  {} (hex settings {})\n",
            feature.name(),
            if on { "on" } else { "off" },
            feature.audience().name(),
            feature.description(),
            feature.settings_key(),
        ));
    }
    out
}

/// `hex experiments …`, from before Labs.
pub fn run_retired_experiments() -> Result<()> {
    println!("Experiments are now Labs: hex labs, hex labs on|off <feature>");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn an_audience_reaches_the_builds_it_names() {
        assert!(Audience::Everyone.reaches(false));
        assert!(Audience::Everyone.reaches(true));
        assert!(Audience::Dev.reaches(true));
        assert!(!Audience::Dev.reaches(false));
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
    fn the_old_master_switch_is_read_and_ignored() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"labs": {"enabled": true, "live_text": true}}"#).unwrap();
        assert!(settings.labs.live_text);
        assert!(!settings.labs.playground);
        let json = serde_json::to_value(&settings).unwrap();
        assert!(json["labs"].get("enabled").is_none());
    }
}
