use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use color_eyre::eyre::{Result, eyre};
use tracing_subscriber::fmt::writer::MakeWriterExt;

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
    #[cfg(target_os = "macos")]
    if is_macos_test_build() {
        return Ok(data.join(data_folder::TEST));
    }
    #[cfg(target_os = "macos")]
    return Ok(data_folder::resolve(&data));
    #[cfg(not(target_os = "macos"))]
    Ok(data.join("voice-control"))
}

/// Renames the macOS `voice-control` data folder to `hex` once, leaving a
/// `voice-control -> hex` symlink for external readers. Call before anything
/// opens a file in Application Support; log the outcome once logging is up.
#[cfg(target_os = "macos")]
pub fn migrate_data_folder() -> Option<data_folder::Outcome> {
    if std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_some() || is_macos_test_build() {
        return None;
    }
    let data = dirs::data_dir()?;
    Some(data_folder::migrate(&data, data_folder::rename_exclusive))
}

#[cfg(target_os = "macos")]
pub fn log_data_folder_migration(outcome: &data_folder::Outcome) {
    use data_folder::Outcome;
    match outcome {
        Outcome::Current => {}
        Outcome::Migrated {
            symlink_error: None,
        } => tracing::info!("moved the data folder from voice-control to hex"),
        Outcome::Migrated {
            symlink_error: Some(error),
        } => tracing::warn!(
            %error,
            "moved the data folder to hex but could not create the voice-control symlink"
        ),
        Outcome::RenameFailed(error) => tracing::warn!(
            %error,
            "could not rename the voice-control data folder to hex; still using voice-control"
        ),
        Outcome::Conflict => tracing::warn!(
            "both voice-control and hex data folders exist; leaving both and using voice-control"
        ),
    }
}

/// Whether the default macOS data folder in use is `hex`.
#[cfg(target_os = "macos")]
pub fn uses_hex_data_folder() -> bool {
    std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_none()
        && support_dir().is_ok_and(|path| path.ends_with(data_folder::CURRENT))
}

#[cfg(target_os = "macos")]
pub mod data_folder {
    use std::path::{Path, PathBuf};

    pub const LEGACY: &str = "voice-control";
    pub const CURRENT: &str = "hex";
    /// `Hex Test.app`'s folder; never migrated or shared with `CURRENT`.
    pub const TEST: &str = "hex-test";

    #[derive(Debug)]
    pub enum Outcome {
        /// No legacy folder; `hex` is used (fresh install or already migrated).
        Current,
        Migrated {
            symlink_error: Option<std::io::Error>,
        },
        /// The rename failed; `voice-control` stays in use until a later launch.
        RenameFailed(std::io::Error),
        /// Both folders are real directories; neither is touched and the legacy
        /// folder, which holds the existing data, stays in use.
        Conflict,
    }

    /// The legacy folder is in use while it exists and is anything other than
    /// the compatibility symlink to `hex`.
    fn legacy_in_use(data: &Path) -> bool {
        let legacy = data.join(LEGACY);
        if is_compat_link(data) {
            return false;
        }
        legacy.exists()
    }

    fn is_compat_link(data: &Path) -> bool {
        std::fs::read_link(data.join(LEGACY))
            .is_ok_and(|target| target == Path::new(CURRENT) || target == data.join(CURRENT))
    }

    /// The folder to use, without changing anything on disk.
    pub fn resolve(data: &Path) -> PathBuf {
        if legacy_in_use(data) {
            data.join(LEGACY)
        } else {
            data.join(CURRENT)
        }
    }

    pub fn migrate(data: &Path, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> Outcome {
        let legacy = data.join(LEGACY);
        let current = data.join(CURRENT);
        if !legacy_in_use(data) {
            // Retry a compatibility symlink an earlier launch could not create.
            if current.is_dir() && legacy.symlink_metadata().is_err() {
                let _ = link(&legacy);
            }
            return Outcome::Current;
        }
        if current.symlink_metadata().is_ok() {
            return Outcome::Conflict;
        }
        if let Err(error) = rename(&legacy, &current) {
            // A concurrent launch may have just completed the same rename.
            if legacy.symlink_metadata().is_err() && current.exists() {
                return Outcome::Migrated {
                    symlink_error: link(&legacy).err(),
                };
            }
            if !legacy_in_use(data) {
                return Outcome::Current;
            }
            return Outcome::RenameFailed(error);
        }
        Outcome::Migrated {
            symlink_error: link(&legacy).err(),
        }
    }

    fn link(legacy: &Path) -> std::io::Result<()> {
        match std::os::unix::fs::symlink(CURRENT, legacy) {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if is_compat_link(legacy.parent().unwrap_or(Path::new("."))) {
                    Ok(())
                } else {
                    Err(error)
                }
            }
            result => result,
        }
    }

    /// One atomic `rename(2)` that refuses to replace an existing `to`, even an
    /// empty directory created by a concurrent process.
    pub fn rename_exclusive(from: &Path, to: &Path) -> std::io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: Both arguments are valid NUL-terminated paths for the call.
        let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        if status == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;

        fn temp(name: &str) -> PathBuf {
            let path = std::env::temp_dir().join(format!(
                "hex-data-folder-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            path
        }

        fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
            rename_exclusive(from, to)
        }

        #[test]
        fn fresh_install_uses_hex_without_touching_disk() {
            let data = temp("fresh");
            assert!(matches!(migrate(&data, rename), Outcome::Current));
            assert_eq!(resolve(&data), data.join(CURRENT));
            assert!(fs::read_dir(&data).unwrap().next().is_none());
            fs::remove_dir_all(data).unwrap();
        }

        #[test]
        fn legacy_folder_moves_to_hex_behind_a_compat_symlink() {
            let data = temp("legacy");
            fs::create_dir_all(data.join(LEGACY).join("logs")).unwrap();
            fs::write(data.join(LEGACY).join("settings.json"), "{}").unwrap();
            fs::write(data.join(LEGACY).join("listener.lock"), "").unwrap();
            assert_eq!(resolve(&data), data.join(LEGACY));

            let outcome = migrate(&data, rename);
            assert!(matches!(
                outcome,
                Outcome::Migrated {
                    symlink_error: None
                }
            ));
            let current = data.join(CURRENT);
            assert!(current.symlink_metadata().unwrap().is_dir());
            assert_eq!(
                fs::read_link(data.join(LEGACY)).unwrap(),
                Path::new(CURRENT)
            );
            assert_eq!(
                fs::read_to_string(current.join("settings.json")).unwrap(),
                "{}"
            );
            assert!(data.join(LEGACY).join("logs").is_dir());
            assert_eq!(resolve(&data), current);
            fs::remove_dir_all(data).unwrap();
        }

        #[test]
        fn a_missing_compat_symlink_is_recreated() {
            let data = temp("relink");
            fs::create_dir_all(data.join(CURRENT)).unwrap();
            assert!(matches!(migrate(&data, rename), Outcome::Current));
            assert_eq!(
                fs::read_link(data.join(LEGACY)).unwrap(),
                Path::new(CURRENT)
            );
            fs::remove_dir_all(data).unwrap();
        }

        #[test]
        fn existing_hex_folder_leaves_both_untouched() {
            let data = temp("both");
            fs::create_dir_all(data.join(LEGACY)).unwrap();
            fs::create_dir_all(data.join(CURRENT)).unwrap();
            assert!(matches!(migrate(&data, rename), Outcome::Conflict));
            assert!(data.join(LEGACY).symlink_metadata().unwrap().is_dir());
            assert!(data.join(CURRENT).symlink_metadata().unwrap().is_dir());
            assert_eq!(resolve(&data), data.join(LEGACY));
            fs::remove_dir_all(data).unwrap();
        }

        #[test]
        fn compat_symlink_already_present_is_a_no_op() {
            let data = temp("migrated");
            fs::create_dir_all(data.join(CURRENT)).unwrap();
            std::os::unix::fs::symlink(CURRENT, data.join(LEGACY)).unwrap();
            assert!(matches!(migrate(&data, rename), Outcome::Current));
            assert_eq!(resolve(&data), data.join(CURRENT));
            assert_eq!(
                fs::read_link(data.join(LEGACY)).unwrap(),
                Path::new(CURRENT)
            );
            fs::remove_dir_all(data).unwrap();
        }

        #[test]
        fn failed_rename_keeps_using_the_legacy_folder() {
            let data = temp("failed");
            fs::create_dir_all(data.join(LEGACY)).unwrap();
            let outcome = migrate(&data, |_, _| {
                Err(std::io::Error::from_raw_os_error(libc::EXDEV))
            });
            assert!(matches!(outcome, Outcome::RenameFailed(_)));
            assert!(data.join(LEGACY).symlink_metadata().unwrap().is_dir());
            assert!(data.join(CURRENT).symlink_metadata().is_err());
            assert_eq!(resolve(&data), data.join(LEGACY));
            fs::remove_dir_all(data).unwrap();
        }

        #[test]
        fn a_concurrent_launch_that_already_renamed_counts_as_migrated() {
            let data = temp("race");
            fs::create_dir_all(data.join(LEGACY)).unwrap();
            let outcome = migrate(&data, |from, to| {
                fs::rename(from, to).unwrap();
                std::os::unix::fs::symlink(CURRENT, from).unwrap();
                Err(std::io::Error::from_raw_os_error(libc::ENOENT))
            });
            assert!(
                matches!(outcome, Outcome::Current | Outcome::Migrated { .. }),
                "{outcome:?}"
            );
            assert_eq!(
                fs::read_link(data.join(LEGACY)).unwrap(),
                Path::new(CURRENT)
            );
            assert_eq!(resolve(&data), data.join(CURRENT));
            fs::remove_dir_all(data).unwrap();
        }

        #[test]
        fn exclusive_rename_never_replaces_an_empty_directory() {
            let data = temp("exclusive");
            fs::create_dir_all(data.join(LEGACY)).unwrap();
            fs::create_dir_all(data.join(CURRENT)).unwrap();
            assert!(rename_exclusive(&data.join(LEGACY), &data.join(CURRENT)).is_err());
            assert!(data.join(LEGACY).is_dir());
            fs::remove_dir_all(data).unwrap();
        }
    }
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(support_dir()?.join("logs"))
}

/// Where the `capture` experiment writes its bundles.
#[cfg(hex_capture)]
pub fn captures_dir() -> Result<PathBuf> {
    Ok(support_dir()?.join("captures"))
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
