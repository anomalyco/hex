//! Labs: features we're unsure about, especially ones people asked for,
//! each behind its own switch, off by default. There is no master switch:
//! Settings › Labs lists every feature as a row with a toggle. A feature
//! with its own pane (Meetings, Voice Filter, Playground) joins the sidebar
//! as a normal item while on and is hidden while off; a behaviour-only
//! feature (Live text, Live chapters, Vocabulary biasing, Terminal paste on
//! Windows) is just its toggle.
//!
//! Gate a feature where its work starts (the worker, thread, or capture
//! that does it), not only where its UI shows.

use std::sync::atomic::{AtomicU8, Ordering};

use clap::Subcommand;
use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::app_settings::{AppSettings, LabsSettings};

/// Where people send feedback on Labs features.
pub const FEEDBACK_URL: &str = "https://github.com/anomalyco/hex/issues/new";

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
}

impl Feature {
    pub const ALL: [Self; 7] = [
        Self::Meetings,
        Self::VoiceFilter,
        Self::Playground,
        Self::LiveText,
        Self::LiveChapters,
        Self::VocabularyBiasing,
        Self::TerminalPaste,
    ];

    /// The features this OS has.
    pub fn available() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(|feature| feature.on_this_os())
    }

    const fn on_this_os(self) -> bool {
        match self {
            Self::Meetings | Self::VoiceFilter | Self::LiveChapters => cfg!(target_os = "macos"),
            Self::TerminalPaste => cfg!(target_os = "windows"),
            Self::Playground | Self::LiveText | Self::VocabularyBiasing => true,
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
            Self::Playground => "Scene and story browser in the window (applies on restart)",
            Self::LiveText => {
                "Show provisional words beside the HUD while dictating (applies on restart)"
            }
            Self::LiveChapters => "Break meeting transcripts into chapters while recording",
            Self::VocabularyBiasing => {
                "Bias Parakeet transcription toward your vocabulary; rewriting and search use it either way"
            }
            Self::TerminalPaste => "Paste with Ctrl+Shift+V, for apps that ignore Ctrl+V",
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
        }
    }
}

/// Set once this process knows its settings.
const KNOWN: u8 = 1;
const LIVE_TEXT: u8 = 1 << 1;
const LIVE_CHAPTERS: u8 = 1 << 2;
const PLAYGROUND: u8 = 1 << 3;
const VOCABULARY_BIASING: u8 = 1 << 4;

/// This process's `labs` switches: unknown until settings load or one is
/// first read.
static STATE: AtomicU8 = AtomicU8::new(0);

fn on(bit: u8) -> bool {
    let state = match STATE.load(Ordering::Acquire) {
        0 => {
            let saved = encode(&AppSettings::saved_labs());
            let _ = STATE.compare_exchange(0, saved, Ordering::AcqRel, Ordering::Acquire);
            STATE.load(Ordering::Acquire)
        }
        state => state,
    };
    state & bit != 0
}

/// Live text: provisional words beside the HUD (read at launch).
pub fn live_text() -> bool {
    on(LIVE_TEXT)
}

/// Live chapters while a meeting records.
pub fn live_chapters() -> bool {
    on(LIVE_CHAPTERS)
}

/// The playground in the window (read at launch).
pub fn playground() -> bool {
    on(PLAYGROUND)
}

/// Transcription biased toward the vocabulary (Parakeet).
pub fn vocabulary_biasing() -> bool {
    on(VOCABULARY_BIASING)
}

/// Records the switches the loaded or saved settings hold.
pub(crate) fn apply(settings: &LabsSettings) {
    STATE.store(encode(settings), Ordering::Release);
}

fn encode(settings: &LabsSettings) -> u8 {
    [
        (settings.live_text, LIVE_TEXT),
        (settings.live_chapters, LIVE_CHAPTERS),
        (settings.playground, PLAYGROUND),
        (settings.vocabulary_biasing, VOCABULARY_BIASING),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(KNOWN, |state, (_, bit)| state | bit)
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
}

/// `hex labs`: edits `settings.json`; a running Hex applies most changes
/// when it next starts.
pub fn run(command: Option<&LabsCommand>) -> Result<()> {
    let mut settings = AppSettings::load()?;
    let (name, on) = match command {
        None | Some(LabsCommand::List) => {
            print!("{}", list(&settings));
            return Ok(());
        }
        Some(LabsCommand::On { feature }) => (feature, true),
        Some(LabsCommand::Off { feature }) => (feature, false),
    };
    let feature = Feature::from_name(name)
        .filter(|feature| feature.on_this_os())
        .ok_or_else(|| {
            eyre!(
                "unknown Labs feature {name:?}; this Hex has: {}",
                Feature::available()
                    .map(Feature::name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    if feature == Feature::VoiceFilter && !on && crate::hex_microphone::driver_bundle_installed() {
        return Err(eyre!(
            "voice-filter stays on while the Hex Microphone driver is installed; uninstall the driver in its pane to turn it off"
        ));
    }
    let state = if on { "on" } else { "off" };
    if feature.on(&settings) == on {
        println!("{} is already {state}.", feature.name());
        return Ok(());
    }
    feature.set(&mut settings, on);
    settings.save()?;
    println!("Turned {} {state}.", feature.name());
    println!("{RESTART_HINT}");
    Ok(())
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
                && crate::hex_microphone::driver_bundle_installed());
        out.push_str(&format!(
            "{:<width$}  {:<3}  {}\n",
            feature.name(),
            if on { "on" } else { "off" },
            feature.description(),
        ));
    }
    out
}

/// `hex experiments …`, from before Labs.
pub fn run_retired_experiments() -> Result<()> {
    println!("Experiments are now Labs: hex labs, hex labs on|off <feature>");
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
const RESTART_HINT: &str = "Restart Hex to apply it: hex restart";
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const RESTART_HINT: &str = "Quit and reopen Hex to apply it.";

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
    fn the_old_master_switch_is_read_and_ignored() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"labs": {"enabled": true, "live_text": true}}"#).unwrap();
        assert!(settings.labs.live_text);
        assert!(!settings.labs.playground);
        let json = serde_json::to_value(&settings).unwrap();
        assert!(json["labs"].get("enabled").is_none());
    }
}
