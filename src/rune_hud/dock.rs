//! The dictation HUD in the overlay dock (`desktop::overlay_dock`). It ranks
//! last: on its edge of its display it sits past every meeting capsule and
//! Annotate surface there, so a dictation during a meeting or a capture shows
//! whole beside them, and it claims where it ends up. On macOS the pump places
//! a shown HUD every frame, and on Linux and Windows every `FOLLOW_EVERY`, so
//! a surface that moves, hides, or shows moves the HUD with it.

use crate::desktop::overlay_dock::{self, Align, Claim, Edge, Surface};

/// Lets the HUD's claim go: it hid.
pub(super) fn release() {
    overlay_dock::claim(Surface::Dictation, None);
}

/// macOS: `placed` moved along its edge past the surfaces in its way on
/// `screen`, and claimed. A HUD in the menu bar or the notch can't step
/// aside there, so one that meets a surface takes `below`, its place under
/// the menu bar, and clears from there.
#[cfg(target_os = "macos")]
pub(super) fn clear(
    placed: super::hud_placement::Placed,
    screen: &super::hud_placement::Screen,
    content: super::hud_placement::Content,
    display: Option<u32>,
    below: impl FnOnce() -> super::hud_placement::Placed,
) -> super::hud_placement::Placed {
    let mut placed = placed;
    let mut natural = macos_claim(&placed, screen, content, display);
    let mut shift = overlay_dock::clearance(Surface::Dictation, &natural);
    if shift > 0.0 && placed.over_menu_bar {
        placed = below();
        natural = macos_claim(&placed, screen, content, display);
        shift = overlay_dock::clearance(Surface::Dictation, &natural);
    }
    // AppKit's y grows upward: down from the top edge, up from the bottom.
    placed.top_left.1 += f64::from(match natural.edge {
        Edge::Top => -shift,
        Edge::Bottom => shift,
    });
    overlay_dock::claim(
        Surface::Dictation,
        Some(Claim {
            margin: natural.margin + shift,
            ..natural
        }),
    );
    placed
}

/// The plate a macOS placement draws, as the dock measures it: centred on
/// its offset from the screen's middle, on the nearer edge.
#[cfg(target_os = "macos")]
fn macos_claim(
    placed: &super::hud_placement::Placed,
    screen: &super::hud_placement::Screen,
    content: super::hud_placement::Content,
    display: Option<u32>,
) -> Claim {
    let (top, bottom) = super::hud_placement::plate_span(placed, content);
    let width = content.plate.0 * f64::from(placed.layout.fit);
    let window_middle = placed.top_left.0 + content.window.0 / 2.0;
    let mut middle = window_middle + f64::from(placed.layout.center[0]);
    if placed.layout.anchor_left {
        middle += width / 2.0;
    }
    let frame = screen.frame;
    let on_top = (top + bottom) / 2.0 > frame.y + frame.height / 2.0;
    Claim {
        display: display.map_or(overlay_dock::Display::Primary, |id| {
            overlay_dock::Display::Id(id.to_string())
        }),
        display_width: Some(frame.width as f32),
        edge: if on_top { Edge::Top } else { Edge::Bottom },
        align: Align::Center,
        inset: (middle - (frame.x + frame.width / 2.0)) as f32,
        margin: if on_top {
            frame.y + frame.height - top
        } else {
            bottom - frame.y
        } as f32,
        size: (width as f32, (top - bottom) as f32),
    }
}

/// Linux and Windows: the edge the HUD's window goes on and its margin there,
/// past the surfaces in its way on the display under the pointer, and
/// claimed. The plate sits in the middle of the window.
#[cfg(not(target_os = "macos"))]
pub(super) fn edge_margin(
    position: crate::app_settings::IndicatorPosition,
) -> (rune::ScreenEdge, f32) {
    use super::{BOTTOM_OFFSET, PLATE_HEIGHT, TOP_OFFSET, WINDOW};
    let (edge, dock_edge, base) = match position {
        crate::app_settings::IndicatorPosition::Bottom => {
            (rune::ScreenEdge::Bottom, Edge::Bottom, BOTTOM_OFFSET)
        }
        _ => (rune::ScreenEdge::Top, Edge::Top, TOP_OFFSET),
    };
    let (display, display_width) = overlay_dock::pointer_display();
    let natural = window_claim(
        display,
        display_width,
        dock_edge,
        base,
        WINDOW,
        PLATE_HEIGHT,
    );
    let shift = overlay_dock::clearance(Surface::Dictation, &natural);
    overlay_dock::claim(
        Surface::Dictation,
        Some(Claim {
            margin: natural.margin + shift,
            ..natural
        }),
    );
    (edge, base + shift)
}

/// A centred window `window` big, `base` from `edge`, drawing a plate
/// `plate_height` tall in its middle, as wide as the window (its widest).
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn window_claim(
    display: overlay_dock::Display,
    display_width: Option<f32>,
    edge: Edge,
    base: f32,
    window: (f32, f32),
    plate_height: f32,
) -> Claim {
    Claim {
        display,
        display_width,
        edge,
        align: Align::Center,
        inset: 0.0,
        margin: base + (window.1 - plate_height) / 2.0,
        size: (window.0, plate_height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::overlay_dock::{Display, Dock, GAP};

    /// Annotate's capsule under the menu bar pushes a HUD there below it; the
    /// window's margin moves by exactly the plate's shift.
    #[test]
    fn a_hud_window_moves_past_the_capsule_by_its_plate() {
        let mut dock = Dock::new();
        dock.claim(
            Surface::AnnotateCapsule,
            Some(Claim {
                display: Display::Primary,
                display_width: None,
                edge: Edge::Top,
                align: Align::Center,
                inset: 0.0,
                margin: 14.0,
                size: (300.0, 44.0),
            }),
        );
        let natural = window_claim(Display::Primary, None, Edge::Top, 12.0, (304.0, 80.0), 26.0);
        assert_eq!(natural.margin, 12.0 + 27.0);
        let shift = dock.clearance(Surface::Dictation, &natural);
        assert_eq!(natural.margin + shift, 14.0 + 44.0 + GAP);
        // At the bottom edge the capsule is nowhere near.
        let bottom = window_claim(
            Display::Primary,
            None,
            Edge::Bottom,
            24.0,
            (304.0, 80.0),
            26.0,
        );
        assert_eq!(dock.clearance(Surface::Dictation, &bottom), 0.0);
    }

    /// macOS: on a 1512 pt laptop the HUD under the menu bar meets a capsule
    /// there and drops below it; in the notch it first comes out under the
    /// menu bar, then clears.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_mac_hud_clears_the_capsule_from_under_the_menu_bar_and_the_notch() {
        use super::super::hud_placement::{self, Content, Rect, Screen};
        use crate::rune_hud::dot_scope::Placement;
        let screen = Screen {
            frame: Rect {
                x: 0.0,
                y: 0.0,
                width: 1512.0,
                height: 982.0,
            },
            visible: Rect {
                x: 0.0,
                y: 0.0,
                width: 1512.0,
                height: 950.0,
            },
            safe_top: 32.0,
            aux_left: 663.0,
            aux_right: 664.0,
            bar: 24.0,
        };
        let content = Content {
            window: (320.0, 128.0),
            plate: (120.0, 26.0),
            island_spread: 60.0,
        };
        let capsule = Claim {
            display: Display::Id("1".into()),
            display_width: Some(1512.0),
            edge: Edge::Top,
            align: Align::Center,
            inset: 0.0,
            margin: 0.0,
            size: (209.0, 80.0),
        };
        overlay_dock::claim(Surface::AnnotateCapsule, Some(capsule));
        let below = || hud_placement::place(Placement::Below, &screen, content, false, 0.0);
        for placement in [Placement::Below, Placement::Notch, Placement::MenuBar] {
            let natural = hud_placement::place(placement, &screen, content, false, 0.0);
            let placed = clear(natural, &screen, content, Some(1), below);
            let claim = macos_claim(&placed, &screen, content, Some(1));
            assert!(
                !placed.over_menu_bar,
                "{placement:?} steps out from the bar"
            );
            assert!(
                claim.margin >= 80.0 + GAP - 0.01,
                "{placement:?}: {claim:?}"
            );
        }
        // On another display nothing moves.
        let natural = hud_placement::place(Placement::Notch, &screen, content, false, 0.0);
        assert_eq!(clear(natural, &screen, content, Some(2), below), natural);
        overlay_dock::claim(Surface::AnnotateCapsule, None);
        release();
    }
}
