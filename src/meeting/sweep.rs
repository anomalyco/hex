//! Removes meeting folders left by an audio import that ended before its
//! manifest was written: a crash, or a quit during a long decode. A folder
//! with a manifest is never touched, and a held lock means an import is still
//! using it.

use std::fs;
use std::path::Path;

use super::IMPORT_PENDING_FILE;

const MANIFEST: &str = "manifest.json";

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
        // Only the marker written before decode and removed after
        // final.pending identifies an interrupted import. Nothing else is
        // guessed at from the files a folder happens to contain.
        if !directory.join(IMPORT_PENDING_FILE).exists() {
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
        // A marker with a partial staging file, before the import's rename.
        let orphan = folder(&root, "orphan");
        fs::write(orphan.join(IMPORT_PENDING_FILE), b"").unwrap();
        fs::write(orphan.join("system.importing.wav"), b"partial").unwrap();

        // A marker with both finished tracks, between rename and manifest.
        let renamed = folder(&root, "renamed");
        fs::write(renamed.join(IMPORT_PENDING_FILE), b"").unwrap();
        fs::write(renamed.join("system.wav"), b"audio").unwrap();
        fs::write(renamed.join("microphone.wav"), b"empty").unwrap();

        // Both tracks with no marker are never assumed to be an import.
        let recording = folder(&root, "recording");
        fs::write(recording.join("system.wav"), b"audio").unwrap();
        fs::write(recording.join("microphone.wav"), b"empty").unwrap();

        // A marker with a manifest is a meeting, pending or finished.
        let finished = folder(&root, "finished");
        fs::write(finished.join(IMPORT_PENDING_FILE), b"").unwrap();
        fs::write(finished.join(MANIFEST), b"{}").unwrap();

        // A held lock means an import still uses the folder.
        let busy = folder(&root, "busy");
        fs::write(busy.join(IMPORT_PENDING_FILE), b"").unwrap();
        fs::write(busy.join("system.importing.wav"), b"partial").unwrap();
        let held = super::super::ActiveMeeting::acquire(&busy).unwrap();

        assert_eq!(remove_orphaned_in(&root), 2);
        assert!(!orphan.exists(), "the pre-rename import was removed");
        assert!(!renamed.exists(), "the renamed import was removed");
        assert!(recording.exists(), "tracks without a marker stay");
        assert!(finished.exists(), "a folder with a manifest stays");
        assert!(busy.exists(), "a held lock keeps its folder");

        drop(held);
        fs::remove_dir_all(root).unwrap();
    }
}
