//! The programs `node:child_process` runs for a script: `execFile` with no
//! shell, `exec` through the platform shell as Node does (`/bin/sh -c` on
//! Unix, `cmd.exe /d /s /c "…"` on Windows), a minimal environment, and a
//! process tree Hex can always end: its own process group on Unix, a job
//! object on Windows. Logs name the program, never its arguments, input,
//! output, or environment.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Node's default `maxBuffer`.
const DEFAULT_MAX_BUFFER: usize = 1024 * 1024;
/// The most output of either stream a script can ask to keep.
const MAX_MAX_BUFFER: usize = 16 << 20;

/// What `execFile` or `exec` asks for, as `child_process.js` sends it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecRequest {
    /// The program, or with `shell` the whole command line.
    pub file: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub shell: bool,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Milliseconds before Hex sends the process tree SIGTERM; 0 is none.
    pub timeout: Option<f64>,
    pub max_buffer: Option<f64>,
    /// Written to stdin, which is then closed. Without it stdin is empty.
    pub input: Option<String>,
}

/// How a process ended, as `child_process.js` turns it into Node's error.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResult {
    /// The error code (`ENOENT`, `EACCES`, …) when it never started.
    pub spawn_error: Option<String>,
    pub code: Option<i32>,
    pub signal: Option<String>,
    pub stdout: String,
    pub stderr: String,
    /// The stream (`stdout` or `stderr`) that outgrew `maxBuffer`.
    pub max_buffer: Option<&'static str>,
    /// Hex ended it at the request's `timeout`.
    pub timed_out: bool,
}

impl ExecRequest {
    /// The program's name for logs: the file, or a command's first word.
    pub fn program(&self) -> String {
        let program = if self.shell {
            self.file.split_whitespace().next().unwrap_or("")
        } else {
            self.file.as_str()
        };
        Path::new(program)
            .file_name()
            .map_or_else(|| program.to_owned(), |name| name.to_string_lossy().into())
    }

    fn max_buffer(&self) -> usize {
        self.max_buffer
            .filter(|limit| *limit >= 0.0)
            .map_or(DEFAULT_MAX_BUFFER, |limit| {
                limit.min(MAX_MAX_BUFFER as f64) as usize
            })
    }
}

/// Starts `request` in `workspace` unless it names another `cwd`. `done`
/// runs once, on another thread, when the process and its output streams
/// have ended, or at once when it cannot start. The returned tree ends
/// everything it started.
pub fn start(
    request: ExecRequest,
    workspace: &Path,
    done: impl FnOnce(ExecResult) + Send + 'static,
) -> Option<ProcessTree> {
    let program = request.program();
    let mut command = if request.shell {
        shell(&request.file)
    } else {
        let mut command = crate::desktop::background_process::command(&request.file);
        command.args(&request.args);
        command
    };
    command.env_clear();
    command.envs(base_environment());
    command.envs(&request.env);
    command.current_dir(request.cwd.as_deref().map_or(workspace, Path::new));
    command
        .stdin(if request.input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    own_process_group(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            tracing::warn!(target: "hex_config", program, error = %error, "exec could not start");
            done(ExecResult {
                spawn_error: Some(error_code(&error).into()),
                ..ExecResult::default()
            });
            return None;
        }
    };
    let tree = match ProcessTree::of(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            tracing::warn!(target: "hex_config", program, error = %error, "exec could not own its process tree");
            done(ExecResult {
                spawn_error: Some(error_code(&error).into()),
                ..ExecResult::default()
            });
            return None;
        }
    };
    let waiter = tree.clone();
    let limit = request.max_buffer();
    let input = request.input;
    let spawned = thread::Builder::new()
        .name("hex-config-exec".into())
        .spawn(move || {
            let started = Instant::now();
            let result = wait(&mut child, input, limit, &waiter);
            tracing::info!(
                target: "hex_config",
                program,
                code = result.code,
                signal = result.signal,
                duration_ms = started.elapsed().as_millis() as u64,
                stdout_bytes = result.stdout.len(),
                stderr_bytes = result.stderr.len(),
                "exec"
            );
            done(result);
        });
    if let Err(error) = spawned {
        tree.kill();
        tracing::warn!(target: "hex_config", error = %error, "exec could not start its waiter");
        return None;
    }
    Some(tree)
}

fn wait(child: &mut Child, input: Option<String>, limit: usize, tree: &ProcessTree) -> ExecResult {
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let ((stdout, stdout_over), (stderr, stderr_over)) = thread::scope(|scope| {
        if let (Some(mut stdin), Some(input)) = (stdin, input) {
            scope.spawn(move || {
                let _ = stdin.write_all(input.as_bytes());
            });
        }
        let errors = scope.spawn(|| read_bounded(stderr, limit, tree));
        let output = read_bounded(stdout, limit, tree);
        (output, errors.join().unwrap_or_default())
    });
    let (code, signal) = match child.wait() {
        Ok(status) => (status.code(), exit_signal(&status)),
        Err(_) => (None, None),
    };
    ExecResult {
        code,
        signal,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        max_buffer: if stdout_over {
            Some("stdout")
        } else if stderr_over {
            Some("stderr")
        } else {
            None
        },
        ..ExecResult::default()
    }
}

/// Up to `limit` bytes of `stream`; past that it ends the tree, as Node does.
fn read_bounded(stream: Option<impl Read>, limit: usize, tree: &ProcessTree) -> (Vec<u8>, bool) {
    let mut output = Vec::new();
    let Some(mut stream) = stream else {
        return (output, false);
    };
    let mut buffer = [0; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return (output, false),
            Ok(read) if output.len() + read > limit => {
                output.extend_from_slice(&buffer[..limit - output.len()]);
                tree.terminate();
                return (output, true);
            }
            Ok(read) => output.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return (output, false),
        }
    }
}

/// Node's error code for a process that could not start.
fn error_code(error: &std::io::Error) -> &'static str {
    match error.kind() {
        ErrorKind::NotFound => "ENOENT",
        ErrorKind::PermissionDenied => "EACCES",
        ErrorKind::InvalidInput => "EINVAL",
        _ => "EIO",
    }
}

/// The variables a program gets before the script's own `env`: enough to
/// find programs and a home folder, nothing else from Hex's environment.
fn base_environment() -> Vec<(OsString, OsString)> {
    #[cfg(unix)]
    const KEEP: &[&str] = &[
        "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "TMPDIR",
    ];
    #[cfg(windows)]
    const KEEP: &[&str] = &[
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "SystemDrive",
        "windir",
        "ComSpec",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "USERNAME",
        "HOMEDRIVE",
        "HOMEPATH",
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramData",
        "ProgramFiles",
    ];
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut environment: Vec<(OsString, OsString)> = KEEP
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| ((*name).into(), value)))
        .collect();
    // Apps opened from the Finder get launchd's short PATH; add Homebrew's.
    #[cfg(target_os = "macos")]
    {
        let current = std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into());
        let mut paths = std::env::split_paths(&current).collect::<Vec<_>>();
        for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
            if !paths.iter().any(|path| path == Path::new(extra)) {
                paths.push(extra.into());
            }
        }
        if let Ok(path) = std::env::join_paths(paths) {
            environment.retain(|(name, _)| name != "PATH");
            environment.push(("PATH".into(), path));
        }
    }
    environment
}

#[cfg(unix)]
fn shell(command_line: &str) -> Command {
    let mut command = crate::desktop::background_process::command("/bin/sh");
    command.arg("-c").arg(command_line);
    command
}

#[cfg(windows)]
fn shell(command_line: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let interpreter = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
    let mut command = crate::desktop::background_process::command(interpreter);
    command.raw_arg(windows_shell_arguments(command_line));
    command
}

/// Node's `exec` arguments for `cmd.exe`, passed verbatim: the command line
/// stays exactly as written, quotes and all, inside one outer pair that
/// `/s` strips.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_shell_arguments(command_line: &str) -> String {
    format!("/d /s /c \"{command_line}\"")
}

#[cfg(unix)]
fn own_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    // The direct child also ends with the thread that starts it (the script
    // worker), so it never outlives a Hex that stopped without cleaning up.
    #[cfg(target_os = "linux")]
    // SAFETY: `prctl` is async-signal-safe and touches only the child.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
}

#[cfg(windows)]
fn own_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn exit_signal(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|signal| {
        match signal {
            libc::SIGHUP => "SIGHUP",
            libc::SIGINT => "SIGINT",
            libc::SIGQUIT => "SIGQUIT",
            libc::SIGILL => "SIGILL",
            libc::SIGTRAP => "SIGTRAP",
            libc::SIGABRT => "SIGABRT",
            libc::SIGBUS => "SIGBUS",
            libc::SIGFPE => "SIGFPE",
            libc::SIGKILL => "SIGKILL",
            libc::SIGUSR1 => "SIGUSR1",
            libc::SIGSEGV => "SIGSEGV",
            libc::SIGUSR2 => "SIGUSR2",
            libc::SIGPIPE => "SIGPIPE",
            libc::SIGALRM => "SIGALRM",
            libc::SIGTERM => "SIGTERM",
            _ => return format!("SIG{signal}"),
        }
        .into()
    })
}

#[cfg(windows)]
fn exit_signal(_status: &std::process::ExitStatus) -> Option<String> {
    None
}

/// Everything one `exec` started. Ending it reaches the program's own
/// children too.
#[derive(Clone)]
pub struct ProcessTree {
    #[cfg(unix)]
    group: libc::pid_t,
    #[cfg(windows)]
    job: std::sync::Arc<windows_job::Job>,
}

impl ProcessTree {
    #[cfg(unix)]
    fn of(child: &Child) -> std::io::Result<Self> {
        Ok(Self {
            group: child.id() as libc::pid_t,
        })
    }

    #[cfg(windows)]
    fn of(child: &Child) -> std::io::Result<Self> {
        let job = windows_job::Job::new()?;
        job.assign(child)?;
        Ok(Self {
            job: std::sync::Arc::new(job),
        })
    }

    /// Asks the tree to stop: SIGTERM on Unix, as Node's `timeout` and
    /// `kill()` send. Windows has no gentler way than ending the job.
    pub fn terminate(&self) {
        #[cfg(unix)]
        self.signal(libc::SIGTERM);
        #[cfg(windows)]
        self.job.terminate();
    }

    /// Ends the tree at once.
    pub fn kill(&self) {
        #[cfg(unix)]
        self.signal(libc::SIGKILL);
        #[cfg(windows)]
        self.job.terminate();
    }

    #[cfg(unix)]
    fn signal(&self, signal: libc::c_int) {
        if self.group > 0 {
            // SAFETY: a negative pid signals the group this tree created.
            unsafe { libc::kill(-self.group, signal) };
        }
    }
}

#[cfg(windows)]
mod windows_job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };

    /// A job object that ends its processes when Hex closes it, or exits.
    pub struct Job(HANDLE);

    // SAFETY: a job handle may be used and closed from any thread.
    unsafe impl Send for Job {}
    // SAFETY: as above; the job functions used here are thread-safe.
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> std::io::Result<Self> {
            // SAFETY: null attributes and name create an anonymous job.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
                // SAFETY: the structure is plain data; zero is its empty value.
                unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `limits` is the structure this information class reads.
            let set = unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if set == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(job)
        }

        pub fn assign(&self, child: &Child) -> std::io::Result<()> {
            // SAFETY: both handles are open for the duration of the call.
            if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        pub fn terminate(&self) {
            // SAFETY: the handle is open until drop.
            unsafe { TerminateJobObject(self.0, 1) };
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is open and closed only here.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_exec_passes_the_command_line_verbatim_inside_one_quoted_pair() {
        assert_eq!(windows_shell_arguments("echo hi"), "/d /s /c \"echo hi\"");
        assert_eq!(
            windows_shell_arguments(r#""C:\Program Files\tool.exe" "a b" & echo done"#),
            r#"/d /s /c ""C:\Program Files\tool.exe" "a b" & echo done""#
        );
    }

    #[test]
    fn logs_name_only_the_program() {
        let request = |file: &str, shell: bool| ExecRequest {
            file: file.into(),
            args: vec!["secret words".into()],
            shell,
            cwd: None,
            env: BTreeMap::new(),
            timeout: None,
            max_buffer: None,
            input: None,
        };
        assert_eq!(request("/usr/local/bin/pandoc", false).program(), "pandoc");
        assert_eq!(request("tr a-z A-Z < notes.txt", true).program(), "tr");
    }
}
