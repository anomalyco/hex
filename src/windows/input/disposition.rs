//! What each low-level hook decided for one event, and whether one of Hex's
//! own shortcuts caused it.

use super::Disposition;
use super::activity;

/// What a hook decided for one event, and whether one of Hex's own shortcut
/// triggers sent it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Handled {
    pub(super) disposition: Disposition,
    trigger: bool,
}

impl Handled {
    /// An event no shortcut claimed.
    pub(super) const PASS: Self = Self {
        disposition: Disposition::Pass,
        trigger: false,
    };

    /// An event one of Hex's shortcut triggers caused, whatever the hook did
    /// with it. Labs › Share shortcut lets the dictation trigger's key reach
    /// the app, and that press is Hex's, not the user's typing.
    pub(super) const fn own(disposition: Disposition) -> Self {
        Self {
            disposition,
            trigger: true,
        }
    }

    /// An event the hook decided for, `trigger` saying whether one of Hex's
    /// shortcut triggers caused it.
    pub(super) const fn new(disposition: Disposition, trigger: bool) -> Self {
        Self {
            disposition,
            trigger,
        }
    }

    /// Whether one of Hex's own shortcuts caused the event, even where the
    /// hook let its key reach the app (Labs › Share shortcut).
    pub(super) fn triggered(self) -> bool {
        self.trigger
    }

    /// Whether the event is the user's own typing or clicking, which ends a
    /// paste continuation: it reached the app in front, and no shortcut of
    /// Hex's triggered it.
    pub(super) fn ends_continuation(
        self,
        is_down: bool,
        modifier: bool,
        extra_info: usize,
    ) -> bool {
        self.disposition == Disposition::Pass
            && !self.trigger
            && activity::ends_continuation(is_down, modifier, extra_info)
    }
}
