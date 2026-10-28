//! Native open dialogs for one file or one folder: `NSOpenPanel` on macOS,
//! `IFileOpenDialog` on Windows, and the FileChooser portal on Linux. Each
//! runs where its OS requires (AppKit's main thread, a dedicated STA thread,
//! a dedicated portal thread) and answers on a channel, so neither the window
//! nor the runtime waits on the person.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

#[cfg(target_os = "macos")]
#[path = "file_dialog/macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "file_dialog/windows.rs"]
mod platform;

#[cfg(target_os = "linux")]
mod platform {
    use super::{Dialog, Reply};

    pub(super) fn open(dialog: Dialog, reply: Reply) {
        let _ = std::thread::Builder::new()
            .name("hex-file-dialog".into())
            .spawn(move || {
                let choice =
                    crate::linux_file_chooser::open(&dialog).map_err(|error| format!("{error:#}"));
                reply.send(choice);
            });
    }
}

pub(crate) struct Dialog {
    pub(crate) title: String,
    /// The confirming button's label.
    pub(crate) accept: String,
    pub(crate) target: Target,
    /// Where the dialog starts, when it exists.
    pub(crate) directory: Option<PathBuf>,
}

pub(crate) enum Target {
    /// One existing file. macOS has no filter menu, so it allows only the
    /// first filter's extensions.
    File(Vec<Filter>),
    /// Only the macOS meetings folder asks for one yet.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Folder,
}

/// A named set of extensions without the dot ("app", "exe"); none means any
/// file.
pub(crate) struct Filter {
    /// macOS shows no filter names.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) label: String,
    pub(crate) extensions: &'static [&'static str],
}

impl Filter {
    /// Glob patterns: `*.app`, or `*` for any file.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) fn patterns(&self) -> Vec<String> {
        if self.extensions.is_empty() {
            return vec!["*".into()];
        }
        self.extensions
            .iter()
            .map(|extension| format!("*.{extension}"))
            .collect()
    }
}

/// The chosen path, `None` when the person cancelled.
pub(crate) type Choice = Result<Option<PathBuf>, String>;

/// Opens `dialog` without waiting for it; `wake` runs once it answers.
pub(crate) fn open(dialog: Dialog, wake: impl Fn() + Send + 'static) -> Pending {
    let (reply, pending) = channel(wake);
    platform::open(dialog, reply);
    pending
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

    #[test]
    fn filters_become_globs() {
        let filter = |extensions| Filter {
            label: String::new(),
            extensions,
        };
        assert_eq!(filter(&["exe"]).patterns(), ["*.exe"]);
        assert_eq!(filter(&[]).patterns(), ["*"]);
    }
}
