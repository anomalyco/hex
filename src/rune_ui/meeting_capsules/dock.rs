//! The meeting capsules in the overlay dock (`desktop::overlay_dock`). They
//! rank first: they stay in the corner the person threw them to, and claim
//! what they draw there (the pill, its open panel, the finished panel, an
//! offer), so Annotate's capsule and the dictation HUD clear them. A claim
//! changes only when the capsule shows, hides, opens, or moves.

use super::panel::{foot_height, open_height_without_notice, open_window, ready_body, rest_window};
use super::placement::corner_placement;
use super::{CAPSULE_INSET, Capsule, MeetingIndicatorCorner, RECORDING_WINDOW, State};
use crate::desktop::overlay_dock::{self, Align, Claim, Display, Edge, Surface};

/// What the dock last heard from one capsule window.
pub(super) struct Docking {
    surface: Surface,
    /// The corner, display, and body it last claimed, `None` while hidden.
    last: Option<(MeetingIndicatorCorner, Option<String>, (f32, f32))>,
}

impl Docking {
    /// The recording window (pill, finished panel) or the panel window
    /// (offers, reminders, a call that ended).
    pub(super) fn new(recording: bool) -> Self {
        Self {
            surface: if recording {
                Surface::MeetingPill
            } else {
                Surface::MeetingPanel
            },
            last: None,
        }
    }
}

impl Capsule {
    /// Brings its claim up to date: each poll, measuring displays only when
    /// what it draws or where changed.
    pub(super) fn dock(&mut self) {
        let shown = self
            .is_shown()
            .then(|| (self.corner, self.display.clone(), self.body()));
        let Some(docking) = &self.dock else {
            return;
        };
        if docking.last == shown {
            return;
        }
        let surface = docking.surface;
        let claim = shown
            .as_ref()
            .map(|(corner, _, body)| self.claim(*corner, *body));
        overlay_dock::claim(surface, claim);
        if let Some(docking) = &mut self.dock {
            docking.last = shown;
        }
    }

    /// What it draws now, with the window's inset: smaller than its window
    /// where the window stays at its largest.
    fn body(&self) -> (f32, f32) {
        if self.presentation.inherits_offer_window() {
            return self.window_size();
        }
        match &self.state {
            State::Recording {
                recording,
                hovered,
                stopping,
                ..
            } if *hovered || *stopping => open_window(Some(recording), foot_height(self)),
            State::Recording { recording, .. } => rest_window(Some(recording)),
            State::Finalizing { .. } => (
                RECORDING_WINDOW.0,
                open_height_without_notice() + 2.0 * CAPSULE_INSET,
            ),
            State::Ready { manifest, .. } => ready_body(manifest),
            _ => self.window_size(),
        }
    }

    /// Its body in `corner` of the display Rune put it on.
    fn claim(&self, corner: MeetingIndicatorCorner, body: (f32, f32)) -> Claim {
        let (_, margin, _, inset) = corner_placement(corner, 0.0, 0.0);
        let (display, display_width) = self.resting_display();
        Claim {
            display,
            display_width,
            edge: if corner.is_top() {
                Edge::Top
            } else {
                Edge::Bottom
            },
            align: if corner.is_right() {
                Align::End
            } else {
                Align::Start
            },
            inset,
            margin,
            size: body,
        }
    }

    /// The display it rests on and that display's width: the saved one while
    /// Labs › Indicator on any display is on and it is connected, else the
    /// primary. Names the primary display for claims that can't.
    fn resting_display(&self) -> (Display, Option<f32>) {
        let scale = self.scale.get();
        let Some(displays) = (self.displays)(scale) else {
            return (Display::Primary, (self.screen)(scale).map(|size| size.0));
        };
        let primary = displays.all.iter().find(|display| display.primary);
        overlay_dock::set_primary(primary.map(|display| display.id.clone()));
        let saved = crate::labs::meeting_indicator_display()
            .then_some(self.display.as_deref())
            .flatten();
        displays
            .all
            .iter()
            .find(|display| Some(display.id.as_str()) == saved)
            .or(primary)
            .map_or((Display::Primary, None), |display| {
                (Display::Id(display.id.clone()), Some(display.size.0))
            })
    }
}
