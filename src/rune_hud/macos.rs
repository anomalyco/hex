//! The HUD on macOS: placed on the screen with the pointer, below the menu
//! bar, in it, or out of the notch (`hud_placement`), from `NSScreen`'s
//! metrics. The main thread's pump hands it the listener's events.

use std::sync::atomic::Ordering;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSScreen, NSStatusBar};
use objc2_foundation::NSRect;
use rune::{DisplayTarget, OverlayHandle, OverlayPlacement};

use super::{Hud, HudEvent, HudSender, window};
use crate::app_settings::IndicatorPosition;
use crate::dictation_indicator::dot_scope_enabled;
use crate::hud_placement;

/// `NSStatusWindowLevel`: over other apps' windows, under the menu bar.
const STATUS_LEVEL: isize = 25;
const CAPSULE_HEIGHT: f64 = 16.0;

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
    pub(super) fn place_on_pointer_screen(&mut self, overlay: &OverlayHandle<Hud>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let pointer = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let Some(primary) = screens.iter().next() else {
            return;
        };
        let Some(screen) = screens
            .iter()
            .find(|screen| {
                let frame = screen.frame();
                pointer.x >= frame.origin.x
                    && pointer.x < frame.origin.x + frame.size.width
                    && pointer.y >= frame.origin.y
                    && pointer.y < frame.origin.y + frame.size.height
            })
            .or_else(|| NSScreen::mainScreen(mtm))
        else {
            return;
        };
        let rect = |r: NSRect| hud_placement::Rect {
            x: r.origin.x,
            y: r.origin.y,
            width: r.size.width,
            height: r.size.height,
        };
        let mut metrics = hud_placement::Screen {
            frame: rect(screen.frame()),
            visible: rect(screen.visibleFrame()),
            safe_top: screen.safeAreaInsets().top,
            aux_left: screen.auxiliaryTopLeftArea().size.width,
            aux_right: screen.auxiliaryTopRightArea().size.width,
            bar: NSStatusBar::systemStatusBar().thickness(),
        };
        let tuning = self.model.dot_scope_tuning();
        let (placement, pretend_notch, island_spread) = tuning
            .map_or_else(Default::default, |tuning| {
                (tuning.placement, tuning.pretend_notch, tuning.island_spread)
            });
        if pretend_notch {
            metrics = metrics.pretending_notch();
        }
        let (width, height) = window();
        let plate_height = if dot_scope_enabled() {
            f64::from(crate::dot_scope::PLATE_HEIGHT)
        } else {
            CAPSULE_HEIGHT
        };
        let placed = hud_placement::place(
            placement,
            &metrics,
            hud_placement::Content {
                window: (f64::from(width), f64::from(height)),
                plate: (f64::from(width) - 24.0, plate_height),
                island_spread: f64::from(island_spread),
            },
            self.position == IndicatorPosition::Bottom,
        );
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
        let primary_top = primary.frame().origin.y + primary.frame().size.height;
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
