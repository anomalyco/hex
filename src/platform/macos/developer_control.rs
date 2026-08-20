use std::sync::mpsc::SyncSender;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeveloperHudState {
    Reset,
    Recording,
    Transcribing,
    Processing,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeveloperPane {
    Settings,
    Modes,
    VoiceAction,
    Replacements,
    History,
    HudLab,
    Commands,
    Meetings,
    Activity,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum DeveloperCommand {
    Status,
    Hud { state: DeveloperHudState },
    ShowPane { pane: DeveloperPane },
    SetCommandsEnabled { enabled: bool },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum DeveloperReply {
    Ok,
    State {
        window_open: bool,
        pane: Option<DeveloperPane>,
        commands_enabled: bool,
    },
    Error {
        code: String,
        message: String,
    },
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
