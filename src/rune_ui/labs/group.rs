//! Each lab's group: the store's headers, chips, and default icons.

use rune::Icon;

use super::super::icons::{CAMERA, KEYBOARD, MIC, PASTE, ZAP};
use super::super::{Pane, Ui};
use super::{Row, shown};
use crate::desktop::i18n::tr;
use crate::labs::Group;

/// A group's header and chip.
pub(super) fn name(group: Group) -> &'static str {
    match group {
        Group::Dictation => tr("Dictation"),
        Group::Output => tr("Output"),
        Group::History => tr("History"),
        Group::Meetings => tr("Meetings"),
        Group::Annotate => tr("Annotate"),
        Group::Shortcuts => tr("Shortcuts"),
        Group::Models => tr("Models"),
    }
}

/// Whether `group`'s cards show: as last opened or closed by hand, else
/// open while it holds a lab that's on.
pub(super) fn open(ui: &Ui, group: Group, rows: &[Row]) -> bool {
    ui.settings
        .labs
        .groups
        .get(group.id())
        .copied()
        .unwrap_or_else(|| rows.iter().any(|row| row.group == group && row.on))
}

/// Opens or closes `group`, remembered in settings.json.
pub(super) fn fold(ui: &mut Ui, group: Group) {
    let open = open(ui, group, &shown(ui));
    ui.update(|settings| {
        settings.labs.groups.insert(group.id().to_string(), !open);
    });
}

/// Before one of `group`'s labs turns off: remembers it open, so turning
/// off its last lab doesn't fold it away.
pub(super) fn keep_open(ui: &mut Ui, group: Group) {
    if ui.settings.labs.groups.contains_key(group.id()) || !open(ui, group, &shown(ui)) {
        return;
    }
    ui.update(|settings| {
        settings.labs.groups.insert(group.id().to_string(), true);
    });
}

/// The icon of a lab with none of its own.
pub(super) fn icon(group: Group) -> &'static Icon {
    match group {
        Group::Dictation => &MIC,
        Group::Output => &PASTE,
        Group::History => Pane::History.icon(),
        Group::Meetings => Pane::Meetings.icon(),
        Group::Annotate => &CAMERA,
        Group::Shortcuts => &KEYBOARD,
        Group::Models => &ZAP,
    }
}
