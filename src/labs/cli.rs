//! `hex labs`: list the features, turn one on or off, and open its row.

use clap::Subcommand;
use color_eyre::Result;
use color_eyre::eyre::eyre;

use super::Feature;
use crate::app_settings::AppSettings;

impl Feature {
    /// Whether saved `settings` turn this on in this build, without applying
    /// them to this process's switches (`labs::apply`). A read-only command
    /// that never loads settings reads the saved file this way instead; the
    /// reader has already enforced any managed lock, and it is checked here
    /// as `encode` does.
    pub fn saved_on(self, settings: &AppSettings) -> bool {
        self.on(settings) && self.shipped() && self.managed().is_none()
    }
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
    // Listing, or handing the change to a running Hex, never writes the file.
    let settings = AppSettings::read()?;
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
    if let Some((_, note)) = GRADUATED.iter().find(|(graduated, _)| *graduated == name) {
        println!("{note}");
        return Ok(());
    }
    let feature = resolve(name, Feature::shipped)?;
    if let Some(lock) = feature.managed() {
        return Err(eyre!(lock.refusal()));
    }
    if feature == Feature::VoiceFilter
        && !on
        && crate::audio::hex_microphone::driver_bundle_installed()
    {
        return Err(eyre!(
            "voice-filter stays on while the Hex Microphone driver is installed; uninstall the driver in its pane to turn it off"
        ));
    }
    if !on && let Some(why) = feature.off_refusal(&settings) {
        return Err(eyre!(why));
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
    let mut settings = AppSettings::load()?;
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
/// Features that left Labs: `hex labs` still knows their names.
const GRADUATED: [(&str, &str); 2] = [
    (
        "custom-models",
        "custom-models graduated: adding a model file and models found on this computer are always on now",
    ),
    (
        "meeting-platform-icons",
        "meeting-platform-icons graduated: the Meetings list always shows each recording's call app icon now",
    ),
];

pub(super) fn resolve(name: &str, reaches: impl Fn(Feature) -> bool + Copy) -> Result<Feature> {
    if let Some((_, note)) = GRADUATED.iter().find(|(graduated, _)| *graduated == name) {
        return Err(eyre!("{note}"));
    }
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
            "{:<width$}  {:<7}  {:<8}  {} (hex settings {})\n",
            feature.name(),
            if feature.managed().is_some() {
                "managed"
            } else if on {
                "on"
            } else {
                "off"
            },
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
