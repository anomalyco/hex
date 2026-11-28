//! What a screenshot card or editor does with its file on disk, off the main thread.

use std::path::{Path, PathBuf};

use rune::OverlayHandle;

use super::Cards;
use crate::desktop::i18n::tr;

/// The file on the clipboard, and a shot's picture with it. Card `id`
/// says what was copied once the clipboard has it, or that it wasn't.
pub(super) fn copy(path: PathBuf, recording: bool, overlay: Option<OverlayHandle<Cards>>, id: u64) {
    spawn("hex-card-copy", move || {
        #[cfg(target_os = "macos")]
        let copied = crate::screenshots::save::copy_saved(&path, recording);
        #[cfg(not(target_os = "macos"))]
        let copied: anyhow::Result<()> = {
            let _ = (&path, recording);
            Err(anyhow::anyhow!("cards copy on macOS only"))
        };
        let note = match copied {
            Ok(()) => copied_note(recording),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "could not copy the capture");
                tr("Couldn't copy")
            }
        };
        if let Some(overlay) = overlay {
            overlay.update(move |s, tx| s.note(id, note.to_owned(), tx));
        }
    });
}

/// What a card says once its copy is on the clipboard: a shot's
/// picture, or a recording's movie file.
pub(super) fn copied_note(recording: bool) -> &'static str {
    if recording {
        tr("Movie copied")
    } else {
        tr("Copied")
    }
}

/// The file selected in Finder or Explorer, or its folder on Linux.
pub(crate) fn reveal(path: &Path) {
    crate::meeting::reveal(path);
}

/// Context-menu label for opening a shot in Preview (macOS) or the default viewer.
pub(crate) fn preview_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        tr("Open in Preview")
    }
    #[cfg(not(target_os = "macos"))]
    {
        tr("Open in default viewer")
    }
}

/// Command that opens `path` in Preview on macOS or the default viewer
/// elsewhere, constructed via `desktop::background_process`.
pub(crate) fn open_in_preview_command(path: &Path) -> std::process::Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = crate::desktop::background_process::command("/usr/bin/open");
        cmd.arg("-a").arg("Preview").arg(path);
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = crate::desktop::background_process::command("cmd");
        cmd.args(["/C", "start", ""]).arg(path);
        cmd
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let mut cmd = crate::desktop::background_process::command("xdg-open");
        cmd.arg(path);
        cmd
    }
}

/// Spawns Preview / the default viewer on `path`, returning any launch error.
pub(crate) fn open_in_preview_sync(path: &Path) -> anyhow::Result<()> {
    open_in_preview_command(path).spawn().map_err(|error| {
        anyhow::anyhow!("could not open {} in Preview: {error}", path.display())
    })?;
    Ok(())
}

/// Opens `path` in Preview / the default viewer off the main thread.
pub(crate) fn open_in_preview(path: PathBuf) {
    spawn("hex-card-preview", move || {
        if let Err(error) = open_in_preview_sync(&path) {
            tracing::warn!(%error, path = %path.display(), "could not open the capture in Preview");
        }
    });
}

pub(crate) fn trash(path: PathBuf) {
    spawn("hex-card-trash", move || {
        #[cfg(target_os = "macos")]
        match crate::screenshots::save::trash(&path) {
            Ok(()) => {
                crate::history::captures::forget(&path);
                crate::history::annotations::forget(&path);
            }
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "could not trash the capture");
                crate::feedback::play(crate::feedback::Tone::Error);
            }
        }
        #[cfg(not(target_os = "macos"))]
        match crate::meeting::move_to_trash(&path) {
            Ok(()) => {
                crate::history::captures::forget(&path);
                #[cfg(hex_capture)]
                crate::history::annotations::forget(&path);
            }
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "could not trash the capture");
                crate::feedback::play(crate::feedback::Tone::Error);
            }
        }
    });
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!(%error, "could not start {name}");
    }
}
