//! Removes meeting folders left by an audio import that ended before its
//! manifest was written: a crash, or a quit during a long decode. A folder
//! with a manifest is never touched, and a held lock means an import is still
//! using it.

use std::fs;
use std::path::Path;

const MANIFEST: &str = "manifest.json";
const STAGING: &str = "system.importing.wav";

/// Removes abandoned import folders under the meetings root. Runs once per
/// launch, before the pending-finals pass; never fails launch.
pub(crate) fn remove_orphaned_imports() -> usize {
    let Ok(root) = super::root() else {
        return 0;
    };
    remove_orphaned_in(&root)
}

fn remove_orphaned_in(root: &Path) -> usize {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let directory = entry.path();
        if !directory.is_dir() {
            continue;
        }
        // A manifest means a real meeting; never look further.
        if directory.join(MANIFEST).exists() {
            continue;
        }
        if !directory.join(STAGING).exists() {
            continue;
        }
        let active = match super::ActiveMeeting::try_acquire(&directory) {
            Ok(Some(active)) => active,
            Ok(None) => continue,
            Err(error) => {
                tracing::warn!(
                    %error,
                    path = %directory.display(),
                    "could not inspect an abandoned import folder"
                );
                continue;
            }
        };
        // The lock was only a check that no import still writes here; drop it
        // so Windows can remove the folder with the lock file inside.
        drop(active);
        match fs::remove_dir_all(&directory) {
            Ok(()) => {
                removed += 1;
                tracing::info!(
                    path = %directory.display(),
                    "removed an abandoned audio import folder"
                );
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    path = %directory.display(),
                    "could not remove an abandoned audio import folder"
                );
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("hex-meeting-sweep-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn folder(root: &Path, name: &str) -> PathBuf {
        let directory = root.join(name);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn only_abandoned_import_folders_are_removed() {
        let root = temp_root("orphans");
        let orphan = folder(&root, "orphan");
        fs::write(orphan.join(STAGING), b"partial").unwrap();

        let finished = folder(&root, "finished");
        fs::write(finished.join(STAGING), b"partial").unwrap();
        fs::write(finished.join(MANIFEST), b"{}").unwrap();

        let recording = folder(&root, "recording");
        fs::write(recording.join("system.wav"), b"audio").unwrap();

        let busy = folder(&root, "busy");
        fs::write(busy.join(STAGING), b"partial").unwrap();
        let held = super::super::ActiveMeeting::acquire(&busy).unwrap();

        assert_eq!(remove_orphaned_in(&root), 1);
        assert!(!orphan.exists(), "the abandoned import was removed");
        assert!(finished.exists(), "a folder with a manifest stays");
        assert!(finished.join(STAGING).exists(), "with its staging file");
        assert!(recording.exists(), "a folder with no staging file stays");
        assert!(busy.exists(), "a held lock keeps its folder");

        drop(held);
        fs::remove_dir_all(root).unwrap();
    }
}
