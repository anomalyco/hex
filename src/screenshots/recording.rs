//! A Screenshots recording once written: its card shows in the corner, from
//! its first frame, as a shot's does. The recording itself is an Annotate
//! capture (`CaptureCommand::Clip`) that skips the clipboard and the router.

use std::path::{Path, PathBuf};

use crate::rune_ui::screenshot_thumbnail::Recording;

/// The capture host's hook for each written recording, on its finishing
/// thread: the card is read on a thread of its own.
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
    let (_, loaded) = scrawl_capture::bundle::load_bundle_in(None, bundle)?;
    let movie = bundle.join(
        loaded
            .video
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("the recording has no movie"))?,
    );
    let duration_ms = helper::probe(&movie)?.duration_ms;
    let frame = helper::frames(&movie, FrameTimes::At(&[0]), super::THUMB_PX)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("the recording has no frames"))?;
    if let Some(overlays) = super::OVERLAYS.get() {
        let screen = super::screen_under_pointer();
        overlays.thumbnail.show_recording(
            bundle.to_owned(),
            Recording { movie, duration_ms },
            frame.width,
            frame.height,
            frame.rgba,
            screen,
        );
    }
    Ok(())
}
