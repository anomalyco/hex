//! Labs: one switch that shows every unfinished feature (Voice Filter, Live
//! text, Live chapters, the playground, and the new onboarding). It is
//! `labs.enabled` in `settings.json`, on by default in every build so people
//! can try the features and tell us how they work; `hex labs off` or the
//! switch on Settings › Labs hides all of them at once.
//!
//! Labs on only shows a feature. A feature with side effects or background
//! work (a device, a worker, a poll, different dictation output, UI outside
//! its pane) has its own switch, off by default, in its pane or its Labs row,
//! and runs only while both are on. Nothing starts at launch because Labs is.
//! Gate such a feature where it starts (the worker, thread, or capture that
//! does its work), not only where its UI shows.

use std::sync::atomic::{AtomicU8, Ordering};

use clap::Subcommand;
use color_eyre::Result;

use crate::app_settings::LabsSettings;

/// Where people send feedback on Labs features.
pub const FEEDBACK_URL: &str = "https://github.com/anomalyco/hex/issues/new";

/// Set once this process knows its settings.
const KNOWN: u8 = 1;
const ENABLED: u8 = 1 << 1;
const LIVE_TEXT: u8 = 1 << 2;
const LIVE_CHAPTERS: u8 = 1 << 3;

/// This process's switches: unknown until settings load or one is first read.
static STATE: AtomicU8 = AtomicU8::new(0);

fn state() -> u8 {
    match STATE.load(Ordering::Acquire) {
        0 => {
            let saved = encode(&crate::app_settings::AppSettings::saved_labs());
            let _ = STATE.compare_exchange(0, saved, Ordering::AcqRel, Ordering::Acquire);
            STATE.load(Ordering::Acquire)
        }
        state => state,
    }
}

/// Whether Labs is on for this process: its features show.
pub fn enabled() -> bool {
    state() & ENABLED != 0
}

/// Whether Live text runs: Labs and its own switch are on.
pub fn live_text() -> bool {
    state() & (ENABLED | LIVE_TEXT) == ENABLED | LIVE_TEXT
}

/// Whether Live chapters run: Labs and its own switch are on.
pub fn live_chapters() -> bool {
    state() & (ENABLED | LIVE_CHAPTERS) == ENABLED | LIVE_CHAPTERS
}

/// Records the switches the loaded or saved settings hold.
pub(crate) fn apply(settings: &LabsSettings) {
    STATE.store(encode(settings), Ordering::Release);
}

fn encode(settings: &LabsSettings) -> u8 {
    let mut state = KNOWN;
    for (on, bit) in [
        (settings.enabled, ENABLED),
        (settings.live_text, LIVE_TEXT),
        (settings.live_chapters, LIVE_CHAPTERS),
    ] {
        if on {
            state |= bit;
        }
    }
    state
}

/// Logs the switch to process.log; call once logging is up in a long-lived Hex.
pub fn log_launch() {
    tracing::info!(
        labs = enabled(),
        live_text = live_text(),
        live_chapters = live_chapters(),
        "labs"
    );
}

#[derive(Debug, Subcommand)]
pub enum LabsCommand {
    /// Show every Labs feature (each with side effects keeps its own switch).
    On,
    /// Hide every Labs feature and stop them all.
    Off,
    /// Show whether Labs is on.
    Status,
}

/// `hex labs`: edits `settings.json`; a running Hex applies it when it next starts.
pub fn run(command: &LabsCommand) -> Result<()> {
    let mut settings = crate::app_settings::AppSettings::load()?;
    let on = match command {
        LabsCommand::Status => {
            println!(
                "Labs is {}.",
                if settings.labs.enabled { "on" } else { "off" }
            );
            return Ok(());
        }
        LabsCommand::On => true,
        LabsCommand::Off => false,
    };
    let state = if on { "on" } else { "off" };
    if settings.labs.enabled == on {
        println!("Labs is already {state}.");
        return Ok(());
    }
    settings.labs.enabled = on;
    settings.save()?;
    println!("Turned Labs {state}.");
    println!("{RESTART_HINT}");
    Ok(())
}

/// `hex experiments …`, from before Labs was one switch.
pub fn run_retired_experiments() -> Result<()> {
    println!("Experiments are now one switch, Labs: hex labs on|off|status");
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
const RESTART_HINT: &str = "Restart Hex to apply it: hex restart";
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const RESTART_HINT: &str = "Quit and reopen Hex to apply it.";
