//! The one OpenCode client both desktop platforms share: executable
//! discovery, the `opencode2 api` transport with deadlines and
//! cancellation, the model catalog, and text generation. The shared dictation
//! policy and both native desktop roots use it for mode rewriting; Windows also
//! drives Voice Action with it.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

const MAX_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
pub const VOICE_ACTION_PROTOCOL_PROMPT: &str = "You execute a one-off voice instruction. When selected text is provided, transform or use it as instructed. When no text is selected, generate the requested text. Return only the exact paste-ready result without an explanation, label, alternative, or Markdown fence.";

/// A generation model reference in OpenCode's request shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Model {
    #[serde(rename = "providerID")]
    pub provider: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub variant: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ModelChoice {
    pub key: String,
    pub name: String,
    pub provider: String,
    // Variants drive the macOS Thinking selector; Windows has no UI for
    // them yet.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub variants: Vec<String>,
}

impl ModelChoice {
    /// The request-shaped model this catalog entry selects.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn model(&self) -> Model {
        let id = self
            .key
            .strip_prefix(&self.provider)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(&self.key)
            .to_string();
        Model {
            provider: self.provider.clone(),
            id,
            variant: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModelCatalog {
    pub models: Vec<ModelChoice>,
    pub default_key: Option<String>,
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub default_name: Option<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct DefaultModelResponse {
    data: Option<ModelInfo>,
}

#[derive(Clone, Deserialize)]
struct ModelInfo {
    #[serde(rename = "modelID")]
    model_id: String,
    #[serde(rename = "providerID")]
    provider_id: String,
    name: String,
    enabled: bool,
    status: String,
    capabilities: ModelCapabilities,
    variants: Vec<ModelVariant>,
}

#[derive(Clone, Deserialize)]
struct ModelCapabilities {
    output: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct ModelVariant {
    id: String,
}

#[derive(Serialize)]
struct Request<'a> {
    prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a Model>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Response {
    Success { data: ResponseData },
    Error { message: String },
}

#[derive(Deserialize)]
struct ResponseData {
    text: String,
}

/// The voice-action prompt both platforms send, so the model is steered
/// identically regardless of OS.
pub fn voice_action_prompt(
    instruction: &str,
    application: Option<&str>,
    browser_host: Option<&str>,
    selected_text: Option<&str>,
) -> String {
    let application = application.unwrap_or("unknown");
    let host = browser_host.unwrap_or("none");
    let selection = selected_text.map_or_else(
        || "No text was selected. Generate the requested paste-ready text.".into(),
        |text| format!("Selected text ({} UTF-8 bytes):\n{text}", text.len()),
    );
    format!(
        "{VOICE_ACTION_PROTOCOL_PROMPT}\n\nForeground application: {application}\nBrowser host: {host}\n\nDictated instruction ({} UTF-8 bytes):\n{instruction}\n\n{selection}",
        instruction.len()
    )
}

pub fn opencode_installed() -> bool {
    let executable = opencode_executable();
    if executable.components().count() > 1 {
        return is_executable(&executable);
    }
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| is_executable(&directory.join(&executable)))
    })
}

pub fn load_model_catalog() -> Result<ModelCatalog> {
    let models: ModelsResponse = opencode_api("/api/model")?;
    let default: DefaultModelResponse = opencode_api("/api/model/default")?;
    Ok(build_model_catalog(models.data, default.data))
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn generate(prompt: &str, model: Option<&Model>, deadline: Duration) -> Result<String> {
    generate_cancellable(prompt, model, deadline, &AtomicBool::new(false))
}

pub fn generate_cancellable(
    prompt: &str,
    model: Option<&Model>,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<String> {
    let request = Request { prompt, model };
    let data = serde_json::to_string(&request)?;
    let started = Instant::now();
    for attempt in 0..2 {
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(eyre!(
                "opencode2 /api/generate exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        let mut command = opencode_command()?;
        command
            .args(["post", "/api/generate", "--data", &data])
            .stdin(Stdio::null());
        let output = run_command(command, remaining, "/api/generate", cancelled)?;
        let status = output.status;
        if !status.success() {
            return Err(eyre!(
                "opencode2 exited with {status}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let response: Response = serde_json::from_slice(&output.stdout)
            .wrap_err("opencode2 returned an invalid generation response")?;
        match response {
            Response::Success { data } => return Ok(data.text),
            Response::Error { message } if attempt == 0 && retryable_generation_error(&message) => {
                tracing::warn!(%message, "OpenCode model unavailable during startup; retrying");
                wait_for_generation_retry(started, deadline, cancelled)?;
            }
            Response::Error { message } => {
                return Err(eyre!("OpenCode generation failed: {message}"));
            }
        }
    }
    unreachable!("generation loop returns on its second attempt")
}

fn retryable_generation_error(message: &str) -> bool {
    message.starts_with("Model unavailable:")
}

fn wait_for_generation_retry(
    started: Instant,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<()> {
    let wait_started = Instant::now();
    while wait_started.elapsed() < Duration::from_millis(1_500) {
        if cancelled.load(Ordering::Acquire) {
            return Err(eyre!("opencode2 /api/generate was cancelled"));
        }
        if started.elapsed() >= deadline {
            return Err(eyre!(
                "opencode2 /api/generate exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn model_is_available(model: &ModelInfo) -> bool {
    model.enabled
        && model.status == "active"
        && model
            .capabilities
            .output
            .iter()
            .any(|output| output == "text")
}

fn build_model_catalog(models: Vec<ModelInfo>, default: Option<ModelInfo>) -> ModelCatalog {
    let mut seen = std::collections::HashSet::new();
    let mut choices: Vec<ModelChoice> = models
        .into_iter()
        .filter(model_is_available)
        .filter_map(|model| {
            let key = format!("{}/{}", model.provider_id, model.model_id);
            seen.insert(key.clone()).then_some(ModelChoice {
                key,
                name: model.name,
                provider: model.provider_id,
                variants: model
                    .variants
                    .into_iter()
                    .map(|variant| variant.id)
                    .collect(),
            })
        })
        .collect();
    if let Some(model) = default.as_ref().filter(|model| model_is_available(model)) {
        let key = format!("{}/{}", model.provider_id, model.model_id);
        if seen.insert(key.clone()) {
            choices.push(ModelChoice {
                key,
                name: model.name.clone(),
                provider: model.provider_id.clone(),
                variants: model
                    .variants
                    .iter()
                    .map(|variant| variant.id.clone())
                    .collect(),
            });
        }
    }
    ModelCatalog {
        default_key: default
            .as_ref()
            .map(|model| format!("{}/{}", model.provider_id, model.model_id)),
        default_name: default.map(|model| model.name),
        models: choices,
    }
}

fn opencode_api<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T> {
    let mut command = opencode_command()?;
    command.args(["get", path]).stdin(Stdio::null());
    let output = run_command(
        command,
        Duration::from_secs(10),
        path,
        &AtomicBool::new(false),
    )?;
    if !output.status.success() {
        return Err(eyre!(
            "OpenCode {path} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .wrap_err_with(|| format!("OpenCode {path} returned invalid JSON"))?;
    if let Some(message) = value.get("message").and_then(|message| message.as_str())
        && value.get("_tag").is_some()
    {
        return Err(eyre!("OpenCode {path} failed: {message}"));
    }
    serde_json::from_value(value)
        .wrap_err_with(|| format!("OpenCode {path} returned an invalid response"))
}

struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_command(
    mut command: Command,
    deadline: Duration,
    operation: &str,
    cancelled: &AtomicBool,
) -> Result<CommandOutput> {
    let mut tree = ProcessTree::prepare(&mut command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                eyre!("opencode2 is not installed")
            } else {
                eyre!("could not start opencode2 for {operation}: {error}")
            }
        })?;
    tree.adopt(&child);
    let stdout = child.stdout.take().expect("opencode2 stdout must be piped");
    let stderr = child.stderr.take().expect("opencode2 stderr must be piped");
    let stdout = thread::spawn(move || read_output(stdout));
    let stderr = thread::spawn(move || read_output(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate(&mut child, &tree);
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(error).wrap_err("could not inspect opencode2");
            }
        }
        if cancelled.load(Ordering::Acquire) {
            terminate(&mut child, &tree);
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(eyre!("opencode2 {operation} was cancelled"));
        }
        if started.elapsed() >= deadline {
            terminate(&mut child, &tree);
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(eyre!(
                "opencode2 {operation} exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    tree.kill(&child);
    let stdout = stdout
        .join()
        .map_err(|_| eyre!("opencode2 stdout reader panicked"))??;
    let stderr = stderr
        .join()
        .map_err(|_| eyre!("opencode2 stderr reader panicked"))??;
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

fn terminate(child: &mut Child, tree: &ProcessTree) {
    tree.kill(child);
    let _ = child.kill();
    let _ = child.wait();
}

/// Kills every process the spawned CLI started, so the pipe reader threads
/// always reach EOF before the joins above: a process group on unix, a job
/// object on Windows.
struct ProcessTree {
    #[cfg(windows)]
    job: Option<windows_job::Job>,
}

impl ProcessTree {
    fn prepare(command: &mut Command) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            Self {}
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
            Self {
                job: windows_job::Job::kill_on_close(),
            }
        }
    }

    fn adopt(&mut self, child: &Child) {
        // Descendants spawned after this inherit the job automatically; the
        // unix process group needs no assignment at all.
        #[cfg(windows)]
        if let Some(job) = &self.job
            && !job.assign(child)
        {
            self.job = None;
        }
        #[cfg(unix)]
        let _ = child;
    }

    fn kill(&self, child: &Child) {
        #[cfg(unix)]
        // run_command creates a new process group whose ID is the child PID.
        unsafe {
            libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
        }
        #[cfg(windows)]
        {
            let _ = child;
            if let Some(job) = &self.job {
                job.terminate();
            }
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

    pub(super) struct Job(HANDLE);

    impl Job {
        /// A job that kills its remaining members when the handle closes, so
        /// even an early return reaps the whole tree.
        pub(super) fn kill_on_close() -> Option<Self> {
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&info).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if set == 0 {
                    CloseHandle(handle);
                    return None;
                }
                Some(Self(handle))
            }
        }

        pub(super) fn assign(&self, child: &Child) -> bool {
            unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) != 0 }
        }

        pub(super) fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn read_output(mut output: impl Read) -> Result<Vec<u8>> {
    read_bounded_output(&mut output, MAX_OUTPUT_BYTES)
}

fn read_bounded_output(mut output: impl Read, max_bytes: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    output
        .by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .wrap_err("could not read opencode2 output")?;
    if bytes.len() as u64 > max_bytes {
        Err(eyre!("opencode2 output exceeded {max_bytes} bytes"))
    } else {
        Ok(bytes)
    }
}

fn opencode_command() -> Result<Command> {
    opencode_command_in(&crate::app_paths::opencode_workspace()?)
}

fn opencode_command_in(workspace: &Path) -> Result<Command> {
    std::fs::create_dir_all(workspace).wrap_err_with(|| {
        format!(
            "could not create OpenCode workspace {}",
            workspace.display()
        )
    })?;
    let workspace = workspace.canonicalize().wrap_err_with(|| {
        format!(
            "could not resolve OpenCode workspace {}",
            workspace.display()
        )
    })?;
    let directory = workspace
        .to_str()
        .ok_or_else(|| eyre!("OpenCode workspace path is not valid UTF-8"))?;
    // Windows canonicalize returns a verbatim path; OpenCode wants the
    // plain drive form.
    let directory = directory.strip_prefix(r"\\?\").unwrap_or(directory);
    let directory = encode_uri_component(directory);
    let mut command = Command::new(opencode_executable());
    command.current_dir(&workspace).args([
        "api",
        "--header",
        &format!("x-opencode-directory:{directory}"),
    ]);
    Ok(command)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(windows)]
    {
        // Spawning only ever runs .exe files; npm's extensionless sh shims
        // must not count as installed.
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            && path.is_file()
    }
}

fn opencode_executable() -> PathBuf {
    let current_directory = std::env::current_dir().ok();
    find_opencode_executable(
        std::env::var_os("VOICE_CONTROL_OPENCODE_CLI").map(PathBuf::from),
        std::env::var_os("PATH").as_deref(),
        dirs::home_dir().as_deref(),
        current_directory.as_deref(),
    )
    .unwrap_or_else(|| "opencode2".into())
}

/// Binary names the CLI ships under, newest first: `@opencode-ai/cli`
/// installs `opencode2`, the original package installs `opencode`. A full
/// discovery pass runs per name, so any `opencode2` beats every `opencode`.
const OPENCODE_BINARY_NAMES: [&str; 2] = ["opencode2", "opencode"];

fn find_opencode_executable(
    configured: Option<PathBuf>,
    path: Option<&OsStr>,
    home: Option<&Path>,
    current_directory: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(configured) = configured {
        return Some(absolute_path(configured, current_directory));
    }
    OPENCODE_BINARY_NAMES
        .into_iter()
        .find_map(|name| find_named_executable(name, path, home, current_directory))
}

#[cfg(windows)]
fn find_named_executable(
    name: &str,
    path: Option<&OsStr>,
    home: Option<&Path>,
    current_directory: Option<&Path>,
) -> Option<PathBuf> {
    let _ = home;
    // Prefer the real CLI binary that npm shims wrap: JSON request
    // bodies survive .exe argv quoting but are shredded by cmd shims.
    path.into_iter()
        .flat_map(std::env::split_paths)
        .flat_map(|directory| {
            [
                directory.join(format!(r"node_modules\@opencode-ai\cli\bin\{name}.exe")),
                directory.join(format!("{name}.exe")),
            ]
        })
        .map(|candidate| absolute_path(candidate, current_directory))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn find_named_executable(
    name: &str,
    path: Option<&OsStr>,
    home: Option<&Path>,
    current_directory: Option<&Path>,
) -> Option<PathBuf> {
    let path_candidates = path
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|directory| absolute_path(directory.join(name), current_directory));
    let home_candidates = home.into_iter().flat_map(|home| {
        [
            home.join(format!(".bun/bin/{name}")),
            home.join(format!("Library/pnpm/{name}")),
            home.join(format!("Library/pnpm/bin/{name}")),
            home.join(format!(".yarn/bin/{name}")),
            home.join(format!(".config/yarn/global/node_modules/.bin/{name}")),
            home.join(format!(".npm-global/bin/{name}")),
            home.join(format!(".local/bin/{name}")),
            home.join(format!("bin/{name}")),
            home.join(format!(".volta/bin/{name}")),
            home.join(format!(".asdf/shims/{name}")),
            home.join(format!(".local/share/mise/shims/{name}")),
            home.join(format!(".nodenv/shims/{name}")),
        ]
    });
    path_candidates
        .chain(home_candidates)
        .chain(version_manager_candidates(home, name))
        .chain([
            PathBuf::from(format!("/opt/homebrew/bin/{name}")),
            PathBuf::from(format!("/usr/local/bin/{name}")),
        ])
        .find(|path| is_executable(path))
}

#[cfg(unix)]
fn version_manager_candidates(home: Option<&Path>, name: &str) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let roots = [
        (home.join(".nvm/versions/node"), format!("bin/{name}")),
        (
            home.join("Library/Application Support/fnm/node-versions"),
            format!("installation/bin/{name}"),
        ),
        (
            home.join(".local/share/fnm/node-versions"),
            format!("installation/bin/{name}"),
        ),
        (
            home.join(".fnm/node-versions"),
            format!("installation/bin/{name}"),
        ),
    ];
    roots
        .into_iter()
        .flat_map(|(root, suffix)| {
            std::fs::read_dir(root)
                .into_iter()
                .flatten()
                .flatten()
                .take(64)
                .map(move |entry| entry.path().join(&suffix))
        })
        .collect()
}

fn absolute_path(path: PathBuf, current_directory: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        current_directory
            .map(|directory| directory.join(&path))
            .unwrap_or(path)
    }
}

fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            encoded.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_components_escape_reserved_characters() {
        assert_eq!(
            encode_uri_component("C:\\Users\\x y"),
            "C%3A%5CUsers%5Cx%20y"
        );
        assert_eq!(encode_uri_component("abc123"), "abc123");
    }

    #[test]
    fn prompts_carry_selection_and_instruction_sizes() {
        let prompt = voice_action_prompt("make it shorter", Some("code"), None, Some("hello"));
        assert!(prompt.contains("Foreground application: code"));
        assert!(prompt.contains("Browser host: none"));
        assert!(prompt.contains("Dictated instruction (15 UTF-8 bytes)"));
        assert!(prompt.contains("Selected text (5 UTF-8 bytes):\nhello"));

        let prompt = voice_action_prompt("write a haiku", None, None, None);
        assert!(prompt.contains("Foreground application: unknown"));
        assert!(prompt.contains("No text was selected."));
    }

    #[test]
    fn model_catalog_keeps_an_available_default_missing_from_the_model_list() {
        let catalog = build_model_catalog(
            Vec::new(),
            Some(ModelInfo {
                model_id: "gemini-3.6-flash".into(),
                provider_id: "opencode".into(),
                name: "Gemini 3.6 Flash".into(),
                enabled: true,
                status: "active".into(),
                capabilities: ModelCapabilities {
                    output: vec!["text".into()],
                },
                variants: vec![ModelVariant { id: "high".into() }],
            }),
        );

        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].key, "opencode/gemini-3.6-flash");
        assert_eq!(catalog.models[0].variants, ["high"]);
        assert_eq!(
            catalog.models[0].model(),
            Model {
                provider: "opencode".into(),
                id: "gemini-3.6-flash".into(),
                variant: None,
            }
        );
    }

    #[test]
    fn opencode_requests_are_scoped_to_the_hex_workspace() {
        let root = std::env::temp_dir().join(format!(
            "hex-opencode-workspace-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let root = root.join("scope %2F café");

        let command = opencode_command_in(&root).unwrap();
        let workspace = root.canonicalize().unwrap();
        let directory = workspace.to_str().unwrap();
        let directory = directory.strip_prefix(r"\\?\").unwrap_or(directory);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(command.get_current_dir(), Some(workspace.as_path()));
        assert_eq!(
            args,
            [
                "api",
                "--header",
                &format!("x-opencode-directory:{}", encode_uri_component(directory))
            ]
        );
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }

    #[test]
    fn command_output_is_bounded() {
        let error = read_bounded_output(std::io::Cursor::new(b"12345"), 4)
            .expect_err("oversized output unexpectedly succeeded");

        assert!(error.to_string().contains("exceeded 4 bytes"));
    }

    #[test]
    fn only_model_unavailability_is_retried() {
        assert!(retryable_generation_error(
            "Model unavailable: openai/gpt-5.6-sol"
        ));
        assert!(!retryable_generation_error("Provider request failed"));
        assert!(!retryable_generation_error("processor returned empty text"));
    }

    #[test]
    fn generation_retry_wait_observes_cancellation() {
        let cancelled = AtomicBool::new(true);
        let started = Instant::now();

        let error = wait_for_generation_retry(started, Duration::from_secs(30), &cancelled)
            .expect_err("cancelled retry unexpectedly waited");

        assert!(error.to_string().contains("was cancelled"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn configured_relative_executable_is_resolved_before_the_workspace_changes() {
        let current = Path::new("/tmp/hex-launch");

        let found = find_opencode_executable(
            Some(PathBuf::from("bin/opencode2")),
            None,
            None,
            Some(current),
        );

        assert_eq!(found, Some(current.join("bin/opencode2")));
    }

    /// A unique temp root per test, so discovery tests never see each
    /// other's fake installs.
    fn discovery_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "hex-opencode-{label}-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ))
    }

    /// Creates a file `is_executable` accepts: a mode-0o755 script on unix,
    /// any regular `.exe` file on Windows.
    fn write_fake_executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = path.metadata().unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn standard_package_manager_location_finds_opencode_outside_the_gui_path() {
        let root = discovery_root("discovery");
        let executable = root.join("Library/pnpm/bin/opencode2");
        write_fake_executable(&executable);

        let found = find_opencode_executable(
            None,
            Some(OsStr::new("/usr/bin:/bin")),
            Some(&root),
            Some(&root),
        );

        assert_eq!(found, Some(executable));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn version_manager_discovery_is_bounded_to_known_roots() {
        let root = discovery_root("nvm");
        let executable = root.join(".nvm/versions/node/v24.4.1/bin/opencode2");
        write_fake_executable(&executable);

        let found = find_opencode_executable(None, None, Some(&root), Some(&root));

        assert_eq!(found, Some(executable));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn opencode2_wins_over_an_opencode_earlier_on_path() {
        let root = discovery_root("prefer2");
        let old = root.join("first/opencode");
        let new = root.join("second/opencode2");
        write_fake_executable(&old);
        write_fake_executable(&new);
        let path = std::env::join_paths([root.join("first"), root.join("second")]).unwrap();

        let found = find_opencode_executable(None, Some(&path), None, Some(&root));

        assert_eq!(found, Some(new));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn plain_opencode_is_found_when_opencode2_is_absent() {
        let root = discovery_root("plain");
        let on_path = root.join("bin/opencode");
        let in_home = root.join("Library/pnpm/bin/opencode");
        write_fake_executable(&on_path);
        write_fake_executable(&in_home);
        let path = std::env::join_paths([root.join("bin")]).unwrap();

        let found = find_opencode_executable(None, Some(&path), Some(&root), Some(&root));
        assert_eq!(found, Some(on_path.clone()));

        std::fs::remove_file(&on_path).unwrap();
        let found = find_opencode_executable(None, Some(&path), Some(&root), Some(&root));
        assert_eq!(found, Some(in_home));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn version_manager_locations_find_the_plain_opencode_name() {
        let root = discovery_root("nvm-plain");
        let executable = root.join(".nvm/versions/node/v24.4.1/bin/opencode");
        write_fake_executable(&executable);

        let found = find_opencode_executable(None, None, Some(&root), Some(&root));

        assert_eq!(found, Some(executable));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn opencode2_wins_over_an_opencode_earlier_on_path() {
        let root = discovery_root("prefer2");
        let old = root.join(r"first\opencode.exe");
        let new = root.join(r"second\opencode2.exe");
        write_fake_executable(&old);
        write_fake_executable(&new);
        let path = std::env::join_paths([root.join("first"), root.join("second")]).unwrap();

        let found = find_opencode_executable(None, Some(&path), None, Some(&root));

        assert_eq!(found, Some(new));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn plain_opencode_is_found_when_opencode2_is_absent() {
        let root = discovery_root("plain");
        let shim = root.join(r"bin\opencode.exe");
        let real = root.join(r"bin\node_modules\@opencode-ai\cli\bin\opencode.exe");
        write_fake_executable(&shim);
        write_fake_executable(&real);
        let path = std::env::join_paths([root.join("bin")]).unwrap();

        // The real npm-installed binary beats the shim exe alongside it.
        let found = find_opencode_executable(None, Some(&path), None, Some(&root));
        assert_eq!(found, Some(real.clone()));

        std::fs::remove_file(&real).unwrap();
        let found = find_opencode_executable(None, Some(&path), None, Some(&root));
        assert_eq!(found, Some(shim));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cancelled_generation_process_is_killed_promptly() {
        let mut command = Command::new("/bin/sleep");
        command.arg("5");
        let cancelled = AtomicBool::new(true);
        let started = Instant::now();

        let error = match run_command(
            command,
            Duration::from_secs(5),
            "test generation",
            &cancelled,
        ) {
            Ok(_) => panic!("cancelled command unexpectedly completed"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("was cancelled"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn descendant_holding_output_cannot_outlive_the_command() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 5 &"]);
        let started = Instant::now();

        let _ = run_command(
            command,
            Duration::from_millis(500),
            "test descendant",
            &AtomicBool::new(false),
        );

        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn model_requests_use_the_opencode_shape() {
        let model = Model {
            provider: "opencode".into(),
            id: "nemotron-3.5-lightning-free".into(),
            variant: None,
        };
        let request = Request {
            prompt: "hi",
            model: Some(&model),
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"prompt":"hi","model":{"providerID":"opencode","id":"nemotron-3.5-lightning-free"}}"#
        );
    }

    /// Real round trip through the local OpenCode install; run manually with
    /// `cargo test opencode_generate_smoke -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn opencode_generate_smoke() {
        let catalog = load_model_catalog().expect("catalog");
        let default_key = catalog.default_key.expect("a default model");
        let (provider, id) = default_key.split_once('/').expect("provider/model key");
        let model = Model {
            provider: provider.into(),
            id: id.into(),
            variant: None,
        };
        let text = generate(
            "Reply with exactly the word: pierogi",
            Some(&model),
            Duration::from_secs(45),
        )
        .expect("generation");
        println!("OpenCode replied: {text}");
        assert!(text.to_lowercase().contains("pierogi"));
    }
}
