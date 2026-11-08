//! The recording capture's ink, kept for the ink overlay: where the strokes
//! and pointer are, and, under Labs › Persistent ink, the capture time
//! Escape last cleared them at.

use std::sync::Mutex;

use super::{InkSource, drawing};

/// The recording capture's [`InkSource`], kept apart from `LiveInput`'s
/// lock: the `capture-input` thread holds that lock while a pen release ends
/// its stroke (`super::end_stroke`), which reads the pointer from here, so
/// reading it through that lock deadlocked the thread and, with it, every
/// view that asks for the ink.
static INK: Mutex<Option<InkSource>> = Mutex::new(None);
/// The capture a clear belongs to (its start, so a new capture starts with
/// its ink) and the capture-relative time it cleared at.
static CLEARED: Mutex<Option<(u64, u64)>> = Mutex::new(None);

/// The host started (`Some`) or ended a capture.
pub(super) fn set(source: Option<InkSource>) {
    *INK.lock().unwrap_or_else(|error| error.into_inner()) = source;
}

/// The recording capture's ink, laser, and pointer, for the ink overlay to
/// draw: `None` until its engine started, and once it ended.
pub(crate) fn source() -> Option<InkSource> {
    INK.lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// The capture-relative time Escape last cleared `source`'s persistent ink
/// at: `None` for a capture nothing was cleared in.
pub(crate) fn cleared_at_ms(source: &InkSource) -> Option<u64> {
    cleared()
        .filter(|(capture, _)| *capture == source.telemetry.started_at_boot_ns())
        .map(|(_, at)| at)
}

fn cleared() -> Option<(u64, u64)> {
    *CLEARED.lock().unwrap_or_else(|error| error.into_inner())
}

/// Whether `source` shows lifted ink under Labs › Persistent ink: it has a
/// stroke whose end is at or after the last clear.
fn visible(source: &InkSource) -> bool {
    if !crate::labs::persistent_ink() {
        return false;
    }
    let cleared = cleared_at_ms(source);
    source
        .telemetry
        .newest_ink_end_ms()
        .is_some_and(|end| cleared.is_none_or(|cleared| end >= cleared))
}

/// Whether Escape belongs to Annotate: drawing, or persistent ink it would
/// clear. Escape stays with the app when neither is true.
pub(crate) fn escape_wanted() -> bool {
    drawing() || source().is_some_and(|source| visible(&source))
}

/// Escape's clear under Labs › Persistent ink: hides every stroke already on
/// screen. True when there was ink to clear.
pub(super) fn clear() -> bool {
    let Some(source) = source().filter(visible) else {
        return false;
    };
    let capture = source.telemetry.started_at_boot_ns();
    *CLEARED.lock().unwrap_or_else(|error| error.into_inner()) = Some((capture, source.now_ms()));
    true
}
