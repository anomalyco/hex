//! Explicit drawing state machine for Annotate.
//!
//! One key does everything: a tap starts a recording and a tap stops it; held
//! past [`DRAW_HOLD`] while recording, the pointer draws without a click until
//! the key comes up. The capsule's pen turns drawing on until clicked again,
//! the key is tapped or held and let go, or Escape.
//!
//! Replaces the previous scattered `DRAW_HELD` / `PEN` / `PEN_DOWN` globals
//! (plus Linux's `draw_pending` loop flag) with one mutex-guarded [`Drawing`]
//! machine that resets whenever a capture starts, attaches, detaches, or ends.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use scrawl_capture::capture::InputEvent;
use scrawl_capture::hosted_input::{HostInput, Modifiers};

use super::{Tapped, ink_source, report_input, wants_input};
use crate::audio::CaptureInstant;

/// A press shorter than this is a tap; held longer while recording, it draws.
pub const DRAW_HOLD: Duration = Duration::from_millis(250);

/// What the Annotate key's release while recording was.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyUp {
    /// Not pressed while recording: nothing.
    Unpressed,
    /// Let go before [`DRAW_HOLD`]: the tap that stops the recording.
    Tap,
    /// Held to draw: drawing ends (the pen too) and the recording goes on.
    Drew,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Mode {
    #[default]
    Off,
    /// `hex annotate draw` started a recording and wants the pen on as soon as
    /// the engine attaches its input sink.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    PendingPen,
    /// The capsule's pen is on; ink flows only while `button_down` is true.
    Pen { button_down: bool },
    /// The Annotate key went down at `since` while recording.
    ///
    /// `pen_before` carries `Some(button_down)` when the capsule pen was
    /// already active when the key went down, so `drawing()` stays true during
    /// the initial [`DRAW_HOLD`] window and releasing after [`DRAW_HOLD`] turns
    /// the pen off too.
    KeyDown {
        since: Instant,
        pen_before: Option<bool>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PenTransition {
    changed: bool,
    on: bool,
    end_stroke: bool,
}

/// Pure, clock-injected drawing state machine.
#[derive(Default)]
struct Drawing {
    mode: Mode,
}

impl Drawing {
    const fn new() -> Self {
        Self { mode: Mode::Off }
    }

    fn drawing(&self, armed: bool, now: Instant) -> bool {
        if !armed {
            return false;
        }
        match self.mode {
            Mode::Off | Mode::PendingPen => false,
            Mode::Pen { .. } => true,
            Mode::KeyDown { since, pen_before } => {
                pen_before.is_some() || now.saturating_duration_since(since) >= DRAW_HOLD
            }
        }
    }

    fn inking(&self, armed: bool, now: Instant) -> bool {
        if !armed {
            return false;
        }
        match self.mode {
            Mode::Off | Mode::PendingPen => false,
            Mode::Pen { button_down } => button_down,
            Mode::KeyDown { since, pen_before } => {
                now.saturating_duration_since(since) >= DRAW_HOLD || pen_before == Some(true)
            }
        }
    }

    /// Mouse button down or up while drawing. Only pen mode tracks button
    /// state; clicks during a held-key draw cannot leave a stale button flag.
    /// Returns `true` when a pen stroke just ended.
    fn pen_button(&mut self, down: bool, armed: bool, now: Instant) -> bool {
        match &mut self.mode {
            Mode::Pen { button_down } => {
                let was = *button_down;
                *button_down = down && armed;
                was && !down
            }
            Mode::KeyDown {
                since,
                pen_before: Some(button_down),
            } => {
                let held = now.saturating_duration_since(*since) >= DRAW_HOLD;
                let was = *button_down;
                *button_down = down && armed && !held;
                was && !down && !held
            }
            Mode::Off
            | Mode::PendingPen
            | Mode::KeyDown {
                pen_before: None, ..
            } => false,
        }
    }

    fn key_down(&mut self, now: Instant) {
        self.mode = match self.mode {
            Mode::Off | Mode::PendingPen => Mode::KeyDown {
                since: now,
                pen_before: None,
            },
            Mode::Pen { button_down } => Mode::KeyDown {
                since: now,
                pen_before: Some(button_down),
            },
            // Autorepeat or duplicate down keeps the original press instant.
            already @ Mode::KeyDown { .. } => already,
        };
    }

    /// Returns the release classification and whether a stroke should end.
    fn key_up(&mut self, now: Instant) -> (KeyUp, bool) {
        let Mode::KeyDown { since, pen_before } = self.mode else {
            return (KeyUp::Unpressed, false);
        };
        if now.saturating_duration_since(since) < DRAW_HOLD {
            self.mode = match pen_before {
                Some(button_down) => Mode::Pen { button_down },
                None => Mode::Off,
            };
            return (KeyUp::Tap, false);
        }
        self.mode = Mode::Off;
        (KeyUp::Drew, true)
    }

    fn pen_is_on(&self) -> bool {
        matches!(
            self.mode,
            Mode::Pen { .. }
                | Mode::KeyDown {
                    pen_before: Some(_),
                    ..
                }
        )
    }

    fn set_pen(&mut self, on: bool, armed: bool) -> PenTransition {
        if !armed {
            if matches!(self.mode, Mode::Pen { .. }) {
                self.mode = Mode::Off;
            }
            return PenTransition {
                changed: false,
                on: false,
                end_stroke: false,
            };
        }
        match (&mut self.mode, on) {
            (Mode::Pen { button_down }, true) => {
                *button_down = false;
                PenTransition {
                    changed: false,
                    on: true,
                    end_stroke: false,
                }
            }
            (Mode::KeyDown { pen_before, .. }, true) => {
                let was = pen_before.is_some();
                *pen_before = Some(false);
                PenTransition {
                    changed: !was,
                    on: true,
                    end_stroke: false,
                }
            }
            (Mode::Off | Mode::PendingPen, true) => {
                self.mode = Mode::Pen { button_down: false };
                PenTransition {
                    changed: true,
                    on: true,
                    end_stroke: false,
                }
            }
            (Mode::Pen { .. }, false) => {
                self.mode = Mode::Off;
                PenTransition {
                    changed: true,
                    on: false,
                    end_stroke: true,
                }
            }
            (Mode::KeyDown { pen_before, .. }, false) => {
                let was = pen_before.take().is_some();
                PenTransition {
                    changed: was,
                    on: false,
                    end_stroke: was,
                }
            }
            (Mode::Off | Mode::PendingPen, false) => PenTransition {
                changed: false,
                on: false,
                end_stroke: false,
            },
        }
    }

    fn exit(&mut self, armed: bool, now: Instant) -> bool {
        let drew = self.drawing(armed, now);
        self.mode = Mode::Off;
        drew
    }

    fn reset(&mut self) {
        self.mode = Mode::Off;
    }

    /// Called when the capture engine attaches its input sink. Returns `true`
    /// if a pending `hex annotate draw` pen just turned on.
    fn on_attach(&mut self) -> bool {
        if matches!(self.mode, Mode::PendingPen) {
            self.mode = Mode::Pen { button_down: false };
            true
        } else {
            self.mode = Mode::Off;
            false
        }
    }

    /// Requests the pen once the starting capture attaches, or turns it on at
    /// once if the capture is already attached.
    #[cfg(any(target_os = "linux", test))]
    fn arm_pen_on_attach(&mut self, armed: bool) -> bool {
        if armed {
            self.set_pen(true, true).changed
        } else {
            self.mode = Mode::PendingPen;
            false
        }
    }

    #[cfg(test)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn age_key_down(&mut self, by: Duration) {
        if let Mode::KeyDown { since, .. } = &mut self.mode {
            *since = Instant::now() - by;
        }
    }
}

static STATE: Mutex<Drawing> = Mutex::new(Drawing::new());

fn state() -> std::sync::MutexGuard<'static, Drawing> {
    STATE.lock().unwrap_or_else(|error| error.into_inner())
}

/// Whether the overlay takes the pointer now: only while a capture records,
/// with the pen on or the Annotate key held past [`DRAW_HOLD`].
pub fn drawing() -> bool {
    state().drawing(wants_input(), Instant::now())
}

/// Whether a pointer move lays ink now: the held key draws as the pointer
/// moves, with no click; the pen, like a real one, only while the button is
/// down.
pub(super) fn inking() -> bool {
    state().inking(wants_input(), Instant::now())
}

/// The mouse button went down (true) or up while the pen is on. Letting go
/// ends the stroke.
pub fn pen_button(down: bool) {
    let ended = {
        let mut s = state();
        s.pen_button(down, wants_input(), Instant::now())
    };
    if ended {
        end_stroke();
    }
}

/// The Annotate key went down while a capture records.
pub fn annotate_key_down() {
    state().key_down(Instant::now());
}

/// The Annotate key came up.
pub fn annotate_key_up() -> KeyUp {
    let (up, ended) = {
        let mut s = state();
        s.key_up(Instant::now())
    };
    if ended {
        end_stroke();
    }
    up
}

/// The capsule's pen: drawing on until it's clicked again.
pub fn toggle_pen() {
    let transition = {
        let mut s = state();
        let on = !s.pen_is_on();
        s.set_pen(on, wants_input())
    };
    if transition.changed {
        tracing::info!(on = transition.on, "Annotate pen");
        if transition.end_stroke {
            end_stroke();
        }
    }
}

/// Escape: stops drawing, never the recording. True when something drew.
pub fn exit_drawing() -> bool {
    let drew = {
        let mut s = state();
        s.exit(wants_input(), Instant::now())
    };
    if drew {
        end_stroke();
    }
    drew
}

/// Forgets the key, pending pen, and active pen: called when a capture starts,
/// ends, or detaches so stale presses never survive into the next capture.
pub(super) fn reset() {
    state().reset();
}

/// Called by [`super::LiveInput::attach`] once `TAP_RECORDING` is set: starts
/// clean, or activates a pen armed by `hex annotate draw`.
pub(super) fn on_attach() {
    if state().on_attach() {
        tracing::info!(on = true, "Annotate pen");
    }
}

/// Arms the pen for a recording started by `hex annotate draw`, or turns it on
/// immediately if the engine already attached.
#[cfg(target_os = "linux")]
pub(super) fn arm_pen_on_attach() {
    if state().arm_pen_on_attach(wants_input()) {
        tracing::info!(on = true, "Annotate pen");
    }
}

#[cfg(test)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn age_key_down_for_test(by: Duration) {
    state().age_key_down(by);
}

/// Ends a stroke under way where the pointer is, as letting go of Shift does.
pub(super) fn end_stroke() {
    let [x, y] = ink_source()
        .map(|source| {
            *source
                .telemetry
                .pointer_screen
                .lock()
                .unwrap_or_else(|error| error.into_inner())
        })
        .unwrap_or_default();
    report_input(
        CaptureInstant::now(),
        Tapped::Pointer(HostInput::Moved {
            x: f64::from(x),
            y: f64::from(y),
            modifiers: Modifiers::default(),
        }),
    );
}

/// Paste Last's picker open over the capture: its clicks, scrolls, and keys
/// are Hex's, so they choose a row and never mark the recording, or ink it
/// while drawing (macOS, the only picker).
#[derive(Default)]
pub(super) struct Picker(#[cfg(target_os = "macos")] crate::paste::browse::Over);

impl Picker {
    /// `event` as the capture records it, drawn with the pen while drawing;
    /// None for what the picker takes, asked before the pen can press.
    pub(super) fn pen(&mut self, event: InputEvent) -> Option<InputEvent> {
        #[cfg(target_os = "macos")]
        if self.0.takes(&event) {
            return None;
        }
        Some(as_pen(event))
    }
}

/// While drawing, every pointer event draws as if Shift alone were held,
/// whatever the keys (the Annotate key's own modifiers included).
pub(super) fn as_pen(event: InputEvent) -> InputEvent {
    if !drawing() {
        return event;
    }
    // The pen draws between the button's press and release; a press or
    // release is never the app's click while drawing.
    match event {
        InputEvent::ButtonDown { right: false, .. } => pen_button(true),
        InputEvent::ButtonUp { right: false, .. } => pen_button(false),
        _ => {}
    }
    match event {
        InputEvent::Move { t_ms, x, y, .. } => InputEvent::Move {
            t_ms,
            x,
            y,
            shift_only: inking(),
        },
        InputEvent::ButtonDown {
            t_ms, x, y, right, ..
        } => InputEvent::ButtonDown {
            t_ms,
            x,
            y,
            right,
            shift_only: true,
        },
        InputEvent::ButtonUp {
            t_ms, x, y, right, ..
        } => InputEvent::ButtonUp {
            t_ms,
            x,
            y,
            right,
            shift_only: true,
        },
        event => event,
    }
}

/// The ink overlay's own space on Linux: the window its pointer moves in
/// and the points its strokes are drawn in.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkSpace {
    /// The overlay is the XWayland window a compositor without layer-shell
    /// gets (GNOME).
    pub xwayland: bool,
    /// The overlay's size in its own points, as the view laid it out, once
    /// known (`Cx::layout_size`).
    pub viewport: Option<[f32; 2]>,
}

/// The recorded screen in the ink overlay's points, so strokes land where
/// they were drawn.
#[cfg(target_os = "linux")]
pub(super) fn ink_points(
    pixels: (u32, u32),
    scale: f32,
    logical: Option<(f64, f64)>,
    space: InkSpace,
) -> [f32; 2] {
    let in_points = [pixels.0 as f32 / scale, pixels.1 as f32 / scale];
    if space.xwayland {
        return space
            .viewport
            .filter(|[width, height]| *width > 0.0 && *height > 0.0)
            .unwrap_or(in_points);
    }
    logical.map_or(in_points, |(width, height)| [width as f32, height as f32])
}

/// A pointer move on the drawing overlay, at `x`, `y` in its points;
/// `scale` is its pixels per point, and `space` is where it lays out
/// (Linux). The capture's target becomes the recorded screen in that same
/// space, so strokes land where they were drawn ([`ink_points`]).
#[cfg(target_os = "linux")]
pub fn draw(x: f32, y: f32, scale: f32, space: InkSpace) {
    if !drawing() {
        return;
    }
    if let Some((width, height)) = scrawl_capture::capture::recorded_screen()
        && scale > 0.0
        && let Some(source) = ink_source()
    {
        let logical = crate::linux::output::pen_points()
            .or_else(scrawl_capture::capture::recorded_screen_points);
        let area = ink_points((width, height), scale, logical, space);
        let mut frame = source
            .telemetry
            .target_frame
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        frame.x = 0.0;
        frame.y = 0.0;
        frame.width = f64::from(area[0]);
        frame.height = f64::from(area[1]);
    }
    report_input(
        CaptureInstant::now(),
        Tapped::Pointer(HostInput::Moved {
            x: f64::from(x),
            y: f64::from(y),
            modifiers: Modifiers {
                shift: inking(),
                ..Modifiers::default()
            },
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_end_while_key_held_resets_so_next_tap_stops() {
        let t0 = Instant::now();
        let mut d = Drawing::new();

        // Capture 1: key held down past DRAW_HOLD, then capture ends without key_up.
        d.key_down(t0);
        let t_held = t0 + 2 * DRAW_HOLD;
        assert!(d.drawing(true, t_held));
        assert!(d.inking(true, t_held));
        d.reset();
        assert!(!d.drawing(true, t_held));
        assert!(!d.inking(true, t_held));

        // Capture 2: a fresh tap must classify as Tap (stops recording), never Drew.
        let t_next = t_held + Duration::from_secs(1);
        d.key_down(t_next);
        assert_eq!(
            d.key_up(t_next + Duration::from_millis(40)),
            (KeyUp::Tap, false)
        );
    }

    #[test]
    fn mouse_button_during_key_hold_never_leaves_stale_pen_button() {
        let t0 = Instant::now();
        let mut d = Drawing::new();

        d.key_down(t0);
        let t_held = t0 + DRAW_HOLD;
        assert!(d.drawing(true, t_held));
        assert!(d.inking(true, t_held));
        // Mouse press/release during a held-key draw with pen off is a no-op.
        assert!(!d.pen_button(true, true, t_held));
        assert!(!d.pen_button(false, true, t_held));
        assert_eq!(
            d.key_up(t_held + Duration::from_millis(10)),
            (KeyUp::Drew, true)
        );

        // Turning the pen on next starts with button up (not inking yet).
        assert!(d.set_pen(true, true).changed);
        assert!(d.drawing(true, t_held));
        assert!(!d.inking(true, t_held));
    }

    #[test]
    fn pen_mode_inks_only_while_button_down_and_ends_stroke_on_release() {
        let now = Instant::now();
        let mut d = Drawing::new();

        assert!(d.set_pen(true, true).changed);
        assert!(d.drawing(true, now));
        assert!(!d.inking(true, now));

        assert!(!d.pen_button(true, true, now));
        assert!(d.inking(true, now));

        assert!(d.pen_button(false, true, now));
        assert!(!d.inking(true, now));
        assert!(d.drawing(true, now));

        // Holding the key past DRAW_HOLD and letting go turns off the pen too.
        d.key_down(now);
        assert!(d.drawing(true, now + Duration::from_millis(20)));
        assert_eq!(d.key_up(now + DRAW_HOLD), (KeyUp::Drew, true));
        assert!(!d.drawing(true, now + DRAW_HOLD));
    }

    #[test]
    fn escape_exits_drawing_and_reports_whether_anything_drew() {
        let t0 = Instant::now();
        let mut d = Drawing::new();
        assert!(!d.exit(true, t0));

        // Key down before DRAW_HOLD is not yet drawing.
        d.key_down(t0);
        assert!(!d.exit(true, t0 + Duration::from_millis(50)));
        assert_eq!(d.key_up(t0 + DRAW_HOLD), (KeyUp::Unpressed, false));

        // Key held past DRAW_HOLD is drawing: Escape exits and returns true.
        d.key_down(t0);
        assert!(d.exit(true, t0 + DRAW_HOLD));
        assert!(!d.drawing(true, t0 + DRAW_HOLD));

        // Pen on: Escape exits and returns true.
        d.set_pen(true, true);
        assert!(d.exit(true, t0 + DRAW_HOLD));
        assert!(!d.drawing(true, t0 + DRAW_HOLD));
    }

    #[test]
    fn pending_pen_activates_on_attach_and_clears_on_reset() {
        let now = Instant::now();
        let mut d = Drawing::new();

        assert!(!d.arm_pen_on_attach(false));
        assert!(!d.drawing(false, now));
        assert!(d.on_attach());
        assert!(d.drawing(true, now));
        assert!(!d.inking(true, now));

        // Failed start resets PendingPen so a later capture doesn't inherit it.
        d.reset();
        assert!(!d.arm_pen_on_attach(false));
        d.reset();
        assert!(!d.on_attach());
        assert!(!d.drawing(true, now));
    }
}
