pub(crate) mod app;
pub(crate) mod atspi;
pub(crate) mod audio_control;
pub(crate) mod clipboard;
pub(crate) mod context;
pub(crate) mod dictation;
pub(crate) mod icons;
// Keep `linux_input::tests` in the test path for check-linux.yml's Xvfb filter.
#[path = "linux/input.rs"]
pub(crate) mod linux_input;
pub(crate) use linux_input as input;
pub(crate) mod input_route;
pub(crate) mod login_item;
pub(crate) mod paste;
pub(crate) mod portal;
pub(crate) mod remote_desktop;
pub(crate) mod rune_hud;
pub(crate) mod service;
pub(crate) mod session;
pub(crate) mod tray;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use clap::{Parser, Subcommand};
use color_eyre::Result;

use crate::control::cli::ControlCli;
use crate::linux::service::{Activation, Launch};

#[derive(Parser)]
#[command(
    name = "hex",
    version = crate::VERSION,
    about = "Local, observable voice recognition (Linux beta)",
    long_about = "Local, observable voice recognition (Linux beta).\n\n\
        Control commands (status, dictation, settings, model, window, …) drive the one running \
        Hex over its owner-only socket and never launch it, except `start` and `restart`. Every \
        one takes --json for a versioned reply (docs/cli.md). Exit codes: 0 done or nothing to \
        change, 1 refused or failed, 2 usage, 3 Hex isn't running.",
    after_help = "Run `hex <command> --help` for details, or `hex settings list` for every setting."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    #[command(flatten)]
    Control(ControlCli),
    /// Open the Hex window, starting Hex when it is not running (the default).
    App {
        /// Start Hex in the background without opening the window.
        #[arg(long)]
        hidden: bool,
        /// Arguments for a headless render, such as `--snap out.png --scene dark`.
        #[arg(last = true)]
        rune_args: Vec<String>,
    },
    /// Run Hex in the foreground without opening the window (systemd's entry point).
    Service {
        /// Run only the local API as a direct child whose stdin is owned by
        /// the host application (the TypeScript SDK).
        #[arg(long, hide = true)]
        embedded: bool,
    },
    /// Quit Hex after its accepted work finishes (same as `hex quit`).
    #[command(hide = true)]
    Stop,
    /// Run X11 or Wayland hotkey dictation and automatic paste until interrupted.
    Dictate {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
    },
    /// List microphone devices visible through ALSA.
    Devices,
}

pub(crate) fn run(shutdown: &'static AtomicBool) -> Result<()> {
    let activation = crate::linux::service::Activation::take_from_environment();
    crate::linux::service::refresh_x_authority();
    color_eyre::install()?;
    let command = Cli::parse().command.unwrap_or(Command::App {
        hidden: false,
        rune_args: Vec::new(),
    });
    // Control and read commands talk to the running Hex or read files: no
    // log, lock, or Hex of their own.
    let command = match command {
        Command::Control(command) => crate::control::cli::finish(crate::control::cli::run(command)),
        Command::Stop => crate::control::cli::finish(crate::control::cli::send(
            crate::control::ControlCommand::Quit { force: false },
            Default::default(),
        )),
        command => command,
    };
    let log_dir = crate::app_paths::init_process_logging(shutdown)?;

    let event_path = log_dir.join("live.ndjson");
    match command {
        // Rune reads its own flags (`--snap`, `--scene`, …) from the process arguments.
        Command::App { rune_args, .. } if !rune_args.is_empty() => {
            crate::rune_ui::render();
            Ok(())
        }
        Command::App { hidden, .. } => launch(&activation, !hidden, event_path, shutdown),
        Command::Service { embedded: true } => {
            crate::local_api::run_service(&log_dir, event_path, true, shutdown)
        }
        Command::Service { embedded: false } => {
            match crate::linux::app::run(event_path, shutdown, None) {
                // A unit start that lost the race to another launch has nothing to do.
                Err(_) if crate::linux::service::another_instance_answers() => Ok(()),
                result => result,
            }
        }
        Command::Dictate { device } => {
            let _instance = crate::instance::acquire("listener")?;
            crate::labs::log_launch();
            let settings = crate::app_settings::AppSettings::load()?;
            crate::linux::dictation::initialize_feedback(&settings);
            crate::linux::dictation::run(
                &event_path,
                device.as_deref(),
                shutdown,
                &settings,
                crate::linux::dictation::DictationOutput::from_settings(
                    &settings,
                    crate::history::History::open_default(settings.history_retention).ok(),
                ),
                None,
                Default::default(),
                // Only shortcuts drive a foreground listener.
                std::sync::mpsc::channel().1.into(),
                std::sync::mpsc::channel().1,
            )
        }
        Command::Devices => {
            for device in crate::audio::input_device_names()? {
                println!("{device}");
            }
            Ok(())
        }
        Command::Control(_) | Command::Stop => unreachable!("handled before logging"),
    }
}

/// Forwards to the running Hex, starts its systemd unit, or becomes Hex.
fn launch(
    activation: &Activation,
    show: bool,
    event_path: PathBuf,
    shutdown: &'static AtomicBool,
) -> Result<()> {
    loop {
        match crate::linux::service::launch(activation, show)? {
            Launch::Forwarded => return Ok(()),
            Launch::BecomeInstance => {}
        }
        match crate::linux::app::run(
            event_path.clone(),
            shutdown,
            show.then(|| activation.clone()),
        ) {
            // Another launch claimed the data root first; forward to it.
            Err(_) if crate::linux::service::another_instance_answers() => continue,
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Cli, Command, ControlCli};

    #[test]
    fn the_shared_control_commands_parse_beside_linuxs_own() {
        Cli::command().debug_assert();
        let parse = |arguments: &[&str]| Cli::try_parse_from(arguments).map(|cli| cli.command);
        for arguments in [
            &["hex", "status", "--json"][..],
            &["hex", "dictation", "stop", "--print"],
            &["hex", "paste-last"],
            &["hex", "settings", "set", "sound_effects", "off"],
            &["hex", "model", "use", "parakeet_v3", "--language", "pl"],
            &["hex", "model", "install"],
            &["hex", "model", "status"],
            &["hex", "window", "show", "history"],
            &["hex", "vocabulary", "add", "Rune"],
            &["hex", "history", "clear", "--yes"],
            &["hex", "check-for-updates"],
            &["hex", "launch-at-login", "on"],
            &["hex", "logs", "-n", "5"],
            &["hex", "meetings", "list"],
            &["hex", "mcp"],
            &["hex", "commands", "check"],
            &["hex", "onboarding", "reset"],
            &["hex", "annotate", "status"],
            &["hex", "capture", "draw"],
            &["hex", "labs"],
            &["hex", "restart", "--force"],
            &["hex", "start"],
        ] {
            assert!(
                matches!(parse(arguments), Ok(Some(Command::Control(_)))),
                "{arguments:?}"
            );
        }
        assert!(matches!(
            parse(&["hex", "quit", "--force"]),
            Ok(Some(Command::Control(ControlCli::Quit { force: true, .. })))
        ));
        assert!(matches!(parse(&["hex", "stop"]), Ok(Some(Command::Stop))));
        assert!(matches!(
            parse(&["hex", "app", "--hidden"]),
            Ok(Some(Command::App { hidden: true, .. }))
        ));
        assert!(matches!(parse(&["hex"]), Ok(None)));
    }
}
