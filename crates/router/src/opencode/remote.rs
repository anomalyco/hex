//! SSH connection reuse and bounded reads. Remote API calls obey the caller's
//! deadline too; in particular the optional pre-send idle check is only 600 ms.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};

use super::{CLI_STDERR_LIMIT, SERVER_RESPONSE_LIMIT};

pub(super) fn reuse_connection(command: &mut Command) {
    // Windows OpenSSH doesn't support connection multiplexing.
    #[cfg(unix)]
    if crate::env::get().live {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        // Keep below sockaddr_un's 104-byte limit, even on macOS where TMPDIR
        // alone can exceed it. This user owns the directory and its sockets.
        let uid = unsafe { libc::geteuid() };
        let directory = std::path::PathBuf::from(format!("/tmp/hex-router-ssh-{uid}"));
        let _ = std::fs::DirBuilder::new().mode(0o700).create(&directory);
        if let Ok(meta) = std::fs::symlink_metadata(&directory)
            && meta.is_dir()
            && meta.uid() == uid
            && meta.mode() & 0o777 == 0o700
        {
            command.args(["-o", "ControlMaster=auto", "-o", "ControlPersist=60"]);
            command
                .arg("-o")
                .arg(format!("ControlPath={}/%C", directory.display()));
        }
    }
    #[cfg(not(unix))]
    let _ = command;
}

/// Drain both pipes while waiting, keeping their byte limits and one wall-clock
/// deadline. A remote command that never closes a pipe cannot extend the wait.
pub(super) fn bounded_output(mut command: Command, timeout: Duration) -> Result<Output> {
    let deadline = Instant::now() + timeout;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Could not reach the agent host")?;
    let stdout = child.stdout.take().context("No agent host output")?;
    let stderr = child.stderr.take().context("No agent host stderr")?;
    let (output_tx, output_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| {
            let mut bytes = Vec::new();
            stdout
                .take(SERVER_RESPONSE_LIMIT + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= SERVER_RESPONSE_LIMIT,
                "OpenCode response too large"
            );
            Ok::<_, anyhow::Error>(bytes)
        })();
        let _ = output_tx.send(result);
    });
    let (error_tx, error_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut kept = Vec::new();
        let _ = (&mut stderr).take(CLI_STDERR_LIMIT).read_to_end(&mut kept);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        let _ = error_tx.send(kept);
    });
    let left = || deadline.saturating_duration_since(Instant::now());
    let result = (|| {
        let stdout = output_rx
            .recv_timeout(left())
            .context("Agent host request timed out")??;
        let stderr = error_rx
            .recv_timeout(left())
            .context("Agent host request timed out")?;
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .context("Could not wait for the agent host")?
            {
                break status;
            }
            ensure!(!left().is_zero(), "Agent host request timed out");
            std::thread::park_timeout(left().min(Duration::from_millis(5)));
        };
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })();
    if result.is_err() {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_hung_remote_command_obeys_the_idle_check_deadline() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 5"]);
        let start = Instant::now();
        let error = bounded_output(command, super::super::IDLE_CHECK).unwrap_err();
        eprintln!("METRIC hung_idle_check_ms={}", start.elapsed().as_millis());
        assert!(error.to_string().contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_descendant_holding_the_pipe_cannot_extend_the_deadline() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 5 & exit 0"]);
        let start = Instant::now();
        assert!(bounded_output(command, Duration::from_millis(80)).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn an_oversized_response_is_rejected_before_waiting_for_exit() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "head -c 16777217 /dev/zero; sleep 5"]);
        let error = bounded_output(command, Duration::from_secs(3)).unwrap_err();
        assert!(
            error.to_string().contains("response too large"),
            "{error:#}"
        );
    }
}
