//! The dictation HUD on every OS: a click-through Rune overlay whose
//! compositor steps `dictation_indicator`'s model on Rune's frame clock (the
//! display's refresh on macOS) and draws Dot scope on the GPU with
//! `rune_hud.wgsl`. The overlay never takes
//! focus or a click: a nonactivating panel on macOS, a layer-shell surface on
//! Wayland, an override-redirect window on X11, and a topmost tool window
//! that never activates on Windows.
//!
//! Listeners report through [`HudSender`], which never waits: each event is
//! an overlay update, applied on the event thread before the next frame.
//!
//! The Sounds sheet draws its preview of the HUD with the same [`Painter`].

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod dot_scope;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod hud_placement;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::{activation_policy_changed, pointer_screen, screen_number};
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod rehome;
pub(crate) mod spring;
/// Windows' first-frame watch and its failure backoff; see its module docs.
mod watch;
/// Windows' own HUD frame pacing; see its module docs.
#[cfg(any(target_os = "windows", test))]
mod windows_pacing;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::compose::{Compositor, Frame};
use rune::{Cx, El, LiveRegion, OverlayHandle, OverlayOptions, OverlaySpec, ScreenEdge, el};

use crate::app_settings::IndicatorPosition;
use crate::desktop::i18n::tr;
use crate::dictation::indicator::{DictationIndicatorEvent, DotFrame, IndicatorModel};
use crate::rune_hud::dot_scope::MAX_DOTS;
#[cfg(not(target_os = "macos"))]
use crate::rune_hud::dot_scope::PLATE_HEIGHT;
use crate::rune_hud::hud_placement::{ISLAND_INSET, ISLAND_PAD, Island, Layout};
use crate::rune_ui::live_text::Spot;
#[cfg(target_os = "windows")]
use watch::{FirstFrameCheck, Watch};

#[cfg(target_os = "macos")]
pub(crate) use macos::DictationIndicatorUi;

pub(crate) type HudEvent = DictationIndicatorEvent;

/// The window's margin from the top or bottom edge on Linux and Windows.
#[cfg(not(target_os = "macos"))]
pub(crate) const TOP_OFFSET: f32 = 12.0;
#[cfg(not(target_os = "macos"))]
pub(crate) const BOTTOM_OFFSET: f32 = 24.0;
/// The HUD window: room for Dot scope's widest plate and drop shadow, and
/// on macOS for an island grown out of the widest notch.
#[cfg(target_os = "macos")]
pub(crate) const WINDOW: (f32, f32) = (320.0, 128.0);
#[cfg(not(target_os = "macos"))]
pub(crate) const WINDOW: (f32, f32) = (304.0, 80.0);
/// The position a HUD shows at: a dictation during an Annotate capture
/// takes the bottom edge, clear of the capture capsule at the screen's top
/// (Labs › Dictate while annotating). Hidden stays hidden.
fn place_during_capture(position: IndicatorPosition, capture: bool) -> IndicatorPosition {
    if capture && position != IndicatorPosition::Hidden {
        IndicatorPosition::Bottom
    } else {
        position
    }
}

/// The notch's own bottom corner radius, which the island starts from.
const NOTCH_RADIUS: f32 = 8.0;
/// The open island's bottom corner radius.
const ISLAND_RADIUS: f32 = 18.0;
/// The notch indicator's bottom corner radius once its ears are out.
const EARS_RADIUS: f32 = 14.0;

/// The live text's gap from the HUD's plate.
const LIVE_TEXT_GAP: f32 = 6.0;

/// Where the live text starts, as of the HUD's last show.
static LIVE_TEXT: Mutex<Option<Spot>> = Mutex::new(None);

/// Where the live text plate starts: just past the HUD's plate, on its
/// screen; with the HUD hidden, where its plate would be.
pub(crate) fn live_text_place() -> Spot {
    let spot = *LIVE_TEXT.lock().unwrap_or_else(|error| error.into_inner());
    spot.unwrap_or(Spot::Edge(ScreenEdge::Top, 40.0))
}

fn set_live_text(spot: Spot) {
    *LIVE_TEXT.lock().unwrap_or_else(|error| error.into_inner()) = Some(spot);
}

/// How a listener reports to the HUD; sending never waits.
#[derive(Clone)]
pub(crate) struct HudSender {
    overlay: OverlayHandle<Hud>,
    /// Whether a capture is active, so an older job's failure never covers
    /// a newer recording's live notice.
    recording: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    animating: Arc<AtomicBool>,
    /// `Hud::shown`, so the pump can check the panel's Space without reading
    /// the overlay's state. It flips when a show or hide is requested, before
    /// Rune applies the queued one.
    #[cfg(target_os = "macos")]
    shown: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    watch: Arc<Mutex<Watch>>,
}

impl HudSender {
    pub(crate) fn send(&self, event: HudEvent) {
        if matches!(&event, HudEvent::Started) {
            self.recording.store(true, Ordering::Release);
        } else if matches!(
            &event,
            HudEvent::Submitted { .. }
                | HudEvent::Discarded
                | HudEvent::Cancelled
                | HudEvent::Failed { .. }
                | HudEvent::Reset
        ) {
            self.recording.store(false, Ordering::Release);
        }
        match &event {
            HudEvent::Started => {
                crate::rune_ui::live_text::capture_started();
            }
            HudEvent::Failed {
                message: Some(message),
            } => crate::rune_ui::live_text::show_failure(message),
            HudEvent::JobFailed {
                message: Some(message),
                ..
            } if !self.recording.load(Ordering::Acquire) => {
                crate::rune_ui::live_text::show_failure(message)
            }
            _ => {}
        }
        // A Windows first-frame attempt opens where the HUD's `handle` runs, on
        // the overlay thread, not here: see `Hud::handle` (#1793).
        //
        // A Windows recording sends a meter with each audio chunk, about 100 a
        // second, and `update` asks for a frame for every one. While the HUD's
        // timer is armed it draws the next level itself and its view reads no
        // loudness, so the meter only updates it; a stalled timer draws from
        // the meter instead (`meter_update`).
        #[cfg(target_os = "windows")]
        if matches!(&event, HudEvent::Meter { .. }) {
            self.overlay
                .update_if(move |hud, _| windows_pacing::meter_update(hud, event));
            return;
        }
        self.overlay.update(move |hud, _| hud.handle(event));
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn meter(&self, samples: &[f32]) {
        if let Some(event) = DictationIndicatorEvent::meter(samples) {
            self.send(event);
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_position(&self, position: IndicatorPosition) {
        #[cfg(target_os = "windows")]
        {
            self.watch
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .position = position;
        }
        self.overlay
            .update(move |hud, _| hud.set_position(position));
    }

    /// Why Windows could not show the HUD, for Settings: Rune's reason, or an
    /// attempt that presented no frame. Logged as an error the first time each
    /// reason is seen; what draws the HUD is logged once it exists.
    #[cfg(target_os = "windows")]
    pub(crate) fn failure(&self) -> Option<String> {
        let mut watch = self.watch.lock().unwrap_or_else(|error| error.into_inner());
        let surface = self.overlay.surface_description();
        if !watch.described
            && let Some(surface) = &surface
        {
            tracing::info!(%surface, "dictation HUD surface");
            watch.described = true;
        }
        let failure = self.overlay.failure();
        let FirstFrameCheck {
            reason,
            hide,
            backoff,
        } = watch.first_frame.poll(
            Instant::now(),
            self.overlay.surface_stats().totals.presents,
            failure.as_deref(),
            surface.as_deref(),
        );
        if let Some(cooldown) = backoff {
            // One line per failed attempt: the HUD stops showing, and is not
            // rebuilt, until the cooldown ends (#2175).
            tracing::warn!(
                cooldown_secs = cooldown.as_secs(),
                failures = watch.first_frame.failed_attempts(),
                "the dictation HUD is backing off after a show presented no frame"
            );
        }
        if let Some(reason) = &reason
            && watch.logged.as_deref() != Some(reason.as_str())
        {
            tracing::error!(%reason, "dictation HUD can't show");
            watch.logged = Some(reason.clone());
        }
        if let Some(hide) = hide {
            drop(watch);
            // Rune only gives a shown overlay up when a frame it draws fades it
            // out, and a HUD that never presents draws nothing: it would stay
            // shown until Hex restarts. Hide it at once instead (the immediate
            // hide, not the exit fade), on the overlay's own thread through
            // `update`, so the HUD's shown state clears, Rune releases its
            // window and GPU surface (a zero idle on Windows), and a re-ask or
            // the next start makes both again.
            let watch = self.watch.clone();
            let overlay = self.overlay.clone();
            self.overlay.update(move |hud, _| {
                let watch = watch.lock().unwrap_or_else(|error| error.into_inner());
                // A frame that presented since the poll, or a start that opened
                // a later attempt, owns the HUD now: this hide was decided for a
                // HUD that is up or on its way (#1793).
                if watch
                    .first_frame
                    .should_apply_hide(hide, overlay.surface_stats().totals.presents)
                {
                    drop(watch);
                    hud.hide();
                    // This hide is the one recovery a frame-less show gets, and
                    // the recording that asked for it gets no further show: a
                    // HUD that backs off must not be rebuilt by its own meters
                    // (#1793, #2175).
                    hud.reshown = true;
                    hud.last_drawn = Some(Instant::now());
                }
            });
        }
        let reason = reason?;
        Some(format!(
            "The recording HUD can't show ({reason}). Dictation still works."
        ))
    }

    /// The overlay, to ask whether the platform could show it.
    #[cfg(target_os = "linux")]
    pub(crate) fn availability(&self) -> rune::Availability {
        self.overlay.availability()
    }

    /// Why Rune could not open the overlay, when it says.
    #[cfg(target_os = "linux")]
    pub(crate) fn failure(&self) -> Option<String> {
        self.overlay.failure()
    }
}

/// Whether the HUD wants another frame: it is on screen and something still
/// moves, its own exit included. A hidden HUD draws nothing, so a timer there
/// would only turn the loop for nothing.
#[cfg(any(target_os = "windows", test))]
fn wants_frame(animating: bool, shown: bool) -> bool {
    animating && shown
}

/// What Hex last asked Rune for, together with the placement that only exists
/// while the overlay is up. Rune can still hide the overlay underneath Hex
/// (display unplug, GPU device loss), so each capture's start calls
/// `overlay.show()` again (`Hud::reveal`) instead of trusting `Shown`.
#[derive(Default)]
pub(super) enum HudPresence {
    #[default]
    Hidden,
    Shown {
        #[cfg(target_os = "macos")]
        placed: Option<crate::rune_hud::hud_placement::Placed>,
        /// The position the overlay was last placed for, while it stays on that
        /// display; a capture's start or a hide picks the display again.
        #[cfg(not(target_os = "macos"))]
        edge_placed: Option<IndicatorPosition>,
    },
}

impl HudPresence {
    fn shown() -> Self {
        Self::Shown {
            #[cfg(target_os = "macos")]
            placed: None,
            #[cfg(not(target_os = "macos"))]
            edge_placed: None,
        }
    }
}

/// The overlay's state: the model and what it last drew.
pub(crate) struct Hud {
    model: IndicatorModel,
    drawn: Option<DotFrame>,
    /// Where the plate sits in the window (macOS placements).
    layout: Layout,
    position: IndicatorPosition,
    /// The overlay itself, to show and hide it.
    this: Option<OverlayHandle<Hud>>,
    presence: HudPresence,
    /// Lock-free mirror of `presence` for the macOS main-thread pump's Space
    /// check. Updated only through `set_presence`; the pump also checks
    /// `panel.isVisible()` for the gap before Rune applies a queued show/hide.
    #[cfg(target_os = "macos")]
    shown: Arc<AtomicBool>,
    /// Rune's Reduce Motion, as of the last frame.
    reduce: bool,
    /// Whether the HUD still moves. The compositor reads it everywhere but
    /// Windows, where the overlay's `poll` asks for the next frame instead.
    animating: Arc<AtomicBool>,
    /// The compositor's painter, so a hide that Rune never draws again still
    /// lets go of it (`release_painter`).
    #[cfg(not(target_os = "macos"))]
    gpu: Arc<Mutex<GpuSlot>>,
    /// Windows' first-frame watch, shared with [`HudSender`]: a start opens its
    /// attempt here, where the show is requested, and tags it so the watchdog's
    /// recovery hide can tell whether a later start took the HUD over (#1793).
    /// A failed attempt also backs shows off, so the HUD's window, swapchain
    /// and device are not rebuilt on every dictation (#2175).
    #[cfg(target_os = "windows")]
    watch: Arc<Mutex<Watch>>,
    /// When a frame last drew, or this recording's start opened the clock: a
    /// show Rune declined or Hex hid gets one more ask (with `reshown`), and a
    /// recovery hide restarts it (#1793). Windows' `poll` also arms its timer
    /// from it, so a show nothing draws stops asking.
    #[cfg(any(target_os = "windows", test))]
    last_drawn: Option<Instant>,
    /// Whether the last poll asked for another frame.
    #[cfg(any(target_os = "windows", test))]
    armed: bool,
    /// When the last poll ran: a frame drawn after it keeps the timer going
    /// even when it is older than `RESHOW_AFTER`, because a poll runs before
    /// its frame's render.
    #[cfg(any(target_os = "windows", test))]
    polled_at: Option<Instant>,
    /// When the model last stepped, for the next frame's step: set only by a
    /// frame that steps it, never by a show, and cleared by a hide.
    #[cfg(target_os = "windows")]
    stepped_at: Option<Instant>,
    #[cfg(target_os = "windows")]
    reshown: bool,
    /// When the HUD last checked which display the pointer is on.
    #[cfg(not(target_os = "macos"))]
    followed: Option<Instant>,
}

/// How often a shown HUD checks which display the pointer is on (#807).
#[cfg(not(target_os = "macos"))]
const FOLLOW_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

/// How long a recording's HUD may go without a frame before it asks Rune for its
/// show once more (#1793).
#[cfg(any(target_os = "windows", test))]
const RESHOW_AFTER: Duration = Duration::from_millis(250);

/// Whether a recording's HUD should ask Rune for its show again: no frame has
/// drawn for `RESHOW_AFTER`, and it has not asked already. Meters are a
/// recording's own events, so this is the one chance to recover a show Rune
/// dropped or Hex hid; a recovery hide re-arms it once, which keeps a HUD that
/// can never show from rebuilding its window and surface on every meter.
#[cfg(any(target_os = "windows", test))]
fn reshow_due(since_frame: Option<Duration>, already: bool) -> bool {
    !already && since_frame.is_some_and(|since| since >= RESHOW_AFTER)
}

/// Whether a blocked show (the HUD is backing off a failed show, or this
/// recording's own attempt already presented nothing) hides the HUD: always on
/// a fresh capture's start (tearing down what an older recording left, while
/// building nothing), otherwise only when none is up. A first frame that
/// arrives after its attempt resolved frame-less keeps its backoff for later
/// dictations, but must not take the HUD away from the recording watching it
/// (#2175).
#[cfg(any(target_os = "windows", test))]
fn blocked_show_hides(fresh_start: bool, shown: bool) -> bool {
    fresh_start || !shown
}

impl Hud {
    pub(super) fn is_shown(&self) -> bool {
        matches!(self.presence, HudPresence::Shown { .. })
    }

    fn set_presence(&mut self, presence: HudPresence) {
        #[cfg(target_os = "macos")]
        self.shown.store(
            matches!(presence, HudPresence::Shown { .. }),
            Ordering::Release,
        );
        self.presence = presence;
    }

    fn clear_placement(&mut self) {
        if self.is_shown() {
            self.presence = HudPresence::shown();
        }
    }

    /// While shown, every `FOLLOW_EVERY`: the HUD and the live text move to
    /// the display under the pointer when it moved to another, at once and
    /// without focus. Windows and X11; a layer-shell surface stays on the
    /// compositor's focused output, and GNOME and KDE on Wayland on the
    /// primary.
    #[cfg(not(target_os = "macos"))]
    fn follow_pointer(&mut self, now: Instant) {
        if !self.is_shown() || self.followed.is_some_and(|at| now < at + FOLLOW_EVERY) {
            return;
        }
        self.followed = Some(now);
        if let Some(overlay) = &self.this {
            overlay.refollow_pointer();
        }
        crate::rune_ui::live_text::refollow_pointer();
    }

    fn handle(&mut self, event: HudEvent) {
        let starts = matches!(&event, HudEvent::Started);
        let meter = matches!(&event, HudEvent::Meter { .. });
        // A capture's start (or dot-scope change) sends the display and
        // placement to Rune again, so a placement Rune resolved wrong on
        // another screen (#873) doesn't stick.
        #[cfg(target_os = "macos")]
        if matches!(&event, HudEvent::ConfigureDotScope(_) | HudEvent::Started) {
            self.clear_placement();
        }
        #[cfg(not(target_os = "macos"))]
        if starts {
            self.clear_placement();
        }
        // A start's own first frame has to arrive before a recovery re-ask can
        // call it late: the frame the previous dictation left is not this one's,
        // and one ask is all a recording gets (#1793). Windows' `poll` arms its
        // timer from this too, so the first frame after the show asks for the
        // next even though `last_drawn` only moves again once a frame draws.
        #[cfg(any(target_os = "windows", test))]
        if starts {
            self.last_drawn = Some(Instant::now());
            #[cfg(target_os = "windows")]
            {
                self.reshown = false;
            }
        }
        // The first-frame attempt opens here, where the show is requested, and
        // not where the listener sent the start. That takes the listener thread
        // and the event queue out of the budget, so it measures the show Rune
        // runs, not the wait in front of it. It does not make a slow show fast:
        // the construction Rune does per show is what a frame-less attempt
        // reports (#1793).
        #[cfg(target_os = "windows")]
        if starts && let Some(overlay) = &self.this {
            let presents = overlay.surface_stats().totals.presents;
            let mut watch = self.watch.lock().unwrap_or_else(|error| error.into_inner());
            if watch.position != IndicatorPosition::Hidden {
                // Every start opens a fresh attempt: its own baseline, clock
                // and hide, even when the last one never resolved. A refused
                // start (the HUD is backing off) opens none, so there is no
                // first frame to wait for: its recording spends its one re-ask
                // up front, and the show below builds nothing (#2175).
                if !watch.first_frame.start(Instant::now(), presents) {
                    self.reshown = true;
                }
            }
        }
        self.model.handle(event, Instant::now(), self.reduce);
        self.animating.store(true, Ordering::Release);
        // A recording's own events are meters (the listener sends `Started`
        // once, then one per chunk), so a HUD lost after its start has nothing
        // left that would ask for it again. Ask once, after its frames stop:
        // that recovers a show that failed in setup or that Hex hid, and it
        // can't rebuild the window and surface on every meter for a HUD that
        // can never show (#1793).
        #[cfg(target_os = "windows")]
        if meter
            && self.model.recording()
            && reshow_due(
                self.last_drawn
                    .map(|at| Instant::now().saturating_duration_since(at)),
                self.reshown,
            )
            && !self.show_blocked(Instant::now())
        {
            self.reshown = true;
            self.reveal(false);
        }
        if starts {
            self.reveal(true);
        } else if !meter {
            self.sync_placement();
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn set_position(&mut self, position: IndicatorPosition) {
        self.position = position;
        self.sync_placement();
    }

    /// Whether the HUD must not be shown: it is backing off a failed show, or
    /// this recording's own attempt already presented nothing. A show then only
    /// builds another window, swapchain and device for a HUD that never
    /// presents, on the thread that pumps Windows' messages (#2175).
    #[cfg(target_os = "windows")]
    fn show_blocked(&self, now: Instant) -> bool {
        let watch = self.watch.lock().unwrap_or_else(|error| error.into_inner());
        watch.first_frame.backing_off(now) || watch.first_frame.failed()
    }

    /// Resolves where the HUD should sit, or `None` when Hidden (after placing
    /// live text where the plate would be).
    fn visible_position(&mut self) -> Option<IndicatorPosition> {
        #[cfg(target_os = "macos")]
        {
            self.position = crate::app_settings::indicator_position();
        }
        // A capture records with its capsule at the screen's top, so a
        // dictation during one shows at the bottom, clear of it (Labs ›
        // Dictate while annotating).
        let position = place_during_capture(self.position, crate::capture_host::recording());
        if position == IndicatorPosition::Hidden {
            #[cfg(target_os = "macos")]
            self.place_live_text_without_hud();
            #[cfg(not(target_os = "macos"))]
            {
                let plate_start = TOP_OFFSET + (WINDOW.1 - PLATE_HEIGHT) / 2.0;
                set_live_text(Spot::Edge(ScreenEdge::Top, plate_start));
            }
            return None;
        }
        #[cfg(target_os = "macos")]
        {
            self.position = position;
        }
        Some(position)
    }

    fn apply_placement(&mut self, overlay: &OverlayHandle<Hud>, position: IndicatorPosition) {
        #[cfg(target_os = "macos")]
        {
            let _ = position;
            self.place_on_pointer_screen(overlay);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let (edge, margin) = match position {
                IndicatorPosition::Bottom => (ScreenEdge::Bottom, BOTTOM_OFFSET),
                _ => (ScreenEdge::Top, TOP_OFFSET),
            };
            // Placing picks the display again (`display`); while shown, the
            // HUD follows the pointer to another display at its own pace
            // (`follow_pointer`).
            if let HudPresence::Shown { edge_placed } = &mut self.presence
                && *edge_placed != Some(position)
            {
                overlay.place(edge, margin);
                *edge_placed = Some(position);
            }
            // The plate sits in the middle of the window.
            let past_plate = margin + (WINDOW.1 + PLATE_HEIGHT) / 2.0 + LIVE_TEXT_GAP;
            set_live_text(Spot::Edge(edge, past_plate));
        }
    }

    /// Asks Rune to show the overlay where the HUD belongs, unless Hidden or
    /// blocked on Windows. Calls `overlay.show()` unconditionally instead of
    /// trusting `self.presence`: Rune can take the overlay away underneath Hex
    /// (display unplug, GPU device loss, #1663, #1793), and Rune's own `show()`
    /// is a no-op when already shown and cancels an exit fade.
    fn reveal(&mut self, fresh_start: bool) {
        let Some(overlay) = self.this.clone() else {
            return;
        };
        let Some(position) = self.visible_position() else {
            overlay.hide();
            self.hide();
            return;
        };
        #[cfg(target_os = "windows")]
        if self.show_blocked(Instant::now()) {
            // Blocked, the HUD makes nothing: no `overlay.show()`, and with it
            // no window, swapchain or device to build while a saturated GPU
            // keeps the last one from presenting (#2175).
            if blocked_show_hides(fresh_start, self.is_shown()) {
                overlay.hide();
                self.hide();
            }
            return;
        }
        if fresh_start || !self.is_shown() {
            self.set_presence(HudPresence::shown());
        }
        self.apply_placement(&overlay, position);
        overlay.show();
    }

    /// Updates a shown HUD's placement (or hides it if Settings switched to
    /// Hidden) without asking Rune to show again.
    pub(super) fn sync_placement(&mut self) {
        if !self.is_shown() {
            return;
        }
        let Some(overlay) = self.this.clone() else {
            return;
        };
        let Some(position) = self.visible_position() else {
            overlay.hide();
            self.hide();
            return;
        };
        self.apply_placement(&overlay, position);
    }

    /// Hides the overlay once its exit has settled, fading the window out
    /// on Windows (macOS hides at once, as before) so frames still queued
    /// for the screen reach it before the window and its GPU surface go
    /// (#1359).
    fn fade_out(&mut self) {
        self.dismiss(true);
    }

    fn hide(&mut self) {
        self.dismiss(false);
    }

    fn dismiss(&mut self, fade: bool) {
        #[cfg(target_os = "windows")]
        {
            // The next show starts a new step, not one from the frame before
            // the hide (`wall_dt`), and its timer arms afresh (`poll`).
            self.armed = false;
            self.polled_at = None;
            self.stepped_at = None;
        }
        let was_shown = self.is_shown();
        self.set_presence(HudPresence::Hidden);
        if was_shown {
            if let Some(overlay) = &self.this {
                #[cfg(target_os = "windows")]
                if fade {
                    overlay.fade_out(EXIT_FADE);
                } else {
                    overlay.hide();
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = fade;
                    overlay.hide();
                }
            }
            #[cfg(not(target_os = "macos"))]
            release_painter(&self.gpu);
        }
    }
}

/// How long the Windows HUD's window fades after its exit settles.
#[cfg(target_os = "windows")]
const EXIT_FADE: Duration = Duration::from_millis(120);

/// How long the hidden HUD keeps its GPU surface: the device it holds costs
/// about 150 MB and the GPU driver's threads waking some 65 times a second.
/// Windows gives it back, window included, once the HUD's exit fade ends: a hidden
/// topmost window that presented with DXGI let NVIDIA's Instant Replay take
/// Hex for the running game (#1308). The device itself waits five minutes for
/// the next show either way, so dictations reuse one device (#1268). The
/// painter goes once Rune's parked device would also have gone, which is this
/// plus the five minutes Rune parks a device (`PAINTER_IDLE`, #1431); macOS
/// keeps it.
#[cfg(not(target_os = "windows"))]
const GPU_IDLE: Duration = Duration::from_secs(5 * 60);
#[cfg(target_os = "windows")]
const GPU_IDLE: Duration = Duration::ZERO;

/// How long the hidden HUD keeps its painter on Windows and Linux, and with it
/// its own hold on the GPU device: Rune holds the surface for `GPU_IDLE` and
/// then parks the device for five more minutes, so the painter goes once that
/// parked device would also have gone and the device actually frees. A burst
/// of dictations within that time reuses one painter (#1431).
#[cfg(not(target_os = "macos"))]
const PAINTER_IDLE: Duration = Duration::from_secs(GPU_IDLE.as_secs() + 5 * 60);

/// The format the HUD draws in on Windows and Linux (Rune drops the
/// surface's sRGB suffix for `srgb_blend`).
const HUD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// A painter made ahead of the HUD's first frame on a device `prepare_gpu`
/// was given.
static PREBUILT: Mutex<Option<(wgpu::Device, Painter)>> = Mutex::new(None);

/// Makes the HUD's pipeline on `device` off the UI thread, for the HUD's next
/// first frame: compiling it there takes 75 to 115 ms on Windows. Unclaimed,
/// it goes after a while, since it keeps the device alive.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn prepare_gpu(device: &wgpu::Device) {
    let lock = || PREBUILT.lock().unwrap_or_else(|error| error.into_inner());
    if lock().as_ref().is_some_and(|(made, _)| made == device) {
        return;
    }
    let painter = Painter::new(device, HUD_FORMAT);
    *lock() = Some((device.clone(), painter));
    let device = device.clone();
    let _ = std::thread::Builder::new()
        .name("hud-prepared".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(70));
            let mut slot = lock();
            if slot.as_ref().is_some_and(|(made, _)| *made == device) {
                slot.take();
            }
        });
}

/// `prepare_gpu`'s painter, when it was made on `device` for `format`.
fn prebuilt(device: &wgpu::Device, format: wgpu::TextureFormat) -> Option<Painter> {
    let mut slot = PREBUILT.lock().unwrap_or_else(|error| error.into_inner());
    slot.as_ref()
        .is_some_and(|(made, painter)| made == device && painter.format == format)
        .then(|| slot.take().map(|(_, painter)| painter))
        .flatten()
}

/// Adds the HUD's overlay to the app; send its events through the returned
/// sender.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> HudSender {
    let animating = Arc::new(AtomicBool::new(false));
    #[cfg(target_os = "macos")]
    let shown = Arc::new(AtomicBool::new(false));
    let now = Instant::now();
    let model = IndicatorModel::new(dot_scope::lab(), now);
    // The painter the compositor draws with and the HUD lets go of when it
    // hides.
    #[cfg(not(target_os = "macos"))]
    let gpu: Arc<Mutex<GpuSlot>> = Arc::default();
    // Windows' first-frame watch: the HUD's own state opens each attempt and
    // tags it, and the sender polls it from the runtime thread (#1793).
    #[cfg(target_os = "windows")]
    let watch: Arc<Mutex<Watch>> = Arc::default();
    let configured = OverlaySpec {
        name: "hex-hud",
        size: WINDOW,
        edge: ScreenEdge::Top,
        margin: 0.0,
        align: rune::Align::Center,
        inset: 0.0,
        interactive: false,
        view,
    }
    .options(OverlayOptions {
        // The first press never waits on the window: macOS keeps the
        // panel drawn at alpha zero; elsewhere only the window waits,
        // and its GPU surface comes with the show (`release_gpu_after`).
        prewarm: true,
        #[cfg(not(target_os = "macos"))]
        display: display(),
        ..OverlayOptions::default()
    })
    // The shaders write display (sRGB-encoded) values.
    .srgb_blend(true)
    // On Linux a burst of dictations keeps the GPU surface, so each HUD
    // shows at once; a Hex at rest gives the GPU device back.
    .release_gpu_after(GPU_IDLE);
    // Windows' overlay has no display link: Hex caps the HUD at ~60 Hz on top
    // of Rune's swap-chain slot wait (`windows_pacing`).
    #[cfg(target_os = "windows")]
    let configured = configured.poll(windows_pacing::poll);
    let overlay = app.overlay(
        configured.compositor(HudCompositor {
            #[cfg(not(target_os = "macos"))]
            gpu: gpu.clone(),
            #[cfg(target_os = "macos")]
            gpu: None,
            #[cfg(not(target_os = "windows"))]
            animating: animating.clone(),
        }),
        Hud {
            model,
            drawn: None,
            layout: Layout::default(),
            position: IndicatorPosition::Top,
            this: None,
            presence: HudPresence::Hidden,
            #[cfg(target_os = "macos")]
            shown: shown.clone(),
            reduce: false,
            animating: animating.clone(),
            #[cfg(not(target_os = "macos"))]
            followed: None,
            #[cfg(not(target_os = "macos"))]
            gpu,
            #[cfg(target_os = "windows")]
            watch: watch.clone(),
            #[cfg(any(target_os = "windows", test))]
            last_drawn: None,
            #[cfg(any(target_os = "windows", test))]
            armed: false,
            #[cfg(any(target_os = "windows", test))]
            polled_at: None,
            #[cfg(target_os = "windows")]
            stepped_at: None,
            #[cfg(target_os = "windows")]
            reshown: false,
        },
    );
    let this = overlay.clone();
    overlay.update(move |hud, _| hud.this = Some(this));
    HudSender {
        overlay,
        recording: Arc::default(),
        #[cfg(target_os = "macos")]
        animating,
        #[cfg(target_os = "macos")]
        shown,
        #[cfg(target_os = "windows")]
        watch,
    }
}

/// The display the HUD and live text appear on, picked at each capture's
/// start: the one under the pointer, as on macOS (#486). Windows and X11 read
/// the pointer and pick its monitor; a layer-shell surface pins no output, so
/// the compositor puts it on the focused one. GNOME (whose HUD runs on
/// XWayland, blind to the pointer over Wayland windows) and KDE keep the
/// primary display on Wayland; their X11 sessions follow the pointer (#1200).
#[cfg(not(target_os = "macos"))]
pub(crate) fn display() -> rune::DisplayTarget {
    #[cfg(target_os = "linux")]
    if keeps_primary_display(
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        crate::linux::session::LinuxSession::detect().is_wayland(),
    ) {
        return rune::DisplayTarget::Primary;
    }
    rune::DisplayTarget::UnderPointer
}

/// Whether the HUD stays on the primary display: GNOME and KDE on Wayland
/// only. On X11 the pointer is readable on every desktop.
#[cfg(any(target_os = "linux", test))]
fn keeps_primary_display(desktops: Option<&str>, wayland: bool) -> bool {
    wayland
        && desktops.is_some_and(|desktops| {
            desktops.split(':').any(|desktop| {
                ["GNOME", "KDE"]
                    .iter()
                    .any(|d| desktop.eq_ignore_ascii_case(d))
            })
        })
}

/// The HUD's one element, which the compositor draws over. Screen readers
/// read it as a status that announces Recording and Transcribing as they
/// begin; it takes no input, as the overlay is click-through.
fn view(hud: &Hud, _: &mut Cx<Hud>) -> El {
    let status = hud.model.status().map_or("", tr);
    el().key("hud")
        .w_full()
        .h_full()
        .role("status")
        .label(status)
        .description(tr("Dictation"))
        .live_region(LiveRegion::Polite)
}

/// `DotUniforms` in `rune_hud.wgsl`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct DotUniforms {
    resolution: [f32; 2],
    backing_scale: f32,
    width: f32,
    height: f32,
    opacity: f32,
    scale: f32,
    softness: f32,
    pitch: f32,
    columns: f32,
    rows: f32,
    glow: f32,
    grid: f32,
    live: f32,
    cool: f32,
    dark: f32,
    base: [f32; 4],
    core: [f32; 4],
    plate: [f32; 4],
    rim: [f32; 4],
    shadow: [f32; 4],
    plate_layout: [f32; 4],
    island: [f32; 4],
    field: [f32; 4],
    /// The notch indicator's done and failed, 0 to 1, then how far right
    /// of the plate's middle the dots' middle sits, pt.
    ears: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<DotUniforms>() == 208);
const DOT_UNIFORM_BYTES: u32 = std::mem::size_of::<DotUniforms>() as u32;
const DOT_BYTES: u64 = (MAX_DOTS * 4) as u64;
/// How `rune_hud.wgsl` declares `u` as a uniform buffer; with immediates it
/// is declared `var<immediate>` instead.
const DOT_UNIFORM_BINDING: &str = "@group(0) @binding(0) var<uniform> u: DotUniforms;";

impl DotUniforms {
    fn new(frame: &DotFrame, layout: Layout, target: Target) -> Self {
        let pose = &frame.pose;
        // The HUD is always a dark surface, including inside a light preview.
        let mut colors = dot_scope::palette(true, pose, frame.tuning.saturation);
        if layout.island.is_some() {
            for color in [&mut colors.plate, &mut colors.rim, &mut colors.shadow] {
                color[3] = 0.0;
            }
        }
        let mut center = layout.center;
        if layout.anchor_left {
            center[0] += pose.width * layout.fit / 2.0;
        }
        let rgba = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
        Self {
            resolution: [target.viewport[2], target.viewport[3]],
            backing_scale: target.scale,
            width: pose.width,
            height: pose.height,
            opacity: pose.opacity,
            scale: pose.scale,
            softness: pose.softness,
            pitch: frame.tuning.spacing,
            columns: frame.columns as f32,
            rows: frame.rows as f32,
            glow: pose.glow,
            grid: pose.grid,
            live: pose.live,
            cool: pose.cool,
            dark: 1.0,
            base: rgba(colors.base),
            core: rgba(colors.core),
            plate: colors.plate,
            rim: colors.rim,
            shadow: colors.shadow,
            plate_layout: [
                center[0],
                center[1],
                layout.fit,
                if layout.island.is_some() { 1.0 } else { 0.0 },
            ],
            island: layout
                .island
                .map_or([0.0; 4], |island| island_shape(island, pose)),
            // The ears draw glyphs, not the field: the notch's half width,
            // the mode, the voice, and the stage's clock.
            field: match layout.island {
                Some(island) if island.ears => [island.notch[0] / 2.0, 1.0, pose.level, pose.clock],
                _ => [field_radius(layout, pose), 0.0, 0.0, 0.0],
            },
            ears: [pose.done, pose.fail, pose.shift, 0.0],
        }
    }
}

/// The island for this pose: the notch itself while hidden, grown to hold
/// the plate as it shows, bouncing with it.
fn island_shape(island: Island, pose: &dot_scope::Pose) -> [f32; 4] {
    let [notch_width, notch_height] = island.notch;
    if island.ears {
        // Grows from the housing's height to the menu bar's, widening to the
        // island's width; folded back, it is the notch itself again.
        let half_width = notch_width / 2.0;
        let grow = pose.reveal;
        let bar = if island.bar > 0.0 {
            island.bar
        } else {
            notch_height
        };
        let height = notch_height + (bar - notch_height) * grow.min(1.0);
        return [
            half_width + (island.width / 2.0 - half_width) * grow,
            height,
            island.top,
            (NOTCH_RADIUS + (EARS_RADIUS - NOTCH_RADIUS) * grow.min(1.0)).min(height / 2.0),
        ];
    }
    let pad = ISLAND_PAD as f32;
    let grow = pose.reveal;
    let half_width = notch_width / 2.0;
    let open_half_width = (pose.width / 2.0 + ISLAND_INSET as f32).max(half_width);
    let open_height = notch_height + 2.0 * pad + pose.height;
    let height = notch_height + (open_height - notch_height) * grow;
    [
        half_width + (open_half_width - half_width) * grow,
        height,
        island.top,
        (NOTCH_RADIUS + (ISLAND_RADIUS - NOTCH_RADIUS) * grow.min(1.0)).min(height / 2.0),
    ]
}

/// The field's corner radius: a pill on a plate, and in an island its
/// corners follow the island's, the same distance in all round.
fn field_radius(layout: Layout, pose: &dot_scope::Pose) -> f32 {
    let half_height = pose.height / 2.0;
    match layout.island {
        Some(_) => (ISLAND_RADIUS - ISLAND_PAD as f32).min(half_height),
        None => half_height,
    }
}

fn bytes<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: `T` is a `repr(C)` struct of `f32`s with no padding, so every
    // byte is initialized.
    unsafe { std::slice::from_raw_parts((value as *const T).cast(), std::mem::size_of::<T>()) }
}

fn packed_dots(frame: &DotFrame) -> Vec<u8> {
    frame
        .dots
        .iter()
        .take(MAX_DOTS)
        .flat_map(|[power, fresh]| [power.to_le_bytes(), fresh.to_le_bytes()])
        .flatten()
        .collect()
}

/// What a frame draws into.
#[derive(Clone, Copy)]
pub(crate) struct Target {
    /// The HUD's window in the target, x, y, width and height in device
    /// pixels.
    pub(crate) viewport: [f32; 4],
    /// The pixels it may touch, x, y, width and height, inside the target.
    pub(crate) clip: [u32; 4],
    /// Device pixels per point.
    pub(crate) scale: f32,
}

impl Target {
    /// All of a target `size` device pixels big.
    fn whole(size: (u32, u32), scale: f32) -> Self {
        Self {
            viewport: [0.0, 0.0, size.0 as f32, size.1 as f32],
            clip: [0, 0, size.0, size.1],
            scale,
        }
    }
}

struct HudCompositor {
    /// The painter and the device its buffers live on: an overlay shown
    /// again can draw on another device (#982). Keyed by Rune's device key,
    /// since wgpu compares a new Rune GPU's device equal to the old one.
    ///
    /// Windows and Linux let go of it once the HUD has been hidden for
    /// `PAINTER_IDLE`, and with it the GPU device its buffers hold; macOS
    /// keeps it (#1431).
    #[cfg(not(target_os = "macos"))]
    gpu: Arc<Mutex<GpuSlot>>,
    #[cfg(target_os = "macos")]
    gpu: Option<(rune::compose::DeviceKey, Painter)>,
    /// Whether the HUD moves. Windows doesn't read it: the overlay asks for one
    /// frame at a time there (`poll`), and `Hud` holds the same flag.
    #[cfg(not(target_os = "windows"))]
    animating: Arc<AtomicBool>,
}

/// What the HUD draws with, shared so it can be let go of while the HUD is
/// hidden (#1431).
#[cfg(not(target_os = "macos"))]
#[derive(Default)]
struct GpuSlot {
    painter: Option<(rune::compose::DeviceKey, Painter)>,
    /// Bumped by every frame the painter draws, so a release thread drops it
    /// only when the HUD drew nothing since it started.
    generation: u64,
    /// The generation a release thread already waits on.
    releasing: Option<u64>,
}

/// The painter that draws `f`, made on its device and format when `gpu` has
/// none for them.
fn painter<'a>(
    gpu: &'a mut Option<(rune::compose::DeviceKey, Painter)>,
    f: &Frame<'_, Hud>,
) -> &'a Painter {
    if gpu
        .as_ref()
        .is_none_or(|(device, painter)| *device != f.device_key() || painter.format != f.format)
    {
        tracing::info!(gpu = %rune::describe_gpu(f.device), "HUD draws");
        let painter =
            prebuilt(f.device, f.format).unwrap_or_else(|| Painter::new(f.device, f.format));
        *gpu = Some((f.device_key(), painter));
    }
    &gpu.as_ref().expect("created above").1
}

impl HudCompositor {
    /// Writes and draws the frame's dots; the lock keeps a pending release
    /// from dropping the painter mid-frame.
    #[cfg(not(target_os = "macos"))]
    fn draw(&self, f: &mut Frame<'_, Hud>, drawn: &DotFrame, layout: Layout, target: Target) {
        let mut gpu = self.gpu.lock().unwrap_or_else(|error| error.into_inner());
        gpu.generation = gpu.generation.wrapping_add(1);
        let painter = painter(&mut gpu.painter, f);
        paint(painter, f, drawn, layout, target);
    }
}

/// Lets go of the painter, and with its buffers, bind group and pipeline the
/// GPU device they hold (#1431): Windows and Linux keep it for `PAINTER_IDLE`
/// hidden, so the device frees once Rune's parked device also goes and a burst
/// of dictations within that time reuses one painter. Both the compositor when
/// the HUD stops drawing and a hide Rune never draws again call it.
#[cfg(not(target_os = "macos"))]
fn release_painter(gpu: &Arc<Mutex<GpuSlot>>) {
    let mut slot = gpu.lock().unwrap_or_else(|error| error.into_inner());
    let generation = slot.generation;
    if slot.painter.is_none() || slot.releasing == Some(generation) {
        return;
    }
    slot.releasing = Some(generation);
    drop(slot);
    let slot = gpu.clone();
    let _ = std::thread::Builder::new()
        .name("hud-release".into())
        .spawn(move || {
            std::thread::sleep(PAINTER_IDLE);
            let mut slot = slot.lock().unwrap_or_else(|error| error.into_inner());
            if slot.generation == generation {
                slot.painter = None;
            }
            if slot.releasing == Some(generation) {
                slot.releasing = None;
            }
        });
}

/// Writes the frame's dots through `painter` and draws them.
fn paint(
    painter: &Painter,
    f: &mut Frame<'_, Hud>,
    drawn: &DotFrame,
    layout: Layout,
    target: Target,
) {
    let uniforms = painter.write(drawn, layout, target, |buffer, data| {
        f.write_buffer(buffer, 0, data)
    });
    painter.draw(f.encoder, f.target, &uniforms);
}

impl Compositor<Hud> for HudCompositor {
    fn features(&self) -> wgpu::Features {
        Painter::FEATURES
    }

    fn animating(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            // No display link paces a Windows overlay, so reporting `true`
            // would keep the loop at `ControlFlow::Poll`; Rune #985 bounds
            // frames in flight, and the HUD's `poll` caps it at a fixed ~60 Hz
            // timer, not the display's refresh.
            false
        }
        #[cfg(not(target_os = "windows"))]
        {
            self.animating.load(Ordering::Acquire)
        }
    }

    /// The view is empty, so Rune's UI pass is skipped: Hex's own pass
    /// clears the target.
    fn render(&mut self, f: &mut Frame<'_, Hud>) {
        let reduce = f.rt.reduced_motion;
        let target = Target::whole(f.size, f.rt.env.scale);
        let hud = &mut f.rt.state;
        hud.reduce = reduce;
        let now = Instant::now();
        if hud.model.active() {
            // Windows paces the HUD with its own timer, and Rune caps the `dt`
            // it hands a compositor at one resume step, so `f.dt` is not the
            // real gap there (`wall_dt`).
            #[cfg(target_os = "windows")]
            let dt = windows_pacing::wall_dt(hud.stepped_at, now, f.dt as f32);
            #[cfg(not(target_os = "windows"))]
            let dt = f.dt as f32;
            hud.drawn = Some(hud.model.step(now, dt, reduce));
            #[cfg(target_os = "windows")]
            {
                hud.stepped_at = Some(now);
            }
        }
        let active = hud.model.active();
        hud.animating.store(active, Ordering::Release);
        #[cfg(not(target_os = "macos"))]
        if active {
            hud.follow_pointer(Instant::now());
        }
        if !active {
            hud.fade_out();
            drop(clear_pass(f.encoder, f.target));
            return;
        }
        let (Some(drawn), layout) = (hud.drawn.take(), hud.layout) else {
            drop(clear_pass(f.encoder, f.target));
            return;
        };
        // The frame clock a recording's HUD is judged by: see `reshow_due`, and
        // the reference Windows' `poll` arms its timer from. `wall_dt` measures
        // from `stepped_at`, which only a stepping frame sets.
        #[cfg(target_os = "windows")]
        {
            hud.last_drawn = Some(now);
        }
        #[cfg(target_os = "macos")]
        paint(painter(&mut self.gpu, f), f, &drawn, layout, target);
        #[cfg(not(target_os = "macos"))]
        self.draw(f, &drawn, layout, target);
        f.rt.state.drawn = Some(drawn);
    }
}

/// The HUD's shaders, ready to draw into targets of one format.
pub(crate) struct Painter {
    pub(crate) format: wgpu::TextureFormat,
    /// `None` when `DotUniforms` travel as immediates, set in the pass with
    /// no buffer write.
    dot_uniforms: Option<wgpu::Buffer>,
    dots: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl Painter {
    /// What a compositor drawing with a `Painter` asks its device for.
    pub(crate) const FEATURES: wgpu::Features = wgpu::Features::IMMEDIATES;

    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Painter {
        Self::with_color_space(device, format, format.is_srgb())
    }

    /// Rune's isolated layers may be linear float targets rather than sRGB
    /// textures; the compositor supplies their blend space explicitly.
    pub(crate) fn with_color_space(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        linear: bool,
    ) -> Painter {
        Self::pipeline(device, format, linear, false)
    }

    /// A field drawn directly in an existing surface. The dot geometry and
    /// colour response are the HUD's; nothing paints between the dots.
    pub(crate) fn dots_only(device: &wgpu::Device, format: wgpu::TextureFormat) -> Painter {
        Self::pipeline(device, format, format.is_srgb(), true)
    }

    fn pipeline(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        linear: bool,
        dots_only: bool,
    ) -> Painter {
        let immediates = device.features().contains(wgpu::Features::IMMEDIATES)
            && device.limits().max_immediate_size >= DOT_UNIFORM_BYTES;
        let source = include_str!("rune_hud.wgsl");
        assert!(source.contains(DOT_UNIFORM_BINDING));
        let source = if immediates {
            source.replace(DOT_UNIFORM_BINDING, "var<immediate> u: DotUniforms;")
        } else {
            source.to_owned()
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hex hud"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let buffer = |label, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let dot_uniforms =
            (!immediates).then(|| buffer("hex hud dot uniforms", u64::from(DOT_UNIFORM_BYTES)));
        let dots = buffer("hex hud dots", DOT_BYTES);
        let entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mut entries = vec![entry(1)];
        let mut bindings = vec![wgpu::BindGroupEntry {
            binding: 1,
            resource: dots.as_entire_binding(),
        }];
        if let Some(buffer) = &dot_uniforms {
            entries.push(entry(0));
            bindings.push(wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            });
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hex hud"),
            entries: &entries,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hex hud"),
            layout: &layout,
            entries: &bindings,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hex hud"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: if immediates { DOT_UNIFORM_BYTES } else { 0 },
        });
        // Display values; a linear target decodes them.
        let constants = [
            ("linear_target", f64::from(linear)),
            ("dots_only", f64::from(dots_only)),
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dot_scope_fragment"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("indicator_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("dot_scope_fragment"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Painter {
            format,
            dot_uniforms,
            dots,
            bind,
            pipeline,
        }
    }

    /// Writes `frame`'s buffers through `write` and returns its uniforms,
    /// for the draw.
    pub(crate) fn write(
        &self,
        frame: &DotFrame,
        layout: Layout,
        target: Target,
        mut write: impl FnMut(&wgpu::Buffer, &[u8]),
    ) -> DotUniforms {
        let uniforms = DotUniforms::new(frame, layout, target);
        if let Some(buffer) = &self.dot_uniforms {
            write(buffer, bytes(&uniforms));
        }
        write(&self.dots, &packed_dots(frame));
        uniforms
    }

    /// Clears `view` and draws Dot scope into it.
    pub(crate) fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        uniforms: &DotUniforms,
    ) {
        self.draw_in(&mut clear_pass(encoder, view), uniforms);
    }

    fn draw_in(&self, pass: &mut wgpu::RenderPass<'_>, uniforms: &DotUniforms) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        if self.dot_uniforms.is_none() {
            pass.set_immediates(0, bytes(uniforms));
        }
        pass.draw(0..3, 0..1);
    }

    /// Draws Dot scope over what `view` holds, in `target`'s viewport and
    /// clip only.
    pub(crate) fn draw_over(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        target: Target,
        uniforms: &DotUniforms,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("hex hud preview"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.draw_slot(&mut pass, target, uniforms);
    }

    /// Draw inside Rune's effect-slot pass, preserving its painter order and
    /// the containing layer's opacity.
    pub(crate) fn draw_slot(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        target: Target,
        uniforms: &DotUniforms,
    ) {
        let [x, y, width, height] = target.viewport;
        pass.set_viewport(x, y, width, height, 0.0, 1.0);
        let [cx, cy, cw, ch] = target.clip;
        pass.set_scissor_rect(cx, cy, cw, ch);
        self.draw_in(pass, uniforms);
    }
}

/// A pass over `view` that starts transparent.
fn clear_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("hex hud"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

#[cfg(test)]
mod tests;
