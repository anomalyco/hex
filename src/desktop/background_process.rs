//! Child processes Hex starts for its own work: the OpenCode CLI, curl, and
//! PowerShell. The native app detaches from its console, so on Windows each
//! console program would otherwise open a terminal window of its own, and
//! closing it would kill the child.

use std::ffi::OsStr;
use std::process::Command;

/// A command for a background child that never shows a window. Use it for
/// every process Hex spawns without a visible purpose.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[cfg(all(test, windows))]
mod tests {
    use windows_sys::Win32::System::Console::GetConsoleWindow;

    const CHILD: &str = "HEX_BACKGROUND_PROCESS_CHILD";

    #[test]
    fn background_child_has_no_console_window() {
        if std::env::var_os(CHILD).is_some() {
            // SAFETY: GetConsoleWindow takes no arguments and only reads state.
            assert!(unsafe { GetConsoleWindow() }.is_null());
            return;
        }
        let output = super::command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "desktop::background_process::tests::background_child_has_no_console_window",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "the background child had a console window: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
