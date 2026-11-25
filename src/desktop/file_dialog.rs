//! Native open and save dialogs for one file or one folder, through Rune's
//! `FileDialogs`: `NSOpenPanel`/`NSSavePanel` on macOS, the common item
//! dialog on Windows, and the FileChooser portal on Linux. Each runs on its
//! own thread and answers on a channel, so neither the window nor the runtime
//! waits on the person.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

fn show(dialog: Dialog, reply: Reply) {
    // A failed spawn drops the reply, which answers "nothing chosen".
    let _ = std::thread::Builder::new()
        .name("hex-file-dialog".into())
        .spawn(move || {
            let dialogs = rune::FileDialogs::native();
            let mut options = rune::FileDialogOptions {
                title: dialog.title,
                directory: dialog.directory.filter(|path| path.is_dir()),
                ..Default::default()
            };
            let choice = match dialog.target {
                Target::Folder => pollster::block_on(dialogs.open_folder(options)),
                Target::File(filters) => {
                    for filter in filters {
                        options = options.filter(filter.label, filter.extensions);
                    }
                    pollster::block_on(dialogs.open(options))
                }
                Target::Save { filters, file_name } => {
                    options.file_name = Some(file_name);
                    for filter in filters {
                        options = options.filter(filter.label, filter.extensions);
                    }
                    pollster::block_on(dialogs.save(options))
                }
            };
            reply.send(choice.map_err(|error| error.to_string()));
        });
}

pub(crate) struct Dialog {
    pub(crate) title: String,
    pub(crate) target: Target,
    /// Where the dialog starts, when it exists.
    pub(crate) directory: Option<PathBuf>,
}

pub(crate) enum Target {
    /// One existing file.
    File(Vec<Filter>),
    /// A file to write, with the name the dialog offers first.
    Save {
        filters: Vec<Filter>,
        file_name: String,
    },
    /// A folder: the meetings and annotations folders, and Labs ›
    /// Screenshots' saves folder.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Folder,
}

/// A named set of extensions without the dot ("app", "exe"); `"*"` is any
/// file.
pub(crate) struct Filter {
    pub(crate) label: String,
    pub(crate) extensions: &'static [&'static str],
}

/// The chosen path, `None` when the person cancelled.
pub(crate) type Choice = Result<Option<PathBuf>, String>;

/// Opens `dialog` without waiting for it; `wake` runs once it answers.
pub(crate) fn open(dialog: Dialog, wake: impl Fn() + Send + 'static) -> Pending {
    let (reply, pending) = channel(wake);
    show(dialog, reply);
    pending
}

/// [`open`] for a save dialog asked from an overlay, such as Labs ›
/// Screenshots' editor. Rune's dialogs attach to a window and refuse
/// overlays, so on macOS this is an `NSSavePanel` of its own, free-standing
/// above every overlay, with Hex brought forward so the panel takes keys.
/// Off the main thread, for other targets, and elsewhere it is [`open`].
pub(crate) fn open_over_overlays(dialog: Dialog, wake: impl Fn() + Send + 'static) -> Pending {
    let (reply, pending) = channel(wake);
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new()
        && matches!(dialog.target, Target::Save { .. })
    {
        save_panel(mtm, dialog, reply);
        return pending;
    }
    show(dialog, reply);
    pending
}

/// Above Rune's overlays, which sit at most at level 27
/// (`OverlayLevel::AboveOverlays`).
#[cfg(target_os = "macos")]
const ABOVE_OVERLAYS: isize = 28;

#[cfg(target_os = "macos")]
fn save_panel(mtm: objc2::MainThreadMarker, dialog: Dialog, reply: Reply) {
    use std::cell::Cell;

    use block2::RcBlock;
    use objc2_app_kit::{NSApplication, NSModalResponse, NSSavePanel};
    use objc2_foundation::{NSArray, NSString, NSURL};

    /// `NSModalResponseOK`.
    const OK: NSModalResponse = 1;

    let Target::Save { filters, file_name } = dialog.target else {
        return;
    };
    let panel = NSSavePanel::savePanel(mtm);
    panel.setTitle(Some(&NSString::from_str(&dialog.title)));
    panel.setNameFieldStringValue(&NSString::from_str(&file_name));
    if let Some(url) = dialog
        .directory
        .filter(|path| path.is_dir())
        .and_then(NSURL::from_directory_path)
    {
        panel.setDirectoryURL(Some(&url));
    }
    let extensions: Vec<_> = filters
        .iter()
        .flat_map(|filter| filter.extensions)
        .filter(|extension| **extension != "*")
        .map(|extension| NSString::from_str(extension))
        .collect();
    if !extensions.is_empty() {
        // `allowedContentTypes` needs UTTypes; the extensions do the same.
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&extensions)));
    }
    panel.setCanCreateDirectories(true);
    panel.setLevel(ABOVE_OVERLAYS);
    NSApplication::sharedApplication(mtm).activate();
    let reply = Cell::new(Some(reply));
    let answered = panel.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        let Some(reply) = reply.take() else {
            return;
        };
        let path = (response == OK)
            .then(|| answered.URL())
            .flatten()
            .and_then(|url| url.to_file_path());
        reply.send(Ok(path));
    });
    panel.beginWithCompletionHandler(&handler);
}

fn channel(wake: impl Fn() + Send + 'static) -> (Reply, Pending) {
    let (sender, receiver) = sync_channel(1);
    let reply = Reply {
        sender,
        wake: Box::new(wake),
        answered: false,
    };
    (reply, Pending(receiver))
}

/// An open dialog's answer, polled from the window.
pub(crate) struct Pending(Receiver<Choice>);

impl Pending {
    /// `None` while the dialog is open.
    pub(crate) fn poll(&self) -> Option<Choice> {
        match self.0.try_recv() {
            Ok(choice) => Some(choice),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Ok(None)),
        }
    }
}

/// The dialog's one answer. Dropped unanswered (a dialog that never opened)
/// it answers "nothing chosen", so no caller waits on a dialog that is gone.
pub(crate) struct Reply {
    sender: SyncSender<Choice>,
    wake: Box<dyn Fn() + Send>,
    answered: bool,
}

impl Reply {
    pub(crate) fn send(mut self, choice: Choice) {
        self.answer(choice);
    }

    fn answer(&mut self, choice: Choice) {
        if std::mem::replace(&mut self.answered, true) {
            return;
        }
        let _ = self.sender.try_send(choice);
        (self.wake)();
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.answer(Ok(None));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn counted() -> (Arc<AtomicUsize>, impl Fn() + Send + 'static) {
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = wakes.clone();
        (wakes, move || {
            counter.fetch_add(1, Ordering::Relaxed);
        })
    }

    #[test]
    fn the_chosen_path_arrives_once_and_wakes_the_window() {
        let (wakes, wake) = counted();
        let (reply, pending) = channel(wake);
        assert_eq!(pending.poll(), None);
        reply.send(Ok(Some(PathBuf::from("/Applications/Zed.app"))));
        assert_eq!(
            pending.poll(),
            Some(Ok(Some(PathBuf::from("/Applications/Zed.app"))))
        );
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_dialog_that_never_answers_reads_as_cancelled() {
        let (wakes, wake) = counted();
        let (reply, pending) = channel(wake);
        drop(reply);
        assert_eq!(pending.poll(), Some(Ok(None)));
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn failures_reach_the_caller() {
        let (_, wake) = counted();
        let (reply, pending) = channel(wake);
        reply.send(Err("no file chooser portal".into()));
        assert_eq!(pending.poll(), Some(Err("no file chooser portal".into())));
    }
}
