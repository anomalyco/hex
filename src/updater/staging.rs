//! The folders macOS updates are unpacked into: `.<name>-update-<version>`
//! beside the installed bundle (see `macos.rs`). A newer staged release
//! replaces the earlier one, and a swap parks the bundle it replaced in the
//! folder that bundle came from until the new version launches, so each is
//! disposable — except the bundle this process runs from.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What `macos::stage` names an unpacked update's folder with, beside the
/// installed bundle. Only this exact prefix is ever removed.
pub const PREFIX: &str = ".Hex-update-";

/// An unpacked update this process could not remove.
///
/// Only macOS logs these; the tests that exercise the sweep elsewhere read
/// nothing from it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct Failure {
    pub path: PathBuf,
    pub error: io::Error,
}

/// Removes every unpacked update beside `bundle` — a directory in the
/// bundle's own folder whose name starts with [`PREFIX`] — except `keep`, and
/// never the bundle itself or anything that is not a directory (a symlink
/// with the prefix is left where it is, never followed).
///
/// Best effort: whatever cannot be removed comes back for the caller to log
/// and the rest still go.
pub fn remove_beside(bundle: &Path, keep: Option<&Path>) -> Vec<Failure> {
    let Some(folder) = bundle.parent() else {
        return Vec::new();
    };
    // The caller may spell the bundle's folder through `..`, `./`, or a
    // symlink; compare everything in one canonical spelling of it.
    let Ok(folder) = folder.canonicalize() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&folder) else {
        return Vec::new();
    };
    let keep = keep.and_then(|keep| kept_name(&folder, keep));
    let bundle = bundle.file_name();
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && name.to_str().is_some_and(|name| name.starts_with(PREFIX))
                && Some(name.as_os_str()) != keep.as_deref()
                && Some(name.as_os_str()) != bundle
        })
        .map(|entry| entry.path())
        .filter_map(|path| {
            fs::remove_dir_all(&path)
                .err()
                .map(|error| Failure { path, error })
        })
        .collect()
}

/// The name `keep` gives a folder in `folder`, whatever spelling it came with.
/// A path outside `folder` keeps nothing, so a caller can never protect an
/// unrelated folder; a path that does not exist yet (the update about to be
/// staged) still names one by its name.
fn kept_name(folder: &Path, keep: &Path) -> Option<OsString> {
    let parent = keep.parent()?;
    match parent.canonicalize() {
        Ok(parent) if parent == folder => keep.file_name().map(ToOwned::to_owned),
        Ok(_) => None,
        Err(_) => keep.file_name().map(ToOwned::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("hex-staging-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// A stand-in for the installed `Hex.app` in `root`.
    fn bundle(root: &Path) -> PathBuf {
        let bundle = root.join("Hex.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join("Contents/MacOS/hex"), "running").unwrap();
        bundle
    }

    /// A stand-in for `stage`'s `.<name>-update-<version>` folder.
    fn staged(root: &Path, version: &str) -> PathBuf {
        let staged = root.join(format!("{PREFIX}{version}"));
        fs::create_dir_all(staged.join("Hex.app/Contents")).unwrap();
        fs::write(staged.join("Hex.app/Contents/version"), version).unwrap();
        staged
    }

    #[test]
    fn launch_cleanup_removes_every_unpacked_update_but_the_running_bundle() {
        let root = temporary("launch");
        let bundle = bundle(&root);
        let older = staged(&root, "0.0.0-dev-1014");
        let swapped = staged(&root, "0.0.0-dev-1046");
        fs::create_dir_all(root.join("Other.app/Contents")).unwrap();
        fs::write(root.join("notes.txt"), "keep me").unwrap();

        let failures = remove_beside(&bundle, None);

        assert!(failures.is_empty());
        assert!(!older.exists(), "an earlier update's folder is removed");
        assert!(
            !swapped.exists(),
            "the swapped-out bundle's folder is removed"
        );
        assert!(bundle.exists(), "the running bundle survives");
        assert_eq!(
            fs::read_to_string(bundle.join("Contents/MacOS/hex")).unwrap(),
            "running"
        );
        assert!(root.join("Other.app").exists(), "another app is not ours");
        assert!(root.join("notes.txt").exists(), "only the prefix is ours");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_newer_update_removes_the_folder_of_the_one_it_replaces() {
        let root = temporary("replaced");
        let bundle = bundle(&root);
        let replaced = staged(&root, "0.0.0-dev-1014");
        let staging = staged(&root, "0.0.0-dev-1017");
        fs::write(replaced.join("Hex.app/Contents/marker"), "replaced").unwrap();

        let failures = remove_beside(&bundle, Some(&staging));

        assert!(failures.is_empty());
        assert!(!replaced.exists(), "the update it replaces is removed");
        assert!(staging.exists(), "the update being staged stays");
        assert!(bundle.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_folder_to_keep_is_recognized_through_a_non_canonical_spelling() {
        let root = temporary("keep-spelling");
        let bundle = bundle(&root);
        let replaced = staged(&root, "0.0.0-dev-1014");
        let staging = staged(&root, "0.0.0-dev-1017");
        fs::create_dir_all(root.join("nested")).unwrap();
        let spelling = root
            .join("nested")
            .join("..")
            .join(format!("{PREFIX}0.0.0-dev-1017"));
        assert_ne!(spelling, staging);

        let failures = remove_beside(&bundle, Some(&spelling));

        assert!(failures.is_empty());
        assert!(!replaced.exists(), "the update it replaces is removed");
        assert!(staging.exists(), "the folder it names stays");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_keep_path_outside_the_bundles_folder_protects_nothing() {
        let root = temporary("keep-elsewhere");
        let bundle = bundle(&root);
        let elsewhere = temporary("keep-elsewhere-other");
        let staging = staged(&root, "0.0.0-dev-1017");
        let lookalike = elsewhere.join(format!("{PREFIX}0.0.0-dev-1017"));

        let failures = remove_beside(&bundle, Some(&lookalike));

        assert!(failures.is_empty());
        assert!(
            !staging.exists(),
            "a path in another folder keeps nothing here"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(elsewhere);
    }

    #[test]
    fn a_bundle_running_from_a_staging_folder_keeps_that_folder() {
        let root = temporary("inside");
        let staging = staged(&root, "0.0.0-dev-1046");
        let bundle = staging.join("Hex.app");

        let failures = remove_beside(&bundle, None);

        assert!(failures.is_empty());
        assert!(bundle.exists());
        assert!(staging.exists(), "the folder this bundle runs from stays");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_named_like_a_staged_update_is_never_followed() {
        let root = temporary("symlink");
        let bundle = bundle(&root);
        let precious = root.join("precious");
        fs::create_dir_all(precious.join("Hex.app")).unwrap();
        std::os::unix::fs::symlink(&precious, root.join(format!("{PREFIX}1.0.0"))).unwrap();

        let failures = remove_beside(&bundle, None);

        assert!(failures.is_empty());
        assert!(
            precious.join("Hex.app").exists(),
            "nothing behind it is touched"
        );
        assert!(
            root.join(format!("{PREFIX}1.0.0")).exists(),
            "the link itself is left alone"
        );
        let _ = fs::remove_dir_all(root);
    }
}
