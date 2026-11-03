//! What each lab is about: its category (the store's chips and icon
//! colours) and where it shows up once on.

use rune::{Color, Icon};

use super::super::Pane;
use super::super::controls::Paint;
use super::super::icons::{KEYBOARD, MIC, PASTE, SPARKLES, ZAP};
use super::Lab;
use crate::desktop::i18n::tr;
use crate::labs::Feature;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::rune_ui) enum Category {
    Dictation,
    Shortcuts,
    Clipboard,
    Meetings,
    Models,
    App,
}

impl Category {
    pub(in crate::rune_ui) const ALL: [Self; 6] = [
        Self::Dictation,
        Self::Shortcuts,
        Self::Clipboard,
        Self::Meetings,
        Self::Models,
        Self::App,
    ];

    /// Its id in `hex://labs?category=<id>`.
    pub(in crate::rune_ui) const fn id(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Shortcuts => "shortcuts",
            Self::Clipboard => "clipboard",
            Self::Meetings => "meetings",
            Self::Models => "models",
            Self::App => "app",
        }
    }

    pub(in crate::rune_ui) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|category| category.id() == id)
    }

    pub(in crate::rune_ui) fn name(self) -> &'static str {
        match self {
            Self::Dictation => tr("Dictation"),
            Self::Shortcuts => tr("Shortcuts"),
            Self::Clipboard => tr("Clipboard"),
            Self::Meetings => tr("Meetings"),
            Self::Models => tr("Models"),
            Self::App => tr("App"),
        }
    }

    /// The icon of a lab with none of its own.
    pub(super) fn icon(self) -> &'static Icon {
        match self {
            Self::Dictation => &MIC,
            Self::Shortcuts => &KEYBOARD,
            Self::Clipboard => &PASTE,
            Self::Meetings => Pane::Meetings.icon(),
            Self::Models => &ZAP,
            Self::App => &SPARKLES,
        }
    }

    /// The theme's colour scale its icon tiles take.
    const fn scale(self) -> &'static str {
        match self {
            Self::Dictation => "cobalt",
            Self::Shortcuts => "lilac",
            Self::Clipboard => "mint",
            Self::Meetings => "amber",
            Self::Models => "coral",
            Self::App => "smoke",
        }
    }

    /// An icon tile's fill, edge, and glyph: a tint of the category's scale,
    /// so every hue reads in both themes.
    pub(super) fn tile(self, p: Paint) -> (Color, Color, Color) {
        let step = |step| rune_theme_opencode::scale(self.scale(), p.k.dark, step);
        (step(4), step(6), step(11))
    }

    /// A featured card's wash and edge.
    pub(super) fn wash(self, p: Paint) -> (Color, Color) {
        let step = |step| rune_theme_opencode::scale(self.scale(), p.k.dark, step);
        (step(2), step(5))
    }

    pub(super) fn of(lab: Lab) -> Self {
        let feature = match lab {
            Lab::Feature(feature) => feature,
            #[cfg(hex_capture)]
            Lab::Annotate => return Self::App,
        };
        match feature {
            Feature::Meetings
            | Feature::LiveChapters
            | Feature::SubtitleExport
            | Feature::StopWhenCallEnds
            | Feature::MovableMeetingCapsules
            | Feature::MeetingSummaryModel
            | Feature::MeetingNotesVocabulary => Self::Meetings,
            Feature::LiveText
            | Feature::SmartSpacing
            | Feature::SpokenFormatting
            | Feature::SpokenSymbols
            | Feature::FillerWords
            | Feature::VocabularyBiasing
            | Feature::PauseBrowser
            | Feature::KeepRecordings
            | Feature::OutputFixes
            | Feature::HistoryStats
            | Feature::SkipPauses => Self::Dictation,
            Feature::MouseShortcuts
            | Feature::SecondDictationShortcut
            | Feature::SidedChords
            | Feature::TapOrHold
            | Feature::RestoreShortcutDefault
            | Feature::StandaloneSpecialKeys
            | Feature::SectionKeyAlone
            | Feature::ShareShortcut => Self::Shortcuts,
            Feature::KeepOnClipboard
            | Feature::CopyOnly
            | Feature::RecentDictations
            | Feature::GnomeClipboardRestore
            | Feature::TerminalPaste => Self::Clipboard,
            Feature::KeyboardModelSwitching
            | Feature::UnloadIdle
            | Feature::PickerSortFilter
            | Feature::BenchmarkDetails
            | Feature::PickerActiveFirst
            | Feature::ModelInPlace
            | Feature::PickerDownloaded
            | Feature::ParakeetUltra => Self::Models,
            Feature::HideMenuBarIcon | Feature::VoiceFilter | Feature::Playground => Self::App,
        }
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
            | Feature::MeetingNotesVocabulary => Self::Pane(Pane::Meetings),
            Feature::StopWhenCallEnds | Feature::MovableMeetingCapsules => {
                Self::Elsewhere(tr("Meeting capsules"))
            }
            // A command, so never translated.
            Feature::SubtitleExport => Self::Elsewhere("hex meetings show"),
            Feature::LiveText => Self::Elsewhere(tr("Dictation indicator")),
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
