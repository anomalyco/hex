//! A Screenshots recording once written: its movie is saved to the
//! screenshots folder as "Hex Recording … .mp4", as a shot's PNG is, and
//! its card shows in the corner from its first frame, pointing at that
//! file. The recording itself is an Annotate capture
//! (`CaptureCommand::Clip`) that skips the clipboard and the router; its
//! bundle keeps Annotate's own copy, which Annotate's retention may trash.

use std::path::{Path, PathBuf};

use crate::history::captures::{CaptureKind, CaptureWord};
use crate::rune_ui::screenshot_thumbnail::Recording;
use crate::rune_ui::screenshot_thumbnail::editor::clip;

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
    // A failed save still shows the card, on Annotate's copy.
    let movie = match save(&recorded, &folder) {
        Ok(movie) => {
            tracing::info!(path = %movie.display(), "recording saved");
            movie
        }
        Err(error) => {
            tracing::warn!(%error, folder = %folder.display(), "could not save the recording");
            crate::feedback::play(crate::feedback::Tone::Error);
            recorded
        }
    };
    let probe = helper::probe(&movie)?;
    let duration_ms = probe.duration_ms;
    let frame = helper::frames(&movie, FrameTimes::At(&[0]), super::THUMB_PX)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("the recording has no frames"))?;
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
        (frame.width, frame.height, &frame.rgba),
    );
    if let Some(overlays) = super::OVERLAYS.get() {
        let screen = super::screen_under_pointer();
        overlays.thumbnail.show_recording(
            movie,
            Recording { duration_ms, words },
            frame.width,
            frame.height,
            frame.rgba,
            screen,
        );
    }
    Ok(())
}

/// `movie` copied to a fresh "Hex Recording … .mp4" in `folder`, making the
/// folder if it's missing. On one volume the copy is a clone, so it's
/// instant and takes no space until either changes.
fn save(movie: &Path, folder: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let path = super::save::recording_path(folder);
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
        let first = save(&source, &folder).unwrap();
        let second = save(&source, &folder).unwrap();
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
}
