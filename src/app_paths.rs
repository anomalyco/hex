use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use color_eyre::eyre::{Result, eyre};
use tracing_subscriber::fmt::writer::MakeWriterExt;

/// Whether this is a local build (`HEX_LOCAL_BUILD=1`): made from a checkout
/// for its owner's own machine, so it never checks for or offers updates.
pub fn is_local_build() -> bool {
    option_env!("HEX_LOCAL_BUILD") == Some("1")
}

/// Whether this is a macOS `Hex Test.app` build (`HEX_TEST_BUILD=1`), which
/// keeps its data, settings, and user config apart from the installed Hex.
pub fn is_macos_test_build() -> bool {
    cfg!(target_os = "macos") && option_env!("HEX_TEST_BUILD") == Some("1")
}

/// The name menus show, so a test build running beside Hex reads as itself.
#[cfg(target_os = "macos")]
pub fn app_name() -> &'static str {
    if is_macos_test_build() {
        "Hex Test"
    } else {
        "Hex"
    }
}

pub fn support_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("HEX_APPLICATION_SUPPORT_DIR") {
        return Ok(path.into());
    }
    let data =
        dirs::data_dir().ok_or_else(|| eyre!("application data directory is unavailable"))?;
    let folder = if is_macos_test_build() {
        "hex-test"
    } else {
        "hex"
    };
    Ok(data.join(folder))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(support_dir()?.join("logs"))
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
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("hex=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr.and(Mutex::new(process_log)))
        .init();
    log_panics();
    ctrlc::set_handler(|| shutdown.store(true, Ordering::Relaxed))?;
    Ok(log_dir)
}

/// Records every panic in `process.log` before the installed report runs.
/// Desktop apps have no console, so the default stderr report is otherwise
/// lost and a crashed worker thread leaves no trace.
fn log_panics() {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(
            thread = std::thread::current().name().unwrap_or("unnamed"),
            backtrace = %std::backtrace::Backtrace::force_capture(),
            "panic: {info}"
        );
        report(info);
    }));
}

pub fn opencode_workspace() -> Result<PathBuf> {
    Ok(support_dir()?.join("opencode"))
}

#[cfg(target_os = "macos")]
pub fn local_api_discovery_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("local-api.json"))
}

pub fn personal_commands_status_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("personal-commands.json"))
}

pub fn personal_commands_workspace() -> Result<PathBuf> {
    user_config_dir()
}

/// The per-user `experiments` file (`crate::experiments`).
pub fn experiments_file() -> Result<PathBuf> {
    Ok(user_config_dir()?.join("experiments"))
}

/// The per-user config folder holding `hex.config.ts` and `experiments` on
/// every OS: `~/.config/hex`, or `~/.config/hex-test` for `Hex Test.app`.
fn user_config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre!("home directory is unavailable"))?;
    Ok(user_config_dir_in(&home, is_macos_test_build()))
}

fn user_config_dir_in(home: &std::path::Path, test_build: bool) -> PathBuf {
    home.join(".config")
        .join(if test_build { "hex-test" } else { "hex" })
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
        fs::rename(&temporary, &destination)?;
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experiments_live_beside_hex_config_ts_in_each_builds_own_folder() {
        let home = std::path::Path::new("/Users/kit");
        assert_eq!(
            user_config_dir_in(home, false),
            home.join(".config").join("hex")
        );
        assert_eq!(
            user_config_dir_in(home, true),
            home.join(".config").join("hex-test")
        );
        let file = experiments_file().unwrap();
        assert_eq!(file.parent(), personal_commands_workspace().ok().as_deref());
        assert_eq!(
            file,
            user_config_dir_in(&dirs::home_dir().unwrap(), is_macos_test_build())
                .join("experiments")
        );
    }
}
