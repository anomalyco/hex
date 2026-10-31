//! The HUD on macOS: placed on the screen with the pointer, below the menu
//! bar, in it, or out of the notch (`hud_placement`), from `NSScreen`'s
//! metrics. The main thread's pump hands it the listener's events.

use std::sync::atomic::Ordering;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSScreen, NSStatusBar};
use objc2_foundation::NSRect;
use rune::{DisplayTarget, OverlayHandle, OverlayPlacement};

use super::{Hud, HudEvent, HudSender, LIVE_TEXT_GAP, WINDOW, set_live_text, set_plate_center};
use crate::app_settings::IndicatorPosition;
use crate::rune_hud::hud_placement;
use crate::rune_ui::live_text::Spot;

/// `NSStatusWindowLevel`: over other apps' windows, under the menu bar.
const STATUS_LEVEL: isize = 25;

/// The screen under the pointer (else the main one) as `hud_placement`
/// reads it, and the primary screen's top, which Rune's global space hangs
/// from.
fn pointer_screen(mtm: MainThreadMarker) -> Option<(hud_placement::Screen, f64)> {
    let pointer = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let primary = screens.iter().next()?;
    let screen = screens
        .iter()
        .find(|screen| {
            let frame = screen.frame();
            pointer.x >= frame.origin.x
                && pointer.x < frame.origin.x + frame.size.width
                && pointer.y >= frame.origin.y
                && pointer.y < frame.origin.y + frame.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm))?;
    let rect = |r: NSRect| hud_placement::Rect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    };
    let metrics = hud_placement::Screen {
        frame: rect(screen.frame()),
        visible: rect(screen.visibleFrame()),
        safe_top: screen.safeAreaInsets().top,
        aux_left: screen.auxiliaryTopLeftArea().size.width,
        aux_right: screen.auxiliaryTopRightArea().size.width,
        bar: NSStatusBar::systemStatusBar().thickness(),
    };
    Some((
        metrics,
        primary.frame().origin.y + primary.frame().size.height,
    ))
}

/// The HUD's window and widest plate, and how far its island
/// spreads past the notch.
fn content(island_spread: f32) -> hud_placement::Content {
    let (width, height) = WINDOW;
    hud_placement::Content {
        window: (f64::from(width), f64::from(height)),
        plate: (
            f64::from(width) - 24.0,
            f64::from(crate::rune_hud::dot_scope::PLATE_HEIGHT),
        ),
        island_spread: f64::from(island_spread),
    }
}

/// The live text's spot just past `placed`'s plate, in Rune's global space.
fn live_text_beside(
    placed: &hud_placement::Placed,
    screen: &hud_placement::Screen,
    content: hud_placement::Content,
    primary_top: f64,
) -> Spot {
    let (bottom, margin) =
        hud_placement::beside_plate(placed, screen, content, f64::from(LIVE_TEXT_GAP));
    let y = if bottom {
        screen.frame.y + margin
    } else {
        screen.frame.y + screen.frame.height - margin
    };
    Spot::Global {
        x: (placed.top_left.0 + content.window.0 / 2.0 + f64::from(placed.layout.center[0])) as f32,
        y: (primary_top - y) as f32,
        below: !bottom,
    }
}

/// The pump's handle on the HUD (main thread).
pub(crate) struct DictationIndicatorUi {
    hud: HudSender,
}

impl DictationIndicatorUi {
    pub(crate) fn new(hud: HudSender) -> Self {
        Self { hud }
    }

    pub(crate) fn handle(&mut self, event: HudEvent) {
        self.hud.send(event);
    }

    /// Keeps a showing HUD on the pointer's screen, and hides it when the
    /// indicator is set to Hidden.
    pub(crate) fn follow_pointer(&mut self) {
        if !self.hud.animating.load(Ordering::Acquire) {
            return;
        }
        self.hud.overlay.update_if(|hud, _| {
            if hud.shown {
                hud.show();
            }
            false
        });
    }
}

impl Hud {
    /// With the HUD hidden, the live text starts where its plate would sit
    /// under the menu bar.
    pub(super) fn place_live_text_without_hud(&self) {
        let Some((screen, primary_top)) = MainThreadMarker::new().and_then(pointer_screen) else {
            return;
        };
        let content = content(0.0);
        let placed = hud_placement::place(Default::default(), &screen, content, false);
        let (top, bottom) = hud_placement::plate_span(&placed, content);
        set_plate_center([
            (screen.frame.x + screen.frame.width / 2.0) as f32,
            (primary_top - (top + bottom) / 2.0) as f32,
        ]);
        set_live_text(Spot::Global {
            x: (screen.frame.x + screen.frame.width / 2.0) as f32,
            y: (primary_top - top) as f32,
            below: true,
        });
    }

    pub(super) fn place_on_pointer_screen(&mut self, overlay: &OverlayHandle<Hud>) {
        let Some((mut metrics, primary_top)) = MainThreadMarker::new().and_then(pointer_screen)
        else {
            return;
        };
        let tuning = self.model.tuning();
        if tuning.pretend_notch {
            metrics = metrics.pretending_notch();
        }
        let (width, height) = WINDOW;
        let content = content(tuning.island_spread);
        let placed = hud_placement::place(
            tuning.placement,
            &metrics,
            content,
            self.position == IndicatorPosition::Bottom,
        );
        set_live_text(live_text_beside(&placed, &metrics, content, primary_top));
        let (top, bottom) = hud_placement::plate_span(&placed, content);
        set_plate_center([
            (placed.top_left.0 + content.window.0 / 2.0 + f64::from(placed.layout.center[0]))
                as f32,
            (primary_top - (top + bottom) / 2.0) as f32,
        ]);
        if self.placed == Some(placed) {
            return;
        }
        if self.placed.map(|old| old.over_menu_bar) != Some(placed.over_menu_bar) {
            // Above the menu bar and its status items, below their menus.
            overlay.set_macos_level(STATUS_LEVEL + isize::from(placed.over_menu_bar));
        }
        self.placed = Some(placed);
        self.layout = placed.layout;
        self.model.set_island(placed.layout.island);
        // Rune's global rect: points from the primary screen's top-left, y down.
        let (x, top) = placed.top_left;
        overlay.set_display(DisplayTarget::Primary);
        overlay.set_placement(Some(OverlayPlacement::Rect {
            x: x as f32,
            y: (primary_top - top) as f32,
            width,
            height,
        }));
    }
}
