//! The Windows install's executable name.
//!
//! Discord's game detection matches a running process named `hex.exe` to the
//! Steam game *HEX: Shards of Fate* (detectable application
//! `496778870104653824`, whose only executable is that bare name), so a
//! Windows install runs `hex-voice.exe` instead. A terminal still runs `hex`:
//! PATHEXT resolves it to `hex.com`, the console entry point beside the GUI
//! executable, which starts it with the same arguments.
//!
//! Releases before the rename were installed by an updater that always wrote
//! `hex.exe`, so an install still running under the old name moves itself on
//! its first launch (`updater/windows.rs`, `take_over_name`).

use std::path::{Path, PathBuf};

/// The GUI executable in the managed version layout.
pub const EXECUTABLE: &str = "hex-voice.exe";

/// What releases before the rename installed and launched.
pub const LEGACY_EXECUTABLE: &str = "hex.exe";

/// The `hex-voice.exe` beside a managed
/// `<support>\versions\<version>\hex.exe` that is still running under its old
/// name and that `current-version` names; `None` for any other executable,
/// including one already renamed, a stale version's copy, and a loose file.
pub fn renamed(running: &Path, support: &Path, current: &str) -> Option<PathBuf> {
    if !running
        .file_name()?
        .to_str()?
        .eq_ignore_ascii_case(LEGACY_EXECUTABLE)
    {
        return None;
    }
    let version = running.parent()?;
    // Windows paths compare without case, and current_exe and the data folder
    // can spell the same profile differently.
    if !version
        .parent()?
        .to_string_lossy()
        .eq_ignore_ascii_case(&support.join("versions").to_string_lossy())
    {
        return None;
    }
    let name = version.file_name()?.to_str()?;
    if !super::protocol::is_version_name(name) || name != current.trim() {
        return None;
    }
    Some(version.join(EXECUTABLE))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("hex-name-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path.canonicalize().unwrap()
    }

    #[test]
    fn only_a_managed_legacy_install_moves_to_the_new_name() {
        let root = temporary("rename");
        let support = root.join("hex");
        let version = support.join("versions/3.3.0");
        fs::create_dir_all(&version).unwrap();
        let running = version.join(LEGACY_EXECUTABLE);
        fs::write(&running, b"hex").unwrap();
        let running = running.canonicalize().unwrap();
        let support = support.canonicalize().unwrap();

        assert_eq!(
            renamed(&running, &support, "3.3.0"),
            Some(version.canonicalize().unwrap().join(EXECUTABLE))
        );
        // A stale version's executable must not take the launch surfaces from
        // the version `current-version` names.
        assert_eq!(renamed(&running, &support, "3.4.0"), None);
        assert_eq!(renamed(&running, &support, ""), None);
        // Already renamed: nothing to do.
        assert_eq!(renamed(&version.join(EXECUTABLE), &support, "3.3.0"), None);
        // A copy outside the version layout belongs to nobody.
        let loose = support.join(LEGACY_EXECUTABLE);
        fs::write(&loose, b"hex").unwrap();
        assert_eq!(renamed(&loose, &support, "3.3.0"), None);
        // Another data folder's layout is not this install's.
        let elsewhere = temporary("rename-elsewhere");
        fs::create_dir_all(elsewhere.join("versions/3.3.0")).unwrap();
        assert_eq!(renamed(&running, &elsewhere, "3.3.0"), None);
        // The version folder must be one the updater could have made.
        let junk = support.join("versions/not a version");
        fs::create_dir_all(&junk).unwrap();
        fs::write(junk.join(LEGACY_EXECUTABLE), b"hex").unwrap();
        assert_eq!(
            renamed(&junk.join(LEGACY_EXECUTABLE), &support, "not a version"),
            None
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(elsewhere);
    }
}
