//! The Modes editor's Neutral voice row (Labs › Neutral voice, macOS): a
//! mode may have its dictations rewritten into plain, neutral wording on this
//! Mac. Global is a normal mode here (the default mode), so it gets the row
//! like every other.

use rune::El;

use super::super::Ui;
use super::super::controls::{Paint, toggle_row};
use super::{mode, update_mode};
use crate::desktop::i18n::tr;

/// The row while the lab is on, macOS has it, and a mode is selected.
pub(super) fn row(ui: &Ui, p: Paint) -> Option<El> {
    if !crate::labs::neutral_voice() {
        return None;
    }
    let enabled = mode(&ui.settings, ui.modes.selected)?.neutral_voice;
    Some(toggle_row(
        p,
        "neutral-voice",
        tr("Neutral voice"),
        None,
        enabled,
        |ui: &mut Ui| {
            let Some(current) = mode(&ui.settings, ui.modes.selected) else {
                return;
            };
            let enabled = !current.neutral_voice;
            update_mode(ui, |mode| mode.neutral_voice = enabled);
        },
    ))
}
