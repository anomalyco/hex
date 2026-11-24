//! Where a capsule's window rests, and the drag that moves it: the corner
//! placement every capsule shares, and a release that lands in a corner of the
//! display under the pointer while Labs › Indicator on any display is on.

use super::*;

impl Capsule {
    /// The capsule follows the pointer one to one. On release it glides
    /// from where it was let go, at the throw's speed, into the corner the
    /// throw heads for (under Reduce Motion it jumps there). A cancelled
    /// drag (Escape, window blur) returns to the corner it started from and
    /// saves nothing.
    pub(super) fn drag(&mut self, drag: &Drag, tx: &mut Tx) {
        let Some(shift) = self.shift.clone() else {
            return;
        };
        let now = tx.clock();
        match drag.phase {
            Phase::Down => self.throw.press(now),
            Phase::Move => {
                let (x, y) = self.throw.track(now, drag.dx, drag.dy);
                shift(self.corner, x, y);
            }
            Phase::Up if drag.cancelled() => {
                self.throw.stop();
                shift(self.corner, 0.0, 0.0);
            }
            Phase::Up => {
                let offset = self.throw.track(now, drag.dx, drag.dy);
                let velocity = self.throw.velocity(now);
                self.release(&shift, offset, velocity, now, tx);
            }
        }
    }

    /// A release: the corner it lands in, and, while Labs › Indicator on any
    /// display is on, the display under the pointer. Landing on the display
    /// it was dragged on glides as before; landing on another display jumps
    /// there, because a glide is measured within one display.
    fn release(
        &mut self,
        shift: &Shift,
        offset: (f32, f32),
        velocity: (f32, f32),
        now: f64,
        tx: &mut Tx,
    ) {
        let Some(screen) = (self.screen)(self.scale.get()) else {
            self.throw.stop();
            shift(self.corner, 0.0, 0.0);
            return;
        };
        // Where it lands: the saved corner on the display it rests on, unless
        // a display under the pointer takes it.
        match self.elsewhere(offset, velocity) {
            Some(landing) if landing.elsewhere => {
                self.throw.stop();
                if let Some(retarget) = &self.retarget {
                    retarget(Some(&landing.display));
                }
                shift(landing.corner, 0.0, 0.0);
                self.settle(
                    Anchor {
                        corner: landing.corner,
                        display: Some(landing.display),
                    },
                    tx,
                );
            }
            elsewhere => {
                // The same display: its own size decides the corner, and the
                // throw glides into it.
                let screen = elsewhere.as_ref().map_or(screen, |landing| landing.size);
                let landing =
                    throw::land(self.corner, offset, velocity, screen, self.window_size());
                let corner = landing.corner;
                let glide = if self.reduced.get() {
                    self.throw.stop();
                    None
                } else {
                    self.throw.glide(landing, now)
                };
                match glide {
                    Some((x, y)) => {
                        shift(corner, x, y);
                        tx.wake_after(Duration::ZERO);
                    }
                    None => shift(corner, 0.0, 0.0),
                }
                let display =
                    elsewhere.map_or_else(|| self.display.clone(), |landing| Some(landing.display));
                // Off, every release is the one display it was always measured
                // on, so nothing here changes its display.
                self.settle(Anchor { corner, display }, tx);
            }
        }
    }

    /// Where a release lands on any display, while Labs › Indicator on any
    /// display is on: the corner of the display under the pointer nearest
    /// where the capsule's centre would coast to rest.
    fn elsewhere(&self, offset: (f32, f32), velocity: (f32, f32)) -> Option<screen::Landing> {
        if !anywhere() {
            return None;
        }
        let displays = (self.displays)(self.scale.get())?;
        screen::release(
            &displays,
            self.display.as_deref(),
            self.corner,
            offset,
            velocity,
            self.window_size(),
        )
    }

    /// Records and saves the corner and display the capsule now rests in.
    fn settle(&mut self, anchor: Anchor, tx: &mut Tx) {
        if anchor.corner == self.corner && anchor.display == self.display {
            return;
        }
        // The pill changes sides with the window; it must not slide across it.
        let sides_changed = anchor.corner != self.corner;
        tracing::info!(
            corner = ?anchor.corner,
            display = ?anchor.display,
            "meeting capsule moved"
        );
        self.corner = anchor.corner;
        self.display = anchor.display;
        (self.save_anchor)(self.corner, self.display.as_deref());
        if sides_changed {
            tx.instant();
        }
    }
}

/// Where a corner's capsule window sits: its edge and margin, and its
/// alignment and inset along that edge, after a drag of (dx, dy) points.
pub(super) fn corner_placement(
    corner: MeetingIndicatorCorner,
    dx: f32,
    dy: f32,
) -> (ScreenEdge, f32, Align, f32) {
    let (edge, margin) = if corner.is_top() {
        (ScreenEdge::Top, WINDOW_MARGIN + MENU_BAR + dy)
    } else {
        (ScreenEdge::Bottom, WINDOW_MARGIN + DOCK - dy)
    };
    let (align, inset) = if corner.is_right() {
        (Align::End, WINDOW_MARGIN - dx)
    } else {
        (Align::Start, WINDOW_MARGIN + dx)
    };
    (edge, margin, align, inset)
}

pub(super) fn place_capsule(
    handle: &OverlayHandle<Capsule>,
    corner: MeetingIndicatorCorner,
    dx: f32,
    dy: f32,
) {
    let (edge, margin, align, inset) = corner_placement(corner, dx, dy);
    handle.place(edge, margin);
    handle.set_align(align, inset);
}

/// Pins a capsule's window to the display `display` names while Labs ›
/// Indicator on any display is on; `None` is the primary display. Off, Rune
/// keeps opening every capsule on the primary display, as it always has.
pub(super) fn place_on(handle: &OverlayHandle<Capsule>, display: Option<&str>) {
    if crate::labs::meeting_indicator_display() {
        handle.set_display(screen::target(display));
    }
}

#[cfg(test)]
thread_local! {
    /// A test's answer for Labs › Indicator on any display, which this
    /// process's switch cannot turn on for the other OS's tests.
    static ANYWHERE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// A test's answer for whether a release may land on any display.
#[cfg(test)]
pub(super) fn force_anywhere(on: Option<bool>) {
    ANYWHERE.set(on);
}

/// Whether a release may land on any display (Labs › Indicator on any
/// display), as this process's switches hold it.
fn anywhere() -> bool {
    #[cfg(test)]
    if let Some(on) = ANYWHERE.with(Cell::get) {
        return on;
    }
    crate::labs::meeting_indicator_display()
}
