//! Windows installs: the per-user version layout our installers receipt
//! (`%APPDATA%\hex\install.json`) updates itself by staging the
//! new `hex.exe` into its own version directory and switching the
//! `current-version` pointer. Before anything runs, the download must carry
//! a valid Authenticode signature from Anomaly's Artifact Signing identity.
//! Administrators turn updates off with the `DisableUpdates` policy value.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use color_eyre::eyre::{Result, WrapErr, bail};
use fs2::FileExt;

use super::Method;
use super::protocol::is_version_name;
use crate::background_process;

#[cfg(target_arch = "x86_64")]
pub const TARGET: &str = "windows-x64";
#[cfg(target_arch = "aarch64")]
pub const TARGET: &str = "windows-arm64";
#[cfg(target_arch = "x86_64")]
pub const DOWNLOAD_TARGET: &str = "windows-x64-setup";
#[cfg(target_arch = "aarch64")]
pub const DOWNLOAD_TARGET: &str = "windows-arm64-setup";

/// The common name of the certificate Azure Artifact Signing issues to
/// Anomaly (as on OpenCode's signed Windows builds); updates signed by
/// anyone else are refused.
pub const AUTHENTICODE_SIGNER: &str = "Anomaly Innovations, Inc https://anoma.ly/";
/// `HKLM` (or `HKCU`) `\SOFTWARE\Policies\Anomaly\Hex`, value `DisableUpdates`.
const POLICY_KEY: &str = r"SOFTWARE\Policies\Anomaly\Hex";
/// The Apps & Features entry that packaging/windows/hex.iss registers.
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{6ED48C92-5177-41F4-B8D6-37B7B02CFDF4}_is1";

/// The activated `hex.exe`.
#[derive(Clone, Debug)]
pub struct Staged(PathBuf);

/// Only a receipted install running from `<support>\versions\<version>`
/// inside the user's own profile updates itself: default profile ACLs deny
/// other users write access there.
pub fn method() -> Method {
    let installed = (|| {
        let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
        let support = crate::app_paths::support_dir().ok()?.canonicalize().ok()?;
        let profile = dirs::config_dir()?.canonicalize().ok()?;
        let versions = support.join("versions").canonicalize().ok()?;
        (support.starts_with(&profile) && executable.starts_with(&versions)).then_some(support)
    })();
    match installed.and_then(|support| super::receipt_channel(&support)) {
        Some(channel) => Method::Direct { channel },
        None => Method::Unknown,
    }
}

/// The `windows-*` target is the signed executable our installers copy
/// unchanged, so this build is the file whose SHA-256 it has.
pub fn running_sha256() -> Option<String> {
    crate::transcription_models::sha256_hex(&std::env::current_exe().ok()?, None).ok()
}

pub fn updates_disabled_by_policy() -> bool {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
        .into_iter()
        .any(|hive| {
            RegKey::predef(hive)
                .open_subkey(POLICY_KEY)
                .and_then(|key| key.get_value::<u32, _>("DisableUpdates"))
                .is_ok_and(|value| value != 0)
        })
}

pub fn stage(file: &Path, version: &str) -> Result<Staged> {
    verify_authenticode(file, AUTHENTICODE_SIGNER)?;
    let support = crate::app_paths::support_dir()?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(support.join("updates").join("update.lock"))?;
    lock.try_lock_exclusive()
        .wrap_err("another Windows update is already in progress")?;
    let staging = support.join("updates").join("hex.exe.staging");
    let _ = fs::remove_file(&staging);
    fs::copy(file, &staging)?;
    let activation =
        validate_executable(&staging, version).and_then(|()| activate(&support, version, &staging));
    if activation.is_err() {
        let _ = fs::remove_file(&staging);
    }
    let executable = activation?;
    if let Err(error) = crate::windows_login_item::repoint(&executable) {
        tracing::warn!(%error, "could not repoint Launch at login to the updated Hex");
    }
    if let Err(error) = repoint_start_menu_shortcut(&executable) {
        tracing::warn!(%error, "could not repoint the Start Menu shortcut to the updated Hex");
    }
    if let Err(error) = update_uninstall_entry(version) {
        tracing::warn!(%error, "could not update the Apps & Features version");
    }
    Ok(Staged(executable))
}

/// Requires a valid Authenticode signature (chain and timestamp checked by
/// Windows) whose signer certificate's common name is `signer`.
pub fn verify_authenticode(file: &Path, signer: &str) -> Result<()> {
    let output = background_process::command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$s = Get-AuthenticodeSignature -LiteralPath $env:HEX_UPDATE_FILE; \
             [Console]::Out.Write(\"$($s.Status)`n\"); \
             if ($s.SignerCertificate) { [Console]::Out.Write($s.SignerCertificate.GetNameInfo('SimpleName', $false)) }",
        ])
        .env("HEX_UPDATE_FILE", file)
        // A PowerShell 7 parent's module path breaks Windows PowerShell's
        // own modules, Microsoft.PowerShell.Security among them.
        .env_remove("PSModulePath")
        .stdin(Stdio::null())
        .output()
        .wrap_err("could not check the update's Authenticode signature")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let (status, name) = text.split_once('\n').unwrap_or((text.trim(), ""));
    if status.trim() != "Valid" {
        bail!("the update's Authenticode signature is {}", status.trim());
    }
    if name.trim() != signer {
        bail!("the update is signed by {:?}, not {signer:?}", name.trim());
    }
    Ok(())
}

/// Retarget the installer-created Start Menu shortcut at the activated
/// version, so pinned and Start Menu launches follow updates. Only a
/// shortcut carrying the installer's managed description is touched.
fn repoint_start_menu_shortcut(executable: &Path) -> Result<()> {
    let Some(appdata) = dirs::config_dir() else {
        return Ok(());
    };
    let shortcut = appdata.join(r"Microsoft\Windows\Start Menu\Programs\Hex.lnk");
    if !shortcut.exists() {
        return Ok(());
    }
    let status = background_process::command("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:HEX_SHORTCUT); \
             if ($s.Description -eq 'Hex managed install') { \
             $s.TargetPath = $env:HEX_EXECUTABLE; \
             $s.WorkingDirectory = (Split-Path -Parent $env:HEX_EXECUTABLE); \
             $s.Save() }",
        ])
        .env("HEX_SHORTCUT", &shortcut)
        .env("HEX_EXECUTABLE", executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .wrap_err("could not update the Start Menu shortcut")?;
    if !status.success() {
        bail!("the Start Menu shortcut update exited with {status}");
    }
    Ok(())
}

/// Show the activated version in Apps & Features when Setup.exe installed
/// Hex; script installs have no entry and are left alone.
fn update_uninstall_entry(version: &str) -> Result<()> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};

    let key =
        match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(UNINSTALL_KEY, KEY_WRITE) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).wrap_err("could not open the Apps & Features entry"),
        };
    key.set_value("DisplayVersion", &version)
        .wrap_err("could not write the Apps & Features version")?;
    Ok(())
}

fn validate_executable(path: &Path, version: &str) -> Result<()> {
    let output = background_process::command(path)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .wrap_err("could not start the downloaded Windows update")?;
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .last()
            != Some(version)
    {
        bail!("the downloaded Windows update reports the wrong version");
    }
    Ok(())
}

fn sync_file(path: &Path) -> Result<()> {
    OpenOptions::new().write(true).open(path)?.sync_all()?;
    Ok(())
}

/// Stage the executable into its version directory and switch the
/// `current-version` pointer atomically; the running version's files are
/// never touched, and every version but the new, running, and previous one
/// is pruned.
fn activate(support: &Path, version: &str, staging: &Path) -> Result<PathBuf> {
    let versions = support.join("versions");
    let version_dir = versions.join(version);
    fs::create_dir_all(&version_dir)?;
    let executable = version_dir.join("hex.exe");
    let staged = version_dir.join("hex.exe.partial");
    let _ = fs::remove_file(&staged);
    fs::rename(staging, &staged)?;
    // FlushFileBuffers requires write access on Windows, unlike fsync.
    sync_file(&staged)?;
    fs::rename(&staged, &executable)?;

    let pointer = support.join("current-version");
    let previous = fs::read_to_string(&pointer)
        .ok()
        .map(|content| content.trim().to_string());
    let pointer_partial = support.join("current-version.partial");
    fs::write(&pointer_partial, format!("{version}\n"))?;
    sync_file(&pointer_partial)?;
    fs::rename(&pointer_partial, &pointer)?;

    let running = crate::VERSION;
    for entry in fs::read_dir(&versions)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name != version
            && name != running
            && previous.as_deref() != Some(name)
            && is_version_name(name)
            && let Err(error) = fs::remove_dir_all(entry.path())
        {
            tracing::warn!(%error, path = %entry.path().display(), "could not prune an old Hex version");
        }
    }
    Ok(executable)
}

/// Hand off to the staged executable once this process exits. The watcher
/// only starts the new version when the old process is actually gone, so a
/// slow shutdown can never produce two running instances.
pub fn restart_into(staged: &Staged) -> Result<()> {
    let pid = std::process::id();
    background_process::command("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &format!(
                "Wait-Process -Id {pid} -Timeout 300 -ErrorAction SilentlyContinue; \
                 if (-not (Get-Process -Id {pid} -ErrorAction SilentlyContinue)) {{ \
                 Start-Process -FilePath $env:HEX_EXECUTABLE -ArgumentList 'app' }}"
            ),
        ])
        .env("HEX_EXECUTABLE", &staged.0)
        .env_remove("PSModulePath")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("could not schedule the updated Hex version")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_switches_the_pointer_without_removing_the_old_version() {
        let root = std::env::temp_dir().join(format!("hex-win-activate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("versions/1.0.0")).unwrap();
        fs::write(root.join("versions/1.0.0/hex.exe"), b"old").unwrap();
        fs::create_dir_all(root.join("versions/0.9.0")).unwrap();
        fs::write(root.join("versions/0.9.0/hex.exe"), b"older").unwrap();
        fs::write(root.join("current-version"), "1.0.0\n").unwrap();
        let staging = root.join("hex.exe.staging");
        fs::write(&staging, b"new").unwrap();

        let executable = activate(&root, "1.1.0", &staging).unwrap();

        assert_eq!(fs::read(&executable).unwrap(), b"new");
        assert_eq!(
            fs::read_to_string(root.join("current-version"))
                .unwrap()
                .trim(),
            "1.1.0"
        );
        assert_eq!(
            fs::read(root.join("versions/1.0.0/hex.exe")).unwrap(),
            b"old"
        );
        assert!(!root.join("versions/0.9.0").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn authenticode_requires_a_valid_signature_from_the_expected_signer() {
        // A Microsoft-signed program every Windows install has; system files
        // may be signed through a catalog, which Get-AuthenticodeSignature
        // also validates.
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        let signer_of = |path: &Path| {
            let output = background_process::command("powershell.exe")
                .args([
                    "-NoProfile",
                    "-Command",
                    "$s = Get-AuthenticodeSignature -LiteralPath $env:HEX_UPDATE_FILE; \
                     if ($s.Status -eq 'Valid') { $s.SignerCertificate.GetNameInfo('SimpleName', $false) }",
                ])
                .env("HEX_UPDATE_FILE", path)
                .env_remove("PSModulePath")
                .output()
                .unwrap();
            let signer = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!signer.is_empty()).then_some(signer)
        };
        let (signed, signer) = [
            "explorer.exe",
            r"System32\notepad.exe",
            r"System32\WindowsPowerShell\v1.0\powershell.exe",
        ]
        .into_iter()
        .map(|name| system.join(name))
        .find_map(|path| signer_of(&path).map(|signer| (path, signer)))
        .expect("a validly signed system program");
        verify_authenticode(&signed, &signer).unwrap();
        assert!(verify_authenticode(&signed, AUTHENTICODE_SIGNER).is_err());

        let unsigned =
            std::env::temp_dir().join(format!("hex-unsigned-{}.exe", std::process::id()));
        fs::write(&unsigned, b"MZ not a signed program").unwrap();
        assert!(verify_authenticode(&unsigned, &signer).is_err());
        let _ = fs::remove_file(unsigned);
    }
}
