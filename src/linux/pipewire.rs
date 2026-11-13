//! The one `pw-cat` flag whose meaning changed under Hex's two Linux audio
//! paths: `--raw`.
//!
//! Up to PipeWire 1.2, `pw-cat` (and its `pw-record` symlink) treats `-` as
//! raw pipe I/O and has no `--raw` option, so passing it is an unknown
//! option and the tool exits before recording or playing anything. From
//! 1.3.81 on, `-` goes through libsndfile as a WAV file and `--raw` brings
//! back the raw pipe. The flag is therefore passed only when the installed
//! `pw-cat`'s own help lists it.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// The arguments that make `executable` speak raw `f32` on `-`: `--raw`
/// where PipeWire knows it, nothing where `-` is already raw.
///
/// One answer per process: every caller runs the same `pw-cat` binary,
/// whichever name it was found under.
pub(crate) fn raw_args(executable: impl AsRef<Path>) -> &'static [&'static str] {
    static RAW: OnceLock<bool> = OnceLock::new();
    if *RAW.get_or_init(|| offers_raw(executable.as_ref())) {
        &["--raw"]
    } else {
        &[]
    }
}

/// Whether `pw-cat --help` lists `--raw`.
fn offers_raw(executable: &Path) -> bool {
    let help = Command::new(executable)
        // The usage text is translated, so ask for the untranslated option
        // names the probe looks for.
        .env("LC_ALL", "C")
        .arg("--help")
        .stdin(Stdio::null())
        .output();
    let Ok(help) = help else {
        // A `pw-cat` that cannot even be run keeps the flag: a newer
        // PipeWire is the common case, and its output needs the flag to stay
        // raw instead of becoming a WAV file.
        return true;
    };
    if !help.status.success() {
        // A help that failed says nothing trustworthy about the flags.
        return true;
    }
    let stdout = String::from_utf8_lossy(&help.stdout);
    let stderr = String::from_utf8_lossy(&help.stderr);
    lists_raw(&stdout) || lists_raw(&stderr)
}

/// Whether a usage text lists `--raw`, as PipeWire 1.3.81's does.
fn lists_raw(help: &str) -> bool {
    help.contains("--raw")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact `pw-cat --help` output of PipeWire 1.0.5's `show_usage`
    /// (`src/tools/pw-cat.c`) with its defaults filled in, from a build
    /// without `-Dpw-cat-ffmpeg` (Arch's and Ubuntu's), so no
    /// `-o, --encoded` line: `-` is raw pipe I/O and `--raw` does not exist.
    const USAGE_1_0_5: &str = r#"pw-cat [options] [<file>|-]
  -h, --help                            Show this help
      --version                         Show version
  -v, --verbose                         Enable verbose operations

  -R, --remote                          Remote daemon name
      --media-type                      Set media type (default Audio)
      --media-category                  Set media category (default Playback)
      --media-role                      Set media role (default Music)
      --target                          Set node target serial or name (default auto)
                                          0 means don't link
      --latency                         Set node latency (default 100ms)
                                          Xunit (unit = s, ms, us, ns)
                                          or direct samples (256)
                                          the rate is the one of the source file
  -P  --properties                      Set node properties

      --rate                            Sample rate (req. for rec) (default 48000)
      --channels                        Number of channels (req. for rec) (default 2)
      --channel-map                     Channel map
                                            one of: "stereo", "surround-51",... or
                                            comma separated list of channel names: eg. "FL,FR"
      --format                          Sample format (ulaw|alaw|u8|s8|s16|s32|f32|f64) (req. for rec) (default s16)
      --volume                          Stream volume 0-1.0 (default 1.000)
  -q  --quality                         Resampler quality (0 - 15) (default 4)

  -p, --playback                        Playback mode
  -r, --record                          Recording mode
  -m, --midi                            Midi mode
  -d, --dsd                             DSD mode

"#;

    /// The same from PipeWire 1.4.0, whose `show_usage` adds `--raw`.
    const USAGE_1_4_0: &str = r#"pw-cat [options] [<file>|-]
  -h, --help                            Show this help
      --version                         Show version
  -v, --verbose                         Enable verbose operations

  -R, --remote                          Remote daemon name
      --media-type                      Set media type (default Audio)
      --media-category                  Set media category (default Playback)
      --media-role                      Set media role (default Music)
      --target                          Set node target serial or name (default auto)
                                          0 means don't link
      --latency                         Set node latency (default 100ms)
                                          Xunit (unit = s, ms, us, ns)
                                          or direct samples (256)
                                          the rate is the one of the source file
  -P  --properties                      Set node properties

      --rate                            Sample rate (req. for rec) (default 48000)
      --channels                        Number of channels (req. for rec) (default 2)
      --channel-map                     Channel map
                                            one of: "stereo", "surround-51",... or
                                            comma separated list of channel names: eg. "FL,FR"
      --format                          Sample format (ulaw|alaw|u8|s8|s16|s32|f32|f64) (req. for rec) (default s16)
      --volume                          Stream volume 0-1.0 (default 1.000)
  -q  --quality                         Resampler quality (0 - 15) (default 4)
  -a, --raw                             RAW mode

  -p, --playback                        Playback mode
  -r, --record                          Recording mode
  -m, --midi                            Midi mode
  -d, --dsd                             DSD mode

"#;

    #[test]
    fn raw_is_passed_only_to_pipewire_that_lists_it() {
        assert!(!lists_raw(USAGE_1_0_5));
        assert!(lists_raw(USAGE_1_4_0));
    }
}
