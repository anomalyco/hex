//! Which Hex Microphone driver copy an install may use, and the signature
//! check a shipped build's copy must pass before the admin prompt.
//!
//! A shipped build installs only the copy inside its own `Hex.app`: a cache
//! the user can write, or another installed app, must never supply the driver
//! code that runs with audio privileges. Local builds may use those copies to
//! install a driver built from this checkout.

use std::path::{Path, PathBuf};

const DRIVER_BUNDLE_NAME: &str = "HexMicrophone.driver";
const NOT_FOUND: &str = "Bundled HexMicrophone.driver was not found in Hex.app";

/// Anomaly's Developer ID requirement for the driver. The driver is nested
/// code, which a stapled app does not ticket separately, so there is no
/// `notarized` clause.
const DRIVER_REQUIREMENT: &str = concat!(
    "anchor apple generic ",
    "and identifier \"anomaly.hex.microphone.driver\" ",
    "and certificate 1[field.1.2.840.113635.100.6.2.6] exists ",
    "and certificate leaf[field.1.2.840.113635.100.6.1.13] exists ",
    "and certificate leaf[subject.OU] = \"5NZ4Q7NXJ4\""
);

/// A driver copy an install can use, with the version it reports.
#[derive(Debug)]
pub(super) struct DriverSource {
    pub(super) path: PathBuf,
    pub(super) version: String,
}

/// Whether this build may install a driver copy from outside its own app
/// bundle and skip the Developer ID check: debug, test, source, and
/// ad-hoc-signed test builds. Distributed dev-channel builds are Developer ID
/// signed, so they are not local.
pub(super) fn allows_local_source() -> bool {
    cfg!(debug_assertions)
        || cfg!(test)
        || crate::app_paths::is_local_build()
        || crate::app_paths::is_macos_test_build()
}

/// Resolves the copy this process may install, validating it with `validate`.
///
/// `contents` is the running executable's own app bundle `Contents` folder.
/// A copy there is the only one a shipped build can use: when it exists and
/// fails validation, that failure is final instead of trying another path.
pub(super) fn select(
    contents: Option<&Path>,
    home: Option<&Path>,
    local: bool,
    validate: impl Fn(&Path) -> Result<String, String>,
) -> Result<DriverSource, String> {
    if let Some(bundle) = own_bundle_copy(contents) {
        let version = validate(&bundle)
            .map_err(|reason| format!("Bundled HexMicrophone.driver is invalid: {reason}"))?;
        return Ok(DriverSource {
            path: bundle,
            version,
        });
    }
    if !local {
        return Err(NOT_FOUND.to_string());
    }
    let mut candidates = Vec::new();
    if let Some(home) = home {
        candidates.push(home.join("Library/Caches/hex-mic").join(DRIVER_BUNDLE_NAME));
    }
    candidates.push(Path::new("/Applications/Hex.app/Contents/Library").join(DRIVER_BUNDLE_NAME));
    candidates
        .into_iter()
        .find_map(|candidate| {
            validate(&candidate).ok().map(|version| DriverSource {
                path: candidate,
                version,
            })
        })
        .ok_or_else(|| NOT_FOUND.to_string())
}

/// The driver inside the running app bundle, preferring `Contents/Library`.
fn own_bundle_copy(contents: Option<&Path>) -> Option<PathBuf> {
    let contents = contents?;
    ["Library", "Resources"]
        .into_iter()
        .map(|folder| contents.join(folder).join(DRIVER_BUNDLE_NAME))
        .find(|bundle| bundle.exists())
}

/// Resolves the source for this process: its own app bundle, then the copies
/// only a local build may use.
pub(super) fn resolve(
    validate: impl Fn(&Path) -> Result<String, String>,
) -> Result<DriverSource, String> {
    let contents = std::env::current_exe().ok().and_then(|exe| {
        let macos = exe.parent()?;
        macos.parent().map(Path::to_path_buf)
    });
    let home = dirs::home_dir();
    select(
        contents.as_deref(),
        home.as_deref(),
        allows_local_source(),
        validate,
    )
}

/// Refuses a shipped build's driver copy unless it carries Anomaly's
/// Developer ID signature, before the admin prompt can install it.
#[cfg(target_os = "macos")]
pub(super) fn verify_signature(bundle: &Path) -> Result<(), String> {
    let output = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(format!("-R={DRIVER_REQUIREMENT}"))
        .arg(bundle)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("Could not check the driver's code signature: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("codesign rejected it")
        .chars()
        .take(160)
        .collect::<String>();
    Err(format!(
        "HexMicrophone.driver is not Anomaly's Developer ID-signed copy: {detail}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temporary(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("hex-driver-source-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// A driver bundle the test validator accepts, reporting `version`.
    fn valid_bundle(dir: &Path, version: &str) -> PathBuf {
        let bundle = dir.join(DRIVER_BUNDLE_NAME);
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("version"), version).unwrap();
        bundle
    }

    fn invalid_bundle(dir: &Path) -> PathBuf {
        let bundle = dir.join(DRIVER_BUNDLE_NAME);
        fs::create_dir_all(&bundle).unwrap();
        bundle
    }

    /// Stands in for `validate_driver_bundle`: the bundle reports a version
    /// only when it is complete.
    fn test_validation(bundle: &Path) -> Result<String, String> {
        fs::read_to_string(bundle.join("version"))
            .map(|version| version.trim().to_owned())
            .map_err(|_| "Driver Info.plist is missing or invalid".to_string())
    }

    #[test]
    fn an_invalid_copy_in_the_app_bundle_fails_instead_of_falling_back() {
        let root = temporary("invalid-own");
        let contents = root.join("Hex.app/Contents");
        let own = invalid_bundle(&contents.join("Library"));
        let home = root.join("home");
        let cache = valid_bundle(&home.join("Library/Caches/hex-mic"), "9.9.9");

        for local in [false, true] {
            let error = select(Some(&contents), Some(&home), local, test_validation).unwrap_err();
            assert!(
                error.contains("Driver Info.plist is missing or invalid"),
                "{error}"
            );
            assert!(error.contains("invalid"), "{error}");
        }
        assert!(own.exists() && cache.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_invalid_copy_in_resources_fails_instead_of_falling_back() {
        let root = temporary("invalid-resources");
        let contents = root.join("Hex.app/Contents");
        let own = invalid_bundle(&contents.join("Resources"));
        let home = root.join("home");
        let cache = valid_bundle(&home.join("Library/Caches/hex-mic"), "9.9.9");

        let error = select(Some(&contents), Some(&home), true, test_validation).unwrap_err();
        assert!(error.contains("invalid"), "{error}");
        assert!(own.exists() && cache.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_shipped_build_without_its_own_copy_finds_no_driver() {
        let root = temporary("shipped-no-own");
        let contents = root.join("Hex.app/Contents");
        fs::create_dir_all(contents.join("Library")).unwrap();
        let home = root.join("home");
        valid_bundle(&home.join("Library/Caches/hex-mic"), "9.9.9");

        let error = select(Some(&contents), Some(&home), false, test_validation).unwrap_err();
        assert_eq!(error, NOT_FOUND);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_local_build_falls_back_to_the_cache_copy() {
        let root = temporary("local-cache");
        let contents = root.join("Hex.app/Contents");
        fs::create_dir_all(contents.join("Library")).unwrap();
        let home = root.join("home");
        let cache = valid_bundle(&home.join("Library/Caches/hex-mic"), "9.9.9");

        let source = select(Some(&contents), Some(&home), true, test_validation).unwrap();
        assert_eq!(source.path, cache);
        assert_eq!(source.version, "9.9.9");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_valid_copy_in_the_app_bundle_wins() {
        let root = temporary("own-wins");
        let contents = root.join("Hex.app/Contents");
        let own = valid_bundle(&contents.join("Library"), "2.0.0");
        let home = root.join("home");
        valid_bundle(&home.join("Library/Caches/hex-mic"), "1.0.0");

        let source = select(Some(&contents), Some(&home), true, test_validation).unwrap();
        assert_eq!(source.path, own);
        assert_eq!(source.version, "2.0.0");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_resources_copy_is_used_when_library_has_none() {
        let root = temporary("own-resources");
        let contents = root.join("Hex.app/Contents");
        let resources = valid_bundle(&contents.join("Resources"), "3.0.0");

        let source = select(Some(&contents), None, false, test_validation).unwrap();
        assert_eq!(source.path, resources);
        assert_eq!(source.version, "3.0.0");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_driver_requirement_compiles_and_rejects_an_ad_hoc_copy() {
        let status = std::process::Command::new("/usr/bin/csreq")
            .arg(format!("-r={DRIVER_REQUIREMENT}"))
            .arg("-t")
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());

        let root = temporary("ad-hoc");
        let bundle = root.join(DRIVER_BUNDLE_NAME);
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::copy(
            "/usr/bin/true",
            bundle.join("Contents/MacOS/HexMicrophoneDriver"),
        )
        .unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
             <key>CFBundleIdentifier</key><string>anomaly.hex.microphone.driver</string>\
             <key>CFBundleExecutable</key><string>HexMicrophoneDriver</string>\
             <key>CFBundleShortVersionString</key><string>9.9.9</string>\
             </dict></plist>",
        )
        .unwrap();
        let signed = std::process::Command::new("/usr/bin/codesign")
            .args([
                "--force",
                "--sign",
                "-",
                "--identifier",
                "anomaly.hex.microphone.driver",
            ])
            .arg(&bundle)
            .status()
            .unwrap();
        assert!(signed.success());

        let error = verify_signature(&bundle).unwrap_err();
        assert!(error.contains("Developer ID"), "{error}");
        let _ = fs::remove_dir_all(root);
    }
}
