//! `hex annotate inspect|zoom|frames|skill`: reading a capture bundle for a
//! coding agent. These run in this process on every OS — they only read files
//! and decode the bundle's video — instead of over the running Hex's control
//! socket, and Labs › Agent tools for captures gates them.

use clap::Args;

use super::fail;

/// Why these commands refuse while the Labs feature is off, naming its switch.
const LAB_OFF: &str = "Agent tools for captures is a Labs feature and is off. Turn it on with `hex labs on annotate-agent-tools` or in Settings › Labs.";

/// What `hex annotate skill` prints, and what the Annotate pane's Agents
/// section writes into an agent's folder: the skill an agent installs.
pub(crate) const SKILL: &str = include_str!("../../../skills/hex-annotate/SKILL.md");

#[derive(Args, Debug)]
pub(crate) struct Inspect {
    /// A bundle name, path, or `latest`.
    #[arg(default_value = "latest")]
    capture: String,
    /// List the marks instead of the summary.
    #[arg(long)]
    marks: bool,
    /// Print JSON instead of text: scrawl.json, or with --marks the marks.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct Zoom {
    /// A bundle name, path, or `latest`.
    #[arg(default_value = "latest")]
    capture: String,
    /// The moment: a time (`0:12`, `12.5`) or a phrase from the narration.
    #[arg(long, required_unless_present = "mark", conflicts_with = "mark")]
    at: Option<String>,
    /// A mark: its number in the summary (`2` for ✎②) or its id (`m2`).
    #[arg(long)]
    mark: Option<String>,
    /// The keyframe whose ✎① or ✎② --mark counts, when several have one.
    #[arg(long, requires = "mark")]
    frame: Option<usize>,
    /// Seconds of video to cover: a quarter before the moment, the rest after.
    #[arg(long, default_value_t = 2.0)]
    span: f64,
    /// Frames in the sheet.
    #[arg(long, default_value_t = 12)]
    frames: usize,
    /// Show the whole screen even when the bundle's keyframes are auto-cropped.
    #[arg(long)]
    full: bool,
}

#[derive(Args, Debug)]
pub(crate) struct Frames {
    /// A bundle name, path, or `latest`.
    #[arg(default_value = "latest")]
    capture: String,
    /// Start time (`0:02` or `2.0`).
    #[arg(long)]
    from: String,
    /// End time (`0:05` or `5.0`).
    #[arg(long)]
    to: String,
    /// Frames per second to extract (1 to 240).
    #[arg(long, default_value_t = 4)]
    fps: u32,
}

/// `hex annotate inspect`.
pub(super) fn inspect(args: Inspect) -> u8 {
    print_text(require_lab().and_then(|()| inspect_capture(&args)))
}

/// `hex annotate zoom`.
pub(super) fn zoom(args: Zoom) -> u8 {
    print_text(require_lab().and_then(|()| zoom_capture(&args)))
}

/// `hex annotate frames`.
pub(super) fn frames(args: Frames) -> u8 {
    print_text(require_lab().and_then(|()| frames_capture(&args)))
}

/// `hex annotate skill`: the guide this Hex ships (`skills/hex-annotate`).
pub(super) fn skill() -> u8 {
    print_text(require_lab().map(|()| SKILL.to_owned()))
}

/// [`LAB_OFF`] unless this build and the saved `settings.json` turn the Labs
/// feature on. Read-only: it never writes settings.
fn require_lab() -> Result<(), String> {
    lab_check(crate::labs::annotate_agent_tools())
}

fn lab_check(on: bool) -> Result<(), String> {
    if on { Ok(()) } else { Err(LAB_OFF.into()) }
}

fn print_text(result: Result<String, String>) -> u8 {
    match result {
        Ok(text) => {
            use std::io::Write;
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(text.as_bytes());
            let _ = stdout.flush();
            0
        }
        Err(message) => fail(&message),
    }
}

#[cfg(hex_capture)]
fn inspect_capture(args: &Inspect) -> Result<String, String> {
    crate::capture_host::annotate_read::inspect(&args.capture, args.marks, args.json)
        .map_err(|error| error.message)
}

#[cfg(not(hex_capture))]
fn inspect_capture(_: &Inspect) -> Result<String, String> {
    Err(unsupported())
}

#[cfg(hex_capture)]
fn zoom_capture(args: &Zoom) -> Result<String, String> {
    crate::capture_host::annotate_read::zoom(
        &args.capture,
        args.at.as_deref(),
        args.mark.as_deref(),
        args.frame,
        args.span,
        args.frames,
        args.full,
    )
    .map_err(|error| error.message)
}

#[cfg(not(hex_capture))]
fn zoom_capture(_: &Zoom) -> Result<String, String> {
    Err(unsupported())
}

#[cfg(hex_capture)]
fn frames_capture(args: &Frames) -> Result<String, String> {
    crate::capture_host::annotate_read::frames(&args.capture, &args.from, &args.to, args.fps)
        .map_err(|error| error.message)
}

#[cfg(not(hex_capture))]
fn frames_capture(_: &Frames) -> Result<String, String> {
    Err(unsupported())
}

/// A build without the capture engine has nothing to read.
#[cfg_attr(hex_capture, allow(dead_code))]
fn unsupported() -> String {
    "Annotate isn't available on this OS yet".into()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::control::cli::{CaptureCommand, ControlCli};

    /// The shared command tree every OS's `hex` flattens; the top-level `Cli`
    /// is macOS's own, so tests parse beside it as `transcript_cli_tests` does.
    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: Option<ControlCli>,
    }

    fn parse(arguments: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(
            ["hex", "annotate"]
                .into_iter()
                .chain(arguments.iter().copied()),
        )
    }

    fn command(arguments: &[&str]) -> CaptureCommand {
        let parsed = parse(arguments).expect("the command parses");
        match parsed.command {
            Some(ControlCli::Annotate { command }) => command,
            _ => panic!("expected annotate"),
        }
    }

    #[test]
    fn the_read_commands_parse() {
        match command(&["inspect", "latest", "--marks", "--json"]) {
            CaptureCommand::Inspect(args) => {
                assert_eq!(args.capture, "latest");
                assert!(args.marks && args.json);
            }
            other => panic!("expected inspect, got {other:?}"),
        }
        match command(&["inspect"]) {
            CaptureCommand::Inspect(args) => assert_eq!(args.capture, "latest"),
            other => panic!("expected inspect, got {other:?}"),
        }
        match command(&[
            "zoom", "0:12", "--at", "0:12", "--span", "1.5", "--frames", "16",
        ]) {
            CaptureCommand::Zoom(args) => {
                assert_eq!(args.capture, "0:12");
                assert_eq!(args.at.as_deref(), Some("0:12"));
                assert_eq!(args.span, 1.5);
                assert_eq!(args.frames, 16);
                assert!(!args.full);
            }
            other => panic!("expected zoom, got {other:?}"),
        }
        match command(&["zoom", "--mark", "2", "--frame", "3", "--full"]) {
            CaptureCommand::Zoom(args) => {
                assert_eq!(args.mark.as_deref(), Some("2"));
                assert_eq!(args.frame, Some(3));
                assert!(args.full);
                assert_eq!(args.span, 2.0);
                assert_eq!(args.frames, 12);
            }
            other => panic!("expected zoom, got {other:?}"),
        }
        match command(&["frames", "--from", "0:04", "--to", "0:06"]) {
            CaptureCommand::Frames(args) => {
                assert_eq!(args.capture, "latest");
                assert_eq!(args.fps, 4);
            }
            other => panic!("expected frames, got {other:?}"),
        }
        assert!(matches!(command(&["skill"]), CaptureCommand::Skill));
    }

    #[test]
    fn zoom_requires_exactly_one_moment_and_a_frame_needs_a_mark() {
        let parse = |arguments: &[&str]| {
            parse(
                &["zoom"]
                    .into_iter()
                    .chain(arguments.iter().copied())
                    .collect::<Vec<_>>(),
            )
        };
        assert!(parse(&[]).is_err(), "no --at and no --mark");
        assert!(parse(&["--at", "0:01", "--mark", "2"]).is_err(), "both");
        assert!(parse(&["--frame", "2"]).is_err(), "--frame needs --mark");
        assert!(parse(&["--mark", "2"]).is_ok());
        assert!(parse(&["--at", "0:01"]).is_ok());
    }

    #[test]
    fn the_off_message_names_its_switch() {
        assert!(lab_check(true).is_ok());
        assert_eq!(lab_check(false), Err(LAB_OFF.into()));
        assert!(LAB_OFF.contains("hex labs on annotate-agent-tools"));
        assert!(LAB_OFF.contains("Settings › Labs"));
    }

    #[test]
    fn skill_is_the_bundled_guide() {
        assert!(SKILL.starts_with("---\nname: hex-annotate\n"));
        assert!(SKILL.contains("npx skills add anomalyco/hex --skill hex-annotate"));
        assert!(SKILL.contains("labs.annotate_agent_tools"));
    }
}
