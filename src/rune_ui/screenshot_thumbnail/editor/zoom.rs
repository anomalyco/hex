//! The shot flying between its card and the editor. A click on a card
//! opens the editor with the shot exactly where the card is, and it grows
//! from there into its place in the panel, losing the card's corner and
//! shadow as it goes; the panel around it comes in once it has landed.
//! Closing reverses it: the panel goes, and the shot flies back down into
//! its card, which shows again under it before the editor hides. A shot
//! whose card has gone shrinks a little and fades instead.
//!
//! The card waits unseen while the editor draws its shot (`Lent`), and
//! the shot keeps its velocity through every turn: a close mid-flight
//! heads straight back, and a card moved by a new shot is followed.
//! Reduce Motion cross-fades the card and the panel instead.

use std::time::Duration;

use rune::{Anim, El, Spring, Transition, Tx, el};

use super::super::flight::{Flight, lifted, mix};
use super::super::{Lent, RADIUS as CARD_RADIUS, home, lend};
use super::Editor;
use crate::rune_ui::controls::Paint;

/// The panel around the shot coming in once it has landed, and going.
pub(super) const CHROME: Spring = Spring::new(0.16, 0.0);
/// How long [`CHROME`] takes to come in, before the flying shot gives way
/// to the panel's own.
const CHROME_S: f64 = 0.30;
/// How near its place in the panel, in points, the shot is when the panel
/// starts to come in: the eye reads it as landed well before it rests.
const ARRIVED: f32 = 2.0;
/// The shot that landed on its card dissolving into it. The card draws a
/// smaller copy of the picture, so a cut between them would flicker at
/// every edge.
const HAND_BACK: Spring = Spring::new(0.09, 0.0);
/// How long [`HAND_BACK`] takes, before the editor hides.
const HAND_BACK_S: f64 = 0.10;
/// How small a shot whose card has gone shrinks as it fades.
const GONE_SCALE: f32 = 0.92;
/// Its fade.
const GONE: Spring = Spring::new(0.20, 0.0);
/// The shot's corner in the panel: [`super::view`]'s inner radius.
const SHOT_RADIUS: f32 = CARD_RADIUS - super::PAD;

/// The card a shot was opened from.
#[derive(Clone, Copy, Debug)]
pub(crate) struct From {
    pub(crate) card: u64,
    /// Where the card is on the display, in points.
    pub(crate) rect: [f32; 4],
    /// Whether Reduce Motion is on: the card and the panel cross-fade.
    pub(crate) reduced: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    In,
    Out,
}

pub(super) struct Zoom {
    card: u64,
    reduced: bool,
    way: Way,
    flight: Flight,
    /// Where its card is, for how far the shot has come from it.
    card_rect: [f32; 4],
    stepped: Option<f64>,
    /// Frames polled since it opened: the card hides once the editor has
    /// drawn its shot.
    frames: u32,
    lent: bool,
    /// Animation seconds it landed, in the panel or on its card.
    landed: Option<f64>,
    /// Near enough its place in the panel for the panel to come in.
    arrived: bool,
    /// Its card has gone: the shot fades where it is.
    gone: bool,
}

impl Zoom {
    pub(super) fn new(from: From) -> Self {
        Self {
            card: from.card,
            reduced: from.reduced,
            way: Way::In,
            flight: Flight::at(from.rect, 1.0),
            card_rect: from.rect,
            stepped: None,
            frames: 0,
            lent: false,
            landed: None,
            arrived: false,
            gone: false,
        }
    }

    /// Whether the flying shot shows, over the panel's own.
    /// Whether it has landed back on its card, which shows under it.
    fn handed_back(&self) -> bool {
        self.way == Way::Out && self.landed.is_some()
    }

    fn flies(&self, now: f64) -> bool {
        !self.reduced
            && match self.way {
                Way::In => self.landed.is_none_or(|at| now - at < CHROME_S),
                Way::Out => true,
            }
    }

    /// How far the flying shot at `rect` has come from its card toward
    /// `shot`, its place in the panel, by its width: 0 on the card.
    fn toward_panel(&self, rect: [f32; 4], shot: [f32; 4]) -> f32 {
        let (card, panel) = (self.card_rect[2], shot[2]);
        if (panel - card).abs() < 1.0 {
            1.0
        } else {
            ((rect[2] - card) / (panel - card)).clamp(0.0, 1.0)
        }
    }
}

impl Editor {
    /// The shot's place in the panel, on the display.
    fn shot_rect(&self) -> [f32; 4] {
        let layout = self.layout();
        let [px, py, ..] = layout.panel;
        let [ix, iy, iw, ih] = layout.image;
        [px + ix, py + iy, iw, ih]
    }

    /// Whether the shot is flying back to its card, or fading, after a
    /// close: the panel stays, unseen, until it's done.
    pub(super) fn zooming_out(&self) -> bool {
        self.zoom
            .as_ref()
            .is_some_and(|zoom| zoom.way == Way::Out && !zoom.reduced)
    }

    /// The card the editor holds goes back to the stack, if it holds one.
    pub(super) fn give_back(&mut self) {
        if let Some(zoom) = self.zoom.take() {
            lend(zoom.card, Lent::No);
        }
    }

    /// A close: the shot heads back to its card from wherever it is, or
    /// fades when the card is gone. False when nothing flies, so the panel
    /// leaves as it always has.
    pub(super) fn zoom_out(&mut self) -> bool {
        let shot = self.shot_rect();
        if self.zoom.as_ref().is_some_and(|zoom| zoom.reduced) {
            self.give_back();
        }
        let Some(zoom) = self.zoom.as_mut() else {
            return false;
        };
        if zoom.way == Way::In && zoom.landed.is_some() {
            zoom.flight = Flight::at(shot, 1.0);
        }
        zoom.way = Way::Out;
        zoom.landed = None;
        zoom.arrived = false;
        true
    }

    /// One frame of the zoom; true while the view changes. Hides the
    /// editor once the shot is back on its card.
    pub(super) fn step_zoom(&mut self, now: f64, tx: &mut Tx) -> bool {
        let shot = self.shot_rect();
        let Some(zoom) = self.zoom.as_mut() else {
            return false;
        };
        zoom.frames += 1;
        // The editor's first frame has drawn the shot over the card: the
        // card can go.
        if zoom.way == Way::In && !zoom.lent {
            if zoom.frames >= 2 || zoom.reduced {
                zoom.lent = true;
                lend(zoom.card, Lent::Away);
            }
            // It leaves the card only once the card has gone, so no frame
            // shows the shot twice.
            if !zoom.reduced {
                zoom.stepped = None;
                tx.wake_after(Duration::ZERO);
                return false;
            }
        }
        if zoom.reduced {
            return false;
        }
        let dt = super::super::flight::tick(&mut zoom.stepped, now);
        let to = match zoom.way {
            Way::In => shot,
            Way::Out => match home(zoom.card).filter(|_| !zoom.gone) {
                Some(card) => {
                    zoom.card_rect = card;
                    card
                }
                None => {
                    zoom.gone = true;
                    let [x, y, w, h] = shot;
                    let [gw, gh] = [w * GONE_SCALE, h * GONE_SCALE];
                    [x + (w - gw) * 0.5, y + (h - gh) * 0.5, gw, gh]
                }
            },
        };
        if zoom.landed.is_none() {
            if zoom.flight.step(to, 1.0, dt) {
                if zoom.way == Way::In {
                    let rect = zoom.flight.rect();
                    zoom.arrived |= rect.iter().zip(to).all(|(a, b)| (a - b).abs() <= ARRIVED);
                }
                tx.wake_after(Duration::ZERO);
                return true;
            }
            zoom.landed = Some(now);
            zoom.arrived = zoom.way == Way::In;
            if zoom.way == Way::Out && !zoom.gone {
                lend(zoom.card, Lent::No);
            }
        }
        let landed = zoom.landed.unwrap_or(now);
        match zoom.way {
            Way::In => {
                // It stays on the panel's shot, wherever the panel moves.
                zoom.flight.jump(shot, 1.0);
                let left = landed + CHROME_S - now;
                if left > 0.0 {
                    tx.wake_after(Duration::from_secs_f64(left));
                }
                // The panel comes in as it lands, and the flying shot goes
                // once it's in.
                now == landed || left <= 0.0
            }
            Way::Out => {
                let left = landed + HAND_BACK_S - now;
                if left > 0.0 {
                    tx.wake_after(Duration::from_secs_f64(left));
                    return now == landed;
                }
                // Its card has it again.
                self.zoom = None;
                self.leaving = None;
                self.shot = None;
                self.clip = None;
                if let Some(overlay) = &self.overlay {
                    overlay.hide();
                }
                true
            }
        }
    }
}

/// The editor couldn't open the shot card `from` asked for: the card is
/// the stack's again.
pub(super) fn unopened(from: Option<From>) {
    if let Some(from) = from {
        lend(from.card, Lent::No);
    }
}

/// How much of the panel around the shot shows: none while the shot flies.
pub(super) fn chrome(s: &Editor, panel: El) -> El {
    let shows = match &s.zoom {
        Some(zoom) if !zoom.reduced => zoom.way == Way::In && zoom.arrived,
        _ => true,
    };
    panel
        .opacity(if shows { 1.0 } else { 0.0 })
        .opacity_motion(CHROME)
}

/// The panel's own shot, hidden while its flying copy goes back to the
/// card, so no copy of it fades in place.
pub(super) fn panel_shot(s: &Editor, shot: El) -> El {
    shot.opacity(if s.zooming_out() { 0.0 } else { 1.0 })
}

/// The shot in flight, over everything, from its card's corner and
/// shadow to the panel's corner, and back.
pub(super) fn flying(p: Paint, s: &Editor) -> Option<El> {
    let zoom = s.zoom.as_ref().filter(|zoom| zoom.flies(s.now))?;
    let picture = s.picture()?;
    let k = p.k;
    let rect = zoom.flight.rect();
    let [x, y, w, h] = rect;
    let t = zoom.toward_panel(rect, s.shot_rect());
    let radius = mix(CARD_RADIUS, SHOT_RADIUS, t);
    let mut shot = el()
        .key("zoom")
        .absolute(x, y)
        .size(w, h)
        .radius(radius)
        .fill(k.c.bg_base);
    for shadow in &k.elevation.floating {
        shot = shot.box_shadow(lifted(*shadow, 1.0 - t));
    }
    Some(
        shot.pixels_fit(picture.image.pixels(), rune::SurfaceFit::Fit)
            .child(
                el().absolute(0.0, 0.0)
                    .size(w, h)
                    .radius(radius)
                    .border(p.hair, k.c.border_weak),
            )
            .opacity(if zoom.gone || zoom.handed_back() {
                0.0
            } else {
                1.0
            })
            .opacity_motion(Anim::from(if zoom.gone { GONE } else { HAND_BACK }))
            .motion(Spring::NONE)
            .transition(Transition::none()),
    )
}

#[cfg(test)]
mod tests;
