//! The wheel over the picker's lists while Paste Last's keys may still be
//! held. Holding Shift, as ⌥⇧V does, turns a mouse wheel sideways: macOS
//! does it before any app sees the event, and Rune leaves it so there, as
//! AppKit's own lists do (elsewhere Rune turns it itself). The picker's lists
//! scroll only up and down, so they would never move under a held ⌥⇧. A
//! sideways delta with Shift held scrolls them as the wheel was turned.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use rune::{El, ScrollPos, WheelEvent};

/// The vertical delta `ev` means over a list that only scrolls up and down,
/// when Shift turned it sideways; None for a delta the list takes as it is.
pub(super) fn turned(ev: &WheelEvent) -> Option<f32> {
    (ev.mods.shift && ev.dy == 0.0 && ev.dx != 0.0).then_some(ev.dx)
}

/// Where the list stands, as its last frame drew it, and where a turned
/// delta takes it: kept ahead of the frames, so deltas arriving between two
/// frames add up instead of each starting from the same place.
#[derive(Default)]
struct Place {
    offset: Cell<f32>,
    max: Cell<f32>,
}

impl Place {
    fn drawn(&self, at: ScrollPos) {
        self.offset.set(at.offset[1]);
        self.max.set(at.max[1]);
    }
    /// Moves by `dy` points, positive moving the content down, as Rune's
    /// own wheel does; returns the new offset.
    fn by(&self, dy: f32) -> f32 {
        let next = (self.offset.get() - dy).clamp(0.0, self.max.get().max(0.0));
        self.offset.set(next);
        next
    }
}

thread_local! {
    /// One place per list, whichever commit's handlers hear the frame and
    /// the wheel: the view builds new ones each commit, and Rune may still
    /// call an older commit's.
    static PLACES: RefCell<HashMap<&'static str, Rc<Place>>> = RefCell::default();
}

/// `viewport`, a vertical scroll keyed `key`, moving under the wheel with
/// Shift held too.
pub(super) fn vertical<S: 'static>(viewport: El, key: &'static str) -> El {
    let place = PLACES.with_borrow_mut(|places| places.entry(key).or_default().clone());
    let drawn = place.clone();
    viewport
        .on_scroll_offset(move |at| drawn.drawn(at))
        .on_wheel(move |_: &mut S, ev, tx| {
            let Some(dy) = turned(&ev) else {
                return false;
            };
            tx.scroll_to_y(key, place.by(dy));
            true
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shift() -> rune::Mods {
        rune::Mods {
            shift: true,
            ..Default::default()
        }
    }

    /// Only Shift's sideways turn is undone: a plain or diagonal delta, or a
    /// sideways one without Shift (a trackpad's), scrolls as Rune scrolls it.
    #[test]
    fn only_a_wheel_shift_turned_sideways_is_turned_back() {
        assert_eq!(
            turned(&WheelEvent::new(-40.0, 0.0).mods(shift())),
            Some(-40.0)
        );
        assert_eq!(
            turned(&WheelEvent::new(40.0, 0.0).mods(shift())),
            Some(40.0)
        );
        assert_eq!(turned(&WheelEvent::new(0.0, -40.0).mods(shift())), None);
        assert_eq!(turned(&WheelEvent::new(-3.0, -40.0).mods(shift())), None);
        assert_eq!(turned(&WheelEvent::new(-40.0, 0.0)), None);
        assert_eq!(turned(&WheelEvent::new(0.0, 0.0).mods(shift())), None);
    }

    /// Deltas between two frames add up, and never pass either end.
    #[test]
    fn turned_deltas_add_up_between_frames_and_stop_at_the_ends() {
        let place = Place::default();
        place.offset.set(100.0);
        place.max.set(500.0);
        assert_eq!(place.by(-40.0), 140.0);
        assert_eq!(place.by(-40.0), 180.0);
        assert_eq!(place.by(1_000.0), 0.0);
        assert_eq!(place.by(-10_000.0), 500.0);
    }
}
