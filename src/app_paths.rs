use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use color_eyre::eyre::{Result, eyre};
use tracing_subscriber::fmt::writer::MakeWriterExt;

pub fn support_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("HEX_APPLICATION_SUPPORT_DIR") {
        return Ok(path.into());
    }
    Ok(dirs::data_dir()
        .ok_or_else(|| eyre!("application data directory is unavailable"))?
        .join("voice-control"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(support_dir()?.join("logs"))
}

#[cfg(target_os = "macos")]
pub fn scrawl_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SCRAWL_CAPTURES_DIR") {
        return Ok(path.into());
    }
    Ok(support_dir()?.join("scrawl"))
}

/// Creates the logs directory, appends process diagnostics to `process.log`
/// alongside stderr, installs the global tracing subscriber, and routes Ctrl-C
/// to `shutdown`. Returns the logs directory.
pub fn init_process_logging(shutdown: &'static AtomicBool) -> Result<PathBuf> {
    let log_dir = logs_dir()?;
    fs::create_dir_all(&log_dir)?;
    let process_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("process.log"))?;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("voice_control=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr.and(Mutex::new(process_log)))
        .init();
    ctrlc::set_handler(|| shutdown.store(true, Ordering::Relaxed))?;
    Ok(log_dir)
}

pub fn opencode_workspace() -> Result<PathBuf> {
    Ok(support_dir()?.join("opencode"))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn local_api_discovery_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("local-api.json"))
}

pub fn personal_commands_status_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("personal-commands.json"))
}

pub fn personal_commands_workspace() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| eyre!("home directory is unavailable"))?
        .join(".config/hex"))
}

#[cfg(target_os = "macos")]
pub fn personal_commands_sdk() -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    if let Some(contents) = executable.parent().and_then(|path| path.parent()) {
        let bundled = contents.join("Resources/commands-sdk");
        if bundled.join("dist/bin.js").is_file() {
            return Ok(bundled);
        }
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sdk/commands");
    if source.join("dist/bin.js").is_file() {
        Ok(source)
    } else {
        Err(eyre!("personal command SDK resources were not found"))
    }
}

/// A source checkout's SDK, otherwise the SDK sources embedded in this
/// executable, written once per content hash under Application Support.
#[cfg(not(target_os = "macos"))]
pub fn personal_commands_sdk() -> Result<PathBuf> {
    use std::fmt::Write as _;

    use sha2::{Digest, Sha256};

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sdk/commands");
    if source.join("src/bin.ts").is_file() {
        return Ok(source);
    }
    macro_rules! sdk_files {
        ($($path:literal),* $(,)?) => {
            &[$(($path, include_bytes!(concat!("../sdk/commands/", $path)) as &[u8])),*]
        };
    }
    const FILES: &[(&str, &[u8])] = sdk_files![
        "package.json",
        "src/bin.ts",
        "src/captures.ts",
        "src/effect.ts",
        "src/host.ts",
        "src/index.ts",
        "src/model.ts",
        "src/protocol.ts",
        "workspace-template/AGENTS.md",
        "workspace-template/hex.config.ts",
        "workspace-template/package.json",
        "workspace-template/tsconfig.json",
        "workspace-template/.agents/skills/personal-commands/SKILL.md",
    ];
    let mut hasher = Sha256::new();
    for (path, bytes) in FILES {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
    }
    let mut fingerprint = String::with_capacity(16);
    for byte in &hasher.finalize()[..8] {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to a string cannot fail");
    }
    let root = support_dir()?.join("commands-sdk").join(fingerprint);
    for (relative, bytes) in FILES {
        let destination = root.join(relative);
        if fs::read(&destination).is_ok_and(|existing| existing == *bytes) {
            continue;
        }
        let parent = destination
            .parent()
            .ok_or_else(|| eyre!("embedded SDK path has no parent"))?;
        fs::create_dir_all(parent)?;
        let temporary = destination.with_extension(format!("{}.partial", std::process::id()));
        fs::write(&temporary, bytes)?;
        crate::transcription_models::atomic_replace(&temporary, &destination)?;
    }
    Ok(root)
}
