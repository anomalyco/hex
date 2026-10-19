//! Opt-in Scrawl (`⌥⇧S`) screen + voice capture coordinator for macOS.
//!
//! Reuses Hex's authoritative `DictationAudio` microphone stream (via `set_scrawl_sink`, never
//! opening a second `cpal` stream) and Hex's warm `DictationWorker` transcription engine (via
//! `HexWordTranscriber`), feeds captured screen frames and interactions from
//! `scrawl-capture-macos` through `scrawl-core`'s composable stages (`plan_keyframes` →
//! `solve_crop` → `render_frames` → `assemble_bundle`), writes the `.scrawl` bundle under
//! `~/Library/Application Support/voice-control/scrawl/`, and copies the summary + artifact paths
//! to the clipboard.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use color_eyre::eyre::{Result, eyre};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;
use scrawl_capture_macos::{MacosInputSource, ScreenCaptureSession};
use scrawl_core::{
    InputSource, Interactions, OutputOptions, Recording, ScrawlBundle, TargetInfo, TranscriptData,
    assemble_bundle, make_bundle_id, now_epoch_ms, plan_keyframes, render_frames, solve_crop,
};

use crate::dictation::resample_for_parakeet;
use crate::dictation_audio::DictationAudio;
use crate::feedback::{self, Tone};
use crate::parakeet::{DictationWorker, HexWordTranscriber};

const TRANSCRIPTION_SAMPLE_RATE: u32 = 16_000;

pub struct ScrawlController {
    active: Option<ActiveScrawlSession>,
}

struct ActiveScrawlSession {
    id: String,
    created_at_ms: u64,
    bundle_dir: PathBuf,
    target: TargetInfo,
    auto_crop: bool,
    screen: ScreenCaptureSession,
    input: MacosInputSource,
    audio_worker: Option<JoinHandle<(Vec<f32>, bool)>>,
}

impl Default for ScrawlController {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrawlController {
    pub fn new() -> Self {
        Self { active: None }
    }

    /// Toggles Scrawl capture on `⌥⇧S`. Escape does not cancel an active Scrawl recording in M1.
    pub fn toggle(
        &mut self,
        dictation_audio: &DictationAudio,
        worker: &DictationWorker,
    ) -> Result<()> {
        if let Some(session) = self.active.take() {
            return self.finish_session(session, dictation_audio, worker);
        }
        if !crate::app_settings::scrawl_enabled() {
            return Ok(());
        }
        if !scrawl_capture_macos::screen_capture_granted() {
            feedback::play(Tone::Error);
            tracing::warn!(
                "Scrawl capture refused: Screen & System Audio Recording permission is not granted"
            );
            return Ok(());
        }
        self.start_session(dictation_audio)
    }

    fn start_session(&mut self, dictation_audio: &DictationAudio) -> Result<()> {
        scrawl_capture_macos::record_external_frontmost_app();
        let id = make_bundle_id();
        let created_at_ms = now_epoch_ms();
        let root = crate::app_paths::scrawl_dir()?;
        let bundle_dir = root.join(format!("{id}.scrawl"));
        fs::create_dir_all(&bundle_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&root, fs::Permissions::from_mode(0o700));
            let _ = fs::set_permissions(&bundle_dir, fs::Permissions::from_mode(0o700));
        }

        let target = scrawl_capture_macos::discover_target(None);
        let auto_crop = crate::app_settings::scrawl_auto_crop();
        let video_path = bundle_dir.join("recording.mp4");
        let excluded_window_ids = Arc::new(Mutex::new(Vec::new()));
        let screen =
            ScreenCaptureSession::start(target.clone(), Some(video_path), excluded_window_ids);
        let input = MacosInputSource::start(target.clone(), Some(screen.urgent_trigger()));

        let (audio_tx, audio_rx) = mpsc::sync_channel::<(Vec<f32>, u32)>(256);
        dictation_audio.set_scrawl_sink(Some(audio_tx))?;
        let wav_path = bundle_dir.join("audio.wav");
        let audio_worker = thread::Builder::new()
            .name("hex-scrawl-audio".into())
            .spawn(move || collect_scrawl_audio(audio_rx, &wav_path))
            .ok();

        feedback::play(Tone::DictationStart);
        tracing::info!(
            bundle_id = %id,
            display_id = target.display_id,
            app = ?target.app,
            "Scrawl recording started"
        );
        self.active = Some(ActiveScrawlSession {
            id,
            created_at_ms,
            bundle_dir,
            target,
            auto_crop,
            screen,
            input,
            audio_worker,
        });
        Ok(())
    }

    fn finish_session(
        &mut self,
        mut session: ActiveScrawlSession,
        dictation_audio: &DictationAudio,
        worker: &DictationWorker,
    ) -> Result<()> {
        let _ = dictation_audio.set_scrawl_sink(None);
        feedback::play(Tone::DictationStop);
        let word_transcriber = worker.word_transcriber();
        let audio_worker = session.audio_worker.take();

        thread::Builder::new()
            .name("hex-scrawl-finalize".into())
            .spawn(move || {
                let screen = session.screen.stop();
                let batch = session.input.finish();
                let (samples_16khz, audio_written) = audio_worker
                    .and_then(|handle| handle.join().ok())
                    .unwrap_or_default();
                if let Err(error) = finalize_scrawl_bundle(
                    session.id,
                    session.created_at_ms,
                    session.bundle_dir,
                    session.target,
                    session.auto_crop,
                    screen,
                    Interactions::from_batch(batch),
                    samples_16khz,
                    audio_written,
                    word_transcriber,
                ) {
                    feedback::play(Tone::Error);
                    tracing::error!(%error, "Scrawl bundle finalization failed");
                }
            })?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn finalize_scrawl_bundle(
    id: String,
    created_at_ms: u64,
    bundle_dir: PathBuf,
    target: TargetInfo,
    auto_crop: bool,
    screen: scrawl_capture_macos::ScreenRecording,
    interactions: Interactions,
    samples_16khz: Vec<f32>,
    audio_written: bool,
    word_transcriber: HexWordTranscriber,
) -> Result<()> {
    let transcript = match word_transcriber.transcribe_samples_16khz(samples_16khz) {
        Ok(transcript) => transcript,
        Err(error) => {
            tracing::warn!(%error, "Scrawl word-timed transcription failed; continuing with empty transcript");
            TranscriptData {
                source: "hex".into(),
                ..Default::default()
            }
        }
    };

    let video = screen
        .video
        .as_ref()
        .and_then(|path| path.file_name()?.to_str().map(str::to_owned));
    let recording = Recording {
        target,
        created_at_ms: Some(created_at_ms),
        duration_ms: screen.duration_ms,
        frames: screen.frames,
        video,
        video_offset_ms: screen.video_offset_ms,
        audio: audio_written.then(|| "audio.wav".into()),
        source: Some("hex".into()),
        ..Default::default()
    };
    let mut options = OutputOptions::for_target(&recording.target);
    if !auto_crop {
        options.auto_crop = None;
    }

    // Run `scrawl-core`'s composable stages:
    let plan = plan_keyframes(
        &recording,
        &interactions,
        &transcript,
        options.full_frames.as_deref(),
    );
    let cropped = solve_crop(plan, &recording, &interactions, options.auto_crop.as_ref());
    let rendered = render_frames(recording.duration_ms, cropped);
    let built = assemble_bundle(
        id,
        recording,
        interactions,
        transcript,
        options.note,
        rendered,
    );
    let (dir, bundle, summary) = built
        .write_to(&bundle_dir)
        .map_err(|error| eyre!("{error}"))?;

    let clipboard_text = format_clipboard_summary(&summary, &dir, &bundle);
    copy_to_clipboard(&clipboard_text)?;
    tracing::info!(
        bundle_dir = %dir.display(),
        keyframes = bundle.keyframes.len(),
        words = bundle.transcript.words.len(),
        "Scrawl bundle written and copied to clipboard"
    );
    Ok(())
}

/// Formats the clipboard payload with the Markdown summary, contact-sheet path, and keyframe paths.
pub fn format_clipboard_summary(summary: &str, bundle_dir: &Path, bundle: &ScrawlBundle) -> String {
    let mut out = summary.trim_end().to_string();
    let contact_sheet = bundle_dir.join(&bundle.contact_sheet);
    let contact_line = format!("Contact sheet: {}", contact_sheet.display());
    if !out.contains(&contact_line) {
        out.push_str("\n\n");
        out.push_str(&contact_line);
    }
    for kf in &bundle.keyframes {
        let frame_path = bundle_dir.join(&kf.file);
        let display = frame_path.display().to_string();
        if !out.contains(&display) {
            out.push_str(&format!("\nFrame {}: {display}", kf.index));
        }
    }
    out.push('\n');
    out
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    let ns_string = NSString::from_str(text);
    let wrote = unsafe { pasteboard.setString_forType(&ns_string, NSPasteboardTypeString) };
    if !wrote {
        return Err(eyre!("failed to copy Scrawl summary to clipboard"));
    }
    Ok(())
}

fn collect_scrawl_audio(rx: Receiver<(Vec<f32>, u32)>, wav_path: &Path) -> (Vec<f32>, bool) {
    let mut raw_samples: Vec<f32> = Vec::new();
    let mut source_rate = TRANSCRIPTION_SAMPLE_RATE;
    while let Ok((chunk, rate)) = rx.recv() {
        if rate > 0 {
            source_rate = rate;
        }
        raw_samples.extend_from_slice(&chunk);
    }
    let samples_16khz = if raw_samples.is_empty() {
        Vec::new()
    } else {
        resample_for_parakeet(&raw_samples, source_rate)
    };
    let written = !samples_16khz.is_empty() && write_wav_16khz(wav_path, &samples_16khz).is_ok();
    (samples_16khz, written)
}

pub fn write_wav_16khz(path: &Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TRANSCRIPTION_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in samples {
        let clamped = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        writer.write_sample(clamped)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
pub fn read_wav_16khz(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let mono: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            let samples: Vec<f32> = reader
                .samples::<f32>()
                .collect::<std::result::Result<_, _>>()?;
            samples
                .chunks(channels)
                .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
                .collect()
        }
        hound::SampleFormat::Int => {
            let scale = 1.0 / f32::from(i16::MAX);
            let samples: Vec<i16> = reader
                .samples::<i16>()
                .collect::<std::result::Result<_, _>>()?;
            samples
                .chunks(channels)
                .map(|frame| {
                    frame.iter().map(|&s| f32::from(s) * scale).sum::<f32>() / channels as f32
                })
                .collect()
        }
    };
    Ok(resample_for_parakeet(&mono, spec.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_summary_includes_transcript_contact_sheet_and_keyframe_paths() {
        let root = std::env::temp_dir().join(make_bundle_id());
        let (dir, bundle, summary) =
            scrawl_core::generate_demo_bundle(Some(root.join("demo.scrawl"))).unwrap();
        let payload = format_clipboard_summary(&summary, &dir, &bundle);
        assert!(payload.contains("When I click Used 12 tools"));
        assert!(payload.contains(&dir.display().to_string()));
        assert!(payload.contains(&dir.join("contact-sheet.png").display().to_string()));
        assert!(payload.contains(&dir.join("frames/frame-01.png").display().to_string()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wav_roundtrip_preserves_16khz_mono_samples() {
        let path = std::env::temp_dir().join(format!("{}.wav", make_bundle_id()));
        let input = vec![0.0_f32, 0.25, -0.25, 0.5, -0.5];
        write_wav_16khz(&path, &input).unwrap();
        let output = read_wav_16khz(&path).unwrap();
        assert_eq!(output.len(), input.len());
        for (a, b) in input.iter().zip(&output) {
            assert!((a - b).abs() < 0.001, "{a} vs {b}");
        }
        let _ = fs::remove_file(path);
    }
}
