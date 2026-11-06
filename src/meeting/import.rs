//! Import a local audio file as a meeting (macOS; Labs › Import audio).
//!
//! The native bridge decodes the file to 16 kHz mono float WAV in bounded
//! blocks, then the meeting is written exactly as a recording's would be:
//! its audio is the system track, the microphone track is empty, and the
//! existing final pass transcribes, exports, and publishes it.

use std::ffi::{CStr, CString, c_char};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use color_eyre::eyre::{Result, WrapErr, eyre};

use super::{
    ActiveMeeting, FINAL_PENDING_FILE, MeetingManifest, MeetingStatus, SAMPLE_RATE, TrackManifest,
    create_empty_track, finish_transcription, now_ms, root, set_owner_only, sync_directory,
    sync_file, write_manifest,
};

/// The extensions the picker offers and the native decoder reads.
const EXTENSIONS: &[&str] = &["wav", "m4a", "mp3", "aiff", "aif", "caf"];

unsafe extern "C" {
    /// Converts `input` to a 16 kHz mono float WAV at `output`, answering 0
    /// on success and a message in `error` otherwise.
    fn hex_meeting_import_decode(
        input: *const c_char,
        output: *const c_char,
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
    fs::create_dir_all(&directory).wrap_err("could not create meeting directory")?;
    set_owner_only(&directory, true)?;
    let _active = ActiveMeeting::acquire(&directory)?;

    // A quit before the manifest exists leaves nothing to recover; take the
    // folder with it.
    if crate::desktop::process_exit::quitting() {
        let _ = fs::remove_dir_all(&directory);
        return Err(eyre!("Hex is quitting; the audio was not imported"));
    }
    let decoded = directory.join("system.wav.importing");
    if let Err(error) = decode(source, &decoded) {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }
    let system_path = directory.join("system.wav");
    let prepared = (|| -> Result<()> {
        fs::rename(&decoded, &system_path).wrap_err("could not keep the imported audio")?;
        set_owner_only(&system_path, false)?;
        create_empty_track(directory.join("microphone.wav"))?;
        sync_file(&system_path)?;
        sync_directory(&directory)?;
        Ok(())
    })();
    if let Err(error) = prepared {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }

    let samples = wav_samples(&system_path)?;
    let duration_ms = samples.saturating_mul(1_000) / u64::from(SAMPLE_RATE);
    let mut manifest = MeetingManifest {
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
        imported: true,
    };
    write_manifest(&directory, &manifest)?;
    fs::write(directory.join(FINAL_PENDING_FILE), b"")?;
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
    // message into `error`.
    let status = unsafe { hex_meeting_import_decode(input.as_ptr(), output.as_ptr(), &mut error) };
    if status == 0 {
        return Ok(());
    }
    Err(take_error(error))
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
}
