//! The x86_64 CPU floor. Linux and Windows x86_64 releases build ggml for
//! x86-64-v3 (AVX2, BMI2, FMA, F16C; `.github/release/check-x86-baseline.sh`),
//! so on an older processor the first transcription would die with an
//! illegal instruction (issue #592). `main` calls [`require`] first, before
//! anything that could reach ggml; Hex's own Rust is compiled for the
//! baseline x86-64, so this check itself runs anywhere.
//!
//! `--version` and `--help` stay available on any CPU: installers and the
//! updater run `hex --version` to check what they install.

pub const MESSAGE: &str = "Hex needs a 2013 or newer processor (Intel Haswell / AMD Excavator or later) with AVX2. This PC's processor isn't supported.";

/// Whether this processor has every x86-64-v3 feature ggml is built with.
pub fn supported() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
        && std::arch::is_x86_feature_detected!("bmi2")
        && std::arch::is_x86_feature_detected!("fma")
        && std::arch::is_x86_feature_detected!("f16c")
}

fn exempt(arguments: &[std::ffi::OsString]) -> bool {
    matches!(
        arguments.get(1).and_then(|argument| argument.to_str()),
        Some("--version" | "-V" | "--help" | "-h" | "help")
    ) && arguments.len() == 2
}

/// Exits with a clear message on a processor below the floor.
pub fn require() {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    if supported() || exempt(&arguments) {
        return;
    }
    eprintln!("{MESSAGE}");
    // Under a service manager (hex.service: Restart=on-failure), a failure
    // exit would restart Hex and repeat the message forever; the journal
    // keeps the message, and a clean exit ends it.
    let supervised =
        std::env::var_os("INVOCATION_ID").is_some() || std::env::var_os("NOTIFY_SOCKET").is_some();
    if !supervised && !interactive(&arguments) {
        show();
    }
    std::process::exit(if supervised { 0 } else { 1 });
}

/// A terminal already shows stderr, and `hex service` never opens windows.
fn interactive(arguments: &[std::ffi::OsString]) -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
        || arguments
            .get(1)
            .is_some_and(|argument| argument == "service")
}

#[cfg(target_os = "windows")]
fn show() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (text, title) = (wide(MESSAGE), wide("Hex"));
    // Both strings are NUL-terminated UTF-16 that outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

/// For Hex opened from the launcher, where nobody sees stderr: a desktop
/// dialog when one of these tools is installed and a display is up.
#[cfg(target_os = "linux")]
fn show() {
    use std::process::{Command, Stdio};
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    let attempts: [(&str, &[&str]); 3] = [
        ("zenity", &["--error", "--title=Hex", "--text", MESSAGE]),
        ("kdialog", &["--title", "Hex", "--error", MESSAGE]),
        ("notify-send", &["--urgency=critical", "Hex", MESSAGE]),
    ];
    for (program, arguments) in attempts {
        let shown = Command::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if shown {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_help_run_on_any_processor() {
        let args = |list: &[&str]| {
            list.iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        };
        assert!(exempt(&args(&["hex", "--version"])));
        assert!(exempt(&args(&["hex", "--help"])));
        assert!(!exempt(&args(&["hex"])));
        assert!(!exempt(&args(&["hex", "--version", "app"])));
        assert!(!exempt(&args(&["hex", "transcribe", "a.wav"])));
    }
}
