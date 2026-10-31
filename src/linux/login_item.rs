//! Launch at login on Linux: the XDG autostart entry
//! `~/.config/autostart/anomaly.hex.desktop`, named for Hex's app id, the
//! same managed file the installer writes. Hex only replaces or removes an entry that carries its
//! `X-Hex-Managed=true` mark; the file on disk is the only state.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};

const MANAGED_MARK: &str = "X-Hex-Managed=true";

fn autostart_dir() -> Result<PathBuf> {
    let config = dirs::config_dir().ok_or_else(|| eyre!("the config directory is unavailable"))?;
    Ok(config.join("autostart"))
}

fn entry_path() -> Result<PathBuf> {
    Ok(autostart_dir()?.join(format!("{}.desktop", crate::linux::portal::APP_ID)))
}

/// Renames Hex's managed `hex.desktop` autostart entry, the name before
/// #579, to the app id's, unless an entry already has the new name, then
/// names Hex's icon in its managed entry (#652). A symlink is the user's,
/// whatever it points to.
pub fn tidy() -> Result<()> {
    let directory = autostart_dir()?;
    move_legacy_entry_in(&directory)?;
    name_icon_in(&directory)
}

fn name_icon_in(directory: &Path) -> Result<()> {
    let entry = directory.join(format!("{}.desktop", crate::linux::portal::APP_ID));
    match fs::symlink_metadata(&entry) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("could not read {}", entry.display()));
        }
    }
    let current = fs::read_to_string(&entry)
        .wrap_err_with(|| format!("could not read {}", entry.display()))?;
    if !current.lines().any(|line| line.trim() == MANAGED_MARK) {
        return Ok(());
    }
    let named = crate::linux::portal::with_icon(&current);
    if named == current {
        return Ok(());
    }
    write_atomically(&entry, &named)
}

fn move_legacy_entry_in(directory: &Path) -> Result<()> {
    let legacy = directory.join("hex.desktop");
    match fs::symlink_metadata(&legacy) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("could not read {}", legacy.display()));
        }
    }
    let old = fs::read_to_string(&legacy)
        .wrap_err_with(|| format!("could not read {}", legacy.display()))?;
    if !old.lines().any(|line| line.trim() == MANAGED_MARK) {
        return Ok(());
    }
    let entry = directory.join(format!("{}.desktop", crate::linux::portal::APP_ID));
    if fs::symlink_metadata(&entry).is_ok() {
        fs::remove_file(&legacy)
    } else {
        fs::rename(&legacy, &entry)
    }
    .wrap_err_with(|| format!("could not move {}", legacy.display()))
}

/// Whether the desktop starts Hex at login.
pub fn is_enabled() -> Result<bool> {
    let path = entry_path()?;
    match fs::read_to_string(&path) {
        Ok(entry) => Ok(starts(&entry)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).wrap_err_with(|| format!("could not read {}", path.display())),
    }
}

/// Writes or removes Hex's autostart entry.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let path = entry_path()?;
    let existing = match fs::read_to_string(&path) {
        Ok(entry) => Some(entry),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("could not read {}", path.display()));
        }
    };
    if let Some(entry) = &existing
        && !entry.lines().any(|line| line.trim() == MANAGED_MARK)
    {
        bail!(
            "{} is not Hex's; edit or remove it yourself",
            path.display()
        );
    }
    if enabled {
        let executable = launcher()?;
        write_atomically(&path, &entry(&executable))
    } else if existing.is_some() {
        fs::remove_file(&path).wrap_err_with(|| format!("could not remove {}", path.display()))
    } else {
        Ok(())
    }
}

/// The executable the entry starts: a managed install's `current/hex`,
/// which outlives the version directory this process runs from.
fn launcher() -> Result<PathBuf> {
    let executable = std::env::current_exe().wrap_err("could not locate the Hex executable")?;
    let current = crate::app_paths::support_dir()?.join("current").join("hex");
    let managed = executable
        .parent()
        .and_then(Path::parent)
        .is_some_and(|versions| versions.ends_with("versions"))
        && fs::canonicalize(&current).is_ok_and(|target| target == executable);
    Ok(if managed { current } else { executable })
}

/// Whether an autostart entry starts its program: the spec's `Hidden` and
/// GNOME's enabled key can both turn it off.
fn starts(entry: &str) -> bool {
    !entry.lines().map(str::trim).any(|line| {
        line.eq_ignore_ascii_case("Hidden=true")
            || line.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
    })
}

fn entry(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Hex\n\
         Comment=Local voice dictation\n\
         Exec={} start\n\
         Icon={}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n\
         {MANAGED_MARK}\n",
        exec_argument(&executable.to_string_lossy()),
        crate::linux::portal::APP_ID,
    )
}

/// `value` quoted as one Exec argument (Desktop Entry Specification).
fn exec_argument(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| eyre!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(directory)
        .wrap_err_with(|| format!("could not create {}", directory.display()))?;
    let staged = directory.join(format!(".{}.desktop.next", crate::linux::portal::APP_ID));
    let mut file = fs::File::create(&staged)
        .wrap_err_with(|| format!("could not write {}", staged.display()))?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    fs::rename(&staged, path).wrap_err_with(|| format!("could not write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_starts_hex_with_a_quoted_path() {
        let entry = entry(Path::new("/home/a b/100%/hex$"));
        assert!(entry.contains("Exec=\"/home/a b/100%%/hex\\$\" start\n"));
        assert!(entry.contains("X-Hex-Managed=true\n"));
        assert!(starts(&entry));
    }

    #[test]
    fn only_hexs_old_regular_entry_moves() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("hex-autostart-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let legacy = directory.join("hex.desktop");
        let entry = directory.join("anomaly.hex.desktop");
        fs::write(
            &legacy,
            "[Desktop Entry]\nExec=hex start\nX-Hex-Managed=true\n",
        )
        .unwrap();
        move_legacy_entry_in(&directory).unwrap();
        assert!(!legacy.exists());
        assert!(starts(&fs::read_to_string(&entry).unwrap()));
        fs::write(&legacy, "[Desktop Entry]\nExec=mine\n").unwrap();
        move_legacy_entry_in(&directory).unwrap();
        assert!(legacy.exists());
        fs::remove_file(&legacy).unwrap();
        std::os::unix::fs::symlink(&entry, &legacy).unwrap();
        move_legacy_entry_in(&directory).unwrap();
        assert!(legacy.is_symlink());
        fs::remove_file(&legacy).unwrap();
        fs::write(
            &entry,
            "[Desktop Entry]\nIcon=audio-input-microphone\nX-Hex-Managed=true\n",
        )
        .unwrap();
        name_icon_in(&directory).unwrap();
        assert!(
            fs::read_to_string(&entry)
                .unwrap()
                .contains("\nIcon=anomaly.hex\n")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn hidden_or_disabled_entries_do_not_start() {
        assert!(!starts("[Desktop Entry]\nHidden=true\n"));
        assert!(!starts(
            "[Desktop Entry]\nX-GNOME-Autostart-enabled=false\n"
        ));
    }
}
