//! Annotate › Folder's Change… on Linux and Windows: the window opens the
//! folder chooser itself and saves the pick as `capture.folder` through its
//! own settings, as Labs › Screenshots' folder does. macOS's host does the
//! same for its pane.

use super::super::Ui;
use crate::desktop::file_dialog::{Choice, Dialog, Pending, Target};
use crate::desktop::i18n::tr;

/// Opens the chooser, one at a time, starting where captures go now.
pub(in crate::rune_ui) fn choose(ui: &mut Ui) {
    if ui.capture_folder.is_some() {
        return;
    }
    let waker = ui.modes.waker.clone();
    let wake = move || {
        if let Some(waker) = &waker {
            waker();
        }
    };
    ui.capture_folder = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Annotations folder").into(),
            target: Target::Folder,
            directory: Some(ui.settings.capture.captures_dir()),
        },
        wake,
    ));
}

/// The chooser's answer, polled from the window. True when the pane shows
/// something new.
pub(in crate::rune_ui) fn poll(ui: &mut Ui) -> bool {
    let Some(choice) = ui.capture_folder.as_ref().and_then(Pending::poll) else {
        return false;
    };
    ui.capture_folder = None;
    apply(ui, choice)
}

/// A pick is saved and clears the row's note, a cancel changes nothing, and
/// a chooser that couldn't open or a refused save reads under the row.
pub(in crate::rune_ui) fn apply(ui: &mut Ui, choice: Choice) -> bool {
    let error = match choice {
        Ok(Some(folder)) => {
            if ui.update(|settings| settings.capture.folder = Some(folder)) {
                None
            } else {
                ui.error.clone()
            }
        }
        Ok(None) => return false,
        Err(error) => Some(error),
    };
    if let Some(view) = ui.platform.scrawl.as_mut() {
        view.folder_error = error;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::AppSettings;
    use crate::rune_ui::Service;
    use crate::rune_ui::platform::ScrawlView;

    /// A pick is saved as `capture.folder`, a cancel changes nothing, and a
    /// chooser that couldn't open says why under the row. The dialog itself
    /// never opens here: a test would wait on a real chooser.
    #[test]
    fn a_chosen_captures_folder_is_saved_and_a_failed_chooser_says_why() {
        let mut ui = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
        ui.platform.scrawl = Some(ScrawlView::default());
        let picked = std::env::temp_dir().join("hex-annotations");

        assert!(apply(&mut ui, Ok(Some(picked.clone()))));
        assert_eq!(ui.settings.capture.folder, Some(picked.clone()));
        assert_eq!(ui.settings.capture.captures_dir(), picked);

        assert!(!apply(&mut ui, Ok(None)), "a cancel changes nothing");
        assert_eq!(ui.settings.capture.folder, Some(picked.clone()));

        assert!(apply(&mut ui, Err("no file chooser portal".into())));
        let view = ui.platform.scrawl.as_ref().expect("the pane's state");
        assert_eq!(view.folder_error.as_deref(), Some("no file chooser portal"));
        assert_eq!(ui.settings.capture.folder, Some(picked.clone()));

        assert!(apply(&mut ui, Ok(Some(picked))));
        let view = ui.platform.scrawl.as_ref().expect("the pane's state");
        assert_eq!(view.folder_error, None, "a saved pick clears the note");
    }
}
