//! The x86_64 CPU floor. Linux and Windows x86_64 releases build ggml for
//! x86-64-v3 (AVX2, BMI2, FMA, F16C; `.github/release/check-x86-baseline.sh`),
//! so on an older processor the first transcription would die with an
//! illegal instruction (issue #592). `main` calls [`require`] first, before
//! anything that could reach ggml; Hex's own Rust is compiled for the
//! baseline x86-64, so this check itself runs anywhere.
//!
//! An opt-in source build can target Ivy Bridge (2012: SSE4.2, AVX, F16C)
//! instead of x86-64-v3, with `HEX_X86_BASELINE=ivybridge` and the matching
//! `TRANSCRIBE_CMAKE_ARGS` (`build.rs` refuses either without the other;
//! docs/linux.md and docs/windows.md). Such a build is not an official
//! release, is slower on newer CPUs, and never self-updates.
//!
//! `--version` and `--help` stay available on any CPU: installers and the
//! updater run `hex --version` to check what they install.

/// One build's CPU floor: the ggml features it requires and the message a
/// processor below it shows.
struct Floor {
    features: &'static [&'static str],
    message: &'static str,
}

/// The official Linux and Windows x64 release floor, x86-64-v3.
const V3: Floor = Floor {
    features: &["avx2", "bmi2", "fma", "f16c"],
    message: "Hex needs a 2013 or newer processor (Intel Haswell / AMD Excavator or later) with AVX2. This PC's processor isn't supported.",
};

/// `HEX_X86_BASELINE=ivybridge`: Ivy Bridge (2012) and newer, with AVX.
const IVYBRIDGE: Floor = Floor {
    features: &["sse4.2", "avx", "f16c"],
    message: "Hex needs a 2012 or newer processor (Intel Ivy Bridge / AMD Bulldozer or later) with AVX. This PC's processor isn't supported.",
};

/// The floor this build detects, both lists stay visible to the test below.
const fn floor() -> &'static Floor {
    if cfg!(hex_x86_ivybridge) {
        &IVYBRIDGE
    } else {
        &V3
    }
}

pub const MESSAGE: &str = floor().message;

/// Whether this processor has every feature ggml is built with.
pub fn supported() -> bool {
    floor().features.iter().all(|feature| detected(feature))
}

fn detected(feature: &str) -> bool {
    match feature {
        "sse4.2" => std::arch::is_x86_feature_detected!("sse4.2"),
        "avx" => std::arch::is_x86_feature_detected!("avx"),
        "avx2" => std::arch::is_x86_feature_detected!("avx2"),
        "bmi2" => std::arch::is_x86_feature_detected!("bmi2"),
        "fma" => std::arch::is_x86_feature_detected!("fma"),
        "f16c" => std::arch::is_x86_feature_detected!("f16c"),
        feature => unreachable!("unknown CPU floor feature `{feature}`"),
    }
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
    // `hex service` never opens windows; its stderr reaches the journal.
    if arguments
        .get(1)
        .is_none_or(|argument| argument != "service")
    {
        show();
    }
    std::process::exit(1);
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
/// dialog when one of these tools is installed and a display is up. A
/// terminal already shows stderr, and a unit under a service manager
/// (hex.service sets NOTIFY_SOCKET) must not block on a dialog that its
/// restarts would repeat.
#[cfg(target_os = "linux")]
fn show() {
    use std::io::IsTerminal;
    use std::process::{Command, Stdio};
    if std::io::stderr().is_terminal()
        || std::env::var_os("NOTIFY_SOCKET").is_some()
        || (std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none())
    {
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
    fn the_release_floor_is_x86_64_v3_and_the_switch_drops_to_ivy_bridge() {
        assert_eq!(V3.features, ["avx2", "bmi2", "fma", "f16c"].as_slice());
        assert_eq!(IVYBRIDGE.features, ["sse4.2", "avx", "f16c"].as_slice());
        assert!(V3.message.contains("AVX2"));
        assert!(IVYBRIDGE.message.contains("with AVX."));
        assert_ne!(V3.message, IVYBRIDGE.message);
        #[cfg(hex_x86_ivybridge)]
        assert_eq!(floor().features, IVYBRIDGE.features);
        #[cfg(not(hex_x86_ivybridge))]
        assert_eq!(floor().features, V3.features);
    }

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
