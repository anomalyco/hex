//! Labs › Transform typed text (Linux): the shortcut copies the focused
//! field's text, runs the Output steps a dictation would, checks the field
//! still holds that text, and pastes the result over it. Everything runs on
//! the output worker, never on audio capture.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::{WrapErr, eyre};

use crate::dictation::processor::Source;
use crate::linux::clipboard::Snapshot;
use crate::linux::paste::LinuxPaster;

/// How long the focused app gets to apply Ctrl+A before Ctrl+C follows.
/// Sending both in one breath can start the copy before the selection settles.
const SELECT_SETTLE: Duration = Duration::from_millis(30);
/// How long the target gets to take Ctrl+C before the job reads the
/// clipboard. On GNOME every read spawns `wl-paste`, which briefly maps its
/// own surface; a read that starts with the copy can take the focus the copy
/// needs, so the clipboard stays empty.
const COPY_SETTLE: Duration = Duration::from_millis(40);
/// How long the focused app gets to put the selection on the clipboard.
/// The job polls until text appears, so a fast app is not held back.
const COPY_DEADLINE: Duration = Duration::from_millis(600);
/// How often the job re-reads the clipboard while it waits.
const COPY_POLL: Duration = Duration::from_millis(40);
/// Typed text has no spoken language. The only language-sensitive step is
/// the dictation-only filler-word filter, which skips typed text, so this
/// value never changes the result.
const LANGUAGE: &str = "en";

/// The clipboard and key operations one typed-text job uses. [`LinuxPaster`]
/// is the real implementation; tests substitute a fake that can ignore a
/// copy, change the selection between copies, and fail a clear or paste.
pub(crate) trait ClipboardJob {
    /// Takes the clipboard for this job, adopting a paste's pending original
    /// when one is waiting to be restored.
    fn begin(&mut self) -> Result<Option<Snapshot>>;
    /// Empties the clipboard before a copy, so stale text cannot pass for a
    /// fresh selection.
    fn clear(&mut self) -> Result<()>;
    fn select_all(&mut self) -> Result<()>;
    fn copy_selection(&mut self) -> Result<()>;
    /// The clipboard's text now; `None` while it holds none.
    fn clipboard_text(&mut self) -> Result<Option<String>>;
    /// Pastes over the selection, then restores `original` after the paste.
    fn paste_over(&mut self, text: &str, original: Snapshot) -> Result<()>;
    fn paste(&mut self, text: &str) -> Result<()>;
    fn restore(&mut self, snapshot: Snapshot) -> Result<()>;
}

impl ClipboardJob for LinuxPaster {
    fn begin(&mut self) -> Result<Option<Snapshot>> {
        self.begin_clipboard_job()
    }

    fn clear(&mut self) -> Result<()> {
        self.clear_clipboard()
    }

    fn select_all(&mut self) -> Result<()> {
        LinuxPaster::select_all(self)
    }

    fn copy_selection(&mut self) -> Result<()> {
        LinuxPaster::copy_selection(self)
    }

    fn clipboard_text(&mut self) -> Result<Option<String>> {
        LinuxPaster::clipboard_text(self)
    }

    fn paste_over(&mut self, text: &str, original: Snapshot) -> Result<()> {
        LinuxPaster::paste_over(self, text, original)
    }

    fn paste(&mut self, text: &str) -> Result<()> {
        LinuxPaster::paste(self, text, false)
    }

    fn restore(&mut self, snapshot: Snapshot) -> Result<()> {
        LinuxPaster::restore(self, snapshot)
    }
}

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
    if paster.pastes_with_shift() {
        tracing::info!(
            "refusing typed text while Terminal paste is on; it would send Ctrl+Shift+V"
        );
        return Err(eyre!(
            "Transform typed text is unavailable while Terminal paste is on: its \
             Ctrl+V would become Ctrl+Shift+V. Turn Terminal paste off in Settings › \
             General and try again."
        ));
    }
    run_with(
        paster,
        |text| {
            let context = crate::linux::context::capture(modes.reads_page());
            modes
                .process(text, LANGUAGE, &context, transformations, Source::TypedText)
                .text
        },
        stop,
    )
}

/// The job itself, over any [`ClipboardJob`], so tests drive every branch.
fn run_with<C, P>(clipboard: &mut C, process: P, stop: &AtomicBool) -> Result<()>
where
    C: ClipboardJob,
    P: FnOnce(&str) -> String,
{
    let mut original = clipboard.begin()?;
    let result = replace_selection(clipboard, process, stop, &mut original);
    // Every path that did not hand the original to a paste puts it back.
    if let Some(snapshot) = original.take() {
        put_back(clipboard, snapshot);
    }
    result
}

fn replace_selection<C, P>(
    clipboard: &mut C,
    process: P,
    stop: &AtomicBool,
    original: &mut Option<Snapshot>,
) -> Result<()>
where
    C: ClipboardJob,
    P: FnOnce(&str) -> String,
{
    // Clear before every copy, whatever the snapshot did: a Ctrl+C the
    // target ignores must not leave older clipboard text looking like the
    // selection. A clear that fails or leaves text means the copy cannot be
    // trusted, so the job stops before it reads or pastes anything.
    clear(clipboard, "before copying the selection")?;
    clipboard.select_all()?;
    settle(stop, SELECT_SETTLE)?;
    clipboard.copy_selection()?;
    let Some(text) = wait_for_text(clipboard, stop)? else {
        return Err(eyre!("no text was selected"));
    };
    if text.trim().is_empty() {
        return Err(eyre!("no text was selected"));
    }

    let processed = process(&text);
    let Some(replacement) = replacement_text(&text, &processed) else {
        return Ok(());
    };

    // The selection must still be the text copied before processing: typing
    // over it or switching windows changes what a fresh Ctrl+C returns.
    clear(clipboard, "before re-copying the selection")?;
    clipboard.copy_selection()?;
    let recopied = wait_for_text(clipboard, stop)?;
    if !selection_unchanged(&text, recopied.as_deref()) {
        return Err(eyre!("the selected text changed; nothing was replaced"));
    }

    match original.take() {
        Some(snapshot) => {
            // Keep the original for the failure path: a paste that fails may
            // still have replaced the clipboard.
            let backup = snapshot.clone();
            if let Err(error) = clipboard.paste_over(replacement, snapshot) {
                put_back(clipboard, backup);
                return Err(error);
            }
        }
        None => clipboard.paste(replacement)?,
    }
    Ok(())
}

/// Clears the clipboard and proves it: a copy the target ignores could
/// otherwise read text the clear left behind. One read is enough; a clear
/// that failed to empty it is not worth polling.
fn clear<C: ClipboardJob>(clipboard: &mut C, when: &str) -> Result<()> {
    clipboard
        .clear()
        .wrap_err_with(|| format!("could not clear the clipboard {when}"))?;
    if clipboard
        .clipboard_text()?
        .is_some_and(|text| !text.is_empty())
    {
        return Err(eyre!("the clipboard still held text {when}"));
    }
    Ok(())
}

/// Reads the clipboard until the target has put text on it or
/// [`COPY_DEADLINE`] passes, checking the stop flag between reads. Waits
/// [`COPY_SETTLE`] first so the first read cannot race the copy.
fn wait_for_text<C: ClipboardJob>(clipboard: &mut C, stop: &AtomicBool) -> Result<Option<String>> {
    settle(stop, COPY_SETTLE)?;
    let deadline = Instant::now() + COPY_DEADLINE;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("typed text cancelled while stopping the listener"));
        }
        if let Some(text) = clipboard.clipboard_text()?
            && !text.is_empty()
        {
            return Ok(Some(text));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(COPY_POLL);
    }
}

/// Sleeps up to `duration`, checking the stop flag between sleeps so a
/// cancel still ends the job promptly.
fn settle(stop: &AtomicBool, duration: Duration) -> Result<()> {
    let deadline = Instant::now() + duration;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("typed text cancelled while stopping the listener"));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        thread::sleep(remaining.min(COPY_POLL));
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

fn put_back<C: ClipboardJob>(clipboard: &mut C, snapshot: Snapshot) {
    if let Err(error) = clipboard.restore(snapshot) {
        tracing::warn!(
            error = format!("{error:#}"),
            "could not restore the clipboard after a typed-text job"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug)]
    struct Fake {
        original: Snapshot,
        clipboard: Option<String>,
        copies: VecDeque<Option<String>>,
        snapshot_available: bool,
        clear_fails: bool,
        clear_leaves_text: bool,
        paste_fails: bool,
        clears: usize,
        selects: usize,
        copy_calls: usize,
        pasted: Vec<String>,
        paste_original: Option<Snapshot>,
        restored: Vec<Snapshot>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                original: Snapshot::Wayland(vec![(
                    "text/plain;charset=utf-8".into(),
                    b"before the job".to_vec(),
                )]),
                clipboard: Some("before the job".into()),
                copies: VecDeque::new(),
                snapshot_available: true,
                clear_fails: false,
                clear_leaves_text: false,
                paste_fails: false,
                clears: 0,
                selects: 0,
                copy_calls: 0,
                pasted: Vec::new(),
                paste_original: None,
                restored: Vec::new(),
            }
        }

        fn copies(mut self, copies: &[Option<&str>]) -> Self {
            self.copies = copies.iter().map(|copy| copy.map(str::to_owned)).collect();
            self
        }

        fn original(&self) -> Snapshot {
            self.original.clone()
        }
    }

    impl ClipboardJob for Fake {
        fn begin(&mut self) -> Result<Option<Snapshot>> {
            Ok(self.snapshot_available.then(|| self.original.clone()))
        }

        fn clear(&mut self) -> Result<()> {
            self.clears += 1;
            if self.clear_fails {
                return Err(eyre!("the test clear failed"));
            }
            if !self.clear_leaves_text {
                self.clipboard = None;
            }
            Ok(())
        }

        fn select_all(&mut self) -> Result<()> {
            self.selects += 1;
            Ok(())
        }

        fn copy_selection(&mut self) -> Result<()> {
            self.copy_calls += 1;
            self.clipboard = self.copies.pop_front().flatten();
            Ok(())
        }

        fn clipboard_text(&mut self) -> Result<Option<String>> {
            Ok(self.clipboard.clone())
        }

        fn paste_over(&mut self, text: &str, original: Snapshot) -> Result<()> {
            if self.paste_fails {
                return Err(eyre!("the test paste failed"));
            }
            self.pasted.push(text.to_owned());
            self.paste_original = Some(original);
            Ok(())
        }

        fn paste(&mut self, text: &str) -> Result<()> {
            if self.paste_fails {
                return Err(eyre!("the test paste failed"));
            }
            self.pasted.push(text.to_owned());
            Ok(())
        }

        fn restore(&mut self, snapshot: Snapshot) -> Result<()> {
            self.restored.push(snapshot);
            Ok(())
        }
    }

    /// A Ctrl+C the target ignores leaves the cleared clipboard empty: the
    /// job must not paste and must put the original clipboard back.
    #[test]
    fn an_ignored_copy_pastes_nothing_and_restores_the_clipboard() {
        let mut fake = Fake::new().copies(&[None]);
        let original = fake.original();
        let result = run_with(&mut fake, |text| text.to_owned(), &AtomicBool::new(false));

        assert!(result.is_err());
        assert!(fake.pasted.is_empty());
        assert_eq!(fake.restored, [original]);
        assert_eq!(fake.clears, 1, "cleared before the only Ctrl+C");
    }

    /// The second Ctrl+C proves the selection is unchanged. If the target
    /// ignores it, the empty clipboard must abort the replacement.
    #[test]
    fn an_ignored_second_copy_pastes_nothing() {
        let mut fake = Fake::new().copies(&[Some("typed"), None]);
        let original = fake.original();
        let result = run_with(
            &mut fake,
            |text| format!("{text}."),
            &AtomicBool::new(false),
        );

        assert!(result.is_err());
        assert!(fake.pasted.is_empty());
        assert_eq!(fake.restored, [original]);
        assert_eq!(fake.clears, 2, "each Ctrl+C is preceded by a clear");
    }

    #[test]
    fn text_changed_between_copies_pastes_nothing() {
        let mut fake = Fake::new().copies(&[Some("typed"), Some("typed!")]);
        let original = fake.original();
        let result = run_with(
            &mut fake,
            |text| format!("{text}."),
            &AtomicBool::new(false),
        );

        let error = result.unwrap_err();
        assert!(error.to_string().contains("changed"), "{error}");
        assert!(fake.pasted.is_empty());
        assert_eq!(fake.restored, [original]);
    }

    #[test]
    fn a_failed_clear_aborts_before_copying_or_pasting() {
        let mut fake = Fake::new().copies(&[Some("typed"), Some("typed")]);
        fake.clear_fails = true;
        let original = fake.original();
        let result = run_with(
            &mut fake,
            |text| format!("{text}."),
            &AtomicBool::new(false),
        );

        assert!(result.is_err());
        assert_eq!(fake.selects, 0, "nothing is selected after a failed clear");
        assert_eq!(fake.copy_calls, 0);
        assert!(fake.pasted.is_empty());
        assert_eq!(fake.restored, [original]);
    }

    /// A clear that reports success but leaves text behind cannot be
    /// trusted: abort before selecting, copying, or pasting.
    #[test]
    fn a_clear_that_leaves_text_aborts_before_copying() {
        let mut fake = Fake::new().copies(&[Some("typed")]);
        fake.clear_leaves_text = true;
        let original = fake.original();
        let result = run_with(
            &mut fake,
            |text| format!("{text}."),
            &AtomicBool::new(false),
        );

        assert!(result.is_err());
        assert_eq!(fake.selects, 0);
        assert_eq!(fake.copy_calls, 0);
        assert!(fake.pasted.is_empty());
        assert_eq!(fake.restored, [original]);
    }

    /// A paste that fails may still have taken the clipboard. The job puts
    /// the saved original back rather than leaving the result there.
    #[test]
    fn a_failed_paste_restores_the_original() {
        let mut fake = Fake::new().copies(&[Some("typed"), Some("typed")]);
        fake.paste_fails = true;
        let original = fake.original();
        let result = run_with(
            &mut fake,
            |text| format!("{text}."),
            &AtomicBool::new(false),
        );

        assert!(result.is_err());
        assert!(fake.pasted.is_empty());
        assert_eq!(fake.restored, [original]);
    }

    #[test]
    fn success_pastes_the_result_and_hands_over_the_original() {
        let mut fake = Fake::new().copies(&[Some("typed"), Some("typed")]);
        let original = fake.original();
        let result = run_with(
            &mut fake,
            |text| format!("{text}."),
            &AtomicBool::new(false),
        );

        assert!(result.is_ok());
        assert_eq!(fake.pasted, ["typed."]);
        assert_eq!(fake.paste_original, Some(original));
        assert!(
            fake.restored.is_empty(),
            "a successful paste_over owns the restore"
        );
    }

    #[test]
    fn unchanged_processing_pastes_nothing_and_restores() {
        let mut fake = Fake::new().copies(&[Some("typed")]);
        let original = fake.original();
        let result = run_with(&mut fake, |text| text.to_owned(), &AtomicBool::new(false));

        assert!(result.is_ok());
        assert!(fake.pasted.is_empty());
        assert_eq!(fake.copy_calls, 1, "no second copy without a change");
        assert_eq!(fake.restored, [original]);
    }

    /// A clipboard Hex cannot snapshot does not stop the job from clearing
    /// and copying; it just leaves the result behind, as a normal paste does.
    #[test]
    fn a_job_without_a_snapshot_still_clears_and_pastes() {
        let mut fake = Fake::new().copies(&[Some("typed"), Some("typed")]);
        fake.snapshot_available = false;
        let result = run_with(
            &mut fake,
            |text| format!("{text}."),
            &AtomicBool::new(false),
        );

        assert!(result.is_ok());
        assert_eq!(fake.clears, 2);
        assert_eq!(fake.pasted, ["typed."]);
        assert_eq!(fake.paste_original, None);
        assert!(fake.restored.is_empty(), "nothing to restore");
    }

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
