//! Labs › Transform typed text (Linux): the shortcut copies the focused
//! field's text, runs the Output steps a dictation would, checks the field
//! still holds that text, and pastes the result over it. Everything runs on
//! the output worker, never on audio capture.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::dictation::processor::Source;
use crate::linux::clipboard::Snapshot;
use crate::linux::paste::LinuxPaster;

/// How long the focused app gets to select all before Hex copies.
const SELECT_SETTLE: Duration = Duration::from_millis(80);
/// How long the focused app gets to put the selection on the clipboard.
const COPY_SETTLE: Duration = Duration::from_millis(100);
/// Typed text has no spoken language; the only language-sensitive step is
/// the dictation-only filler-word filter, so this never changes the result.
const LANGUAGE: &str = "en";

/// Runs one typed-text job from the output worker: clones the mode runtime
/// so a settings write never waits on processing, then copies, processes,
/// re-checks, and pastes.
pub(crate) fn run_job(
    paster: &mut LinuxPaster,
    modes: &std::sync::RwLock<crate::dictation::modes::ModeRuntime>,
    transformations: &crate::personal_commands::TransformationClient,
    stop: &AtomicBool,
) -> Result<()> {
    let modes = modes
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    run(paster, &modes, transformations, stop)
}

/// Runs one typed-text job: Ctrl+A, Ctrl+C, the matching mode's Output
/// steps, a second Ctrl+C to prove the selection is unchanged, then Ctrl+V
/// over it. `paster` and the mode runtime belong to the output worker.
pub(crate) fn run(
    paster: &mut LinuxPaster,
    modes: &crate::dictation::modes::ModeRuntime,
    transformations: &crate::personal_commands::TransformationClient,
    stop: &AtomicBool,
) -> Result<()> {
    // A clipboard Hex cannot read (GNOME without data-control and without
    // Labs › Restore clipboard on GNOME) still lets the job run; the paste
    // then leaves its text on the clipboard, as a normal paste there does.
    let mut original = match paster.snapshot() {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            tracing::info!(
                error = format!("{error:#}"),
                "not restoring the clipboard after this typed-text job"
            );
            None
        }
    };
    let result = replace_selection(paster, modes, transformations, stop, &mut original);
    // Every path that did not paste the result puts the saved clipboard
    // back; `paste_over` owns the restore once it takes the snapshot.
    if original.is_some() {
        put_back(paster, &mut original);
    }
    result
}

fn replace_selection(
    paster: &mut LinuxPaster,
    modes: &crate::dictation::modes::ModeRuntime,
    transformations: &crate::personal_commands::TransformationClient,
    stop: &AtomicBool,
    original: &mut Option<Snapshot>,
) -> Result<()> {
    // Clearing first means a copy the target ignores cannot leave older
    // clipboard text looking like the selection. Only do it when Hex can put
    // the clipboard back; a GNOME session without data-control reads through
    // wl-paste and cannot restore, so it leaves the copy there instead.
    if original.is_some() {
        paster.clear_clipboard()?;
    }
    paster.select_all()?;
    settle(stop, SELECT_SETTLE)?;
    paster.copy_selection()?;
    settle(stop, COPY_SETTLE)?;
    let Some(text) = paster.clipboard_text()? else {
        return Err(eyre!("no text was selected"));
    };
    if text.trim().is_empty() {
        return Err(eyre!("no text was selected"));
    }

    let context = crate::linux::context::capture(modes.reads_page());
    let processed = modes.process(
        &text,
        LANGUAGE,
        &context,
        transformations,
        Source::TypedText,
    );
    let Some(replacement) = replacement_text(&text, &processed.text) else {
        return Ok(());
    };

    // The selection must still be the text copied before processing: typing
    // over it or switching windows changes what a fresh Ctrl+C returns.
    if original.is_some() {
        paster.clear_clipboard()?;
    }
    paster.copy_selection()?;
    settle(stop, COPY_SETTLE)?;
    let recopied = paster.clipboard_text()?;
    if !selection_unchanged(&text, recopied.as_deref()) {
        return Err(eyre!("the selected text changed; nothing was replaced"));
    }

    match original.take() {
        Some(snapshot) => paster.paste_over(replacement, snapshot),
        None => paster.paste(replacement),
    }
}

/// What to paste over `original`, or `None` when processing changed nothing
/// (or produced nothing to paste).
fn replacement_text<'a>(original: &str, processed: &'a str) -> Option<&'a str> {
    (processed != original && !processed.trim().is_empty()).then_some(processed)
}

/// Whether the second copy returned exactly the text copied before
/// processing. `None` is a cleared or unreadable clipboard, never a match.
fn selection_unchanged(original: &str, recopied: Option<&str>) -> bool {
    recopied == Some(original)
}

/// Waits out `delay`, failing early when the listener is stopping.
fn settle(stop: &AtomicBool, delay: Duration) -> Result<()> {
    thread::sleep(delay);
    if stop.load(Ordering::Acquire) {
        return Err(eyre!("typed text cancelled while stopping the listener"));
    }
    Ok(())
}

fn put_back(paster: &LinuxPaster, snapshot: &mut Option<Snapshot>) {
    if let Some(snapshot) = snapshot.take()
        && let Err(error) = paster.restore(snapshot)
    {
        tracing::warn!(
            error = format!("{error:#}"),
            "could not restore the clipboard after a typed-text job"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_or_empty_processing_pastes_nothing() {
        assert_eq!(replacement_text("typed", "typed"), None);
        assert_eq!(replacement_text("typed", "  typed  "), Some("  typed  "));
        assert_eq!(replacement_text("typed", ""), None);
        assert_eq!(replacement_text("typed", "  "), None);
        assert_eq!(replacement_text("typed", "Typed."), Some("Typed."));
    }

    #[test]
    fn a_changed_second_copy_aborts_the_replacement() {
        assert!(selection_unchanged("typed text", Some("typed text")));
        assert!(!selection_unchanged("typed text", Some("typed text!")));
        assert!(!selection_unchanged("typed text", Some("")));
        assert!(!selection_unchanged("typed text", None));
    }
}
