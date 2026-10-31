//! The Linux recording HUD: the shared Rune HUD (`rune_hud`), the capsule or
//! Dot scope drawn on the GPU, as an overlay in the Hex process.
//! Rune makes it a layer-shell surface on Wayland (overlay layer, no
//! keyboard, empty input region) and an override-redirect window with an
//! empty input shape on X11, so it never takes focus or a click. On a
//! Wayland compositor without layer-shell (GNOME), Rune opens only the HUD
//! as that X11 window on XWayland; the rest of Hex stays on Wayland.
//!
//! It only observes: the listener reports its capture, meter, and pending
//! output through `LinuxIndicator`, which never waits on the HUD and turns
//! those reports into the HUD's events. The listener can start before the
//! window's app exists; reports before `install` are dropped.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use rune::Availability;

use crate::app_settings::IndicatorPosition;
use crate::rune_hud::{HudEvent, HudSender};

/// The newest listener's generation: an older listener's reports stop counting.
static GENERATION: Mutex<u64> = Mutex::new(0);
/// Where the newest listener wants the HUD, for an `install` after it starts.
static POSITION: Mutex<IndicatorPosition> = Mutex::new(IndicatorPosition::Top);
static HUD: OnceLock<HudSender> = OnceLock::new();

/// A listener's handle on the HUD; dropping it hides what it reported.
pub(crate) struct LinuxIndicator {
    generation: Option<u64>,
    recording: bool,
    /// Submitted dictations still being transcribed or pasted, oldest first.
    jobs: VecDeque<u64>,
    next_job: u64,
}

impl LinuxIndicator {
    pub(crate) fn new(position: IndicatorPosition) -> Self {
        *POSITION.lock().unwrap_or_else(|error| error.into_inner()) = position;
        if let Some(sender) = HUD.get() {
            sender.set_position(position);
        }
        let generation = (position != IndicatorPosition::Hidden).then(|| {
            let mut current = GENERATION.lock().unwrap_or_else(|error| error.into_inner());
            *current += 1;
            *current
        });
        let indicator = Self {
            generation,
            recording: false,
            jobs: VecDeque::new(),
            next_job: 0,
        };
        indicator.send(HudEvent::Reset);
        indicator
    }

    fn sender(&self) -> Option<&'static HudSender> {
        let generation = self.generation?;
        let current = *GENERATION.lock().unwrap_or_else(|error| error.into_inner());
        (generation == current).then(|| HUD.get()).flatten()
    }

    fn send(&self, event: HudEvent) {
        if let Some(sender) = self.sender() {
            sender.send(event);
        }
    }

    /// The listener's state after each turn: whether it records, and how
    /// many dictations it submitted that have not finished.
    pub(crate) fn update(&mut self, recording: bool, pending: usize) {
        if recording && !self.recording {
            self.send(HudEvent::Started);
        }
        let submitted = pending > self.jobs.len();
        while pending > self.jobs.len() {
            self.next_job += 1;
            self.jobs.push_back(self.next_job);
            self.send(HudEvent::Submitted {
                job_id: self.next_job,
            });
        }
        if !recording && self.recording && !submitted {
            self.send(HudEvent::Cancelled);
        }
        // Dictations that ended without `finished` (the listener stopping).
        while pending < self.jobs.len() {
            if let Some(job_id) = self.jobs.pop_front() {
                self.send(HudEvent::JobCancelled { job_id });
            }
        }
        self.recording = recording;
    }

    /// The oldest submitted dictation finished: pasted, or failed.
    pub(crate) fn finished(&mut self, pasted: bool) {
        if let Some(job_id) = self.jobs.pop_front() {
            self.send(if pasted {
                HudEvent::JobCompleted { job_id }
            } else {
                HudEvent::JobFailed { job_id }
            });
        }
    }

    /// Captured audio while recording, for the level the HUD shows.
    pub(crate) fn meter(&self, samples: &[f32]) {
        if let Some(sender) = self.sender() {
            sender.meter(samples);
        }
    }
}

impl Drop for LinuxIndicator {
    fn drop(&mut self) {
        self.send(HudEvent::Reset);
    }
}

/// Adds the HUD overlay to the window's app.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    let sender = crate::rune_hud::install(app);
    sender.set_position(*POSITION.lock().unwrap_or_else(|error| error.into_inner()));
    let _ = HUD.set(sender);
}

/// Why the HUD can't show: Rune tried and could not, or the Wayland
/// compositor offers no layer-shell (GNOME's Mutter) and there is no
/// XWayland for Rune to show it on instead. Logged once, the first time it
/// is known.
pub(crate) fn limitation() -> Option<String> {
    static NO_OVERLAY: OnceLock<bool> = OnceLock::new();
    static LOGGED: std::sync::Once = std::sync::Once::new();
    let rune_refused = HUD
        .get()
        .is_some_and(|hud| hud.availability() == Availability::Unavailable);
    let unavailable = rune_refused
        || *NO_OVERLAY.get_or_init(|| {
            let no_layer_shell = crate::linux::session::LinuxSession::detect().is_wayland()
                && wayland_reachable()
                && !crate::linux::paste::wayland_offers(LAYER_SHELL);
            if no_layer_shell && xwayland() {
                tracing::info!("HUD shows on XWayland: compositor has no layer-shell");
            }
            no_layer_shell && !xwayland()
        });
    if !unavailable {
        return None;
    }
    LOGGED.call_once(|| {
        tracing::warn!(
            "HUD unavailable: compositor has no layer-shell and XWayland is not running; dictation still works"
        )
    });
    Some(LIMITATION.into())
}

const LAYER_SHELL: &str = "zwlr_layer_shell_v1";
const LIMITATION: &str = "Recording HUD unavailable: this compositor does not support Wayland layer-shell and XWayland is not running. Dictation still works.";

/// Whether a Wayland connection opens at all, so a missing display is never
/// read as a compositor without layer-shell.
fn wayland_reachable() -> bool {
    wayland_client::Connection::connect_to_env().is_ok()
}

/// Whether the session runs XWayland, which Rune shows the HUD on when the
/// compositor has no layer-shell.
fn xwayland() -> bool {
    std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty())
}
