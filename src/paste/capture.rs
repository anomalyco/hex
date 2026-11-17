//! Insertion only. Neither selecting a capture nor preparing its two parts
//! should take the clipboard, reactivate an app, or affect an Annotate send.

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
    #[allow(dead_code, reason = "staged until the mixed picker is connected")]
    pub(crate) fn paste_capture(&mut self, capture: &Payload, target: i32) -> Result<()> {
        let revision = self.activity.revision();
        let focus = crate::accessibility::PasteFocus::capture(target)
            .ok_or_else(|| eyre!("could not confirm the capture paste target"))?;
        self.continuation = None;
        self.prepared_clipboard = None;
        insert(
            &self.clipboard,
            &mut self
                .clipboard_restore
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            capture.first(),
            &capture.summary,
            || {
                self.activity.revision() == revision
                    && crate::dictation::context::frontmost_pid() == Some(target)
                    && focus.unchanged()
            },
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
    mut paste: impl FnMut() -> Result<()>,
    mut wait: impl FnMut(Duration),
) -> Result<()> {
    if summary.trim().is_empty() || !focused() {
        return Err(eyre!("no capture text or the paste target changed"));
    }
    let sheet = sheet.map(|s| s.item()).transpose()?;
    let summary = Part::Text(summary).item()?;
    let initial = board.changeCount();
    let snapshot = capture_clipboard(board)?;
    let mut owned = None;
    let mut attempted = false;
    let outcome = (|| {
        for item in sheet.into_iter().chain([summary]) {
            if !focused() || board.changeCount() != owned.unwrap_or(initial) {
                return Err(eyre!("paste target or clipboard changed"));
            }
            // clearContents returns *our* ownership count. Reading changeCount
            // after writeObjects could instead adopt an intervening copy.
            let count = board.clearContents();
            owned = Some(count);
            if board.changeCount() != count {
                return Err(eyre!("clipboard changed before writing the capture"));
            }
            write_items(board, &[item])?;
            wait(Duration::from_millis(20));
            if !focused() || board.changeCount() != count {
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
        let previous = if pending.last_change_count == Some(initial) {
            pending.original.take().unwrap_or(snapshot)
        } else {
            snapshot
        };
        pending.keep();
        if attempted && board.changeCount() == count {
            wait(Duration::from_millis(500));
        }
        if board.changeCount() == count {
            restore_clipboard(board, &previous)
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
mod tests {
    use super::*;
    use crate::paste::write_clipboard_text;
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};

    fn text(board: &NSPasteboard) -> String {
        board
            .stringForType(unsafe { NSPasteboardTypeString })
            .unwrap()
            .to_string()
    }

    #[test]
    fn target_switch_after_first_paste_restores_the_original_without_posting_again() {
        let board = NSPasteboard::pasteboardWithUniqueName();
        write_clipboard_text(&board, "original", false).unwrap();
        let focused = std::cell::Cell::new(true);
        let result = insert(
            &board,
            &mut ClipboardRestore::default(),
            Some(Part::Text("sheet.png")),
            "caption",
            || focused.get(),
            || {
                assert!(focused.replace(false));
                assert_eq!(text(&board), "sheet.png");
                Ok(())
            },
            |_| {},
        );
        assert!(result.unwrap_err().to_string().contains("partially pasted"));
        assert_eq!(text(&board), "original");
    }

    #[test]
    fn a_capture_after_dictation_restores_the_pre_dictation_clipboard() {
        let board = NSPasteboard::pasteboardWithUniqueName();
        write_clipboard_text(&board, "original", false).unwrap();
        let mut pending = ClipboardRestore::default();
        let original_count = board.changeCount();
        let original = capture_clipboard(&board).unwrap();
        write_clipboard_text(&board, "dictation", true).unwrap();
        pending.register(original, original_count, board.changeCount());
        let mut posted = Vec::new();
        insert(
            &board,
            &mut pending,
            Some(Part::Text("sheet.png")),
            "caption",
            || true,
            || {
                posted.push(text(&board));
                Ok(())
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(posted, ["sheet.png", "caption"]);
        assert_eq!(text(&board), "original");
    }

    #[test]
    fn an_external_copy_after_the_sheet_stops_the_summary_and_survives_restore() {
        let board = NSPasteboard::pasteboardWithUniqueName();
        write_clipboard_text(&board, "original", false).unwrap();
        let mut posts = 0;
        let result = insert(
            &board,
            &mut ClipboardRestore::default(),
            Some(Part::Png(b"sample sheet")),
            "Move the heading down.",
            || true,
            || {
                posts += 1;
                write_clipboard_text(&board, "external copy", false)
            },
            |_| {},
        );
        assert!(result.unwrap_err().to_string().contains("partially pasted"));
        assert_eq!(posts, 1);
        assert_eq!(
            board
                .stringForType(unsafe { NSPasteboardTypeString })
                .unwrap()
                .to_string(),
            "external copy"
        );
    }
}
