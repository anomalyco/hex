//! Voice Action on Windows: a held shortcut records an instruction, the
//! transcription plus the focused application's selected text go through
//! the shared OpenCode client, and its reply is pasted at the cursor.

use std::time::Duration;

use color_eyre::eyre::Result;

pub use crate::opencode::{Model, opencode_installed};

pub const GENERATION_DEADLINE: Duration = Duration::from_secs(45);

/// Fulfil a voice instruction with the configured model; without one the
/// request omits the model and OpenCode's own default answers, matching
/// the behavior before in-app selection existed.
pub fn fulfil(
    instruction: &str,
    application: Option<&str>,
    browser_host: Option<&str>,
    selected_text: Option<&str>,
    model: Option<&Model>,
) -> Result<String> {
    crate::opencode::fulfil_voice_action(
        instruction,
        application,
        browser_host,
        selected_text,
        model,
        GENERATION_DEADLINE,
    )
}
