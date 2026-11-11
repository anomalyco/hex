use std::io::ErrorKind;
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Hex";

/// Return the Windows-owned startup registration state.
pub fn is_enabled() -> Result<bool> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let key = match current_user.open_subkey_with_flags(RUN_KEY, KEY_READ) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).wrap_err("could not read Windows startup applications"),
    };
    match key.get_value::<String, _>(VALUE_NAME) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).wrap_err("could not read the Hex startup registration"),
    }
}

/// Register or unregister the current executable for the current Windows user.
pub fn set_enabled(enabled: bool) -> Result<()> {
    if enabled {
        let executable = std::env::current_exe().wrap_err("could not locate the Hex executable")?;
        write_startup_command(&executable, "could not register Hex to launch at login")?;
    } else {
        let current_user = RegKey::predef(HKEY_CURRENT_USER);
        let key = match current_user.open_subkey_with_flags(RUN_KEY, KEY_WRITE) {
            Ok(key) => key,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).wrap_err("could not open Windows startup applications");
            }
        };
        if let Err(error) = key.delete_value(VALUE_NAME)
            && error.kind() != ErrorKind::NotFound
        {
            return Err(error).wrap_err("could not unregister Hex from Windows startup");
        }
    }
    Ok(())
}

/// Rewrite an existing startup registration to a new executable, e.g.
/// after a self-update activates a new version directory. Does nothing
/// when launch at login is disabled.
pub fn repoint(executable: &Path) -> Result<()> {
    if !is_enabled()? {
        return Ok(());
    }
    write_startup_command(executable, "could not repoint the Hex startup registration")
}

/// Points Launch at login at this Hex when it starts another version of the
/// same managed install. Setup and install-windows.ps1 switch
/// `current-version` without touching the Run value, which names a
/// version's `hex.exe`, so after a manual upgrade signing in started the
/// version it replaced. Runs at every launch, before the single-instance
/// claim, so opening the new version once fixes it even while the old one
/// still runs. A disabled registration, a value outside the managed
/// `versions` folder, and a copy that isn't the current version stay as they
/// are.
pub fn follow_current_version() -> Result<()> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let key = match current_user.open_subkey_with_flags(RUN_KEY, KEY_READ) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).wrap_err("could not read Windows startup applications"),
    };
    let command = match key.get_value::<String, _>(VALUE_NAME) {
        Ok(command) => command,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).wrap_err("could not read the Hex startup registration"),
    };
    let executable = std::env::current_exe().wrap_err("could not locate the Hex executable")?;
    let support = crate::app_paths::support_dir()?;
    let current = std::fs::read_to_string(support.join("current-version")).ok();
    if !starts_another_version(&command, &executable, &support, current.as_deref()) {
        return Ok(());
    }
    write_startup_command(
        &executable,
        "could not repoint the Hex startup registration",
    )?;
    tracing::info!(
        previous = %command,
        "pointed Launch at login at this version of Hex"
    );
    Ok(())
}

/// Whether the Run value `command` starts a `hex.exe` in `support\versions`
/// other than `executable`, which is that install's current version.
fn starts_another_version(
    command: &str,
    executable: &Path,
    support: &Path,
    current: Option<&str>,
) -> bool {
    let versions = format!(
        r"{}\",
        support
            .join("versions")
            .display()
            .to_string()
            .to_lowercase()
    );
    let running = executable.display().to_string().to_lowercase();
    let Some(version) = running.strip_prefix(&versions) else {
        return false;
    };
    let is_current = current.is_some_and(|current| {
        version.eq_ignore_ascii_case(&format!(r"{}\hex.exe", current.trim()))
    });
    is_current
        && command.to_lowercase().contains(&versions)
        && !command.eq_ignore_ascii_case(&startup_command(executable))
}

fn write_startup_command(executable: &Path, context: &'static str) -> Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .wrap_err("could not open Windows startup applications")?;
    key.set_value(VALUE_NAME, &startup_command(executable))
        .wrap_err(context)?;
    Ok(())
}

fn startup_command(executable: &Path) -> String {
    format!(r#""{}" app --hidden"#, executable.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_command_quotes_paths_with_spaces() {
        assert_eq!(
            startup_command(Path::new(r"C:\Program Files\Hex\hex.exe")),
            r#""C:\Program Files\Hex\hex.exe" app --hidden"#
        );
    }

    /// Setup or install-windows.ps1 put 3.4.0 over 3.3.0 and left Launch at
    /// login on the 3.3.0 executable.
    #[test]
    fn a_manual_upgrade_moves_launch_at_login_to_the_current_version() {
        let support = Path::new(r"C:\Users\Jürgen\AppData\Roaming\hex");
        let running = support.join(r"versions\3.4.0\hex.exe");
        let old = r#""C:\Users\Jürgen\AppData\Roaming\hex\versions\3.3.0\hex.exe" app --hidden"#;
        let moves = |command: &str, executable: &Path, current: Option<&str>| {
            starts_another_version(command, executable, support, current)
        };
        assert!(moves(old, &running, Some("3.4.0\n")));
        // Windows paths compare without case.
        assert!(moves(&old.to_uppercase(), &running, Some("3.4.0")));
        // Already this version's.
        assert!(!moves(&startup_command(&running), &running, Some("3.4.0")));
        // Not the version current-version names: an older copy started by
        // hand, or an update staged while this one runs.
        assert!(!moves(old, &running, Some("3.5.0")));
        assert!(!moves(old, &running, None));
        // A value outside the managed layout is the user's.
        assert!(!moves(
            r#""D:\Tools\hex.exe" app --hidden"#,
            &running,
            Some("3.4.0")
        ));
        // A copy running outside the layout, such as a build.
        assert!(!moves(
            old,
            Path::new(r"C:\src\hex\target\release\hex.exe"),
            Some("3.4.0")
        ));
    }
}
