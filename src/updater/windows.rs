//! Windows installs: the per-user version layout our installers receipt
//! (`%APPDATA%\hex\install.json`) updates itself by staging the
//! new `hex.exe` and its console entry point `hex.com` (crates/console) into
//! their own version directory and switching the `current-version` pointer.
//! `bin\hex.com`, the copy PATH names, follows. Before anything runs, every
//! download must carry a valid Authenticode signature from Anomaly's
//! Artifact Signing identity. Administrators turn updates off with the
//! `DisableUpdates` policy value.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, bail};
use fs2::FileExt;

use super::Method;
use super::protocol::is_version_name;
use crate::desktop::background_process;

#[cfg(target_arch = "x86_64")]
pub const TARGET: &str = "windows-x64";
#[cfg(target_arch = "aarch64")]
pub const TARGET: &str = "windows-arm64";
#[cfg(target_arch = "x86_64")]
pub const CONSOLE_TARGET: &str = "windows-x64-cli";
#[cfg(target_arch = "aarch64")]
pub const CONSOLE_TARGET: &str = "windows-arm64-cli";
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

/// Why this copy of Hex can't update itself; the window says so beside
/// Download update, with what to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ineligible {
    /// Not running from `<support>\versions\<version>`: a downloaded or
    /// copied `hex.exe`, or a build.
    NotInstalled,
    /// The data folder is outside the user's own profile.
    OutsideProfile,
    /// `install.json` names another app, artifact, or install method.
    Receipt,
}

impl Ineligible {
    fn message(self) -> &'static str {
        match self {
            Self::NotInstalled => {
                "This copy of Hex can't update itself because the Hex installer didn't install it. Install Hex from anoma.ly/hex to get updates in the app."
            }
            Self::OutsideProfile => {
                "This copy of Hex can't update itself because it's installed outside your user folder. Reinstall Hex from anoma.ly/hex to get updates in the app."
            }
            Self::Receipt => {
                "This copy of Hex can't update itself because its install record is from another installer. Reinstall Hex from anoma.ly/hex to get updates in the app."
            }
        }
    }
}

/// Only a receipted install running from `<support>\versions\<version>`
/// inside the user's own profile updates itself: default profile ACLs deny
/// other users write access there.
pub fn method() -> Method {
    match eligibility() {
        Ok(channel) => Method::Direct { channel },
        Err(why) => {
            tracing::warn!(?why, "this Hex can't update itself");
            Method::Unknown
        }
    }
}

/// What the window shows when this Hex can't update itself.
pub fn blocked() -> Option<&'static str> {
    eligibility().err().map(Ineligible::message)
}

fn eligibility() -> std::result::Result<String, Ineligible> {
    let executable = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|_| Ineligible::NotInstalled)?;
    let root = crate::app_paths::support_dir().map_err(|_| Ineligible::NotInstalled)?;
    let support = root.canonicalize().map_err(|_| Ineligible::NotInstalled)?;
    let profile = dirs::config_dir()
        .and_then(|profile| profile.canonicalize().ok())
        .ok_or(Ineligible::OutsideProfile)?;
    check_layout(&executable, &support, &profile)?;
    if let Some(channel) = super::receipt_channel(&support) {
        return Ok(channel);
    }
    repair_receipt(&root)?;
    super::receipt_channel(&support).ok_or(Ineligible::Receipt)
}

fn check_layout(
    executable: &Path,
    support: &Path,
    profile: &Path,
) -> std::result::Result<(), Ineligible> {
    let versions = support
        .join("versions")
        .canonicalize()
        .map_err(|_| Ineligible::NotInstalled)?;
    if !executable.starts_with(&versions) {
        return Err(Ineligible::NotInstalled);
    }
    if !support.starts_with(profile) {
        return Err(Ineligible::OutsideProfile);
    }
    Ok(())
}

/// The layout says this is our installer's install, but its receipt doesn't
/// verify (#1352): Setup before 3.3.1 wrote `install.json` in the ANSI code
/// page, which a non-ASCII profile path turns into invalid UTF-8, and a
/// missing or stale `root` fails the same way. Rewrites the receipt as UTF-8
/// for this root, keeping its channel, unless it names another app or method.
fn repair_receipt(root: &Path) -> std::result::Result<(), Ineligible> {
    let path = root.join("install.json");
    let existing = fs::read(&path).ok().and_then(|bytes| {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        serde_json::from_str::<serde_json::Value>(&String::from_utf8_lossy(bytes)).ok()
    });
    let field = |name: &str| {
        existing
            .as_ref()
            .and_then(|receipt| receipt.get(name))
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    };
    if existing.is_some()
        && (field("app").as_deref() != Some(super::APP)
            || field("artifact").as_deref() != Some(super::ARTIFACT)
            || field("method").as_deref() != Some("direct"))
    {
        return Err(Ineligible::Receipt);
    }
    let channel = field("channel")
        .filter(|channel| ["stable", "beta", "dev"].contains(&channel.as_str()))
        .unwrap_or_else(|| super::build_channel().name().to_owned());
    let receipt = serde_json::json!({
        "app": super::APP,
        "artifact": super::ARTIFACT,
        "channel": channel,
        "method": "direct",
        "root": root,
    });
    let partial = root.join("install.json.partial");
    let written =
        fs::write(&partial, receipt.to_string()).and_then(|()| fs::rename(&partial, &path));
    match written {
        Ok(()) => {
            tracing::info!(%channel, "repaired this install's receipt");
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            tracing::warn!(%error, "could not repair this install's receipt");
            Err(Ineligible::Receipt)
        }
    }
}

/// The `windows-*` target is the signed executable our installers copy
/// unchanged, so this build is the file whose SHA-256 it has.
pub fn running_sha256() -> Option<String> {
    crate::transcription::models::sha256_hex(&std::env::current_exe().ok()?, None).ok()
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

/// Stages `file` (hex.exe) and `console` (hex.com), when the release has
/// one, as `version`.
pub fn stage(file: &Path, console: Option<&Path>, version: &str) -> Result<Staged> {
    verify_authenticode(file, AUTHENTICODE_SIGNER)?;
    if let Some(console) = console {
        verify_authenticode(console, AUTHENTICODE_SIGNER)?;
    }
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
    let console_staging = support.join("updates").join("hex.com.staging");
    let _ = fs::remove_file(&staging);
    let _ = fs::remove_file(&console_staging);
    fs::copy(file, &staging)?;
    if let Some(console) = console {
        fs::copy(console, &console_staging)?;
    }
    let console_staging = console.map(|_| console_staging);
    let activation = validate_executable(&staging, version)
        .and_then(|()| activate(&support, version, &staging, console_staging.as_deref()));
    if activation.is_err() {
        let _ = fs::remove_file(&staging);
        if let Some(console_staging) = &console_staging {
            let _ = fs::remove_file(console_staging);
        }
    }
    let executable = activation?;
    // A release without hex.com keeps the one in bin, which finds hex.exe
    // through current-version.
    if console.is_some()
        && let Err(error) = publish_console(&support, &executable.with_file_name("hex.com"))
    {
        tracing::warn!(%error, "could not put the updated hex.com on PATH");
    }
    if let Err(error) = crate::windows::login_item::repoint(&executable) {
        tracing::warn!(%error, "could not repoint Launch at login to the updated Hex");
    }
    if let Err(error) = repoint_shortcuts(&support, &executable) {
        tracing::warn!(%error, "could not repoint shortcuts to the updated Hex");
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

/// Retarget every shortcut that belongs to this install in one PowerShell
/// run: the Start Menu, Desktop, and taskbar pins. Only a shortcut whose
/// target is `hex.exe` in this install's `versions\<version>` folder is
/// touched; its arguments and name stay, and an icon from an old version
/// follows. A managed Start Menu link that still exists but reads as empty
/// (zero-filled, for example) is recreated as the installer writes it. One bad
/// folder or file never stops the rest.
fn repoint_shortcuts(support: &Path, executable: &Path) -> Result<()> {
    let Some(appdata) = dirs::config_dir() else {
        return Ok(());
    };
    let folders = shortcut_folders(&appdata, dirs::desktop_dir());
    retarget_shortcuts(&folders, support, executable)
}

/// The Start Menu Programs root, the taskbar pins folder, and the Desktop
/// from `dirs::desktop_dir` (so OneDrive redirection is followed), in that
/// order: the Desktop may be a slow network location, so it goes last. An
/// absent Desktop is empty and skipped.
fn shortcut_folders(appdata: &Path, desktop: Option<PathBuf>) -> [PathBuf; 3] {
    [
        appdata.join(r"Microsoft\Windows\Start Menu\Programs"),
        appdata.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"),
        desktop.unwrap_or_default(),
    ]
}

/// Runs the one PowerShell process over `folders` and waits for it with a
/// bound: `stage` holds the update lock meanwhile, and a redirected Desktop on
/// an unreachable network share can stall a shortcut read. `support\updates`
/// holds the temporary link each save replaces its shortcut with; it is
/// created here and cleared after the run.
fn retarget_shortcuts(folders: &[PathBuf; 3], support: &Path, executable: &Path) -> Result<()> {
    let [programs, taskbar, desktop] = folders;
    let staging = support.join("updates");
    fs::create_dir_all(&staging).wrap_err("could not create the shortcut staging folder")?;
    let mut child = background_process::command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            SHORTCUT_REPOINT_SCRIPT,
        ])
        .env("HEX_PROGRAMS", programs)
        .env("HEX_DESKTOP", desktop)
        .env("HEX_TASKBAR", taskbar)
        .env("HEX_VERSIONS", support.join("versions"))
        .env("HEX_STAGING", &staging)
        .env("HEX_EXECUTABLE", executable)
        .env_remove("PSModulePath")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("could not update shortcuts")?;
    let waited = wait_for_exit(&mut child);
    // A killed PowerShell can leave its temporary link behind; it never became
    // a shortcut, and the next run removes it before using the path again.
    let _ = fs::remove_file(staging.join("shortcut.tmp.lnk"));
    let status = waited?;
    if !status.success() {
        bail!("the shortcut update exited with {status}");
    }
    Ok(())
}

/// How long the shortcut update may run before the update lock is released:
/// cold PowerShell on a slow ARM machine can take many seconds.
const SHORTCUT_UPDATE_TIMEOUT: Duration = Duration::from_secs(60);

/// Waits for `child` without blocking Hex's update lock forever: a redirected
/// Desktop on an unreachable network share can keep PowerShell from finishing.
fn wait_for_exit(child: &mut std::process::Child) -> Result<std::process::ExitStatus> {
    let started = Instant::now();
    loop {
        match child
            .try_wait()
            .wrap_err("could not wait for the shortcut update")?
        {
            Some(status) => return Ok(status),
            None if started.elapsed() >= SHORTCUT_UPDATE_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "the shortcut update did not finish within {} s",
                    SHORTCUT_UPDATE_TIMEOUT.as_secs()
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Every path arrives as an environment variable, so no path is interpolated
/// into the script and its quoting cannot break out of it. A Start Menu link
/// that still exists but reads as empty (zero-filled, for example) is
/// recreated as the installer writes it. Every save writes a flushed temporary
/// link beside the update and replaces the shortcut with it on the same
/// volume; another volume or a failed replace falls back to the old in-place
/// save, or copies the new link back when the replace lost the shortcut, and
/// the recreated broken link is copied over instead. Each folder and each file
/// is independent: a missing folder or a failed save leaves the rest alone,
/// and the failure still reaches `stage`'s warning.
const SHORTCUT_REPOINT_SCRIPT: &str = "\
    $ErrorActionPreference = 'Stop'; \
    $shell = New-Object -ComObject WScript.Shell; \
    $executable = $env:HEX_EXECUTABLE; \
    $working = [IO.Path]::GetDirectoryName($executable); \
    $versions = $env:HEX_VERSIONS.TrimEnd('\\') + '\\'; \
    $staging = $env:HEX_STAGING; \
    $temp = if ($staging) { Join-Path $staging 'shortcut.tmp.lnk' } else { '' }; \
    $folders = @($env:HEX_PROGRAMS, $env:HEX_TASKBAR, $env:HEX_DESKTOP); \
    $failed = $false; \
    $programs = $env:HEX_PROGRAMS; \
    if ($programs) { \
      try { \
        $managed = Join-Path $programs 'Hex.lnk'; \
        $broken = $false; \
        if (Test-Path -LiteralPath $managed -PathType Leaf -ErrorAction SilentlyContinue) { \
          try { $existing = $shell.CreateShortcut($managed); if (-not [string]$existing.TargetPath) { $broken = $true } } catch { $broken = $true } } \
        if ($broken) { \
          if (-not $temp) { throw 'no shortcut staging folder' } \
          if (Test-Path -LiteralPath $temp -ErrorAction SilentlyContinue) { Remove-Item -LiteralPath $temp -Force } \
          $fresh = $shell.CreateShortcut($temp); \
          $fresh.TargetPath = $executable; \
          $fresh.Arguments = 'app'; \
          $fresh.Description = 'Hex managed install'; \
          $fresh.WorkingDirectory = $working; \
          $fresh.Save(); \
          $fs = [IO.File]::Open($temp, 'Open', 'ReadWrite'); \
          $fs.Flush($true); \
          $fs.Close(); \
          $replaced = $false; \
          if ([string]::Equals([IO.Path]::GetPathRoot($temp), [IO.Path]::GetPathRoot($managed), [StringComparison]::OrdinalIgnoreCase)) { \
            try { [IO.File]::Replace($temp, $managed, [NullString]::Value); $replaced = $true } catch { } } \
          if (-not $replaced) { Copy-Item -LiteralPath $temp -Destination $managed -Force } \
          Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue } \
      } catch { \
        $failed = $true; \
        if ($temp -and (Test-Path -LiteralPath $temp -ErrorAction SilentlyContinue)) { Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue } } } \
    foreach ($folder in $folders) { \
      if (-not $folder) { continue } \
      try { \
        if (-not (Test-Path -LiteralPath $folder -ErrorAction SilentlyContinue)) { continue } \
        foreach ($file in @(Get-ChildItem -LiteralPath $folder -Force -Filter '*.lnk' -File -ErrorAction SilentlyContinue)) { \
          if (($file.Attributes -band 0x441000) -ne 0) { continue } \
          $matched = $false; \
          try { \
            $s = $shell.CreateShortcut($file.FullName); \
            $target = [string]$s.TargetPath; \
            if (-not $target -or -not $target.StartsWith($versions, [StringComparison]::OrdinalIgnoreCase)) { continue } \
            if ([IO.Path]::GetFileName($target) -ine 'hex.exe') { continue } \
            $relative = $target.Substring($versions.Length).Split('\\'); \
            if ($relative.Count -ne 2 -or -not $relative[0]) { continue } \
            $matched = $true; \
            $changed = $false; \
            $newIcon = ''; \
            if (-not [string]::Equals($s.TargetPath, $executable, [StringComparison]::OrdinalIgnoreCase)) { $changed = $true } \
            if (-not [string]::Equals($s.WorkingDirectory, $working, [StringComparison]::OrdinalIgnoreCase)) { $changed = $true } \
            $icon = [string]$s.IconLocation; \
            $comma = $icon.LastIndexOf(','); \
            $iconPath = if ($comma -ge 0) { $icon.Substring(0, $comma) } else { $icon }; \
            $cleanIcon = [Environment]::ExpandEnvironmentVariables($iconPath.Trim().Trim('\"')); \
            if ($cleanIcon.StartsWith($versions, [StringComparison]::OrdinalIgnoreCase)) { \
              $iconRelative = $cleanIcon.Substring($versions.Length).Split('\\'); \
              if ($iconRelative.Count -eq 2 -and $iconRelative[0] -and $iconRelative[1] -ieq 'hex.exe') { \
                $index = if ($comma -ge 0) { $icon.Substring($comma) } else { ',0' }; \
                $newIcon = $executable + $index; \
                if (-not [string]::Equals($icon, $newIcon, [StringComparison]::OrdinalIgnoreCase)) { $changed = $true } } } \
            if ($changed) { \
              if (-not $temp) { throw 'no shortcut staging folder' } \
              if (Test-Path -LiteralPath $temp -ErrorAction SilentlyContinue) { Remove-Item -LiteralPath $temp -Force } \
              Copy-Item -LiteralPath $file.FullName -Destination $temp -Force; \
              [IO.File]::SetAttributes($temp, [IO.FileAttributes]::Normal); \
              $s = $shell.CreateShortcut($temp); \
              $s.TargetPath = $executable; \
              $s.WorkingDirectory = $working; \
              if ($newIcon) { $s.IconLocation = $newIcon } \
              $s.Save(); \
              $fs = [IO.File]::Open($temp, 'Open', 'ReadWrite'); \
              $fs.Flush($true); \
              $fs.Close(); \
              $replaced = $false; \
              if ([string]::Equals([IO.Path]::GetPathRoot($temp), [IO.Path]::GetPathRoot($file.FullName), [StringComparison]::OrdinalIgnoreCase)) { \
                try { [IO.File]::Replace($temp, $file.FullName, [NullString]::Value); $replaced = $true } catch { } } \
              if (-not $replaced) { \
                if (-not (Test-Path -LiteralPath $file.FullName -ErrorAction SilentlyContinue)) { \
                  Copy-Item -LiteralPath $temp -Destination $file.FullName -Force } \
                else { \
                  $orig = $shell.CreateShortcut($file.FullName); \
                  $orig.TargetPath = $executable; \
                  $orig.WorkingDirectory = $working; \
                  if ($newIcon) { $orig.IconLocation = $newIcon } \
                  $orig.Save() } } \
              Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue } \
          } catch { \
            if ($matched) { $failed = $true }; \
            if ($temp -and (Test-Path -LiteralPath $temp -ErrorAction SilentlyContinue)) { Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue } } } \
      } catch { } } \
    if ($failed) { exit 1 }";

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

/// `hex.com --version` runs the `hex.exe` beside it and reports its version.
fn validate_console(path: &Path, version: &str) -> Result<()> {
    let output = background_process::command(path)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .wrap_err("could not start the downloaded hex.com")?;
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .last()
            != Some(version)
    {
        bail!("the downloaded hex.com doesn't run this version's hex.exe");
    }
    Ok(())
}

/// Whether this install lacks `hex.com` beside the running `hex.exe` or in
/// `bin`: Setup and updaters before it installed only `hex.exe`.
pub fn console_missing() -> bool {
    let (Ok(executable), Ok(support)) = (std::env::current_exe(), crate::app_paths::support_dir())
    else {
        return false;
    };
    !executable.with_file_name("hex.com").is_file()
        || !support.join("bin").join("hex.com").is_file()
}

/// Adds a release's verified `hex.com` beside the running `hex.exe` and on
/// PATH, for an install whose updater predates it. hex.com only starts the
/// `hex.exe` beside it, so any release's copy serves this version.
pub fn install_console(file: &Path) -> Result<()> {
    verify_authenticode(file, AUTHENTICODE_SIGNER)?;
    let support = crate::app_paths::support_dir()?;
    let beside = std::env::current_exe()?.with_file_name("hex.com");
    replace_file(file, &beside)?;
    validate_console(&beside, crate::VERSION)?;
    publish_console(&support, &beside)
}

/// Copies `console` to `bin\hex.com` and keeps `bin` on the user's PATH,
/// where a terminal finds `hex` across updates.
fn publish_console(support: &Path, console: &Path) -> Result<()> {
    let bin = support.join("bin");
    fs::create_dir_all(&bin)?;
    replace_file(console, &bin.join("hex.com"))?;
    add_to_path(&bin)
}

/// Copies `source` over `destination` through a `.partial` file. A
/// destination that's running can't be replaced, but it can be renamed
/// aside; the next replacement removes that copy.
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    let partial = destination.with_extension("com.partial");
    let aside = destination.with_extension("com.old");
    let _ = fs::remove_file(&partial);
    let _ = fs::remove_file(&aside);
    fs::copy(source, &partial)?;
    sync_file(&partial)?;
    if fs::rename(&partial, destination).is_err() {
        fs::rename(destination, &aside)
            .and_then(|()| fs::rename(&partial, destination))
            .inspect_err(|_| {
                let _ = fs::remove_file(&partial);
            })
            .wrap_err_with(|| format!("could not replace {}", destination.display()))?;
    }
    Ok(())
}

/// Appends `folder` to the user's PATH (`HKCU\Environment`) unless it's
/// there, and tells running programs, so new terminals find it.
fn add_to_path(folder: &Path) -> Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ};
    use winreg::{RegKey, RegValue};

    let (environment, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .wrap_err("could not open the user environment")?;
    let existing = environment.get_raw_value("Path").ok();
    let current = existing.as_ref().map_or_else(String::new, |value| {
        let units: Vec<u16> = value
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|&unit| unit != 0)
            .collect();
        String::from_utf16_lossy(&units)
    });
    let wanted = folder.to_string_lossy();
    if path_contains(&current, &wanted) {
        return Ok(());
    }
    let updated = if current.trim_end_matches(';').is_empty() {
        wanted.into_owned()
    } else {
        format!("{};{wanted}", current.trim_end_matches(';'))
    };
    let bytes = updated
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    let vtype = existing.map_or(REG_EXPAND_SZ, |value| value.vtype);
    environment
        .set_raw_value("Path", &RegValue { bytes, vtype })
        .wrap_err("could not add Hex to PATH")?;
    let name: Vec<u16> = "Environment".encode_utf16().chain([0]).collect();
    // SAFETY: `name` is NUL-terminated and outlives the call; a hung window
    // is skipped after the timeout.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            name.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5_000,
            std::ptr::null_mut(),
        )
    };
    Ok(())
}

fn path_contains(path: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches('\\');
    path.split(';').any(|entry| {
        entry
            .trim()
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(folder)
    })
}

fn sync_file(path: &Path) -> Result<()> {
    OpenOptions::new().write(true).open(path)?.sync_all()?;
    Ok(())
}

/// Stage the executable, and hex.com beside it when there is one, into its
/// version directory and switch the `current-version` pointer atomically;
/// the running version's files are never touched, and every version but the
/// new, running, and previous one is pruned.
pub(super) fn activate(
    support: &Path,
    version: &str,
    staging: &Path,
    console: Option<&Path>,
) -> Result<PathBuf> {
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
    if let Some(console) = console {
        let installed = version_dir.join("hex.com");
        let staged = version_dir.join("hex.com.partial");
        let _ = fs::remove_file(&staged);
        fs::rename(console, &staged)?;
        sync_file(&staged)?;
        fs::rename(&staged, &installed)?;
        validate_console(&installed, version)?;
    }

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
/// only starts the new version when the old process has actually exited, so
/// a slow shutdown can never produce two running instances.
pub fn restart_into(staged: &Staged) -> Result<()> {
    background_process::command("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &restart_script(std::process::id()),
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

/// The watcher waits on its own handle to the old process and asks that
/// handle whether it exited. It never looks the pid up again after waiting:
/// the watcher's handle keeps the exited process listed, so `Get-Process`
/// still found it and the new version never started.
fn restart_script(pid: u32) -> String {
    format!(
        "$old = Get-Process -Id {pid} -ErrorAction SilentlyContinue; \
         if ($old -and -not $old.WaitForExit(300000)) {{ exit 1 }}; \
         Start-Process -FilePath $env:HEX_EXECUTABLE -ArgumentList 'app'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A profile with `<profile>\hex\versions\3.3.0\hex.exe`, the layout both
    /// Setup.exe and install-windows.ps1 create (#1352).
    fn installed(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let profile = std::env::temp_dir().join(format!("hex-win-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&profile);
        let support = profile.join("hex");
        fs::create_dir_all(support.join(r"versions\3.3.0")).unwrap();
        let executable = support.join(r"versions\3.3.0\hex.exe");
        fs::write(&executable, b"hex").unwrap();
        let canonical = |path: &Path| path.canonicalize().unwrap();
        (
            canonical(&profile),
            canonical(&support),
            canonical(&executable),
        )
    }

    #[test]
    fn the_installed_layout_is_eligible_and_others_say_why() {
        let (profile, support, executable) = installed("layout");
        assert_eq!(check_layout(&executable, &support, &profile), Ok(()));
        let loose = profile.join("hex.exe");
        fs::write(&loose, b"hex").unwrap();
        assert_eq!(
            check_layout(&loose.canonicalize().unwrap(), &support, &profile),
            Err(Ineligible::NotInstalled)
        );
        let elsewhere = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(
            check_layout(&executable, &support, &elsewhere.join("other-profile")),
            Err(Ineligible::OutsideProfile)
        );
        let _ = fs::remove_dir_all(profile);
    }

    #[test]
    fn both_installers_receipts_verify_and_an_ansi_one_is_repaired() {
        let (profile, support, _) = installed("receipt");
        let root = profile.join("hex");
        let receipt = |root: &str| {
            format!(
                r#"{{"app":"hex","artifact":"desktop","channel":"dev","method":"direct","root":"{}"}}"#,
                root.replace('\\', "\\\\")
            )
        };
        let path = root.to_str().unwrap();
        // install-windows.ps1: UTF-8, no byte order mark.
        fs::write(root.join("install.json"), receipt(path)).unwrap();
        assert_eq!(
            super::super::receipt_channel(&support).as_deref(),
            Some("dev")
        );
        // A Setup.exe that writes a byte order mark.
        fs::write(
            root.join("install.json"),
            format!("\u{feff}{}", receipt(path)),
        )
        .unwrap();
        assert_eq!(
            super::super::receipt_channel(&support).as_deref(),
            Some("dev")
        );
        // Setup.exe before 3.3.1: the ANSI code page, so a profile like
        // C:\Users\Jürgen became bytes that aren't UTF-8.
        let mut ansi = receipt(&format!("{path}\u{fc}")).into_bytes();
        let at = ansi.iter().position(|byte| *byte == 0xc3).unwrap();
        ansi.splice(at..at + 2, [0xfc]);
        fs::write(root.join("install.json"), ansi).unwrap();
        assert_eq!(super::super::receipt_channel(&support), None);
        assert_eq!(repair_receipt(&root), Ok(()));
        assert_eq!(
            super::super::receipt_channel(&support).as_deref(),
            Some("dev")
        );
        // A record another installer wrote is left alone.
        fs::write(
            root.join("install.json"),
            r#"{"app":"hex","artifact":"desktop","channel":"stable","method":"msix","root":"x"}"#,
        )
        .unwrap();
        assert_eq!(repair_receipt(&root), Err(Ineligible::Receipt));
        let _ = fs::remove_dir_all(profile);
    }

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

        let executable = activate(&root, "1.1.0", &staging, None).unwrap();

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
    fn path_entries_match_without_case_or_a_trailing_backslash() {
        let path = r"C:\Windows;C:\Users\Jo\AppData\Roaming\hex\bin\;D:\tools";
        assert!(path_contains(path, r"c:\users\jo\appdata\roaming\HEX\bin"));
        assert!(!path_contains(path, r"C:\Users\Jo\AppData\Roaming\hex"));
        assert!(!path_contains("", r"C:\hex\bin"));
    }

    /// Creates all three shortcuts in one PowerShell process and returns the
    /// managed link's icon location as stored, before any retargeting.
    fn create_shortcuts(links: [(&Path, &Path, &str, &str, &str); 3]) -> String {
        let mut command = background_process::command("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$shell = New-Object -ComObject WScript.Shell; \
             foreach ($n in 1..3) { \
               $s = $shell.CreateShortcut([Environment]::GetEnvironmentVariable('HEX_LINK_' + $n)); \
               $s.TargetPath = [Environment]::GetEnvironmentVariable('HEX_TARGET_' + $n); \
               $s.Description = [Environment]::GetEnvironmentVariable('HEX_DESC_' + $n); \
               $s.Arguments = [Environment]::GetEnvironmentVariable('HEX_ARGS_' + $n); \
               $icon = [Environment]::GetEnvironmentVariable('HEX_ICON_' + $n); \
               if ($icon) { $s.IconLocation = $icon }; \
               $s.Save() }; \
             $check = $shell.CreateShortcut($env:HEX_LINK_1); \
             [Console]::Out.Write($check.IconLocation)",
        ]);
        for (index, (link, target, description, arguments, icon)) in links.iter().enumerate() {
            let number = index + 1;
            command
                .env(format!("HEX_LINK_{number}"), link)
                .env(format!("HEX_TARGET_{number}"), target)
                .env(format!("HEX_DESC_{number}"), description)
                .env(format!("HEX_ARGS_{number}"), arguments)
                .env(format!("HEX_ICON_{number}"), icon);
        }
        let output = command
            .env_remove("PSModulePath")
            .output()
            .expect("could not create shortcuts");
        assert!(
            output.status.success(),
            "creating shortcuts failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// Reads target, working directory, arguments, and icon for all three
    /// links in one PowerShell process.
    fn read_shortcuts(links: [&Path; 3]) -> [(String, String, String, String); 3] {
        let mut command = background_process::command("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$shell = New-Object -ComObject WScript.Shell; \
             $nl = [Environment]::NewLine; \
             foreach ($n in 1..3) { \
               $s = $shell.CreateShortcut([Environment]::GetEnvironmentVariable('HEX_LINK_' + $n)); \
               [Console]::Out.Write($s.TargetPath + $nl + $s.WorkingDirectory + $nl + $s.Arguments + $nl + $s.IconLocation + $nl) }; \
             [Console]::Out.Write('END')",
        ]);
        for (index, link) in links.iter().enumerate() {
            command.env(format!("HEX_LINK_{}", index + 1), link);
        }
        let output = command
            .env_remove("PSModulePath")
            .output()
            .expect("could not read shortcuts");
        assert!(
            output.status.success(),
            "reading shortcuts failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        let mut lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines.pop(),
            Some("END"),
            "unexpected shortcut output: {text}"
        );
        assert_eq!(lines.len(), 12, "unexpected shortcut output: {text}");
        let (chunks, remainder) = lines.as_chunks::<4>();
        assert!(remainder.is_empty(), "unexpected shortcut output: {text}");
        let read = |chunk: &[&str; 4]| {
            (
                chunk[0].to_owned(),
                chunk[1].to_owned(),
                chunk[2].to_owned(),
                chunk[3].to_owned(),
            )
        };
        [read(&chunks[0]), read(&chunks[1]), read(&chunks[2])]
    }

    #[cfg(windows)]
    #[test]
    fn shortcuts_pointing_at_an_old_version_are_retargeted() {
        // A long path: WScript.Shell expands an 8.3 short name (`RUNNER~1`)
        // when it saves a shortcut, and the match is a prefix compare.
        let local = dirs::data_local_dir().expect("a local app data folder");
        let root = local.join(format!("hex-win-shortcuts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let support = root.join("hex");
        let old = support.join(r"versions\1.0.0\hex.exe");
        let new = support.join(r"versions\2.0.0\hex.exe");
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::create_dir_all(new.parent().unwrap()).unwrap();
        fs::write(&old, b"old").unwrap();
        fs::write(&new, b"new").unwrap();
        let folder = root.join("shortcuts");
        fs::create_dir_all(&folder).unwrap();
        let elsewhere = root.join("elsewhere").join("hex.exe");
        fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
        fs::write(&elsewhere, b"other").unwrap();

        // The stored icon keeps `%LOCALAPPDATA%`, so the updater has to
        // expand it before matching.
        let icon = format!(
            "%LOCALAPPDATA%\\{},0",
            old.strip_prefix(&local).unwrap().display()
        );
        let managed = folder.join("Hex.lnk");
        let foreign = folder.join("Foreign.lnk");
        let unrelated = folder.join("Unrelated.lnk");
        let stored_icon = create_shortcuts([
            (&managed, &old, "Hex managed install", "app", &icon),
            (&foreign, &elsewhere, "Hex managed install", "app", ""),
            (&unrelated, &elsewhere, "", "", ""),
        ]);
        assert_eq!(
            stored_icon, icon,
            "WScript.Shell changed the stored environment-variable icon"
        );

        // Programs, taskbar, then Desktop: the shortcut folder fills the
        // first slot.
        let folders = [folder.clone(), PathBuf::new(), PathBuf::new()];
        retarget_shortcuts(&folders, &support, &new).unwrap();

        let [managed, foreign, unrelated] = read_shortcuts([&managed, &foreign, &unrelated]);
        assert_eq!(managed.0, new.display().to_string());
        assert_eq!(managed.1, new.parent().unwrap().display().to_string());
        assert_eq!(managed.2, "app");
        assert_eq!(managed.3, format!("{},0", new.display()));
        let elsewhere = elsewhere.display().to_string();
        // WScript's default icon location for a link that never set one.
        assert_eq!(
            foreign,
            (
                elsewhere.clone(),
                String::new(),
                "app".to_owned(),
                ",0".to_owned()
            )
        );
        assert_eq!(
            unrelated,
            (elsewhere, String::new(), String::new(), ",0".to_owned())
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn a_zero_filled_managed_shortcut_is_recreated() {
        // A zero-filled Start Menu link reads as no target at all, which the
        // update would otherwise skip forever.
        let local = dirs::data_local_dir().expect("a local app data folder");
        let root = local.join(format!("hex-win-broken-shortcut-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let support = root.join("hex");
        let executable = support.join(r"versions\2.0.0\hex.exe");
        let working = executable.parent().unwrap().to_path_buf();
        fs::create_dir_all(&working).unwrap();
        fs::write(&executable, b"new").unwrap();
        // retarget_shortcuts creates the update staging folder it names here.
        let staging = support.join("updates");
        let folder = root.join("shortcuts");
        fs::create_dir_all(&folder).unwrap();
        let managed = folder.join("Hex.lnk");
        let folders = [folder, PathBuf::new(), PathBuf::new()];

        // A link the user removed stays removed: the update never creates one.
        retarget_shortcuts(&folders, &support, &executable).unwrap();
        assert!(
            !managed.exists(),
            "the update created a Start Menu link that was not there"
        );

        fs::write(&managed, vec![0u8; 1254]).unwrap();
        retarget_shortcuts(&folders, &support, &executable).unwrap();

        let output = background_process::command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:HEX_LINK); \
                 $nl = [Environment]::NewLine; \
                 [Console]::Out.Write($s.TargetPath + $nl + $s.Arguments + $nl + $s.Description + $nl + $s.WorkingDirectory)",
            ])
            .env("HEX_LINK", &managed)
            .env_remove("PSModulePath")
            .output()
            .expect("could not read the repaired shortcut");
        assert!(
            output.status.success(),
            "reading the repaired shortcut failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            [
                executable.display().to_string(),
                "app".to_owned(),
                "Hex managed install".to_owned(),
                working.display().to_string(),
            ],
            "unexpected repaired shortcut: {text}"
        );

        let leftovers: Vec<PathBuf> = fs::read_dir(&staging)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("lnk"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "the shortcut save left temporary links: {leftovers:?}"
        );

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
