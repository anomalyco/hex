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
/// keeps Hex's managed entry current: its icon (#652), and its command when
/// the program it starts is gone, as after a downloaded binary moved or a
/// package replaced it (#736). A symlink is the user's, whatever it points to.
pub fn tidy() -> Result<()> {
    let directory = autostart_dir()?;
    move_legacy_entry_in(&directory)?;
    refresh_in(&directory, &launcher()?)
}

fn refresh_in(directory: &Path, launcher: &Path) -> Result<()> {
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
    let refreshed = if starts(&current)
        && program(&current).is_none_or(|program| superseded(&program, launcher))
        && launcher.is_file()
    {
        tracing::info!(entry = %entry.display(), "pointed Launch at login at this Hex");
        self::entry(launcher)
    } else {
        crate::linux::portal::with_icon(&current)
    };
    if refreshed == current {
        return Ok(());
    }
    write_atomically(&entry, &refreshed)
}

/// The program a managed entry's `Exec=` starts: its first argument, quoted
/// as [`exec_argument`] writes it.
pub(crate) fn program(entry: &str) -> Option<PathBuf> {
    let exec = entry
        .lines()
        .find_map(|line| line.trim().strip_prefix("Exec="))?;
    // The key file's string escapes first, then the Exec quoting.
    let mut unescaped = String::new();
    let mut escapes = exec.chars();
    while let Some(character) = escapes.next() {
        if character != '\\' {
            unescaped.push(character);
            continue;
        }
        match escapes.next() {
            Some('s') => unescaped.push(' '),
            Some('n') => unescaped.push('\n'),
            Some('t') => unescaped.push('\t'),
            Some('r') => unescaped.push('\r'),
            Some('\\') => unescaped.push('\\'),
            Some(other) => {
                unescaped.push('\\');
                unescaped.push(other);
            }
            None => unescaped.push('\\'),
        }
    }
    let mut characters = unescaped.strip_prefix('"')?.chars();
    let mut program = String::new();
    loop {
        match characters.next()? {
            '"' => return Some(PathBuf::from(program)),
            '\\' => program.push(characters.next()?),
            '%' => {
                characters.next()?;
                program.push('%');
            }
            character => program.push(character),
        }
    }
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

/// Writes or removes Hex's autostart entry: through the desktop's Background
/// portal where it has one (#812), else Hex's own managed file.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let mut changed = CHANGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *changed = true;
    let path = entry_path()?;
    let existing = read(&path)?;
    if let Some(entry) = &existing
        && !ours(entry)
    {
        bail!(
            "{} is not Hex's; edit or remove it yourself",
            path.display()
        );
    }
    let executable = launcher()?;
    match crate::linux::background::request(enabled, &executable) {
        Ok(Some(autostart)) if autostart == enabled => return Ok(()),
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(error = format!("{error:#}"), "the Background portal failed")
        }
    }
    let existing = read(&path)?;
    if enabled {
        write_atomically(&path, &entry(&executable))
    } else if existing.is_some() {
        fs::remove_file(&path).wrap_err_with(|| format!("could not remove {}", path.display()))
    } else {
        Ok(())
    }
}

/// Tells the desktop's Background portal, once at startup, that Hex runs in
/// the background, keeping Launch at login as it is. Asked only while the
/// autostart entry is absent or Hex's: any request rewrites or removes it.
pub fn announce_background() {
    let changed = CHANGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // A Launch at login change already told the portal.
    if *changed {
        return;
    }
    let ask = || -> Result<()> {
        let path = entry_path()?;
        let existing = read(&path)?;
        if existing.as_deref().is_some_and(|entry| !ours(entry)) {
            return Ok(());
        }
        let enabled = existing.as_deref().is_some_and(starts);
        crate::linux::background::request(enabled, &launcher()?)?;
        Ok(())
    };
    if let Err(error) = ask() {
        tracing::warn!(error = format!("{error:#}"), "the Background portal failed");
    }
}

/// Launch at login changes and the startup announcement, one at a time:
/// whether a change has run.
static CHANGES: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

/// The autostart entry's contents; a symlink or anything but a regular file
/// reads as empty, which is never Hex's, so Hex leaves it alone.
fn read(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => return Ok(Some(String::new())),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("could not read {}", path.display()));
        }
    }
    match fs::read_to_string(path) {
        Ok(entry) => Ok(Some(entry)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).wrap_err_with(|| format!("could not read {}", path.display())),
    }
}

/// Hex's own entry: its managed mark, or the Background portal's, which
/// writes the entry under Hex's app id when Hex asks (#812).
fn ours(entry: &str) -> bool {
    entry.lines().map(str::trim).any(|line| {
        line == MANAGED_MARK || line == format!("X-XDP-Autostart={}", crate::linux::portal::APP_ID)
    })
}

/// The executable the entry starts: a managed install's `current/hex`,
/// which outlives the version directory this process runs from.
pub(crate) fn launcher() -> Result<PathBuf> {
    let executable = std::env::current_exe().wrap_err("could not locate the Hex executable")?;
    // Linux names a running binary that was removed or replaced "… (deleted)".
    let executable = match executable
        .to_str()
        .and_then(|path| path.strip_suffix(" (deleted)"))
    {
        Some(path) if !executable.is_file() => PathBuf::from(path),
        _ => executable,
    };
    let current = crate::app_paths::support_dir()?.join("current").join("hex");
    let managed = executable
        .parent()
        .and_then(Path::parent)
        .is_some_and(|versions| versions.ends_with("versions"))
        && fs::canonicalize(&current).is_ok_and(|target| target == executable);
    if managed {
        return Ok(current);
    }
    Ok(shim(&executable).unwrap_or(executable))
}

/// A version manager's shim for `executable` (mise, asdf: `<root>/shims/hex`
/// beside `<root>/installs/…/hex`), which outlives the version it runs: only
/// one inside the home directory, executable, the user's own and writable by
/// no one else, that runs Hex from the home directory (a shim whose tool has
/// no version set there fails).
fn shim(executable: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let shim = shim_path(executable, &home)?;
    static CHECKED: std::sync::Mutex<Option<(PathBuf, bool)>> = std::sync::Mutex::new(None);
    let mut checked = CHECKED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((path, runs)) = checked.as_ref()
        && *path == shim
    {
        return runs.then_some(shim);
    }
    let runs = owned(&shim) && owned(shim.parent()?) && runs_hex(&shim, &home);
    *checked = Some((shim.clone(), runs));
    runs.then_some(shim)
}

/// Where a version manager keeps `executable`'s shim, inside `home`.
fn shim_path(executable: &Path, home: &Path) -> Option<PathBuf> {
    let name = executable.file_name()?;
    let installs = executable
        .ancestors()
        .take_while(|ancestor| *ancestor != home)
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "installs"))?;
    installs.starts_with(home).then_some(())?;
    let shim = installs.parent()?.join("shims").join(name);
    shim.is_file().then_some(shim)
}

/// Executable or searchable, the user's, and writable by no one else.
fn owned(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).is_ok_and(|metadata| {
        // SAFETY: geteuid has no preconditions.
        metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o022 == 0
            && metadata.mode() & 0o111 != 0
    })
}

/// Whether `shim --version`, run from `home`, says it's Hex.
fn runs_hex(shim: &Path, home: &Path) -> bool {
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new(shim)
        .arg("--version")
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return false,
            Ok(None) if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
    let mut output = String::new();
    child
        .stdout
        .take()
        .is_some_and(|mut stdout| std::io::Read::read_to_string(&mut stdout, &mut output).is_ok())
        && output.trim_start().starts_with("hex ")
}

/// Whether an entry that starts `program` should start `launcher` instead:
/// `program` is gone, or `launcher` is the version manager's shim that runs
/// it (#810), so an upgrade's prune can't strand the entry.
pub(crate) fn superseded(program: &Path, launcher: &Path) -> bool {
    program != launcher
        && launcher.is_file()
        && (!program.is_file() || shim(program).as_deref() == Some(launcher))
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
        crate::linux::portal::exec_argument(&executable.to_string_lossy()),
        crate::linux::portal::APP_ID,
    )
}

/// `value` quoted as one Exec argument (Desktop Entry Specification).
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
        assert!(entry.contains("Exec=\"/home/a b/100%%/hex\\\\$\" start\n"));
        assert!(entry.contains("X-Hex-Managed=true\n"));
        assert!(starts(&entry));
    }

    #[test]
    fn a_version_managers_shim_outlives_the_version() {
        let root = std::env::temp_dir().join(format!("hex-login-shim-{}", std::process::id()));
        let executable = root.join("mise/installs/hex/3.1.1/hex");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "").unwrap();
        assert_eq!(shim_path(&executable, &root), None);
        fs::create_dir_all(root.join("mise/shims")).unwrap();
        fs::write(root.join("mise/shims/hex"), "").unwrap();
        assert_eq!(
            shim_path(&executable, &root),
            Some(root.join("mise/shims/hex"))
        );
        assert_eq!(shim_path(Path::new("/usr/bin/hex"), &root), None);
        // Outside the home directory a shim is nobody's to trust.
        assert_eq!(shim_path(&executable, Path::new("/home/nobody")), None);
        // The entry's program reads back through both levels of escaping.
        let odd = Path::new("/tmp/my $dir/100%/a\"b\\c/hex");
        assert_eq!(program(&entry(odd)).as_deref(), Some(odd));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_symlinked_entry_is_never_hexs() {
        let root = std::env::temp_dir().join(format!("hex-login-link-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("target.desktop");
        fs::write(&target, "[Desktop Entry]\nX-XDP-Autostart=anomaly.hex\n").unwrap();
        assert!(ours(&read(&target).unwrap().unwrap()));
        let link = root.join("anomaly.hex.desktop");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(!ours(&read(&link).unwrap().unwrap()));
        assert_eq!(read(&root.join("missing.desktop")).unwrap(), None);
        fs::remove_dir_all(root).unwrap();
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
        refresh_in(&directory, Path::new("/bin/sh")).unwrap();
        assert!(
            fs::read_to_string(&entry)
                .unwrap()
                .contains("\nIcon=anomaly.hex\n")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_entry_whose_program_is_gone_starts_this_hex() {
        assert_eq!(
            program(&entry(Path::new("/home/a b/100%/hex$"))),
            Some(PathBuf::from("/home/a b/100%/hex$"))
        );
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("hex-autostart-refresh-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let launcher = directory.join("hex");
        fs::write(&launcher, "").unwrap();
        let path = directory.join("anomaly.hex.desktop");
        // A downloaded binary that has since moved.
        fs::write(&path, entry(&directory.join("Downloads/hex"))).unwrap();
        refresh_in(&directory, &launcher).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), entry(&launcher));
        // Turned off in the desktop's settings: left off.
        let off = format!("{}Hidden=true\n", entry(&directory.join("gone")));
        fs::write(&path, &off).unwrap();
        refresh_in(&directory, &launcher).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("gone"));
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
