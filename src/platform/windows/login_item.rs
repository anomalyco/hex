use std::io::ErrorKind;
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "HEX";

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
        Err(error) => Err(error).wrap_err("could not read the HEX startup registration"),
    }
}

/// Register or unregister the current executable for the current Windows user.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    if enabled {
        let (key, _) = current_user
            .create_subkey(RUN_KEY)
            .wrap_err("could not open Windows startup applications")?;
        let executable = std::env::current_exe().wrap_err("could not locate the HEX executable")?;
        key.set_value(VALUE_NAME, &startup_command(&executable))
            .wrap_err("could not register HEX to launch at login")?;
    } else {
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
            return Err(error).wrap_err("could not unregister HEX from Windows startup");
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
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = current_user
        .create_subkey(RUN_KEY)
        .wrap_err("could not open Windows startup applications")?;
    key.set_value(VALUE_NAME, &startup_command(executable))
        .wrap_err("could not repoint the HEX startup registration")?;
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
            startup_command(Path::new(r"C:\Program Files\HEX\hex.exe")),
            r#""C:\Program Files\HEX\hex.exe" app --hidden"#
        );
    }
}
