//! The meeting capsules' points, springs, and times in one place: the
//! 14/6/8 grammar the Annotate capsule shares.

use std::time::Duration;

use rune::Spring;

use super::{auto_started, call_ended, waveform};
use crate::rune_ui::controls::CAPSULE;

/// The offer, call ended, and ready capsules.
pub(super) const MEETING_PANEL_WIDTH: f32 = 420.0;
/// The offer's "…" choices need room for three buttons beside the name.
#[cfg(test)]
pub(super) const MEETING_MENU_WIDTH: f32 = 560.0;
pub(super) const MEETING_PANEL_HEIGHT: f32 = 64.0;
/// The automatic-recording capsule hugs its two short lines and buttons.
pub(super) const AUTO_STARTED_PANEL_WIDTH: f32 = auto_started::PANEL_WIDTH;
/// Between a capsule's buttons and its right edge.
pub(super) const ACTIONS_INSET: f32 = CAPSULE.button_inset;
/// The reminder: the meeting's title beside up to three buttons.
pub(super) const REMINDER_PANEL_WIDTH: f32 = 520.0;
/// The call-ended capsule's width, beside Keep recording and Stop now /
/// Stop & transcribe (#835).
pub(super) const CALL_ENDED_PANEL_WIDTH: f32 = call_ended::PANEL_WIDTH;
/// How often an open reminder recounts "Starts in N min".
pub(super) const REMINDER_TICK: Duration = Duration::from_secs(5);
/// Between a capsule's window edges and what it draws.
pub(super) const CAPSULE_INSET: f32 = CAPSULE.window_inset;
/// Same header at rest and open so the waveform and timer stay put.
pub(super) const REST_HEIGHT: f32 = HEADER_HEIGHT;
pub(super) const REST_WIDTH: f32 = REST_PL + REST_PX + waveform::WIDTH + METER_GAP + TIMER_WIDTH;
/// The header's sides, at rest and open, so the timer stays put as the pill
/// opens: the inset the rows' labels, the title and the summary keep.
pub(super) const REST_PX: f32 = CAPSULE.text_rail();
/// The header's left side: the field's first column is a faint ghost, so it
/// tucks `FIELD_TUCK` nearer the wall and its first lit one stands 16.5 pt in,
/// as the time's last digit does from the right.
pub(super) const REST_PL: f32 = REST_PX - FIELD_TUCK;
pub(super) const FIELD_TUCK: f32 = 4.0;
/// No red dot before the time: the live waveform already says it records.
pub(super) const SHOWS_REC_DOT: bool = false;
/// The dot and its gap before the time, when it shows.
pub(super) const DOT_ROOM: f32 = if SHOWS_REC_DOT {
    REC_DOT + METER_GAP
} else {
    0.0
};
/// Between the listening field and the time.
pub(super) const METER_GAP: f32 = 6.0;
/// "00:00" in `Paint::clock()` (13 pt tabular medium), reserved as an exact
/// integer width so layout matches across macOS, Linux, and Windows.
pub(super) const TIMER_TEXT_WIDTH: f32 = 38.0;
/// `TIMER_TEXT_WIDTH` plus the recording dot and its gap before it.
pub(super) const TIMER_WIDTH: f32 = TIMER_TEXT_WIDTH + DOT_ROOM;
/// The recording dot before the elapsed time, a status dot's size.
pub(super) const REC_DOT: f32 = 8.0;
/// The open recording pill and the ready panel.
pub(super) const PANEL_WIDTH: f32 = CAPSULE.panel_w;
pub(super) const PANEL_RADIUS: f32 = CAPSULE.radius;
/// Around the open panel's rows.
pub(super) const PANEL_PAD: f32 = CAPSULE.row_inset;
pub(super) const HEADER_HEIGHT: f32 = CAPSULE.header_h;
pub(super) const ROW_HEIGHT: f32 = CAPSULE.row_h;
pub(super) const OPEN_HEADER: f32 = REST_HEIGHT + ROW_HEIGHT;
/// The two copies and the gap under the first.
pub(super) const COPIES_HEIGHT: f32 = 2.0 * ROW_HEIGHT + ROW_GAP;
/// Concentric with the panel across [`PANEL_PAD`].
pub(super) const ROW_RADIUS: f32 = CAPSULE.inner(CAPSULE.row_inset);
/// A row's icon and label inset; titles and summaries share it.
pub(super) const ROW_PX: f32 = CAPSULE.row_px;
/// Between two rows, which both fill under the pointer: under the first
/// copy, and between Stop and the copies.
pub(super) const ROW_GAP: f32 = CAPSULE.row_gap;
/// "Microphone is silent" or "Started automatically" under the header.
pub(super) const NOTICE_HEIGHT: f32 = 20.0;
/// The ready panel's summary: the notes writer's two sentences.
pub(super) const SUMMARY_LINES: usize = 3;
pub(super) const TITLE_LINES: usize = 3;
/// The title line's breathing room in a 32 pt action row, so a one-line
/// title's band is the row every other action takes and nothing under it
/// moves when Stop leaves.
pub(super) const TITLE_INSET: f32 = (ROW_HEIGHT - 20.) / 2.;
pub(super) const RESULT_HEADER_MAX: f32 = REST_HEIGHT + TITLE_LINES as f32 * 20. + 2. * TITLE_INSET;
pub(super) const SUMMARY_LINE: f32 = 19.5;
/// One transparent window with room for the finished panel's longest summary.
pub(super) const RECORDING_WINDOW: (f32, f32) = (
    PANEL_WIDTH + 2.0 * CAPSULE_INSET,
    RESULT_HEADER_MAX
        + SUMMARY_LINE * SUMMARY_LINES as f32
        + ROW_PX
        + ROW_GAP
        + ROW_GAP
        + COPIES_HEIGHT
        + PANEL_PAD
        + 2.0 * CAPSULE_INSET,
);
/// The finished meeting's title and summary grow the panel on this.
pub(super) const RECORDING_OPEN: Spring = Spring::new(0.42, 0.0);
/// A start this quick never says "Starting…": the offer contracts straight
/// into the live pill.
pub(super) const PENDING_LABEL_AFTER: f64 = 0.18;
/// How long a copy says "Copied".
pub(super) const COPIED_FEEDBACK: Duration = Duration::from_millis(1_500);
pub(super) const OFFER_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const READY_TIMEOUT: Duration = Duration::from_secs(15);
/// How long after an offer Linux checks that its overlay opened.
#[cfg(target_os = "linux")]
pub(super) const OFFER_CHECK: Duration = Duration::from_secs(2);
/// How long an automatic recording says so before showing its elapsed time.
pub(super) const AUTOMATIC_LABEL: Duration = Duration::from_secs(10);
pub(super) const WINDOW_MARGIN: f32 = 12.0;
/// Below the menu bar.
pub(super) const MENU_BAR: f32 = 28.0;
/// Above the Dock.
pub(super) const DOCK: f32 = 24.0;
