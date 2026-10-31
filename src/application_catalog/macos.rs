//! macOS applications: `.app` bundles named as Finder shows them, with
//! their bundle identifiers.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};
use objc2::rc::autoreleasepool;
use objc2_foundation::{NSBundle, NSFileManager, NSString};

use super::InstalledApplication;

pub(super) fn applications() -> Vec<InstalledApplication> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for root in application_roots() {
        collect_applications(&root, &mut paths, &mut seen);
    }
    paths
        .into_iter()
        .filter_map(|path| metadata(&path).ok())
        .collect()
}

pub(super) fn metadata(path: &Path) -> Result<InstalledApplication> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("app") {
        return Err(eyre!("select a macOS .app bundle"));
    }
    autoreleasepool(|_| {
        let path_string = NSString::from_str(&path.to_string_lossy());
        let bundle = NSBundle::bundleWithPath(&path_string)
            .ok_or_else(|| eyre!("{} is not a readable application bundle", path.display()))?;
        let display_name = NSFileManager::defaultManager()
            .displayNameAtPath(&path_string)
            .to_string();
        let name = crate::dictation::context::strip_bundle_extension(&display_name).to_owned();
        let bundle_id = bundle
            .bundleIdentifier()
            .map(|identifier| identifier.to_string());
        Ok(InstalledApplication {
            name,
            bundle_id,
            path: path.to_owned(),
        })
    })
}

fn application_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Library/CoreServices/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    roots
}

fn collect_applications(directory: &Path, output: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir()
            || file_type.is_symlink()
            || path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
        {
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) == Some("app") {
            let identity = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if seen.insert(identity) {
                output.push(path);
            }
        } else {
            collect_applications(&path, output, seen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_has_native_picker_metadata() {
        let finder = metadata(Path::new("/System/Library/CoreServices/Finder.app")).unwrap();

        assert_eq!(finder.name, "Finder");
        assert_eq!(finder.bundle_id.as_deref(), Some("com.apple.finder"));
    }
}
