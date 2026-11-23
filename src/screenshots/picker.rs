//! Unified state machine for which Screenshots overlay (picker or recording
//! toolbar) is asking for permission, loading, or shown on screen.
//!
//! Picker and toolbar are mutually exclusive, and every asynchronous
//! permission or device-enumeration step carries a monotonic token so a
//! cancelled or superseded open can never show or accept Return/Space/clicks.

use crate::rune_ui::capture_capsule::flow::Mode;

/// Why the picker was opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Purpose {
    Shot,
    Record,
}

/// Whether the picker is selecting an area or a window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PickMode {
    Area,
    Window,
}

impl PickMode {
    pub(super) fn flipped(self) -> Self {
        match self {
            Self::Area => Self::Window,
            Self::Window => Self::Area,
        }
    }

    pub(super) fn into_mode(self) -> Mode {
        match self {
            Self::Area => Mode::Area,
            Self::Window => Mode::Window,
        }
    }
}

/// What a picker opening is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Intent {
    pub(super) purpose: Purpose,
    pub(super) mode: PickMode,
}

/// Result of resolving Screen Recording permission for a pending picker open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PermissionOutcome {
    /// The picker was cancelled or replaced before permission returned.
    Stale,
    /// Permission was refused for the active open; state is now closed.
    Denied,
    /// Permission succeeded; the picker is now ready on screen.
    Allowed(Intent),
}

/// Result of closing the recording toolbar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ToolbarClose {
    /// Neither loading nor shown.
    NotOpen,
    /// Cancelled while permission/devices were still loading; overlay was not shown.
    CancelledLoading,
    /// Closed a visible toolbar; caller must hide the overlay.
    ClosedShown,
}

/// What's open that takes keys in the session tap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Open {
    Nothing,
    /// Picker permission check is in flight; Escape cancels, Space passes.
    PickerAsking,
    /// Picker is visible on screen.
    Picker,
    /// Recording toolbar permission/devices are loading; Return is refused.
    ToolbarLoading,
    /// Recording toolbar is populated and visible on screen.
    Toolbar,
    /// A menu on the visible toolbar is open; Escape and Return belong to it.
    ToolbarMenu,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum State {
    #[default]
    Closed,
    PickerAsking {
        id: u64,
        intent: Intent,
    },
    PickerReady {
        id: u64,
        intent: Intent,
    },
    ToolbarLoading {
        id: u64,
    },
    ToolbarShown {
        id: u64,
    },
}

/// The single owner of whether the picker or recording toolbar is open.
#[derive(Default)]
pub(super) struct Front {
    next: u64,
    state: State,
}

impl Front {
    pub(super) const fn new() -> Self {
        Self {
            next: 0,
            state: State::Closed,
        }
    }

    pub(super) fn open(&self, menu_open: impl FnOnce() -> bool) -> Open {
        match self.state {
            State::Closed => Open::Nothing,
            State::PickerAsking { .. } => Open::PickerAsking,
            State::PickerReady { .. } => Open::Picker,
            State::ToolbarLoading { .. } => Open::ToolbarLoading,
            State::ToolbarShown { .. } => {
                if menu_open() {
                    Open::ToolbarMenu
                } else {
                    Open::Toolbar
                }
            }
        }
    }

    pub(super) fn begin_picker(&mut self, purpose: Purpose, mode: PickMode) -> Option<u64> {
        if self.state != State::Closed {
            return None;
        }
        self.next += 1;
        let id = self.next;
        self.state = State::PickerAsking {
            id,
            intent: Intent { purpose, mode },
        };
        Some(id)
    }

    pub(super) fn picker_permission(&mut self, id: u64, allowed: bool) -> PermissionOutcome {
        let State::PickerAsking {
            id: current,
            intent,
        } = self.state
        else {
            return PermissionOutcome::Stale;
        };
        if current != id {
            return PermissionOutcome::Stale;
        }
        if allowed {
            self.state = State::PickerReady { id, intent };
            PermissionOutcome::Allowed(intent)
        } else {
            self.state = State::Closed;
            PermissionOutcome::Denied
        }
    }

    pub(super) fn picker_current(&self, id: u64) -> bool {
        matches!(self.state, State::PickerReady { id: current, .. } if current == id)
    }

    pub(super) fn picker_toggle(&mut self) -> Option<(u64, PickMode)> {
        let State::PickerReady { id, intent } = &mut self.state else {
            return None;
        };
        intent.mode = intent.mode.flipped();
        Some((*id, intent.mode))
    }

    pub(super) fn picker_take(&mut self) -> Option<(u64, Purpose)> {
        let State::PickerReady { id, intent } = self.state else {
            return None;
        };
        self.state = State::Closed;
        Some((id, intent.purpose))
    }

    pub(super) fn cancel_picker(&mut self) -> Option<u64> {
        match self.state {
            State::PickerAsking { id, .. } | State::PickerReady { id, .. } => {
                self.state = State::Closed;
                Some(id)
            }
            _ => None,
        }
    }

    pub(super) fn begin_toolbar(&mut self) -> Option<u64> {
        if self.state != State::Closed {
            return None;
        }
        self.next += 1;
        let id = self.next;
        self.state = State::ToolbarLoading { id };
        Some(id)
    }

    /// Permission check failed or thread spawn failed while loading `id`.
    /// Returns `true` only if `id` was still the active loading open.
    pub(super) fn toolbar_permission_denied(&mut self, id: u64) -> bool {
        if matches!(self.state, State::ToolbarLoading { id: current } if current == id) {
            self.state = State::Closed;
            true
        } else {
            false
        }
    }

    /// Marks the toolbar shown on the main thread once populated. Returns
    /// `false` if this open was cancelled while loading.
    pub(super) fn toolbar_show(&mut self, id: u64) -> bool {
        if matches!(self.state, State::ToolbarLoading { id: current } if current == id) {
            self.state = State::ToolbarShown { id };
            true
        } else {
            false
        }
    }

    /// True only once the toolbar has passed permission, been populated, and
    /// is shown on screen.
    pub(super) fn toolbar_shown(&self) -> bool {
        matches!(self.state, State::ToolbarShown { .. })
    }

    pub(super) fn close_toolbar(&mut self) -> ToolbarClose {
        match self.state {
            State::ToolbarLoading { .. } => {
                self.state = State::Closed;
                ToolbarClose::CancelledLoading
            }
            State::ToolbarShown { .. } => {
                self.state = State::Closed;
                ToolbarClose::ClosedShown
            }
            _ => ToolbarClose::NotOpen,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_permission_never_reopens_or_changes_a_new_pick() {
        let mut front = Front::new();
        let old = front.begin_picker(Purpose::Record, PickMode::Area).unwrap();
        assert_eq!(front.cancel_picker(), Some(old));
        assert_eq!(front.picker_permission(old, true), PermissionOutcome::Stale);

        let new = front.begin_picker(Purpose::Shot, PickMode::Area).unwrap();
        assert_eq!(
            front.picker_permission(old, false),
            PermissionOutcome::Stale
        );
        assert_eq!(front.picker_permission(old, true), PermissionOutcome::Stale);
        assert_eq!(front.open(|| false), Open::PickerAsking);
        assert!(!front.picker_current(new));
        assert_eq!(front.picker_toggle(), None);
        assert_eq!(front.picker_take(), None);

        assert_eq!(
            front.picker_permission(new, true),
            PermissionOutcome::Allowed(Intent {
                purpose: Purpose::Shot,
                mode: PickMode::Area,
            })
        );
        assert!(front.picker_current(new));
        assert_eq!(front.open(|| false), Open::Picker);
        assert_eq!(front.picker_toggle(), Some((new, PickMode::Window)));
        assert_eq!(front.picker_take(), Some((new, Purpose::Shot)));
        assert_eq!(front.open(|| false), Open::Nothing);
    }

    #[test]
    fn toolbar_is_not_open_before_permission_and_show_succeed() {
        let mut front = Front::new();
        let first = front.begin_toolbar().unwrap();

        // While permission and devices are loading, toolbar_shown() is false
        // and Open reports ToolbarLoading (never Toolbar or ToolbarMenu).
        assert!(!front.toolbar_shown());
        assert_eq!(front.open(|| true), Open::ToolbarLoading);
        assert!(front.begin_picker(Purpose::Shot, PickMode::Area).is_none());

        // Cancelling before permission finishes invalidates the token.
        assert_eq!(front.close_toolbar(), ToolbarClose::CancelledLoading);
        assert_eq!(front.open(|| false), Open::Nothing);
        assert!(!front.toolbar_show(first));
        assert!(!front.toolbar_permission_denied(first));

        // Permission denial resets to Closed so another open can proceed.
        let denied = front.begin_toolbar().unwrap();
        assert!(front.toolbar_permission_denied(denied));
        assert!(!front.toolbar_shown());
        assert_eq!(front.open(|| false), Open::Nothing);

        // Only after toolbar_show succeeds does toolbar_shown() become true.
        let live = front.begin_toolbar().unwrap();
        assert!(front.toolbar_show(live));
        assert!(front.toolbar_shown());
        assert_eq!(front.open(|| false), Open::Toolbar);
        assert_eq!(front.open(|| true), Open::ToolbarMenu);
        assert_eq!(front.close_toolbar(), ToolbarClose::ClosedShown);
        assert!(!front.toolbar_shown());
        assert_eq!(front.open(|| false), Open::Nothing);
    }
}
