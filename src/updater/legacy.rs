//! Linux and Windows installs from 2.3.1 and earlier keep their executables
//! in `<data>/voice-control/versions/<version>`, and their updater still
//! takes whatever its signed feed names, so that feed hands them a current
//! build. Launched from there (as the app, or as the old systemd unit's
//! `service`), the build moves itself into the layout our installers make
//! (`<data>/hex`, receipted, with the Start Menu shortcut, Launch at login,
//! systemd unit, and desktop entries pointed at it), then relaunches from
//! there so the release service updates it from then on. A failed move is
//! logged and Hex runs in place; the next launch tries again.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use color_eyre::eyre::{Result, WrapErr};

/// The data folder of 2.3.1 and earlier.
const LEGACY_FOLDER: &str = "voice-control";

/// Moves a legacy install into the managed layout and relaunches it from
/// there. Returns whether the relaunched Hex took over, in which case the
/// caller exits; on Linux a successful relaunch replaces this process and
/// never returns.
pub fn take_over() -> bool {
    let Some((legacy, root)) = legacy_install() else {
        return false;
    };
    tracing::info!(legacy = %legacy.display(), root = %root.display(), "moving a 2.3.1 install into the managed layout");
    let relaunched =
        adopt(&legacy, &root).and_then(|executable| relaunch(&executable, &legacy, &root));
    match relaunched {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(error = %format!("{error:#}"), "could not move the 2.3.1 install; running in place");
            false
        }
    }
}

/// The legacy data folder and the managed one, when this executable runs
/// from the legacy version layout.
fn legacy_install() -> Option<(PathBuf, PathBuf)> {
    let data = dirs::data_dir()?;
    let executable = std::env::current_exe().ok()?;
    let legacy = legacy_root(&executable, &data)?;
    Some((legacy, data.join("hex")))
}

fn legacy_root(executable: &Path, data: &Path) -> Option<PathBuf> {
    let legacy = data.join(LEGACY_FOLDER);
    let versions = legacy.join("versions").canonicalize().ok()?;
    executable
        .canonicalize()
        .ok()?
        .starts_with(&versions)
        .then_some(legacy)
}

#[cfg(target_os = "linux")]
use linux::{adopt, relaunch};
#[cfg(target_os = "windows")]
use windows::{adopt, relaunch};

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    use super::*;

    /// The installer the website serves, so an adopted install is exactly
    /// the one it makes.
    const INSTALLER: &str = include_str!("../../.github/release/install-linux.sh");

    /// Files 2.3.1's installer marked as its own, which the current
    /// installer refuses to replace.
    pub(super) struct Legacy {
        unit: PathBuf,
        binary: PathBuf,
        entries: [PathBuf; 2],
    }

    impl Legacy {
        pub(super) fn for_user(home: &Path, data: &Path, config: &Path) -> Self {
            Self {
                unit: config.join("systemd/user/hex.service"),
                binary: home.join(".local/bin/hex"),
                entries: [
                    data.join("applications/hex.desktop"),
                    config.join("autostart/hex.desktop"),
                ],
            }
        }

        /// Moves each legacy file aside (`.<name>.legacy`) and returns the
        /// moves, so they can be undone.
        pub(super) fn retire(&self, legacy: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
            let mut moved = Vec::new();
            let result: Result<()> = (|| {
                let unit = marked(&self.unit, "# X-HEX-Managed=true");
                let binary = fs::read_link(&self.binary)
                    .is_ok_and(|target| target == legacy.join("current/hex"));
                let entries = self
                    .entries
                    .iter()
                    .filter(|entry| marked(entry, "X-HEX-Managed=true"));
                for path in [unit.then_some(&self.unit), binary.then_some(&self.binary)]
                    .into_iter()
                    .flatten()
                    .chain(entries)
                {
                    let aside = aside(path);
                    fs::rename(path, &aside)
                        .wrap_err_with(|| format!("could not move {} aside", path.display()))?;
                    moved.push((path.clone(), aside));
                }
                Ok(())
            })();
            if let Err(error) = result {
                restore(&moved);
                return Err(error);
            }
            Ok(moved)
        }
    }

    fn marked(path: &Path, mark: &str) -> bool {
        !path.is_symlink()
            && fs::read_to_string(path).is_ok_and(|body| body.lines().any(|line| line == mark))
    }

    fn aside(path: &Path) -> PathBuf {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        path.with_file_name(format!(".{name}.legacy"))
    }

    pub(super) fn restore(moved: &[(PathBuf, PathBuf)]) {
        for (path, aside) in moved.iter().rev() {
            if let Err(error) = fs::rename(aside, path) {
                tracing::warn!(%error, path = %path.display(), "could not restore a 2.3.1 file");
            }
        }
    }

    pub(super) fn adopt(legacy: &Path, root: &Path) -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| color_eyre::eyre::eyre!("no home directory"))?;
        let data = dirs::data_dir().ok_or_else(|| color_eyre::eyre::eyre!("no data directory"))?;
        let config =
            dirs::config_dir().ok_or_else(|| color_eyre::eyre::eyre!("no config directory"))?;
        let executable = std::env::current_exe().wrap_err("could not locate Hex")?;
        adopt_with(
            legacy,
            root,
            &executable,
            &Legacy::for_user(&home, &data, &config),
            &[],
        )
    }

    /// `environment` is added to the installer's (tests point it at a
    /// temporary home and stub `systemctl`).
    pub(super) fn adopt_with(
        legacy: &Path,
        root: &Path,
        executable: &Path,
        files: &Legacy,
        environment: &[(&str, &std::ffi::OsStr)],
    ) -> Result<PathBuf> {
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(legacy.join("adopt.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .wrap_err("another launch is already moving the 2.3.1 install")?;
        let moved = files.retire(legacy)?;
        let mut installer = Command::new("sh");
        installer
            .arg("-c")
            .arg(INSTALLER)
            .args(["install-linux.sh", "install"])
            .env("HEX_INSTALL_ARTIFACT", executable)
            .env("HEX_INSTALL_VERSION", crate::VERSION)
            .env("HEX_INSTALL_CHANNEL", "stable")
            .env("HEX_APPLICATION_SUPPORT_DIR", root)
            .env_remove("HEX_INSTALL_DIR")
            .env_remove("HEX_INSTALL_SOURCE")
            .envs(environment.iter().copied())
            .stdin(Stdio::null());
        let output = installer.output().wrap_err("could not run the installer");
        let output = match output {
            Ok(output) if output.status.success() => output,
            result => {
                restore(&moved);
                return Err(match result {
                    Ok(output) => color_eyre::eyre::eyre!(
                        "the installer failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ),
                    Err(error) => error,
                });
            }
        };
        tracing::info!(output = %String::from_utf8_lossy(&output.stdout).trim(), "installed the managed layout");
        for (_, aside) in &moved {
            let _ = fs::remove_file(aside);
        }
        // The new unit reads the session's display from here; the next
        // graphical launch rewrites it.
        let session = root.join("session.env");
        if !session.exists()
            && let Ok(body) = fs::read(legacy.join("session.env"))
        {
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&session)
                .and_then(|mut file| std::io::Write::write_all(&mut file, &body))
                .wrap_err("could not copy the session environment")?;
        }
        Ok(root.join("current/hex"))
    }

    /// Becomes the managed executable with the same arguments. Under the
    /// old unit (which pointed the data folder at the legacy one) it keeps
    /// running as that unit's process; its next start uses the new unit.
    pub(super) fn relaunch(executable: &Path, legacy: &Path, root: &Path) -> Result<()> {
        let mut command = Command::new(executable);
        command.args(std::env::args_os().skip(1));
        if std::env::var_os("HEX_APPLICATION_SUPPORT_DIR")
            .is_some_and(|dir| Path::new(&dir) == legacy)
        {
            command.env("HEX_APPLICATION_SUPPORT_DIR", root);
        }
        Err(command.exec()).wrap_err("could not start the managed Hex")
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use color_eyre::eyre::bail;
    use fs2::FileExt;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use super::*;
    use crate::desktop::background_process;

    /// 2.3.1's Apps & Features entry. Its uninstaller removes the Start Menu
    /// shortcut, which now points at the managed install.
    const LEGACY_UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{8FCF0256-C55C-40C3-865D-5FADB32D13B3}_is1";

    pub(super) fn adopt(_legacy: &Path, root: &Path) -> Result<PathBuf> {
        let updates = root.join("updates");
        fs::create_dir_all(&updates)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(updates.join("update.lock"))?;
        lock.try_lock_exclusive()
            .wrap_err("a Hex install or update is already in progress")?;
        let staging = updates.join("hex.exe.staging");
        let _ = fs::remove_file(&staging);
        fs::copy(std::env::current_exe()?, &staging).wrap_err("could not copy Hex")?;
        let executable = super::super::platform::activate(root, crate::VERSION, &staging)?;
        write_receipt(root)?;
        if super::super::receipt_channel(root).is_none() {
            bail!("the managed install's receipt does not verify");
        }
        if let Err(error) = replace_shortcut(&executable) {
            tracing::warn!(%error, "could not point the Start Menu shortcut at the managed Hex");
        }
        if let Err(error) = crate::windows::login_item::repoint(&executable) {
            tracing::warn!(%error, "could not point Launch at login at the managed Hex");
        }
        match RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(LEGACY_UNINSTALL_KEY) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!(%error, "could not remove the 2.3.1 Apps & Features entry");
            }
            _ => {}
        }
        Ok(executable)
    }

    /// The receipt `install-windows.ps1` writes.
    fn write_receipt(root: &Path) -> Result<()> {
        let receipt = serde_json::json!({
            "app": super::super::APP,
            "artifact": super::super::ARTIFACT,
            "channel": "stable",
            "method": "direct",
            "root": root,
        });
        let partial = root.join("install.json.partial");
        fs::write(&partial, serde_json::to_vec(&receipt)?)?;
        fs::rename(&partial, root.join("install.json"))?;
        Ok(())
    }

    /// Recreates `Hex.lnk` as the installer's managed shortcut (2.3.1 named
    /// its file `HEX.lnk`), unless the user made their own.
    fn replace_shortcut(executable: &Path) -> Result<()> {
        let Some(appdata) = dirs::config_dir() else {
            return Ok(());
        };
        let status = background_process::command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "$shell = New-Object -ComObject WScript.Shell; \
                 if (Test-Path -LiteralPath $env:HEX_SHORTCUT) { \
                 if ($shell.CreateShortcut($env:HEX_SHORTCUT).Description -ne 'Hex managed install') { exit 0 }; \
                 Remove-Item -LiteralPath $env:HEX_SHORTCUT -Force }; \
                 $s = $shell.CreateShortcut($env:HEX_SHORTCUT); \
                 $s.TargetPath = $env:HEX_EXECUTABLE; \
                 $s.Arguments = 'app'; \
                 $s.Description = 'Hex managed install'; \
                 $s.WorkingDirectory = (Split-Path -Parent $env:HEX_EXECUTABLE); \
                 $s.Save()",
            ])
            .env(
                "HEX_SHORTCUT",
                appdata.join(r"Microsoft\Windows\Start Menu\Programs\Hex.lnk"),
            )
            .env("HEX_EXECUTABLE", executable)
            .env_remove("PSModulePath")
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

    /// Starts the managed executable with the same arguments; this process
    /// exits before claiming the single instance.
    pub(super) fn relaunch(executable: &Path, _legacy: &Path, _root: &Path) -> Result<()> {
        background_process::command(executable)
            .args(std::env::args_os().skip(1))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .wrap_err("could not start the managed Hex")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("hex-legacy-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path.canonicalize().unwrap()
    }

    #[test]
    fn only_an_executable_in_the_legacy_versions_folder_is_legacy() {
        let data = temporary("detect");
        let legacy = data.join("voice-control");
        fs::create_dir_all(legacy.join("versions/3.4.0")).unwrap();
        fs::create_dir_all(data.join("hex/versions/3.4.0")).unwrap();
        let old = legacy.join("versions/3.4.0/hex");
        let new = data.join("hex/versions/3.4.0/hex");
        fs::write(&old, b"").unwrap();
        fs::write(&new, b"").unwrap();
        assert_eq!(legacy_root(&old, &data), Some(legacy));
        assert_eq!(legacy_root(&new, &data), None);
        let _ = fs::remove_dir_all(data);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_legacy_linux_install_moves_into_the_installers_layout() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let home = temporary("linux");
        let data = home.join(".local/share");
        let config = home.join(".config");
        let legacy = data.join("voice-control");
        let root = data.join("hex");
        let version = legacy.join("versions").join(crate::VERSION);
        fs::create_dir_all(&version).unwrap();
        let executable = version.join("hex");
        fs::write(
            &executable,
            format!("#!/bin/sh\necho hex {}\n", crate::VERSION),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        symlink(
            Path::new("versions").join(crate::VERSION),
            legacy.join("current"),
        )
        .unwrap();
        fs::write(legacy.join("session.env"), "DISPLAY=\":0\"\n").unwrap();
        for directory in [
            home.join(".local/bin"),
            data.join("applications"),
            config.join("autostart"),
            config.join("systemd/user"),
        ] {
            fs::create_dir_all(&directory).unwrap();
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        }
        symlink(legacy.join("current/hex"), home.join(".local/bin/hex")).unwrap();
        fs::write(
            config.join("systemd/user/hex.service"),
            "# X-HEX-Managed=true\n[Service]\nExecStart=hex service\n",
        )
        .unwrap();
        fs::write(
            data.join("applications/hex.desktop"),
            "[Desktop Entry]\nX-HEX-Managed=true\n",
        )
        .unwrap();
        fs::write(
            config.join("autostart/hex.desktop"),
            "[Desktop Entry]\nName=Mine\n",
        )
        .unwrap();
        let stubs = home.join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        fs::write(stubs.join("systemctl"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(stubs.join("systemctl"), fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!(
            "{}:{}",
            stubs.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let environment = [
            ("PATH", path.as_ref()),
            ("HOME", home.as_os_str()),
            ("XDG_DATA_HOME", data.as_os_str()),
            ("XDG_CONFIG_HOME", config.as_os_str()),
        ];

        let installed = linux::adopt_with(
            &legacy,
            &root,
            &executable,
            &linux::Legacy::for_user(&home, &data, &config),
            &environment,
        )
        .unwrap();

        assert_eq!(installed, root.join("current/hex"));
        assert_eq!(
            fs::read(&installed).unwrap(),
            fs::read(&executable).unwrap()
        );
        assert_eq!(
            super::super::receipt_channel(&root).as_deref(),
            Some("stable")
        );
        assert_eq!(
            fs::read_link(home.join(".local/bin/hex")).unwrap(),
            root.join("current/hex")
        );
        let unit = fs::read_to_string(config.join("systemd/user/hex.service")).unwrap();
        assert!(
            unit.lines().any(|line| line == "# X-Hex-Managed=true"),
            "{unit}"
        );
        assert!(data.join("applications/anomaly.hex.desktop").exists());
        assert!(!data.join("applications/hex.desktop").exists());
        // An entry the user wrote stays.
        assert!(config.join("autostart/hex.desktop").exists());
        assert_eq!(
            fs::read_to_string(root.join("session.env")).unwrap(),
            "DISPLAY=\":0\"\n"
        );
        assert!(
            fs::read_dir(config.join("systemd/user"))
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".legacy"))
        );
        let _ = fs::remove_dir_all(home);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_failed_move_puts_the_legacy_files_back() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let home = temporary("linux-restore");
        let data = home.join(".local/share");
        let config = home.join(".config");
        let legacy = data.join("voice-control");
        fs::create_dir_all(legacy.join("versions/1.0.0")).unwrap();
        fs::create_dir_all(home.join(".local/bin")).unwrap();
        fs::create_dir_all(config.join("systemd/user")).unwrap();
        let unit = config.join("systemd/user/hex.service");
        fs::write(&unit, "# X-HEX-Managed=true\n").unwrap();
        symlink(legacy.join("current/hex"), home.join(".local/bin/hex")).unwrap();
        // Reports the wrong version, so the installer refuses it.
        let executable = legacy.join("versions/1.0.0/hex");
        fs::write(&executable, "#!/bin/sh\necho hex 0.0.1\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

        let result = linux::adopt_with(
            &legacy,
            &data.join("hex"),
            &executable,
            &linux::Legacy::for_user(&home, &data, &config),
            &[
                ("HOME", home.as_os_str()),
                ("XDG_DATA_HOME", data.as_os_str()),
                ("XDG_CONFIG_HOME", config.as_os_str()),
            ],
        );

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&unit).unwrap(), "# X-HEX-Managed=true\n");
        assert_eq!(
            fs::read_link(home.join(".local/bin/hex")).unwrap(),
            legacy.join("current/hex")
        );
        let _ = fs::remove_dir_all(home);
    }
}
