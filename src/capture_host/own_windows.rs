//! Hex's own windows: which a capture leaves out of its recording, and
//! which it looks through for clicks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use scrawl_capture::capture::OwnWindows;

/// Hex's windows a capture leaves out of its recording, one per role. A
/// capture reads them when it starts.
pub(super) static OWN_WINDOWS: Mutex<OwnWindowIds> = Mutex::new(OwnWindowIds {
    capsule: None,
    ink: None,
    picker: None,
    dims: [None; DIM_WINDOWS],
});

/// How many displays a Screenshots recording dims: one overlay each.
pub const DIM_WINDOWS: usize = 6;

/// The window holding each [`OwnWindow`] role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OwnWindowIds {
    pub(super) capsule: Option<u32>,
    pub(super) ink: Option<u32>,
    pub(super) picker: Option<u32>,
    pub(super) dims: [Option<u32>; DIM_WINDOWS],
}

impl OwnWindowIds {
    /// Every window held, which a recording leaves out.
    pub(super) fn excluded(self) -> Vec<u32> {
        let Self {
            capsule,
            ink,
            picker,
            dims,
        } = self;
        [capsule, ink, picker]
            .into_iter()
            .chain(dims)
            .flatten()
            .collect()
    }
}

/// Which of Hex's windows a capture leaves out.
#[derive(Clone, Copy, Debug)]
pub enum OwnWindow {
    /// A panel far larger than the pill: click-through but for the pill
    /// ([`super::set_capsule_pill`]).
    Capsule,
    /// Click-through, so a click on it never counts as a click on Hex.
    Ink,
    /// The on-screen picker, hidden before the capture it starts records.
    Picker,
    /// A Screenshots recording's dim over display `n`: click-through, so a
    /// click on it is a click on what it dims.
    Dim(usize),
}

/// Leaves `window` out of every later capture's recording, replacing the
/// window that last held `role`. Rune makes the overlays' windows for each
/// show (#1315), so one made while a capture records joins it too.
pub fn exclude_window(role: OwnWindow, window: u32) {
    let mut own = OWN_WINDOWS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *match role {
        OwnWindow::Capsule => &mut own.capsule,
        OwnWindow::Ink => &mut own.ink,
        OwnWindow::Picker => &mut own.picker,
        OwnWindow::Dim(n) => match own.dims.get_mut(n) {
            Some(dim) => dim,
            None => return,
        },
    } = Some(window);
    let joined = own_windows_of(&own);
    drop(own);
    let Some(source) = super::ink_source() else {
        return;
    };
    for (ids, list) in [
        (&*source.telemetry.excluded_window_ids, joined.excluded),
        (&source.telemetry.passthrough_window_ids, joined.passthrough),
    ] {
        let mut ids = ids.lock().unwrap_or_else(|error| error.into_inner());
        for id in list {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
}

/// What a capture leaves out of its recording (every window Hex named) and
/// looks through for clicks: the ink overlay, and the capsule's panel, which
/// lets clicks through everywhere but its pill. Clicks on the pill never
/// reach the engine ([`super::CapsulePill`]).
pub(super) fn own_windows_of(own: &OwnWindowIds) -> OwnWindows {
    OwnWindows {
        excluded: own.excluded(),
        passthrough: [own.ink, own.capsule]
            .into_iter()
            .chain(own.dims)
            .flatten()
            .collect(),
        recorded: Arc::clone(&OWN_WINDOWS_RECORDED),
    }
}

/// `capture.show_overlays_in_recordings`: while set, a capture records every
/// window of Hex's, its overlays included. A recording capture follows a
/// change at once.
static OWN_WINDOWS_RECORDED: LazyLock<Arc<AtomicBool>> = LazyLock::new(Arc::default);

/// Lets captures record Hex's own windows, or leaves them out again.
pub fn record_own_windows(recorded: bool) {
    OWN_WINDOWS_RECORDED.store(recorded, Ordering::Release);
}

/// Whether screen recordings may show Hex's overlays, as last saved.
pub fn own_windows_recorded() -> bool {
    OWN_WINDOWS_RECORDED.load(Ordering::Acquire)
}
