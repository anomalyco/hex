use std::sync::mpsc::SyncSender;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DeveloperHudState {
    Reset,
    Recording,
    Transcribing,
    Processing,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DeveloperPane {
    Settings,
    #[serde(alias = "replacements")]
    #[value(alias = "replacements")]
    Modes,
    History,
    HudLab,
    Meetings,
    Fun,
    Activity,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum DeveloperCommand {
    Status,
    Hud { state: DeveloperHudState },
    ShowPane { pane: DeveloperPane },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum DeveloperReply {
    Ok,
    State {
        window_open: bool,
        pane: Option<DeveloperPane>,
    },
    Error {
        code: String,
        message: String,
    },
}

/// `hex dev voice-filter`: offline renders, run in the `hex` process before
/// any startup.
#[cfg(target_os = "macos")]
#[derive(clap::Subcommand)]
pub enum VoiceFilterCommand {
    /// Apply a voice-filter mode across one or more dictation spans in a WAV file.
    Render {
        /// Input WAV path (16-bit PCM or 32-bit float).
        input: std::path::PathBuf,
        /// Voice filter mode id (muffled, muted, quiet, normal, chipmunk, robot, walkie, underwater, far).
        #[arg(long, default_value = "muffled")]
        mode: String,
        /// Optional JSON mode spec override (matching presets.json).
        #[arg(long)]
        params: Option<String>,
        /// Dictation span(s) in milliseconds, e.g. `1000-3500`.
        #[arg(long = "dictation")]
        dictation: Vec<String>,
        /// Effect strength (`0.0` to `1.0`).
        #[arg(long, default_value_t = 1.0)]
        strength: f64,
        /// Output WAV path.
        #[arg(short = 'o', long = "output")]
        output: std::path::PathBuf,
    },
}

#[cfg(target_os = "macos")]
impl VoiceFilterCommand {
    pub fn run(&self) -> color_eyre::Result<()> {
        let Self::Render {
            input,
            mode,
            params,
            dictation,
            strength,
            output,
        } = self;
        crate::audio::voice_filter::render_wav_command(
            input,
            mode,
            params.as_deref(),
            dictation,
            *strength,
            output,
        )
    }
}

pub struct DeveloperCall {
    pub command: DeveloperCommand,
    pub reply: SyncSender<DeveloperReply>,
}

impl DeveloperReply {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}
