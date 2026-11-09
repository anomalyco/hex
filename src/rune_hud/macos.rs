//! The HUD on macOS: placed on the screen with the pointer, below the menu
//! bar, in it, or out of the notch (`hud_placement`), from `NSScreen`'s
//! metrics. The main thread's pump hands it the listener's events.

use std::sync::atomic::Ordering;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSEvent, NSScreen, NSStatusBar, NSWindow, NSWindowCollectionBehavior,
};
use objc2_foundation::{NSNumber, NSRect, NSString};
use rune::{DisplayTarget, OverlayHandle, OverlayPlacement};

use super::{Hud, HudEvent, HudSender, LIVE_TEXT_GAP, WINDOW, set_live_text};
use crate::app_settings::IndicatorPosition;
use crate::rune_hud::hud_placement;
use crate::rune_ui::live_text::Spot;

/// `NSStatusWindowLevel`: over other apps' windows, under the menu bar.
const STATUS_LEVEL: isize = 25;

/// The screen under the pointer (else the main one) as `hud_placement`
/// reads it, the primary screen's top, which Rune's global space hangs
/// from, the screen's backing scale, and its `NSScreenNumber`, which Rune's
/// `NativeDisplayId::MacOS` pins an overlay to.
pub(crate) fn pointer_screen(
    mtm: MainThreadMarker,
) -> Option<(hud_placement::Screen, f64, f64, Option<u32>)> {
    let pointer = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let primary = screens.iter().next()?;
    // The half-open rule Rune resolves `UnderPointer` with (closed at a
    // screen's top, open at its bottom), so the capsule's panel and the
    // notch it hangs from pick the same display where two share an edge.
    // The fallback stays the main screen: Rune's "previous display" would
    // need state the HUD's placement doesn't keep.
    let screen = screens
        .iter()
        .find(|screen| {
            let frame = screen.frame();
            pointer.x >= frame.origin.x
                && pointer.x < frame.origin.x + frame.size.width
                && pointer.y > frame.origin.y
                && pointer.y <= frame.origin.y + frame.size.height
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
        screen.backingScaleFactor(),
        screen_number(&screen),
    ))
}

/// The screen's `NSScreenNumber` (`CGDirectDisplayID`), the id Rune's
/// `NativeDisplayId::MacOS` matches: winit reports a monitor's native id
/// the same way (`CGDisplayGetDisplayIDFromUUID`), so a measured notch can
/// pin the panel to the display it was read on.
fn screen_number(screen: &NSScreen) -> Option<u32> {
    let description = screen.deviceDescription();
    let key = NSString::from_str("NSScreenNumber");
    let number = description.objectForKey(&key)?;
    Some(number.downcast_ref::<NSNumber>()?.unsignedIntValue())
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
    /// The HUD's panel and the window number it had when found: Rune makes a
    /// new window, with a new number, when it remakes the overlay.
    panel: Option<(u32, Retained<NSWindow>)>,
    /// A re-home was attempted for this show and the panel has not been seen
    /// back on the active Space since. It resets when the HUD hides, so each
    /// show gets one attempt, and when the panel is seen on the active Space,
    /// so a later Space change that strands it gets one more.
    rehomed: bool,
}

impl DictationIndicatorUi {
    pub(crate) fn new(hud: HudSender) -> Self {
        Self {
            hud,
            panel: None,
            rehomed: false,
        }
    }

    pub(crate) fn handle(&mut self, event: HudEvent) {
        self.hud.send(event);
    }

    /// Whether the HUD wants the display's frames: the pump ticks every
    /// frame only while it does.
    pub(crate) fn animating(&self) -> bool {
        self.hud.animating.load(Ordering::Acquire)
    }

    /// Keeps a showing HUD on the pointer's screen and the active Space, and
    /// hides it when the indicator is set to Hidden.
    pub(crate) fn follow_pointer(&mut self) {
        let animating = self.hud.animating.load(Ordering::Acquire);
        let shown = self.hud.shown.load(Ordering::Acquire);
        if !shown {
            // Each show gets a fresh attempt.
            self.rehomed = false;
        } else if let Some(mtm) = MainThreadMarker::new() {
            self.rehome_on_active_space(mtm);
        }
        if !animating {
            return;
        }
        self.hud.overlay.update_if(|hud, _| {
            if hud.shown.load(Ordering::Acquire) {
                hud.show();
            }
            false
        });
    }

    /// The HUD's panel, found among the app's windows once per Rune window.
    fn panel(&mut self, mtm: MainThreadMarker) -> Option<(u32, Retained<NSWindow>)> {
        let Some(number) = self.hud.overlay.window_number() else {
            self.panel = None;
            return None;
        };
        if let Some((cached, panel)) = &self.panel
            && *cached == number
        {
            return Some((number, panel.clone()));
        }
        let panel = NSApplication::sharedApplication(mtm)
            .windows()
            .iter()
            .find(|window| window.windowNumber() == number as isize);
        let Some(panel) = panel else {
            self.panel = None;
            return None;
        };
        self.panel = Some((number, panel.clone()));
        Some((number, panel))
    }

    /// Puts a shown HUD back on the active Space. Changing the activation
    /// policy (the Dock icon follows Settings) can tie Rune's panel to one
    /// Space while AppKit still reports `CanJoinAllSpaces`, and Rune repairs
    /// that only as its panel shows (#1084). Clearing the mask and setting it
    /// back makes the window server apply it again; the panel is ordered out
    /// and back in, so this reorders the panel but leaves Rune's presence
    /// state and alpha alone. It is attempted once per show, and once more
    /// after the panel has been seen on the active Space again, so a repair
    /// that did not take cannot loop.
    fn rehome_on_active_space(&mut self, mtm: MainThreadMarker) {
        let Some((number, panel)) = self.panel(mtm) else {
            return;
        };
        // Rune applies the queued show later than `shown` flips.
        if !panel.isVisible() {
            return;
        }
        if panel.isOnActiveSpace() {
            // Back where it belongs: a later Space change gets another try.
            self.rehomed = false;
            return;
        }
        if self.rehomed {
            return;
        }
        self.rehomed = true;
        let behavior = panel.collectionBehavior();
        panel.orderOut(None);
        panel.setCollectionBehavior(NSWindowCollectionBehavior::Default);
        panel.setCollectionBehavior(behavior);
        panel.orderFrontRegardless();
        tracing::info!(
            window = number,
            active_space = panel.isOnActiveSpace(),
            "re-homed the dictation HUD on the active Space"
        );
    }
}

impl Hud {
    /// With the HUD hidden, the live text starts where its plate would sit
    /// under the menu bar.
    pub(super) fn place_live_text_without_hud(&self) {
        let Some((screen, primary_top, _, _)) = MainThreadMarker::new().and_then(pointer_screen)
        else {
            return;
        };
        let content = content(0.0);
        let placed = hud_placement::place(Default::default(), &screen, content, false);
        let (top, _) = hud_placement::plate_span(&placed, content);
        set_live_text(Spot::Global {
            x: (screen.frame.x + screen.frame.width / 2.0) as f32,
            y: (primary_top - top) as f32,
            below: true,
        });
    }

    pub(super) fn place_on_pointer_screen(&mut self, overlay: &OverlayHandle<Hud>) {
        let Some((mut metrics, primary_top, _, _)) =
            MainThreadMarker::new().and_then(pointer_screen)
        else {
            return;
        };
        let tuning = self.model.tuning();
        if tuning.pretend_notch {
            metrics = metrics.pretending_notch();
        }
        let (width, height) = WINDOW;
        let content = content(tuning.island_spread);
        // Labs' notch indicator takes any shown position on a notched screen;
        // Hidden and flat screens leave HUD Lab's placement standing.
        let ears = hud_placement::notch_placement(
            crate::labs::notch_indicator(),
            self.position,
            &metrics,
            content,
        );
        let placed = ears.unwrap_or_else(|| {
            hud_placement::place(
                tuning.placement,
                &metrics,
                content,
                self.position == IndicatorPosition::Bottom,
            )
        });
        set_live_text(live_text_beside(&placed, &metrics, content, primary_top));
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
