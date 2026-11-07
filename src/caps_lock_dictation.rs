//! Labs › Caps Lock dictation (macOS): a Caps Lock press starts a
//! hands-free dictation, the next press finishes it, and the key's light
//! shows Hex's state. The platform-independent decisions (press → start or
//! finish, dictation events → light pattern, the HID/AlphaShift press gate)
//! live here; [`macos`] owns the IOKit work.
//!
//! A physical press is only the combination of two observations: the raw HID
//! tracker sees Caps Lock usage 0x39 go down below macOS's keyboard filter,
//! and the event tap sees the AlphaShift flag change the press causes. A key
//! remapped to Control or Escape changes no AlphaShift flag, so it never
//! toggles dictation. Hex's own lock reset produces an AlphaShift edge with no
//! pending HID press, so it is ignored too. When the HID tracker is inactive
//! (Input Monitoring is not granted), an AlphaShift on edge is the fallback
//! press and off edges remain resets.
//!
//! The light needs the same Input Monitoring permission: macOS exposes
//! keyboard LED output elements only to a process it lets read HID
//! keyboards. Without it, dictation still works but the light stays under
//! macOS's control. The system Caps Lock state is reset off through the
//! `IOHIDSystem` parameter connection on every AlphaShift change, so a press
//! never uppercases what follows.

use std::collections::BTreeSet;
use std::time::Duration;

#[cfg(target_os = "macos")]
use std::sync::LazyLock;
#[cfg(target_os = "macos")]
use std::sync::Mutex;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc::Sender;

use crate::audio::CaptureInstant;
use crate::dictation::indicator::DictationIndicatorEvent;

/// `kCGEventFlagMaskAlphaShift`: Caps Lock's software state in event flags.
pub(crate) const ALPHA_SHIFT_MASK: u64 = 1 << 16;

/// How long an AlphaShift edge may follow its HID Caps Lock press and still
/// confirm it. macOS's Caps Lock activation delay is normally absent, but a
/// user can configure one; this window stays below a normal double press.
pub(crate) const PRESS_CONFIRM_WINDOW: Duration = Duration::from_millis(300);

/// Caps Lock's position in `DictationHotkey`'s state machine, as far as the
/// pure press decision cares.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapsState {
    /// Nothing records; a press starts a hands-free capture.
    Idle,
    /// A hands-free (locked) capture records; a press finishes it.
    Recording,
    /// A hold capture, a pending double tap, or another shortcut records.
    Busy,
}

/// What a physical Caps Lock press does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapsAction {
    Start,
    Finish,
    Ignore,
}

/// A press toggles a hands-free capture, and never disturbs a hold or another
/// shortcut's capture.
pub(crate) const fn press_action(state: CapsState) -> CapsAction {
    match state {
        CapsState::Idle => CapsAction::Start,
        CapsState::Recording => CapsAction::Finish,
        CapsState::Busy => CapsAction::Ignore,
    }
}

/// What the Caps Lock light should do while the lab is on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum LedPattern {
    #[default]
    Off,
    Solid,
    Blink,
}

impl LedPattern {
    /// The worker's shared-memory encoding.
    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Solid => 1,
            Self::Blink => 2,
        }
    }

    pub(crate) const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Solid,
            2 => Self::Blink,
            _ => Self::Off,
        }
    }
}

/// The dictation phases the light follows: on while a capture records,
/// blinking while any accepted job is transcribed or processed, off once
/// nothing is left. The model mirrors the HUD's event vocabulary without a
/// second reader of its channel.
#[derive(Debug, Default)]
pub(crate) struct LedModel {
    recording: bool,
    jobs: BTreeSet<u64>,
}

impl LedModel {
    pub(crate) fn handle(&mut self, event: DictationIndicatorEvent) {
        match event {
            DictationIndicatorEvent::Reset => {
                self.recording = false;
                self.jobs.clear();
            }
            DictationIndicatorEvent::Started => self.recording = true,
            // Only submission accepts a job. A stage event for an older or
            // unknown job must not flip a newer capture back to blinking.
            DictationIndicatorEvent::Submitted { job_id } => {
                self.recording = false;
                self.jobs.insert(job_id);
            }
            DictationIndicatorEvent::JobCompleted { job_id }
            | DictationIndicatorEvent::JobCancelled { job_id }
            | DictationIndicatorEvent::JobFailed { job_id } => {
                self.jobs.remove(&job_id);
            }
            DictationIndicatorEvent::Discarded
            | DictationIndicatorEvent::Cancelled
            | DictationIndicatorEvent::Failed => self.recording = false,
            DictationIndicatorEvent::ConfigureDotScope(_)
            | DictationIndicatorEvent::DemoVoice
            | DictationIndicatorEvent::Transcribing { .. }
            | DictationIndicatorEvent::Processing { .. }
            | DictationIndicatorEvent::Meter { .. } => {}
        }
    }

    pub(crate) fn pattern(&self) -> LedPattern {
        if self.recording {
            LedPattern::Solid
        } else if self.jobs.is_empty() {
            LedPattern::Off
        } else {
            LedPattern::Blink
        }
    }
}

/// The raw HID press waiting for the AlphaShift edge that confirms it.
///
/// A HID key-down alone is not a press: a Caps Lock remapped to Control or
/// Escape still reports usage 0x39 but never changes AlphaShift. Conversely,
/// Hex's own lock reset changes AlphaShift with nothing pending. The first
/// edge after a HID press consumes it, whether the system toggled on or off.
#[derive(Debug, Default)]
pub(crate) struct PressGate {
    pending: Option<CaptureInstant>,
}

impl PressGate {
    pub(crate) const fn new() -> Self {
        Self { pending: None }
    }

    /// A physical Caps Lock key-down from the HID tracker, with its press
    /// timestamp.
    pub(crate) fn hid_press(&mut self, at: CaptureInstant) {
        self.pending = Some(at);
    }

    /// Drops an unconfirmed press, as a lab toggle or shutdown does.
    pub(crate) fn clear(&mut self) {
        self.pending = None;
    }

    /// An AlphaShift change edge. With a recent pending HID press it confirms
    /// that press; without one, only the fallback (no HID tracker) treats an
    /// on edge as a press.
    pub(crate) fn alpha_shift(
        &mut self,
        tracker_active: bool,
        shift: bool,
        at: CaptureInstant,
    ) -> Option<CaptureInstant> {
        let Some(pressed) = self.pending.take() else {
            return (!tracker_active && shift).then_some(at);
        };
        at.checked_duration_since(pressed)
            .filter(|elapsed| *elapsed <= PRESS_CONFIRM_WINDOW)
            .map(|_| pressed)
    }
}

/// Whether the lab is on, mirroring [`crate::labs::caps_lock_dictation`] for
/// the event tap and HID threads, which cannot read settings.
#[cfg(target_os = "macos")]
pub(crate) static ENABLED: AtomicBool = AtomicBool::new(false);

/// The light pattern the listener wants, read by the worker.
#[cfg(target_os = "macos")]
static LED_PATTERN: AtomicU8 = AtomicU8::new(0);

/// The raw HID press waiting for its AlphaShift confirmation.
#[cfg(target_os = "macos")]
static HID_GATE: Mutex<PressGate> = Mutex::new(PressGate::new());

/// Confirmed presses waiting for the listener, never dropped.
#[cfg(target_os = "macos")]
static PRESSES: LazyLock<Mutex<Option<Sender<CaptureInstant>>>> =
    LazyLock::new(|| Mutex::new(None));

/// The LED worker's wake/lock-reset commands, sent without dropping.
#[cfg(target_os = "macos")]
static WORKER: LazyLock<Mutex<Option<Sender<macos::WorkerCommand>>>> =
    LazyLock::new(|| Mutex::new(None));

#[cfg(target_os = "macos")]
fn send_press(at: CaptureInstant) {
    if let Some(sender) = PRESSES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
    {
        let _ = sender.send(at);
    }
}

#[cfg(target_os = "macos")]
fn wake_worker(command: macos::WorkerCommand) {
    if let Some(sender) = WORKER
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
    {
        let _ = sender.send(command);
    }
}

/// A raw HID Caps Lock key-down from the tracker. It only arms a press; the
/// AlphaShift edge confirms it.
#[cfg(target_os = "macos")]
pub(crate) fn physical_press(at: CaptureInstant) {
    if ENABLED.load(Ordering::Acquire) {
        HID_GATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .hid_press(at);
    }
}

/// An AlphaShift change observed by the event tap, with the event's own
/// timestamp. The press is forwarded to the listener before the worker is
/// asked to touch IOKit.
#[cfg(target_os = "macos")]
pub(crate) fn alpha_shift(shift: bool, at: CaptureInstant) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    let tracker_active = crate::suppression::physical_keys::snapshot().is_some();
    let pressed = HID_GATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .alpha_shift(tracker_active, shift, at);
    if let Some(pressed) = pressed {
        send_press(pressed);
    }
    wake_worker(macos::WorkerCommand::LockOff);
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use self::macos::CapsLockDictation;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_light_is_solid_recording_blinking_working_and_off_after() {
        let mut model = LedModel::default();
        assert_eq!(model.pattern(), LedPattern::Off);

        model.handle(DictationIndicatorEvent::Started);
        assert_eq!(model.pattern(), LedPattern::Solid);

        model.handle(DictationIndicatorEvent::Submitted { job_id: 3 });
        assert_eq!(model.pattern(), LedPattern::Blink);

        model.handle(DictationIndicatorEvent::JobCompleted { job_id: 3 });
        assert_eq!(model.pattern(), LedPattern::Off);
    }

    #[test]
    fn the_light_waits_for_every_accepted_job() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Submitted { job_id: 1 });
        model.handle(DictationIndicatorEvent::Submitted { job_id: 2 });
        model.handle(DictationIndicatorEvent::JobCompleted { job_id: 1 });
        assert_eq!(model.pattern(), LedPattern::Blink);
        model.handle(DictationIndicatorEvent::JobFailed { job_id: 2 });
        assert_eq!(model.pattern(), LedPattern::Off);
    }

    #[test]
    fn an_older_jobs_stage_event_never_interrupts_a_newer_capture() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Submitted { job_id: 1 });
        assert_eq!(model.pattern(), LedPattern::Blink);

        model.handle(DictationIndicatorEvent::Started);
        assert_eq!(model.pattern(), LedPattern::Solid);
        model.handle(DictationIndicatorEvent::Transcribing { job_id: 1 });
        model.handle(DictationIndicatorEvent::Processing { job_id: 1 });
        assert_eq!(
            model.pattern(),
            LedPattern::Solid,
            "an older job's stage must not clear recording or blink"
        );

        model.handle(DictationIndicatorEvent::JobCompleted { job_id: 1 });
        assert_eq!(model.pattern(), LedPattern::Solid);
    }

    #[test]
    fn a_stage_event_for_an_unknown_job_changes_nothing() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Transcribing { job_id: 42 });
        model.handle(DictationIndicatorEvent::Processing { job_id: 42 });
        assert_eq!(model.pattern(), LedPattern::Off);

        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Transcribing { job_id: 42 });
        assert_eq!(model.pattern(), LedPattern::Solid);
    }

    #[test]
    fn cancelling_or_failing_a_capture_turns_the_light_off() {
        for event in [
            DictationIndicatorEvent::Discarded,
            DictationIndicatorEvent::Cancelled,
            DictationIndicatorEvent::Failed,
        ] {
            let mut model = LedModel::default();
            model.handle(DictationIndicatorEvent::Started);
            model.handle(event);
            assert_eq!(model.pattern(), LedPattern::Off, "{event:?}");
        }
    }

    #[test]
    fn a_delayed_alpha_shift_edge_confirms_one_hid_press() {
        let pressed = CaptureInstant::from_nanos(1_000_000);
        let mut gate = PressGate::new();
        gate.hid_press(pressed);
        assert_eq!(
            gate.alpha_shift(true, true, pressed + Duration::from_millis(150)),
            Some(pressed),
            "the press keeps its HID timestamp, not the edge's"
        );
        assert_eq!(
            gate.alpha_shift(true, false, pressed + Duration::from_millis(180)),
            None,
            "the reset's own edge has no pending press"
        );
    }

    #[test]
    fn a_hid_press_without_an_alpha_shift_edge_never_presses() {
        let pressed = CaptureInstant::from_nanos(2_000_000);
        let mut gate = PressGate::new();
        gate.hid_press(pressed);
        // A remapped Caps Lock never changes AlphaShift. A much later edge is
        // a different press; the stale pending press is discarded, not used.
        let stale_edge = pressed + PRESS_CONFIRM_WINDOW + Duration::from_millis(1);
        assert_eq!(gate.alpha_shift(true, true, stale_edge), None);
    }

    #[test]
    fn without_the_hid_tracker_an_on_edge_is_the_fallback_press() {
        let at = CaptureInstant::from_nanos(3_000_000);
        let mut gate = PressGate::new();
        assert_eq!(gate.alpha_shift(false, false, at), None);
        assert_eq!(gate.alpha_shift(false, true, at), Some(at));
        assert_eq!(gate.alpha_shift(false, false, at), None);
    }

    #[test]
    fn the_light_pattern_codes_round_trip() {
        for pattern in [LedPattern::Off, LedPattern::Solid, LedPattern::Blink] {
            assert_eq!(LedPattern::from_code(pattern.code()), pattern);
        }
    }
}
