//! `hex start` and `hex restart`'s relaunch on every OS: the only commands
//! that open Hex. Everything else in the CLI talks to a Hex already running.

#[cfg(not(target_os = "macos"))]
use super::running;
use super::{ControlReply, Output, print};

/// `hex start` on macOS: `open`s the Hex.app this `hex` belongs to, then
/// waits until it answers.
#[cfg(target_os = "macos")]
pub(super) fn start(output: Output) -> u8 {
    use std::time::{Duration, Instant};

    if let Some(pid) = crate::local_api::running_pid() {
        return print(
            ControlReply::unchanged(format!("Hex is already running (pid {pid})")),
            output,
        );
    }
    let arguments = open_arguments(
        installed_bundle().as_deref(),
        crate::app_paths::is_macos_test_build(),
        std::env::vars_os(),
    );
    match std::process::Command::new("/usr/bin/open")
        .args(arguments)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(_) | Err(_) => {
            return print(
                ControlReply::error("failed", "couldn't open Hex.app; is it installed?"),
                output,
            );
        }
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if let Some(pid) = crate::local_api::running_pid() {
            return print(
                ControlReply::done(format!("Started Hex (pid {pid})")),
                output,
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    print(
        ControlReply::done("Opened Hex; it isn't answering yet (setup may be waiting)"),
        output,
    )
}

/// `hex start` on Linux and Windows: a detached `hex app --hidden`, which
/// forwards to the running Hex, starts its systemd unit, or becomes Hex,
/// then waits until it answers.
#[cfg(not(target_os = "macos"))]
pub(super) fn start(output: Output) -> u8 {
    print(start_reply(), output)
}

/// After a restart's quit on Linux and Windows: waits for the old Hex to
/// go, then starts the new one. A Linux unit restarts by itself.
#[cfg(not(target_os = "macos"))]
pub(super) fn reopen_after_restart(old: Option<u32>) -> ControlReply {
    use std::time::{Duration, Instant};

    #[cfg(target_os = "linux")]
    if crate::linux::service::supervised() {
        return ControlReply::done("Restarting Hex");
    }
    // The old process's local API stops before its drain finishes, while the
    // instance it holds (Windows' named event, Linux's lock) is released only
    // when it exits: a successor started in that gap exited at once because
    // another Hex still owned the instance.
    let gone = match old {
        Some(pid) => crate::local_api::wait_for_exit(pid, Duration::from_secs(30)),
        None => {
            let started = Instant::now();
            while running() && started.elapsed() <= Duration::from_secs(30) {
                std::thread::sleep(Duration::from_millis(200));
            }
            !running()
        }
    };
    if !gone {
        return ControlReply::error(
            "failed",
            "Hex is still finishing its work; run `hex start` once it quits",
        );
    }
    start_reply()
}

#[cfg(not(target_os = "macos"))]
fn start_reply() -> ControlReply {
    use std::time::{Duration, Instant};

    if running() {
        return ControlReply::unchanged("Hex is already running");
    }
    #[cfg(target_os = "linux")]
    if crate::linux::service::instance_answers() {
        return super::older_hex();
    }
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return ControlReply::error("failed", format!("couldn't find hex: {error}")),
    };
    let mut command = std::process::Command::new(executable);
    command
        .args(["app", "--hidden"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "linux")]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: no console, and
        // Ctrl-C in this terminal doesn't reach it.
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ControlReply::error("failed", format!("couldn't start Hex: {error}"));
        }
    };
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if running() {
            return ControlReply::done("Started Hex");
        }
        // A launch that exits at once found another Hex still holding the
        // instance, which is what a predecessor that hadn't finished its
        // drain looks like. Waiting the rest of the 20 s would report a Hex
        // that isn't there as one that is still loading its model.
        if matches!(child.try_wait(), Ok(Some(_))) {
            return ControlReply::error(
                "failed",
                "Hex started and exited right away; another Hex is probably still shutting \
                 down, so run `hex start` again",
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    ControlReply::done("Started Hex; it isn't answering yet (it may be loading its model)")
}

/// `open` arguments for `hex start`: the caller's `HEX_*` environment, then
/// the Hex.app this `hex` belongs to, or the installed app by bundle id.
#[cfg(target_os = "macos")]
fn open_arguments(
    bundle: Option<&std::path::Path>,
    test_build: bool,
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<std::ffi::OsString> {
    let mut arguments = crate::desktop::onboarding::open_env_arguments(vars);
    match bundle {
        Some(bundle) => arguments.push(bundle.into()),
        None => arguments.extend([
            "-b".into(),
            if test_build {
                "anomaly.hex.test"
            } else {
                "anomaly.hex"
            }
            .into(),
        ]),
    }
    arguments
}

/// The Hex.app this `hex` belongs to, when it runs from (or links into) one.
#[cfg(target_os = "macos")]
fn installed_bundle() -> Option<std::path::PathBuf> {
    let executable = std::fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    executable
        .ancestors()
        .nth(3)
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(ToOwned::to_owned)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::open_arguments;

    #[test]
    fn start_forwards_the_callers_hex_environment() {
        let vars = || {
            [
                ("HOME", "/Users/alex"),
                ("HEX_RETAIN_DICTATION_AUDIO", "1"),
                ("HEX_ENABLE_LLM", "1"),
            ]
            .map(|(key, value)| (key.into(), value.into()))
        };
        let strings = |arguments: Vec<std::ffi::OsString>| {
            arguments
                .into_iter()
                .map(|argument| argument.into_string().unwrap())
                .collect::<Vec<_>>()
        };
        let env = [
            "--env",
            "HEX_ENABLE_LLM=1",
            "--env",
            "HEX_RETAIN_DICTATION_AUDIO=1",
        ];
        let bundle = std::path::Path::new("/Applications/Hex.app");
        assert_eq!(
            strings(open_arguments(Some(bundle), false, vars())),
            [&env[..], &["/Applications/Hex.app"]].concat()
        );
        assert_eq!(
            strings(open_arguments(None, false, vars())),
            [&env[..], &["-b", "anomaly.hex"]].concat()
        );
        assert_eq!(
            strings(open_arguments(None, true, [])),
            ["-b", "anomaly.hex.test"]
        );
    }
}
