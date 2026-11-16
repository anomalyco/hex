//! `hex completions <shell>`: a completion script for `hex`, generated at
//! runtime from the same clap definitions the CLI parses, so it stays in sync
//! with every command and flag instead of being written out by hand. It works
//! with Hex running or stopped.

use std::io::Write;

use clap::Command;
use clap_complete::Shell;

/// `hex completions <shell>`: the script on stdout and nothing else.
pub(super) fn run(shell: Shell, mut top_level: Command) -> u8 {
    let script = script(shell, &mut top_level);
    let mut stdout = std::io::stdout().lock();
    // A reader that stops early (`hex completions bash | head`) is not a
    // failure: a closed pipe has nowhere to write and needs no message.
    match stdout.write_all(&script).and_then(|()| stdout.flush()) {
        Ok(()) => 0,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(error) => super::fail(&format!("couldn't write the completion script: {error}")),
    }
}

/// The completion script `shell` runs for `command`'s tree, whose binary is
/// `hex`.
fn script(shell: Shell, command: &mut Command) -> Vec<u8> {
    let mut script = Vec::new();
    clap_complete::generate(shell, command, "hex", &mut script);
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser, Subcommand};

    /// The shape every OS's top-level `Cli` has: a `hex` command whose
    /// subcommands are the flattened [`super::super::ControlCli`] beside that
    /// OS's own.
    #[derive(Parser)]
    #[command(name = "hex")]
    struct Top {
        #[command(subcommand)]
        command: Option<TopCommand>,
    }

    #[derive(Subcommand)]
    enum TopCommand {
        #[command(flatten)]
        Control(super::super::ControlCli),
    }

    fn generated(shell: Shell) -> String {
        String::from_utf8(script(shell, &mut Top::command())).unwrap()
    }

    #[test]
    fn every_shell_gets_a_script_with_the_command_tree() {
        for shell in [
            Shell::Bash,
            Shell::Elvish,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Zsh,
        ] {
            let script = generated(shell);
            assert!(!script.is_empty(), "{shell:?} has no script");
            assert!(
                script.contains("dictation"),
                "{shell:?} script has no dictation"
            );
            assert!(
                script.contains("completions"),
                "{shell:?} script has no completions"
            );
        }
    }
}
