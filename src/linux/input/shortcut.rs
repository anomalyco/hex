//! The shortcuts a Linux backend reports, their stable portal ids, and the
//! dictation flavour each starts captures with.

use crate::dictation::DictationFlavour;

/// A shortcut a backend reports pressed or released.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Shortcut {
    Dictation,
    PasteLast,
    TypedText,
    Capture,
    RawDictation,
    TerminalPaste,
    /// Labs › Second dictation shortcut.
    SecondDictation,
    /// Labs › Model switch shortcut.
    ModelSwitch,
}

// `HeldKeys` keeps one bit per shortcut.
const _: () = assert!(Shortcut::ALL.len() <= u8::BITS as usize);

impl Shortcut {
    /// Every shortcut, in declaration order: `index` is its place here.
    pub(super) const ALL: [Self; 8] = [
        Self::Dictation,
        Self::PasteLast,
        Self::TypedText,
        Self::Capture,
        Self::RawDictation,
        Self::TerminalPaste,
        Self::SecondDictation,
        Self::ModelSwitch,
    ];

    pub(super) fn index(self) -> usize {
        self as usize
    }

    /// The dictation flavour a shortcut starts captures with; `None` for
    /// the shortcuts that start no dictation.
    pub(super) fn flavour(self) -> Option<DictationFlavour> {
        match self {
            Self::Dictation => Some(DictationFlavour::Normal),
            Self::RawDictation => Some(DictationFlavour::Raw),
            Self::TerminalPaste => Some(DictationFlavour::TerminalPaste),
            Self::SecondDictation => Some(DictationFlavour::Second),
            Self::PasteLast | Self::TypedText | Self::Capture | Self::ModelSwitch => None,
        }
    }

    /// The dictation shortcut that starts captures with `flavour`.
    pub(super) fn for_flavour(flavour: DictationFlavour) -> Self {
        match flavour {
            DictationFlavour::Normal => Self::Dictation,
            DictationFlavour::Raw => Self::RawDictation,
            DictationFlavour::TerminalPaste => Self::TerminalPaste,
            DictationFlavour::Second => Self::SecondDictation,
        }
    }

    /// The portal's stable id and the description desktops show.
    pub(super) fn portal_id(self) -> (&'static str, &'static str) {
        match self {
            Self::Dictation => ("dictation", "Dictation"),
            Self::PasteLast => ("paste-last", "Paste last dictation"),
            Self::TypedText => ("typed-text", "Transform typed text"),
            Self::Capture => ("capture", "Annotate"),
            Self::RawDictation => ("raw-dictation", "Dictate without processing"),
            Self::TerminalPaste => ("terminal-paste", "Terminal paste shortcut"),
            Self::SecondDictation => ("second-dictation", "Second dictation shortcut"),
            Self::ModelSwitch => ("model-switch", "Model switch shortcut"),
        }
    }
}
