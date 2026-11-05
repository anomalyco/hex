//! `hex.com`, the console entry point of Hex on Windows. `hex.exe` is a GUI
//! program so launching Hex opens no console window (#850), but shells don't
//! wait for a GUI program or capture its output. A terminal resolves `hex` to
//! `hex.com` first (PATHEXT lists .COM before .EXE), and this runs `hex.exe`
//! with the same arguments, standard handles, and console, waits for it, and
//! exits with its code.
//!
//! It finds `hex.exe` beside itself (`versions\<version>\hex.com`), or, as
//! the copy in `%APPDATA%\hex\bin` that PATH names, the version
//! `current-version` points at.

#[cfg(windows)]
fn main() {
    use windows_sys::Win32::Foundation::TRUE;
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    // Ctrl+C reaches every process on the console: hex.exe handles it, and
    // this stays to report its exit code.
    unsafe extern "system" fn wait_for_hex(_: u32) -> i32 {
        TRUE
    }
    // SAFETY: registers a handler that lives for the whole process.
    unsafe { SetConsoleCtrlHandler(Some(wait_for_hex), TRUE) };

    let hex = match std::env::current_exe().map(|path| locate(&path)) {
        Ok(Some(hex)) => hex,
        _ => {
            eprintln!("hex: couldn't find hex.exe; reinstall Hex from anoma.ly/hex");
            std::process::exit(1);
        }
    };
    match std::process::Command::new(&hex)
        .args(std::env::args_os().skip(1))
        .status()
    {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("hex: couldn't start {}: {error}", hex.display());
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("hex-console is Hex's Windows console entry point");
    std::process::exit(2);
}

/// `hex.exe` beside `console`, or in the version `current-version` beside
/// its folder names.
#[cfg_attr(not(windows), allow(dead_code))]
fn locate(console: &std::path::Path) -> Option<std::path::PathBuf> {
    let folder = console.parent()?;
    let beside = folder.join("hex.exe");
    if beside.is_file() {
        return Some(beside);
    }
    let support = folder.parent()?;
    let version = std::fs::read_to_string(support.join("current-version")).ok()?;
    let version = version.trim();
    let named = !version.is_empty()
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+_-".contains(&byte));
    let current = support.join("versions").join(version).join("hex.exe");
    (named && current.is_file()).then_some(current)
}

#[cfg(test)]
mod tests {
    use super::locate;

    #[test]
    fn finds_hex_beside_it_or_through_current_version() {
        let root = std::env::temp_dir().join(format!("hex-console-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let version = root.join("versions").join("3.4.0");
        std::fs::create_dir_all(&version).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(version.join("hex.exe"), b"").unwrap();

        assert_eq!(
            locate(&version.join("hex.com")),
            Some(version.join("hex.exe"))
        );
        let bin = root.join("bin").join("hex.com");
        assert_eq!(locate(&bin), None, "no current-version yet");
        std::fs::write(root.join("current-version"), "3.4.0\n").unwrap();
        assert_eq!(locate(&bin), Some(version.join("hex.exe")));
        std::fs::write(root.join("current-version"), "..\\..\\elsewhere").unwrap();
        assert_eq!(
            locate(&bin),
            None,
            "a pointer that leaves versions is ignored"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
