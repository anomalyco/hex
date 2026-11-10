//! Import a local audio file as a meeting (macOS; Labs › Import audio).
//!
//! The native bridge decodes the file to 16 kHz mono float WAV in bounded
//! blocks, then the meeting is written exactly as a recording's would be:
//! its audio is the system track, the microphone track is empty, and the
//! existing final pass transcribes, exports, and publishes it.

use std::ffi::{CStr, CString, c_char, c_int};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use color_eyre::eyre::{Result, WrapErr, eyre};

use super::{
    ActiveMeeting, FINAL_PENDING_FILE, IMPORT_PENDING_FILE, MeetingManifest, MeetingStatus,
    SAMPLE_RATE, TrackManifest, create_empty_track, finish::finish_transcription, now_ms, root,
    set_owner_only, sync_directory, sync_file, write_manifest,
};

/// The extensions the picker offers and the native decoder reads.
const EXTENSIONS: &[&str] = &["wav", "m4a", "mp3", "aiff", "aif", "caf"];

unsafe extern "C" {
    /// Converts `input` to a 16 kHz mono float WAV at `output`, answering 0
    /// on success and a message in `error` otherwise. `cancelled` is asked
    /// between decode blocks; a nonzero answer stops the conversion.
    fn hex_meeting_import_decode(
        input: *const c_char,
        output: *const c_char,
        cancelled: extern "C" fn() -> c_int,
        error: *mut *mut c_char,
    ) -> i32;
    fn hex_meeting_import_free_string(string: *mut c_char);
}

/// Imports `source` as a finished meeting. Runs on its own worker thread;
/// the folder lock is held until its transcript is published.
pub(crate) fn import(source: &Path) -> Result<MeetingManifest> {
    if !crate::labs::meeting_import() {
        return Err(eyre!("Labs › Import audio is off"));
    }
    if !is_supported(source) {
        return Err(eyre!(
            "{} is not a supported audio file; choose a WAV, M4A, MP3, AIFF, or CAF file",
            source.display()
        ));
    }
    let (_, selection) = crate::app_settings::transcription_selection();
    let model = crate::transcription::models::validate(&selection)?;
    if !crate::transcription::models::is_installed(model, &selection.language) {
        return Err(eyre!("{} is not installed", model.name));
    }

    let created_at_ms = now_ms();
    let id = format!("{created_at_ms}-{}", std::process::id());
    let directory = root()?.join(&id);

    // Every failure before final.pending exists leaves no entry to recover,
    // so the folder, its lock, the decode, and the manifest share one
    // cleanup scope.
    let prepared = (|| -> Result<(ActiveMeeting, MeetingManifest, u64)> {
        fs::create_dir_all(&directory).wrap_err("could not create meeting directory")?;
        set_owner_only(&directory, true)?;
        let active = ActiveMeeting::acquire(&directory)?;
        // The marker exists from before decode until final.pending is safely
        // on disk: a crash leaves a folder the launch sweep can identify.
        let marker = directory.join(IMPORT_PENDING_FILE);
        fs::write(&marker, b"")?;
        set_owner_only(&marker, false)?;
        sync_file(&marker)?;
        sync_directory(&directory)?;
        if crate::desktop::process_exit::quitting() {
            return Err(eyre!("Hex is quitting; the audio was not imported"));
        }
        let decoded = directory.join("system.importing.wav");
        decode(source, &decoded)?;
        set_owner_only(&decoded, false)?;
        sync_file(&decoded)?;
        create_empty_track(directory.join("microphone.wav"))?;
        sync_directory(&directory)?;

        // Everything fallible happens while the staging file still exists;
        // after the rename, the manifest write is the very next step.
        let samples = wav_samples(&decoded)?;
        let duration_ms = samples.saturating_mul(1_000) / u64::from(SAMPLE_RATE);
        let manifest = MeetingManifest {
            schema_version: 1,
            id: id.clone(),
            title: title_for(source),
            status: MeetingStatus::Transcribing,
            created_at_ms,
            ended_at_ms: Some(now_ms()),
            duration_ms: Some(duration_ms),
            system: TrackManifest {
                file: "system.wav".into(),
                sample_rate: Some(SAMPLE_RATE),
                first_pts_us: Some(0),
                last_pts_us: Some(duration_ms as i64 * 1_000),
                first_arrival_us: Some(0),
                samples,
                dropped_packets: 0,
                gaps: Vec::new(),
                warning: None,
            },
            microphone: TrackManifest {
                file: "microphone.wav".into(),
                sample_rate: Some(SAMPLE_RATE),
                ..TrackManifest::default()
            },
            live_transcription_error: None,
            error: None,
            app: None,
            export_path: None,
            export_error: None,
            echo_suppressed_utterances: 0,
            provisional_title: None,
            notes: None,
            notes_error: None,
            calendar: None,
            title_source: None,
            chapters: Vec::new(),
            chapters_refined: false,
            retention_applies: true,
            export_sha256: None,
            notes_export_path: None,
            notes_export_sha256: None,
            notes_export_removed: false,
            audio_removed_at_ms: None,
            imported: true,
        };
        let system_path = directory.join("system.wav");
        fs::rename(&decoded, &system_path).wrap_err("could not keep the imported audio")?;
        write_manifest(&directory, &manifest)?;
        fs::write(directory.join(FINAL_PENDING_FILE), b"")?;
        // final.pending is durable before the import marker disappears.
        sync_directory(&directory)?;
        fs::remove_file(&marker)?;
        sync_directory(&directory)?;
        Ok((active, manifest, duration_ms))
    })();
    let (_active, mut manifest, duration_ms) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = fs::remove_dir_all(&directory);
            return Err(error);
        }
    };
    tracing::info!(
        meeting = id,
        source = %source.display(),
        duration_ms,
        "importing an audio file as a meeting"
    );

    if crate::desktop::process_exit::quitting() {
        manifest.status = MeetingStatus::Interrupted;
        write_manifest(&directory, &manifest)?;
        tracing::info!(
            meeting = id,
            "Hex quit before an imported transcript ran; it finishes at the next launch"
        );
        return Ok(manifest);
    }
    finish_transcription(&directory, manifest, &selection)
}

/// Whether the picker could have offered this file.
fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// The file name without its extension, as the meeting's title.
fn title_for(source: &Path) -> String {
    source
        .file_stem()
        .map(|stem| stem.to_string_lossy().trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Imported audio".into())
}

/// The decoded WAV's frames, which its manifest records.
fn wav_samples(path: &Path) -> Result<u64> {
    let reader = hound::WavReader::open(path)
        .wrap_err_with(|| format!("could not read {}", path.display()))?;
    Ok(u64::from(reader.duration()))
}

fn decode(input: &Path, output: &Path) -> Result<()> {
    let input = CString::new(input.as_os_str().as_bytes())?;
    let output = CString::new(output.as_os_str().as_bytes())?;
    let mut error = ptr::null_mut();
    // SAFETY: both paths are NUL-terminated strings that outlive the call;
    // the bridge writes only the output file and, on failure, an owned
    // message into `error`. The cancellation callback reads Hex's quit flag
    // and captures nothing.
    let status = unsafe {
        hex_meeting_import_decode(input.as_ptr(), output.as_ptr(), hex_is_quitting, &mut error)
    };
    if status == 0 {
        return Ok(());
    }
    Err(take_error(error))
}

/// The bridge's cancellation check: Hex's own quit flag.
extern "C" fn hex_is_quitting() -> c_int {
    if crate::desktop::process_exit::quitting() {
        1
    } else {
        0
    }
}

fn take_error(error: *mut c_char) -> color_eyre::Report {
    if error.is_null() {
        return eyre!("Hex could not read this audio file");
    }
    // SAFETY: the bridge answers a NUL-terminated string it allocated.
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: the same pointer, freed once through the bridge's own
    // allocator pairing.
    unsafe { hex_meeting_import_free_string(error) };
    eyre!(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_listed_audio_files_are_imported() {
        for extension in EXTENSIONS {
            assert!(is_supported(Path::new(&format!("call.{extension}"))));
            assert!(is_supported(Path::new(&format!(
                "call.{}",
                extension.to_uppercase()
            ))));
        }
        assert!(!is_supported(Path::new("call.mp4")));
        assert!(!is_supported(Path::new("call")));
    }

    #[test]
    fn the_title_is_the_file_name_without_its_extension() {
        assert_eq!(title_for(Path::new("/tmp/Q4 sync.m4a")), "Q4 sync");
        assert_eq!(title_for(Path::new("/")), "Imported audio");
    }

    /// A 1 s stereo 44.1 kHz WAV with a different tone on each channel.
    #[cfg(target_os = "macos")]
    fn write_stereo_wav(path: &Path, bits: u16, format: hound::SampleFormat) {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 44_100,
                bits_per_sample: bits,
                sample_format: format,
            },
        )
        .unwrap();
        for frame in 0..44_100 {
            let time = frame as f32 / 44_100.0;
            // Silence on the left makes a lost downmix fail the test: the
            // right channel is the only tone.
            let left = 0.0_f32;
            let right = (time * 660.0 * std::f32::consts::TAU).sin() * 0.4;
            match format {
                hound::SampleFormat::Float => {
                    writer.write_sample(left).unwrap();
                    writer.write_sample(right).unwrap();
                }
                hound::SampleFormat::Int => {
                    writer
                        .write_sample((left * f32::from(i16::MAX)) as i16)
                        .unwrap();
                    writer
                        .write_sample((right * f32::from(i16::MAX)) as i16)
                        .unwrap();
                }
            }
        }
        writer.finalize().unwrap();
    }

    #[cfg(target_os = "macos")]
    fn test_directory(name: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hex-meeting-import-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[cfg(target_os = "macos")]
    fn assert_decoded(input: &Path, output: &Path) {
        decode(input, output).expect("decoding a stereo test WAV");
        let mut reader = hound::WavReader::open(output).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1, "mono output");
        assert_eq!(spec.sample_rate, 16_000, "16 kHz output");
        assert_eq!(spec.bits_per_sample, 32, "32-bit output");
        assert_eq!(
            spec.sample_format,
            hound::SampleFormat::Float,
            "float output"
        );
        let mut frames = 0_u64;
        let mut peak = 0.0_f32;
        for sample in reader.samples::<f32>() {
            let sample = sample.unwrap();
            peak = peak.max(sample.abs());
            frames += 1;
        }
        assert!(
            (15_800..=16_200).contains(&frames),
            "decoded {frames} frames from one second of audio"
        );
        assert!(peak > 0.01, "decoded audio is silent (peak {peak})");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn decodes_a_stereo_float_wav_to_mono_16_khz() {
        let directory = test_directory("float");
        let input = directory.join("input.wav");
        let output = directory.join("output.wav");
        write_stereo_wav(&input, 32, hound::SampleFormat::Float);
        assert_decoded(&input, &output);
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn decodes_a_stereo_16_bit_wav_to_mono_16_khz() {
        let directory = test_directory("int16");
        let input = directory.join("input.wav");
        let output = directory.join("output.wav");
        write_stereo_wav(&input, 16, hound::SampleFormat::Int);
        assert_decoded(&input, &output);
        fs::remove_dir_all(directory).unwrap();
    }
}
