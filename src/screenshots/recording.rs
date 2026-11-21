//! A Screenshots recording once written: its movie is saved to the
//! screenshots folder as "Hex Recording … .mp4", as a shot's PNG is, and
//! its card shows in the corner from its first frame, pointing at that
//! file. The recording itself is an Annotate capture
//! (`CaptureCommand::Clip`) that skips the clipboard and the router; its
//! bundle keeps Annotate's own copy, which Annotate's retention may trash.

use std::path::{Path, PathBuf};

use crate::history::captures::{CaptureKind, CaptureWord};
use crate::rune_ui::screenshot_thumbnail::editor::clip;
use crate::rune_ui::screenshot_thumbnail::{Recording, Taken};

/// The capture host's hook for each written recording, on its finishing
/// thread: the movie is saved and the card read on a thread of its own.
pub(super) fn written(bundle: PathBuf) {
    let spawned = std::thread::Builder::new()
        .name("hex-recording-card".into())
        .spawn(move || {
            if let Err(error) = show(&bundle) {
                tracing::warn!(%error, bundle = %bundle.display(), "could not show the recording");
                crate::feedback::play(crate::feedback::Tone::Error);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not show the recording");
    }
}

fn show(bundle: &Path) -> anyhow::Result<()> {
    use scrawl_capture::helper::{self, FrameTimes};
    let (recorded, words) = clip::recorded(bundle)?;
    let folder = crate::app_settings::screenshot_settings().folder();
    let sound = sound(bundle);
    // A failed save still shows the card, on Annotate's copy.
    let movie = match save(&recorded, &sound, &folder) {
        Ok(movie) => {
            tracing::info!(path = %movie.display(), "recording saved");
            super::clear_save_error();
            movie
        }
        Err(error) => {
            tracing::warn!(%error, folder = %folder.display(), "could not save the recording");
            super::record_save_error(&folder, &error);
            crate::feedback::play(crate::feedback::Tone::Error);
            recorded
        }
    };
    let probe = helper::probe(&movie)?;
    let duration_ms = probe.duration_ms;
    let frame = helper::frames(&movie, FrameTimes::At(&[0]), super::SHARP_PX)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("the recording has no frames"))?;
    let sharp = super::sharp(frame.width, frame.height, &frame.rgba);
    let (width, height, small) =
        super::shrink(frame.width, frame.height, &frame.rgba, super::THUMB_PX);
    let timed = words
        .iter()
        .map(|word| CaptureWord {
            t0: word.t0,
            t1: word.t1,
            text: word.text.clone(),
        })
        .collect();
    super::remember(
        super::Remembered {
            kind: CaptureKind::Recording,
            file: &movie,
            size: [probe.width, probe.height],
            duration_ms,
            timed,
        },
        (width, height, &small),
    );
    if let Some(overlays) = super::OVERLAYS.get() {
        let (screen, capture) = recorded_area(bundle);
        overlays.thumbnail.show_recording(
            Taken {
                path: movie,
                width,
                height,
                rgba: small,
                sharp,
                screen,
                capture,
            },
            Recording { duration_ms, words },
        );
    }
    Ok(())
}

/// The display a recording was made on, in points, and the area it
/// recorded there, from its bundle's manifest: where its card flies in
/// from. The display under the pointer, and no area, when the manifest
/// doesn't say.
fn recorded_area(bundle: &Path) -> ((f32, f32), Option<[f32; 4]>) {
    use scrawl_capture::screens;
    let frame = capture_core::Manifest::read(bundle)
        .ok()
        .and_then(|manifest| {
            manifest
                .sources
                .iter()
                .find(|source| source.error.is_none())
                .map(|source| source.frame)
        });
    let displays = screens::displays();
    let placed = frame.and_then(|frame| {
        let centre = [frame.x + frame.width * 0.5, frame.y + frame.height * 0.5];
        let display = &displays[screens::index_at(&displays, centre)?];
        let [x, y] = screens::to_local(display.frame, [frame.x, frame.y]);
        let screen = (display.frame[2] as f32, display.frame[3] as f32);
        Some((
            screen,
            [x as f32, y as f32, frame.width as f32, frame.height as f32],
        ))
    });
    match placed {
        Some((screen, area)) => (screen, Some(area)),
        None => (super::screen_under_pointer(), None),
    }
}

/// An audio file the bundle kept beside the movie, and where its time zero
/// sits after the movie's start (before it when negative).
#[derive(Debug, PartialEq)]
struct Sound {
    file: PathBuf,
    offset_ms: i64,
}

/// The sound a recording kept: the microphone's `audio.wav` (sample 0 is
/// the capture's start, `videoOffsetMs` before the movie's first frame)
/// unless it is all silence, as with no microphone, and the computer's
/// audio, placed by the times `capture.json` gives it and the movie.
fn sound(bundle: &Path) -> Vec<Sound> {
    let read = |name: &str| -> Option<serde_json::Value> {
        serde_json::from_slice(&std::fs::read(bundle.join(name)).ok()?).ok()
    };
    let mut sound = Vec::new();
    if let Some(scrawl) = read("scrawl.json")
        && let Some(name) = scrawl["audio"].as_str()
        && let file = bundle.join(name)
        && heard(&file)
    {
        let offset = scrawl["videoOffsetMs"].as_i64().unwrap_or(0);
        sound.push(Sound {
            file,
            offset_ms: -offset,
        });
    }
    if let Some(manifest) = read("capture.json")
        && let Some(video) = manifest["sources"][0]["start_ns"].as_i64()
        && let Some(files) = manifest["files"].as_array()
    {
        for entry in files {
            if entry["kind"]["type"] == "system_audio"
                && let (Some(name), Some(start)) =
                    (entry["name"].as_str(), entry["start_ns"].as_i64())
            {
                sound.push(Sound {
                    file: bundle.join(name),
                    offset_ms: (start - video) / 1_000_000,
                });
            }
        }
    }
    sound
}

/// Whether a 16-bit WAV holds anything but silence: a capture with no
/// microphone, or one that delivered nothing, is all zeros.
fn heard(wav: &Path) -> bool {
    hound::WavReader::open(wav).is_ok_and(|mut reader| {
        reader
            .samples::<i16>()
            .any(|sample| sample.is_ok_and(|sample| sample.unsigned_abs() > 1))
    })
}

/// `movie` saved as a fresh "Hex Recording … .mp4" in `folder`, making the
/// folder if it's missing, with `sound` mixed in as one AAC track. Without
/// sound it's a copy, which on one volume is a clone: instant, and taking no
/// space until either changes. A mix that fails saves the silent copy.
fn save(movie: &Path, sound: &[Sound], folder: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let path = super::save::recording_path(folder);
    if !sound.is_empty() {
        let audio: Vec<_> = sound
            .iter()
            .map(|sound| scrawl_capture::helper::MuxAudio {
                file: &sound.file,
                offset_ms: sound.offset_ms,
            })
            .collect();
        match scrawl_capture::helper::mux(movie, &audio, &path) {
            Ok(()) => return Ok(path),
            Err(error) => {
                tracing::warn!(%error, "could not mix the recording's sound in; saving it silent");
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    std::fs::copy(movie, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_saves_beside_the_shots_and_never_over_one() {
        let folder =
            std::env::temp_dir().join(format!("hex-recording-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let source = std::env::temp_dir().join(format!("hex-recording-{}.mp4", std::process::id()));
        std::fs::write(&source, b"movie").unwrap();
        let first = save(&source, &[], &folder).unwrap();
        let second = save(&source, &[], &folder).unwrap();
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("Hex Recording ") && name.ends_with(".mp4"),
            "{name}"
        );
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&second).unwrap(), b"movie");
        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::remove_file(&source).unwrap();
    }

    fn wav(path: &Path, samples: &[i16]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn a_recording_keeps_the_microphone_it_heard_and_the_computer_audio_on_its_clock() {
        let bundle =
            std::env::temp_dir().join(format!("hex-recording-sound-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bundle);
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(
            bundle.join("scrawl.json"),
            r#"{"video":"recording.mp4","videoOffsetMs":142,"audio":"audio.wav"}"#,
        )
        .unwrap();
        std::fs::write(
            bundle.join("capture.json"),
            r#"{"sources":[{"start_ns":5000000000}],"files":[
                {"name":"recording.mp4","kind":{"type":"video","source":0},"start_ns":5000000000},
                {"name":"system.m4a","kind":{"type":"system_audio"},"start_ns":4980000000}]}"#,
        )
        .unwrap();
        // No microphone: the timeline padded it with silence.
        wav(&bundle.join("audio.wav"), &[0; 1600]);
        let computer = Sound {
            file: bundle.join("system.m4a"),
            offset_ms: -20,
        };
        assert_eq!(sound(&bundle), [computer]);
        wav(&bundle.join("audio.wav"), &[0, 40, -40, 0]);
        let heard = sound(&bundle);
        assert_eq!(
            heard[0],
            Sound {
                file: bundle.join("audio.wav"),
                offset_ms: -142,
            }
        );
        assert_eq!(heard.len(), 2);
        std::fs::remove_dir_all(&bundle).unwrap();
    }
}
