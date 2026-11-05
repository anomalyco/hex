//! What each lab is about: its group (the store's headers, chips and
//! default icons) and where it shows up once on.

use rune::Icon;

use super::super::icons::{CAMERA, KEYBOARD, MIC, PASTE, ZAP};
use super::super::{Pane, Ui};
use super::{Lab, Row, shown};
use crate::desktop::i18n::tr;
use crate::labs::{Feature, Group};

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

/// Where a lab shows up once it is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Place {
    /// A pane, linked while the sidebar shows it.
    Pane(Pane),
    /// The playground window.
    Playground,
    /// Somewhere no link reaches, already translated.
    Elsewhere(&'static str),
}

impl Place {
    pub(super) fn of(lab: Lab) -> Self {
        let feature = match lab {
            Lab::Feature(feature) => feature,
            #[cfg(hex_capture)]
            Lab::Annotate => return Self::Pane(Pane::Annotate),
        };
        match feature {
            Feature::Meetings
            | Feature::LiveChapters
            | Feature::MeetingSummaryModel
            | Feature::MeetingNotesVocabulary
            | Feature::MeetingFinishedEvent
            | Feature::MeetingPlatformIcons
            | Feature::FollowLiveTranscript
            | Feature::CollapsibleMeetingSections => Self::Pane(Pane::Meetings),
            Feature::StopWhenCallEnds
            | Feature::MovableMeetingCapsules
            | Feature::GnomeMeetingCapsules => Self::Elsewhere(tr("Meeting capsules")),
            // A command, so never translated.
            Feature::SubtitleExport => Self::Elsewhere("hex meetings show"),
            Feature::LiveText | Feature::NotchIndicator => {
                Self::Elsewhere(tr("Dictation indicator"))
            }
            Feature::SmartSpacing
            | Feature::SpokenFormatting
            | Feature::SpokenSymbols
            | Feature::FillerWords
            | Feature::KeepOnClipboard
            | Feature::CopyOnly
            | Feature::SkipPauses
            | Feature::VocabularyBiasing
            | Feature::GnomeClipboardRestore
            | Feature::TerminalPaste => Self::Elsewhere(tr("Every dictation")),
            Feature::PauseBrowser
            | Feature::MouseShortcuts
            | Feature::SidedChords
            | Feature::StandaloneSpecialKeys
            | Feature::SectionKeyAlone
            | Feature::SecondDictationShortcut
            | Feature::TapOrHold
            | Feature::RestoreShortcutDefault
            | Feature::ShareShortcut => Self::Pane(Pane::General),
            Feature::KeepRecordings | Feature::HistoryStats => Self::Pane(Pane::History),
            Feature::OutputFixes => Self::Pane(Pane::Output),
            Feature::RecentDictations | Feature::HideMenuBarIcon => Self::Elsewhere(tr("Menu bar")),
            Feature::KeyboardModelSwitching
            | Feature::PickerSortFilter
            | Feature::BenchmarkDetails
            | Feature::PickerActiveFirst
            | Feature::ModelInPlace
            | Feature::PickerDownloaded
            | Feature::ParakeetUltra => Self::Elsewhere(tr("Model picker")),
            Feature::UnloadIdle => Self::Elsewhere(tr("In the background")),
            Feature::VoiceFilter => Self::Pane(Pane::VoiceFilter),
            Feature::Playground => Self::Playground,
            Feature::Screenshots => Self::Elsewhere(tr("Screenshot shortcut")),
            #[cfg(hex_capture)]
            Feature::AnnotateReplay => Self::Pane(Pane::Annotate),
            #[cfg(not(hex_capture))]
            Feature::AnnotateReplay => Self::Pane(Pane::General),
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Pane(pane) => pane.title(),
            Self::Playground => "Playground",
            Self::Elsewhere(label) => label,
        }
    }
}
