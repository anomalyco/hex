//! Windows' own frame pacing for the HUD: Rune's Windows overlays have no
//! display link and no swap-chain wait, so Hex asks for one frame at a time on
//! a fixed ~60 Hz timer while the HUD moves and is still drawing.
//!
//! Rune takes an overlay's drawable through wgpu's DX12 swap chain made
//! `DontWait`, and no display link paces it there (a window has its own
//! swap-chain slot wait; an overlay has none), so an unpaced overlay draws,
//! submits and presents as fast as the loop turns. This is Hex's workaround:
//! ask for one frame each `HUD_FRAME`, a fixed ~60 Hz timer rather than the
//! display's refresh, and report the HUD through `HudCompositor::animating` as
//! not display-paced. Suspected, not measured on Windows: presenting faster
//! than the GPU retires frames keeps their staging chunks and D3D12 allocators
//! alive, and a hide that waits on the present queue stalls the window.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rune::Tx;

use super::{Hud, HudEvent, RESHOW_AFTER, wants_frame};

/// How often the Windows HUD asks Rune for its next frame while it is armed: a
/// fixed ~60 Hz timer, not the display's refresh. Rune floors a poll's timer to
/// a frame interval (`TIMER_FRAME`), so it asks no faster than that.
#[cfg(any(target_os = "windows", test))]
pub(super) const HUD_FRAME: Duration = Duration::from_micros(16_667);

/// Whether the timer should ask for another frame: the HUD moves on screen, and
/// either a frame drew since the last poll, which proves the next frame draws
/// too, or `last_drawn` is recent enough that it will. Rune runs a poll before
/// the frame's render, so without the first check a stall of `RESHOW_AFTER`
/// would disarm the timer on the very frame that just drew: Transcribing and
/// Completed send no meters to restart it, and the HUD would freeze on screen.
#[cfg(any(target_os = "windows", test))]
pub(super) fn should_arm(
    wants: bool,
    last_drawn: Option<Instant>,
    polled_at: Option<Instant>,
    now: Instant,
) -> bool {
    wants
        && last_drawn.is_some_and(|at| {
            polled_at.is_none_or(|polled| at > polled)
                || now.saturating_duration_since(at) < RESHOW_AFTER
        })
}

/// The step into `IndicatorModel` for a frame `now` on Windows: Rune gives a
/// frame's running motion the real gap but caps what it hands over as `dt` at
/// one 120 Hz resume step (`RESUME_STEP`), so `Frame::dt` is not the gap
/// between the HUD's own timer frames. The model takes the gap since its last
/// stepped frame instead; `fallback` is `Frame::dt` before any frame stepped.
#[cfg(target_os = "windows")]
pub(super) fn wall_dt(stepped_at: Option<Instant>, now: Instant, fallback: f32) -> f32 {
    stepped_at.map_or(fallback, |at| {
        now.saturating_duration_since(at).as_secs_f32()
    })
}

/// A Windows meter's update: draw only while the HUD's timer isn't armed, since
/// the timer's next frame shows the level and the view reads no loudness. The
/// event is handled either way; a frame this draws lets the next frame's poll
/// arm the timer again, so a stalled timer starts again from one meter.
#[cfg(any(target_os = "windows", test))]
pub(super) fn meter_update(hud: &mut Hud, event: HudEvent) -> bool {
    let draws = !hud.armed;
    hud.handle(event);
    draws
}

/// Rune's poll for the Windows HUD: arm the next frame while the HUD is moving
/// and drawing, and ask for nothing otherwise, so a show nothing draws never
/// turns the loop at 60 Hz with Hex idle. The frame's own render follows this
/// pass, so `last_drawn` is the previous drawn frame's (the one this poll's own
/// frame drew lands after it); `polled_at` tells them apart. Nothing the view
/// reads changed, so the frame needs no new commit.
#[cfg(any(target_os = "windows", test))]
pub(super) fn poll(hud: &mut Hud, _now: f64, tx: &mut Tx) -> bool {
    let wants = wants_frame(
        hud.animating.load(Ordering::Acquire),
        hud.shown.load(Ordering::Acquire),
    );
    let now = Instant::now();
    hud.armed = should_arm(wants, hud.last_drawn, hud.polled_at, now);
    hud.polled_at = Some(now);
    if hud.armed {
        tx.wake_after(HUD_FRAME);
    }
    false
}
