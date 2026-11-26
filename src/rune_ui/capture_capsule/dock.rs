//! Annotate's capsule in the overlay dock (`desktop::overlay_dock`). Under
//! the menu bar it ranks after the meeting capsules: when its pill would meet
//! one there (a wide list, a narrow display, a meeting panel thrown to the
//! middle), the panel drops just below it and comes back once it has gone.
//! Hung from the notch it never moves (the notch doesn't), and at the pointer
//! it is on no edge. Either way it claims the pill it draws, so the dictation
//! HUD clears it.

use super::PANEL_MARGIN;
use super::notch::Measured;
use super::surface::{Anchor, PANEL_W};
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
    pub(super) fn measure(&mut self, notch: Option<Measured>) {
        let (display, width) = overlay_dock::pointer_display();
        let display = match notch.and_then(|notch| notch.display) {
            Some(id) => Display::Id(id.to_string()),
            None => display,
        };
        self.on = Some((display, width));
    }

    /// The panel's margin from the screen's top under the menu bar.
    pub(super) fn margin(&self) -> f32 {
        PANEL_MARGIN + self.shift
    }

    /// Claims the pill as it shows (`shell`, in panel points) and works out
    /// its shift. True when the capsule under the menu bar must be placed
    /// again: something in its way moved, showed, or went.
    pub(super) fn sync(
        &mut self,
        shown: bool,
        anchor: Anchor,
        notch: Option<Measured>,
        shell: [f32; 4],
    ) -> bool {
        let claim = match (shown, &self.on) {
            (true, Some((display, width))) => natural(anchor, notch, shell, display, *width),
            _ => None,
        };
        let Some(natural) = claim else {
            overlay_dock::claim(Surface::AnnotateCapsule, None);
            let moved = self.shift != 0.0 && shown;
            self.shift = 0.0;
            return moved;
        };
        let shift = match anchor {
            Anchor::Top => overlay_dock::clearance(Surface::AnnotateCapsule, &natural),
            Anchor::Notch | Anchor::Cursor => 0.0,
        };
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

/// The pill `shell` (panel points) at its natural place, on its edge: under
/// the menu bar, centred, or in the notch from the panel's measured left.
/// At the pointer it is on no edge.
fn natural(
    anchor: Anchor,
    notch: Option<Measured>,
    shell: [f32; 4],
    display: &Display,
    display_width: Option<f32>,
) -> Option<Claim> {
    let [x, y, width, height] = shell;
    let (align, inset, margin, height) = match anchor {
        Anchor::Top => (
            Align::Center,
            x + width / 2.0 - PANEL_W / 2.0,
            PANEL_MARGIN + y,
            height,
        ),
        // Its walls run up past the screen's top: the body starts there.
        Anchor::Notch => match notch {
            Some(notch) => (Align::Start, notch.panel_x + x, y.max(0.0), y + height),
            None => (
                Align::Center,
                x + width / 2.0 - PANEL_W / 2.0,
                y.max(0.0),
                y + height,
            ),
        },
        Anchor::Cursor => return None,
    };
    Some(Claim {
        display: display.clone(),
        display_width,
        edge: Edge::Top,
        align,
        inset,
        margin,
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
        let claim = natural(Anchor::Top, None, SHELL, &laptop(), Some(1512.0)).unwrap();
        assert_eq!(claim.align, Align::Center);
        assert_eq!(claim.inset, 0.0);
        assert_eq!(claim.margin, PANEL_MARGIN + 20.0);
        assert_eq!(claim.size, (300.0, 44.0));
        assert!(natural(Anchor::Cursor, None, SHELL, &laptop(), None).is_none());
    }

    /// Hung from the notch, the body starts at the screen's top however far
    /// its walls reach above it.
    #[test]
    fn in_the_notch_the_pill_starts_at_the_top_edge() {
        let notch = super::super::notch::fitted(663.0, 848.0, 2.0);
        let shell = [notch.geometry.left - 12.0, -48.0, 209.0, 48.0 + 36.0];
        let claim = natural(Anchor::Notch, Some(notch), shell, &laptop(), Some(1512.0)).unwrap();
        assert_eq!(claim.align, Align::Start);
        assert_eq!(claim.inset, 663.0 - 12.0);
        assert_eq!(claim.margin, 0.0);
        assert_eq!(claim.size, (209.0, 36.0));
    }

    /// A meeting panel in the middle of the top edge moves the capsule down
    /// below it; once it goes, the capsule moves back, and both changes ask
    /// for a placement. The notch never moves.
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
        assert!(!docked.sync(true, Anchor::Top, None, SHELL));
        assert_eq!(docked.margin(), PANEL_MARGIN);
        overlay_dock::claim(Surface::MeetingPanel, Some(meeting));
        assert!(docked.sync(true, Anchor::Top, None, SHELL), "placed again");
        assert_eq!(docked.margin() + SHELL[1], 40.0 + 60.0 + GAP);
        assert!(!docked.sync(true, Anchor::Top, None, SHELL), "settled");
        // In the notch it stays where the hardware is.
        let notch = super::super::notch::fitted(663.0, 848.0, 2.0);
        docked.sync(true, Anchor::Notch, Some(notch), [0.0, -48.0, 209.0, 84.0]);
        assert_eq!(docked.margin(), PANEL_MARGIN);
        docked.sync(true, Anchor::Top, None, SHELL);
        overlay_dock::claim(Surface::MeetingPanel, None);
        assert!(
            docked.sync(true, Anchor::Top, None, SHELL),
            "back to its place"
        );
        assert_eq!(docked.margin(), PANEL_MARGIN);
        // Hidden, it claims nothing.
        docked.sync(false, Anchor::Top, None, SHELL);
        overlay_dock::claim(Surface::AnnotateCapsule, None);
    }
}
