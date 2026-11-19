//! `hex.com`, the console entry point of Hex on Windows. The GUI executable
//! is a GUI program so launching Hex opens no console window (#850), but
//! shells don't wait for a GUI program or capture its output. A terminal
//! resolves `hex` to `hex.com` first (PATHEXT lists .COM before .EXE), and
//! this runs the GUI executable with the same arguments, standard handles,
//! and console, waits for it, and exits with its code. Opening the app (`hex`,
//! `hex app [--hidden]`) doesn't wait: the app stays resident, so the terminal
//! returns once it starts, as it would for a GUI program.
//!
//! The GUI executable is `hex-voice.exe`, because Discord's game detection
//! matches a running `hex.exe` to *HEX: Shards of Fate* (#445); releases
//! before that rename shipped `hex.exe`, which this still finds. It looks
//! beside itself (`versions\<version>\hex.com`), or, as the copy in
//! `%APPDATA%\hex\bin` that PATH names, in the version `current-version`
//! points at.

#[cfg(windows)]
fn main() {
    use windows_sys::Win32::Foundation::{FALSE, TRUE};
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler,
    };

    // Ctrl+C reaches every process on the console: the GUI executable handles
    // it, and this stays to report its exit code. Closing the console still
    // ends it.
    unsafe extern "system" fn wait_for_hex(event: u32) -> i32 {
        if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
            TRUE
        } else {
            FALSE
        }
    }
    // SAFETY: registers a handler that lives for the whole process.
    unsafe { SetConsoleCtrlHandler(Some(wait_for_hex), TRUE) };

    let hex = match std::env::current_exe().map(|path| locate(&path)) {
        Ok(Some(hex)) => hex,
        _ => {
            eprintln!("hex: couldn't find Hex's executable; reinstall Hex from anoma.ly/hex");
            std::process::exit(1);
        }
    };
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let mut command = std::process::Command::new(&hex);
    command.args(&arguments);
    let result = if opens_app(&arguments) {
        command.spawn().map(|_| 0)
    } else {
        command.status().map(|status| status.code().unwrap_or(1))
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("hex: couldn't start {}: {error}", hex.display());
            std::process::exit(1);
        }
    }
}

/// `hex`, `hex app`, or `hex app --hidden`: the resident app, not a command
/// that finishes. A headless render (`hex app -- --snap …`) or `--help` does.
#[cfg_attr(not(windows), allow(dead_code))]
fn opens_app(arguments: &[std::ffi::OsString]) -> bool {
    match arguments.split_first() {
        None => true,
        Some((first, rest)) => first == "app" && rest.iter().all(|argument| argument == "--hidden"),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("hex-console is Hex's Windows console entry point");
    std::process::exit(2);
}

/// The GUI executables beside `console`, in the order releases install them:
/// `hex-voice.exe` since the rename that keeps Discord's game detection from
/// matching *HEX: Shards of Fate* (#445), then `hex.exe` from releases before
/// it.
const GUI_EXECUTABLES: [&str; 2] = ["hex-voice.exe", "hex.exe"];

/// The GUI executable in `folder`: its new name, else the name releases before
/// the rename installed.
#[cfg_attr(not(windows), allow(dead_code))]
fn existing(folder: &std::path::Path) -> Option<std::path::PathBuf> {
    GUI_EXECUTABLES
        .iter()
        .map(|name| folder.join(name))
        .find(|path| path.is_file())
}

/// The GUI executable beside `console`, or in the version `current-version`
/// beside its folder names. Releases before the rename have only `hex.exe`.
#[cfg_attr(not(windows), allow(dead_code))]
fn locate(console: &std::path::Path) -> Option<std::path::PathBuf> {
    let folder = console.parent()?;
    if let Some(hex) = existing(folder) {
        return Some(hex);
    }
    let support = folder.parent()?;
    let version = std::fs::read_to_string(support.join("current-version")).ok()?;
    let version = version.trim();
    let named = !version.is_empty()
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+_-".contains(&byte));
    (named)
        .then(|| support.join("versions").join(version))
        .and_then(|folder| existing(&folder))
}

#[cfg(test)]
mod tests {
    use super::{locate, opens_app};

    #[test]
    fn only_opening_the_app_returns_without_waiting() {
        let opens =
            |arguments: &[&str]| opens_app(&arguments.iter().map(Into::into).collect::<Vec<_>>());
        assert!(opens(&[]));
        assert!(opens(&["app"]));
        assert!(opens(&["app", "--hidden"]));
        assert!(!opens(&["app", "--", "--snap", "out.png"]));
        assert!(!opens(&["app", "--help"]));
        assert!(!opens(&["status", "--json"]));
        assert!(!opens(&["listen"]));
        assert!(!opens(&["--version"]));
    }

    #[test]
    fn finds_the_renamed_executable_beside_it_or_through_current_version() {
        let root = std::env::temp_dir().join(format!("hex-console-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let version = root.join("versions").join("3.4.0");
        std::fs::create_dir_all(&version).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(version.join("hex-voice.exe"), b"").unwrap();

        assert_eq!(
            locate(&version.join("hex.com")),
            Some(version.join("hex-voice.exe"))
        );
        let bin = root.join("bin").join("hex.com");
        assert_eq!(locate(&bin), None, "no current-version yet");
        std::fs::write(root.join("current-version"), "3.4.0\n").unwrap();
        assert_eq!(locate(&bin), Some(version.join("hex-voice.exe")));
        std::fs::write(root.join("current-version"), "..\\..\\elsewhere").unwrap();
        assert_eq!(
            locate(&bin),
            None,
            "a pointer that leaves versions is ignored"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn finds_a_release_before_the_rename_and_prefers_the_new_name() {
        let root = std::env::temp_dir().join(format!("hex-console-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let version = root.join("versions").join("3.3.0");
        std::fs::create_dir_all(&version).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(version.join("hex.exe"), b"").unwrap();
        std::fs::write(root.join("current-version"), "3.3.0\n").unwrap();

        // A release staged before the rename still starts through hex.com,
        // beside it and through bin\hex.com.
        assert_eq!(
            locate(&version.join("hex.com")),
            Some(version.join("hex.exe"))
        );
        let bin = root.join("bin").join("hex.com");
        assert_eq!(locate(&bin), Some(version.join("hex.exe")));

        std::fs::write(version.join("hex-voice.exe"), b"").unwrap();
        assert_eq!(
            locate(&bin),
            Some(version.join("hex-voice.exe")),
            "the renamed executable wins when both exist"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
