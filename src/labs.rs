//! Labs: one switch for every unfinished feature (Voice Filter, Live text,
//! Live chapters, the playground, and the new onboarding). It is
//! `labs.enabled` in `settings.json`, on by default in every build so people
//! can try the features and tell us how they work; `hex labs off` or the
//! switch on Settings › Labs turns all of them off at once.
//!
//! Gate a Labs feature where it starts (the worker, thread, or capture that
//! does its work), not only where its UI shows: a hidden pane whose worker
//! still runs is still on.

use std::sync::atomic::{AtomicU8, Ordering};

use clap::Subcommand;
use color_eyre::Result;

/// Where people send feedback on Labs features.
pub const FEEDBACK_URL: &str = "https://github.com/anomalyco/hex/issues/new";

const UNKNOWN: u8 = 0;
const OFF: u8 = 1;
const ON: u8 = 2;

/// This process's switch: unknown until settings load or it is first read.
static STATE: AtomicU8 = AtomicU8::new(UNKNOWN);

/// Whether Labs is on for this process.
pub fn enabled() -> bool {
    match STATE.load(Ordering::Acquire) {
        UNKNOWN => {
            let on = crate::app_settings::AppSettings::saved_labs();
            let _ =
                STATE.compare_exchange(UNKNOWN, encode(on), Ordering::AcqRel, Ordering::Acquire);
            STATE.load(Ordering::Acquire) == ON
        }
        state => state == ON,
    }
}

/// Records the switch the loaded or saved settings hold.
pub(crate) fn apply(on: bool) {
    STATE.store(encode(on), Ordering::Release);
}

const fn encode(on: bool) -> u8 {
    if on { ON } else { OFF }
}

/// Logs the switch to process.log; call once logging is up in a long-lived Hex.
pub fn log_launch() {
    tracing::info!(labs = enabled(), "labs");
}

#[derive(Debug, Subcommand)]
pub enum LabsCommand {
    /// Turn every Labs feature on.
    On,
    /// Turn every Labs feature off.
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
