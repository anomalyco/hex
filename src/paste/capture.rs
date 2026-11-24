//! Insertion only. Neither selecting a capture nor preparing its two parts
//! should take the clipboard, reactivate an app, or affect an Annotate send.

use super::clipboard_restore::CLIPBOARD_RESTORE_DELAY;
use super::{
    CLIPBOARD_MANAGER_MARKERS, ClipboardRestore, Paster, capture_clipboard, restore_clipboard,
    write_items,
};
use color_eyre::eyre::{Result, eyre};
use objc2::rc::Retained;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeString};
use objc2_foundation::{NSData, NSString};
use scrawl_capture::model::ScrawlBundle;
use std::path::Path;
use std::time::Duration;

pub(crate) enum Part<'a> {
    Text(&'a str),
    Png(&'a [u8]),
}

enum Sheet {
    Png(Vec<u8>),
    Path(String),
}

pub(crate) struct Payload {
    sheet: Option<Sheet>,
    summary: String,
}

impl Payload {
    /// Annotate's Paste uses the rich summary with the sheet, independent of
    /// the *copy* preference. Do not generate a zip, reactivate an app, or
    /// consult the send queue. Prepare every part before taking the board.
    pub(crate) fn prepare(
        directory: &Path,
        bundle: &ScrawlBundle,
        terminal: bool,
        agent_context: bool,
    ) -> Result<Self> {
        let sheet = if bundle.keyframes.is_empty() {
            None
        } else {
            if bundle.contact_sheet != "contact-sheet.png" {
                return Err(eyre!("unexpected capture sheet"));
            }
            let path = directory.join(&bundle.contact_sheet);
            let bytes = crate::capture_host::paste_source::read_regular(&path, 32 << 20)
                .ok_or_else(|| eyre!("capture sheet unavailable"))?;
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(eyre!("invalid capture sheet"));
            }
            Some(if terminal {
                Sheet::Path(path.display().to_string())
            } else {
                Sheet::Png(bytes)
            })
        };
        Ok(Self {
            sheet,
            summary: scrawl_capture::sketch::summary_markdown_with(
                bundle,
                directory,
                agent_context,
            ),
        })
    }

    fn first(&self) -> Option<Part<'_>> {
        self.sheet.as_ref().map(|sheet| match sheet {
            Sheet::Png(bytes) => Part::Png(bytes),
            Sheet::Path(path) => Part::Text(path),
        })
    }

    #[cfg(test)]
    pub(crate) fn paste_to_fixture(
        &self,
        board: &NSPasteboard,
        paste: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        insert(
            board,
            &mut ClipboardRestore::default(),
            self.first(),
            &self.summary,
            || true,
            paste,
            |_| {},
        )
    }
}

impl Part<'_> {
    fn item(&self) -> Result<Retained<NSPasteboardItem>> {
        let item = NSPasteboardItem::new();
        let ok = match self {
            Self::Text(text) => {
                item.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString })
            }
            Self::Png(bytes) => item.setData_forType(
                &NSData::with_bytes(bytes),
                &NSString::from_str("public.png"),
            ),
        };
        if !ok
            || !CLIPBOARD_MANAGER_MARKERS
                .iter()
                .all(|marker| item.setData_forType(&NSData::new(), &NSString::from_str(marker)))
        {
            return Err(eyre!("could not prepare the capture for paste"));
        }
        Ok(item)
    }
}

impl Paster {
    /// The capture owner supplies the sheet (PNG or terminal path), summary,
    /// and the target PID from the gesture. No arbitrary text/command mode.
    pub(crate) fn paste_capture(
        &mut self,
        capture: &Payload,
        target: &super::recent::Target,
    ) -> Result<()> {
        // Before the clipboard is touched: macOS 27.3 refuses to post ⌘V
        // without its keystroke grant, which would otherwise look like a
        // capture that landed.
        super::ensure_keystroke_access()?;
        // A capture's sheet and summary replace what a running dictation
        // watch has in front of it, exactly as another dictation would.
        super::note_paste();
        let revision = self.activity.revision();
        self.continuation.clear();
        self.prepared_clipboard = None;
        insert(
            &self.clipboard,
            &mut self
                .clipboard_restore
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            capture.first(),
            &capture.summary,
            || self.activity.revision() == revision && target.unchanged(),
            || crate::keyboard::post_command('v'),
            std::thread::sleep,
        )
    }
}

/// A paste key has no delivery acknowledgement. Once attempted, never retry
/// it, including on an error posting a key-up. A later copy always wins.
fn insert(
    board: &NSPasteboard,
    pending: &mut ClipboardRestore,
    sheet: Option<Part<'_>>,
    summary: &str,
    focused: impl Fn() -> bool,
    paste: impl FnMut() -> Result<()>,
    wait: impl FnMut(Duration),
) -> Result<()> {
    if summary.trim().is_empty() || !focused() {
        return Err(eyre!("no capture text or the paste target changed"));
    }
    let sheet = sheet.map(|s| s.item()).transpose()?;
    let summary = Part::Text(summary).item()?;
    transaction(board, pending, sheet, summary, focused, paste, wait)
}

trait Board {
    type Item;
    fn generation(&self) -> isize;
    fn save(&self) -> Result<super::ClipboardSnapshot>;
    fn claim(&self) -> isize;
    fn write(&self, value: Self::Item) -> Result<()>;
    fn restore(&self, original: &super::ClipboardSnapshot) -> Result<()>;
}

impl Board for NSPasteboard {
    type Item = Retained<NSPasteboardItem>;
    fn generation(&self) -> isize {
        self.changeCount()
    }
    fn save(&self) -> Result<super::ClipboardSnapshot> {
        capture_clipboard(self)
    }
    fn claim(&self) -> isize {
        self.clearContents()
    }
    fn write(&self, item: Self::Item) -> Result<()> {
        write_items(self, &[item])
    }
    fn restore(&self, original: &super::ClipboardSnapshot) -> Result<()> {
        restore_clipboard(self, original)
    }
}

fn transaction<B: Board>(
    board: &B,
    pending: &mut ClipboardRestore,
    sheet: Option<B::Item>,
    summary: B::Item,
    focused: impl Fn() -> bool,
    mut paste: impl FnMut() -> Result<()>,
    mut wait: impl FnMut(Duration),
) -> Result<()> {
    let initial = board.generation();
    let snapshot = board.save()?;
    let mut owned = None;
    let mut attempted = false;
    let outcome = (|| {
        for item in sheet.into_iter().chain([summary]) {
            if !focused() || board.generation() != owned.unwrap_or(initial) {
                return Err(eyre!("paste target or clipboard changed"));
            }
            // clearContents returns *our* ownership count. Reading changeCount
            // after writeObjects could instead adopt an intervening copy.
            let count = board.claim();
            owned = Some(count);
            if board.generation() != count {
                return Err(eyre!("clipboard changed before writing the capture"));
            }
            board.write(item)?;
            wait(Duration::from_millis(20));
            if !focused() || board.generation() != count {
                return Err(eyre!("paste target or clipboard changed"));
            }
            attempted = true;
            paste()?;
            wait(Duration::from_millis(160));
        }
        Ok(())
    })();
    if let Some(count) = owned {
        // Share the prior text paste's original, not its transient payload.
        let previous = pending
            .take_pending(|&last| last == initial)
            .unwrap_or(snapshot);
        if attempted && board.generation() == count {
            wait(CLIPBOARD_RESTORE_DELAY);
        }
        if board.generation() == count {
            board
                .restore(&previous)
                .map_err(|e| eyre!("capture paste finished with a clipboard restore error: {e}"))?;
        }
    }
    outcome.map_err(|e| {
        if attempted {
            eyre!("capture may be partially pasted: {e}")
        } else {
            e
        }
    })
}

#[cfg(test)]
mod tests;
