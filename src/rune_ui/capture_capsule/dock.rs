//! Annotate's capsule in the overlay dock (`desktop::overlay_dock`). Under
//! the menu bar it ranks after the meeting capsules: when its pill would meet
//! one there (a wide list, a narrow display, a meeting panel thrown to the
//! middle), the panel drops just below it and comes back once it has gone.
//! It claims the pill it draws, so the dictation HUD clears it.

use super::PANEL_MARGIN;
use super::surface::PANEL_W;
use crate::desktop::overlay_dock::{self, Align, Claim, Display, Edge, Surface};

/// Where the capsule sits in the dock.
#[derive(Debug, Default)]
pub(super) struct Docked {
    /// The display it was last placed on and that display's width, as
    /// measured just before the placement.
    on: Option<(Display, Option<f32>)>,
    /// How far below its natural place it was put under the menu bar.
    shift: f32,
}

impl Docked {
    /// Reads the display under the pointer, which the placement about to
    /// happen puts the panel on.
    pub(super) fn measure(&mut self) {
        self.on = Some(overlay_dock::pointer_display());
    }

    /// The panel's margin from the screen's top under the menu bar.
    pub(super) fn margin(&self) -> f32 {
        PANEL_MARGIN + self.shift
    }

    /// Claims the pill as it shows (`shell`, in panel points) and works out
    /// its shift. True when the capsule under the menu bar must be placed
    /// again: something in its way moved, showed, or went.
    pub(super) fn sync(&mut self, shown: bool, shell: [f32; 4]) -> bool {
        let claim = match (shown, &self.on) {
            (true, Some((display, width))) => natural(shell, display, *width),
            _ => None,
        };
        let Some(natural) = claim else {
            overlay_dock::claim(Surface::AnnotateCapsule, None);
            let moved = self.shift != 0.0 && shown;
            self.shift = 0.0;
            return moved;
        };
        let shift = overlay_dock::clearance(Surface::AnnotateCapsule, &natural);
        overlay_dock::claim(
            Surface::AnnotateCapsule,
            Some(Claim {
                margin: natural.margin + shift,
                ..natural
            }),
        );
        // Half a point either way is the same place.
        let moved = (shift - self.shift).abs() >= 0.5;
        if moved {
            self.shift = shift;
        }
        moved
    }
}

/// The pill `shell` (panel points) under the menu bar, centred.
fn natural(shell: [f32; 4], display: &Display, display_width: Option<f32>) -> Option<Claim> {
    let [x, y, width, height] = shell;
    Some(Claim {
        display: display.clone(),
        display_width,
        edge: Edge::Top,
        align: Align::Center,
        inset: x + width / 2.0 - PANEL_W / 2.0,
        margin: PANEL_MARGIN + y,
        size: (width, height.max(0.0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::overlay_dock::GAP;

    fn laptop() -> Display {
        Display::Id("1".into())
    }

    /// A pill 300 wide, 44 tall, 20 pt into a panel centred under the bar.
    const SHELL: [f32; 4] = [PANEL_W / 2.0 - 150.0, 20.0, 300.0, 44.0];

    #[test]
    fn under_the_menu_bar_the_pill_is_centred_past_the_panel_margin() {
        let claim = natural(SHELL, &laptop(), Some(1512.0)).unwrap();
        assert_eq!(claim.align, Align::Center);
        assert_eq!(claim.inset, 0.0);
        assert_eq!(claim.margin, PANEL_MARGIN + 20.0);
        assert_eq!(claim.size, (300.0, 44.0));
    }

    /// A meeting panel in the middle of the top edge moves the capsule down
    /// below it; once it goes, the capsule moves back, and both changes ask
    /// for a placement.
    #[test]
    fn the_capsule_drops_below_a_meeting_and_returns() {
        let meeting = Claim {
            display: laptop(),
            display_width: Some(1512.0),
            edge: Edge::Top,
            align: Align::Center,
            inset: 0.0,
            margin: 40.0,
            size: (360.0, 60.0),
        };
        let mut docked = Docked {
            on: Some((laptop(), Some(1512.0))),
            shift: 0.0,
        };
        assert!(!docked.sync(true, SHELL));
        assert_eq!(docked.margin(), PANEL_MARGIN);
        overlay_dock::claim(Surface::MeetingPanel, Some(meeting));
        assert!(docked.sync(true, SHELL), "placed again");
        assert_eq!(docked.margin() + SHELL[1], 40.0 + 60.0 + GAP);
        assert!(!docked.sync(true, SHELL), "settled");
        overlay_dock::claim(Surface::MeetingPanel, None);
        assert!(docked.sync(true, SHELL), "back to its place");
        assert_eq!(docked.margin(), PANEL_MARGIN);
        // Hidden, it claims nothing.
        docked.sync(false, SHELL);
        overlay_dock::claim(Surface::AnnotateCapsule, None);
    }
}
