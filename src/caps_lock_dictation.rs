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
//! toggles dictation. The worker marks Hex's own lock-reset edge, so the tap
//! drops that off edge before it can reach the press gate. When the HID
//! tracker is inactive (Input Monitoring is not granted), an AlphaShift on
//! edge is the fallback press and off edges remain resets.
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
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
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
    SlowBlink,
}

impl LedPattern {
    /// The worker's shared-memory encoding.
    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Solid => 1,
            Self::Blink => 2,
            Self::SlowBlink => 3,
        }
    }

    pub(crate) const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Solid,
            2 => Self::Blink,
            3 => Self::SlowBlink,
            _ => Self::Off,
        }
    }

    /// How long each on and off phase lasts; `None` when the light does not
    /// blink. Slow blink is for a recording light, and stays clearly slower
    /// than the transcribing blink.
    pub(crate) const fn interval(self) -> Option<Duration> {
        match self {
            Self::Off | Self::Solid => None,
            Self::Blink => Some(Duration::from_millis(350)),
            Self::SlowBlink => Some(Duration::from_millis(700)),
        }
    }
}

/// Whether the microphone has delivered any audio for the current capture.
/// A press starts [`Self::Waiting`]; only the capture's first `Meter` makes
/// it [`Self::Live`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CaptureState {
    /// No capture records.
    #[default]
    Idle,
    /// A capture started, and the light waits for its first audio.
    Waiting,
    /// The microphone is delivering the capture's audio.
    Live,
}

/// The dictation phases the light follows: solid (or slowly blinking) while
/// a capture records and its microphone delivers audio, blinking while any
/// accepted job is transcribed or processed, off once nothing is left. The
/// model mirrors the HUD's event vocabulary without a second reader of its
/// channel.
#[derive(Debug, Default)]
pub(crate) struct LedModel {
    capture: CaptureState,
    jobs: BTreeSet<u64>,
}

impl LedModel {
    pub(crate) fn handle(&mut self, event: DictationIndicatorEvent) {
        match event {
            DictationIndicatorEvent::Reset => {
                self.capture = CaptureState::Idle;
                self.jobs.clear();
            }
            DictationIndicatorEvent::Started => self.capture = CaptureState::Waiting,
            // The first audio proves the microphone is delivering; a Meter
            // with no capture started must turn nothing on.
            DictationIndicatorEvent::Meter { .. } => {
                if self.capture == CaptureState::Waiting {
                    self.capture = CaptureState::Live;
                }
            }
            // Only submission accepts a job. A stage event for an older or
            // unknown job must not flip a newer capture back to blinking.
            DictationIndicatorEvent::Submitted { job_id } => {
                self.capture = CaptureState::Idle;
                self.jobs.insert(job_id);
            }
            DictationIndicatorEvent::JobCompleted { job_id }
            | DictationIndicatorEvent::JobCancelled { job_id }
            | DictationIndicatorEvent::JobFailed { job_id, .. } => {
                self.jobs.remove(&job_id);
            }
            DictationIndicatorEvent::Discarded
            | DictationIndicatorEvent::Cancelled
            | DictationIndicatorEvent::Failed { .. } => self.capture = CaptureState::Idle,
            DictationIndicatorEvent::ConfigureDotScope(_)
            | DictationIndicatorEvent::DemoVoice
            | DictationIndicatorEvent::Transcribing { .. }
            | DictationIndicatorEvent::Processing { .. } => {}
        }
    }

    /// The pattern to publish. `slow_blink` picks the recording light's slow
    /// blink; while a capture waits for audio the pattern is what it would
    /// be without it.
    pub(crate) fn pattern(&self, slow_blink: bool) -> LedPattern {
        match self.capture {
            CaptureState::Live if slow_blink => LedPattern::SlowBlink,
            CaptureState::Live => LedPattern::Solid,
            CaptureState::Idle | CaptureState::Waiting if self.jobs.is_empty() => LedPattern::Off,
            CaptureState::Idle | CaptureState::Waiting => LedPattern::Blink,
        }
    }
}

/// The raw HID press and AlphaShift edge waiting to be matched.
///
/// A HID key-down alone is not a press: a Caps Lock remapped to Control or
/// Escape still reports usage 0x39 but never changes AlphaShift. Hex's own
/// lock reset is marked by the worker and never reaches this gate. The two
/// observations may arrive in either order and their timestamps may be a few
/// milliseconds apart, so each side is kept until a matching one arrives.
#[derive(Debug, Default)]
pub(crate) struct PressGate {
    press: Option<CaptureInstant>,
    edge: Option<CaptureInstant>,
}

impl PressGate {
    pub(crate) const fn new() -> Self {
        Self {
            press: None,
            edge: None,
        }
    }

    /// A physical Caps Lock key-down, stamped from `IOHIDValueGetTimeStamp`
    /// when the device reports it. An edge that already arrived matches now.
    pub(crate) fn hid_press(&mut self, at: CaptureInstant) -> Option<CaptureInstant> {
        self.press = Some(at);
        self.matched()
    }

    /// An AlphaShift change edge, stamped from the event tap's original event.
    /// A HID press that already arrived matches now.
    pub(crate) fn alpha_shift(
        &mut self,
        tracker_active: bool,
        shift: bool,
        at: CaptureInstant,
    ) -> Option<CaptureInstant> {
        if !tracker_active {
            self.press = None;
            self.edge = None;
            return shift.then_some(at);
        }
        self.edge = Some(at);
        self.matched()
    }

    /// Drops unconfirmed evidence, as a lab toggle or shutdown does.
    pub(crate) fn clear(&mut self) {
        self.press = None;
        self.edge = None;
    }

    /// Consumes both sides only when their timestamps are within the window;
    /// otherwise keeps them for the other side's delayed arrival.
    fn matched(&mut self) -> Option<CaptureInstant> {
        let press = self.press?;
        let edge = self.edge?;
        let gap = if press <= edge {
            edge.checked_duration_since(press)
        } else {
            press.checked_duration_since(edge)
        };
        if !gap.is_some_and(|gap| gap <= PRESS_CONFIRM_WINDOW) {
            return None;
        }
        self.press = None;
        self.edge = None;
        Some(press)
    }
}

/// Whether the lab is on, mirroring [`crate::labs::caps_lock_dictation`] for
/// the event tap and HID threads, which cannot read settings.
#[cfg(target_os = "macos")]
pub(crate) static ENABLED: AtomicBool = AtomicBool::new(false);

/// The AlphaShift bit last observed by the tap, seeded when the lab turns on.
#[cfg(target_os = "macos")]
static LAST_ALPHA_SHIFT: AtomicBool = AtomicBool::new(false);

/// Set while the worker is clearing Hex's own Caps Lock state, so the edge
/// that reset produces is never fed to the press gate.
#[cfg(target_os = "macos")]
static RESETTING: AtomicBool = AtomicBool::new(false);

/// Seeds the tap's last AlphaShift state from the keyboard it is turning on
/// with, so the first edge after enabling is a real change.
#[cfg(target_os = "macos")]
pub(crate) fn seed_alpha_shift(shift: bool) {
    LAST_ALPHA_SHIFT.store(shift, Ordering::Release);
    RESETTING.store(false, Ordering::Release);
}

/// Records an AlphaShift observation and says whether it actually changed.
#[cfg(target_os = "macos")]
pub(crate) fn alpha_shift_changed(shift: bool) -> bool {
    ENABLED.load(Ordering::Acquire) && LAST_ALPHA_SHIFT.swap(shift, Ordering::AcqRel) != shift
}

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
/// AlphaShift edge confirms it. Suspended shortcuts mean no dictation, so the
/// key is only Caps Lock.
#[cfg(target_os = "macos")]
pub(crate) fn physical_press(at: CaptureInstant) {
    if !ENABLED.load(Ordering::Acquire) || crate::app_settings::shortcuts_suspended() {
        return;
    }
    let matched = HID_GATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .hid_press(at);
    if let Some(pressed) = matched {
        send_press(pressed);
    }
}

/// An AlphaShift change observed by the event tap, with the event's own
/// timestamp. The press is forwarded to the listener before the worker is
/// asked to touch IOKit. Suspended shortcuts mean no dictation: the edge
/// neither presses nor resets the lock, so a pause never forces Caps Lock off
/// or treats the key as a dictation key.
#[cfg(target_os = "macos")]
pub(crate) fn alpha_shift(shift: bool, at: CaptureInstant) {
    if !ENABLED.load(Ordering::Acquire) || crate::app_settings::shortcuts_suspended() {
        return;
    }
    // Hex's own lock reset produces an off edge; it must never become a
    // press, even when a remapped HID press happens to be pending nearby.
    // Clear the marker on any edge so a stale one cannot swallow a later
    // on-edge press.
    let was_resetting = RESETTING.swap(false, Ordering::AcqRel);
    if !shift && was_resetting {
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
        assert_eq!(model.pattern(false), LedPattern::Off);

        model.handle(DictationIndicatorEvent::Started);
        assert_eq!(
            model.pattern(false),
            LedPattern::Off,
            "the microphone has not delivered audio yet"
        );

        model.handle(DictationIndicatorEvent::Meter {
            average: 0.1,
            peak: 0.2,
        });
        assert_eq!(model.pattern(false), LedPattern::Solid);

        model.handle(DictationIndicatorEvent::Submitted { job_id: 3 });
        assert_eq!(model.pattern(false), LedPattern::Blink);

        model.handle(DictationIndicatorEvent::JobCompleted { job_id: 3 });
        assert_eq!(model.pattern(false), LedPattern::Off);
    }

    #[test]
    fn the_light_waits_for_the_captures_first_audio() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Started);
        assert_eq!(model.pattern(false), LedPattern::Off);
        assert_eq!(model.pattern(true), LedPattern::Off);

        model.handle(DictationIndicatorEvent::Meter {
            average: 0.1,
            peak: 0.2,
        });
        assert_eq!(model.pattern(false), LedPattern::Solid);
        assert_eq!(model.pattern(true), LedPattern::SlowBlink);
    }

    #[test]
    fn a_meter_without_a_capture_turns_nothing_on() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Meter {
            average: 0.1,
            peak: 0.2,
        });
        assert_eq!(model.pattern(false), LedPattern::Off);
    }

    #[test]
    fn ending_a_waiting_capture_clears_the_wait() {
        for event in [
            DictationIndicatorEvent::Submitted { job_id: 1 },
            DictationIndicatorEvent::Discarded,
            DictationIndicatorEvent::Cancelled,
            DictationIndicatorEvent::Failed { message: None },
            DictationIndicatorEvent::Reset,
        ] {
            let submitted = matches!(&event, DictationIndicatorEvent::Submitted { .. });
            let mut model = LedModel::default();
            model.handle(DictationIndicatorEvent::Started);
            model.handle(event.clone());
            model.handle(DictationIndicatorEvent::Meter {
                average: 0.1,
                peak: 0.2,
            });
            let expected = if submitted {
                LedPattern::Blink
            } else {
                LedPattern::Off
            };
            assert_eq!(model.pattern(false), expected, "{event:?}");
        }
    }

    #[test]
    fn a_waiting_capture_keeps_earlier_jobs_blinking() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Submitted { job_id: 7 });
        model.handle(DictationIndicatorEvent::Started);
        assert_eq!(
            model.pattern(false),
            LedPattern::Blink,
            "the previous job still transcribes while the microphone opens"
        );

        model.handle(DictationIndicatorEvent::Meter {
            average: 0.1,
            peak: 0.2,
        });
        assert_eq!(model.pattern(false), LedPattern::Solid);
        model.handle(DictationIndicatorEvent::Submitted { job_id: 8 });
        assert_eq!(model.pattern(false), LedPattern::Blink);
    }

    #[test]
    fn the_slow_blink_setting_only_changes_the_recording_light() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Meter {
            average: 0.1,
            peak: 0.2,
        });
        assert_eq!(model.pattern(true), LedPattern::SlowBlink);
        assert_eq!(
            LedPattern::SlowBlink.interval(),
            Some(Duration::from_millis(700))
        );

        model.handle(DictationIndicatorEvent::Submitted { job_id: 1 });
        assert_eq!(
            model.pattern(true),
            LedPattern::Blink,
            "transcribing stays at its own blink"
        );
        assert_eq!(
            LedPattern::Blink.interval(),
            Some(Duration::from_millis(350))
        );
    }

    #[test]
    fn the_light_waits_for_every_accepted_job() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Submitted { job_id: 1 });
        model.handle(DictationIndicatorEvent::Submitted { job_id: 2 });
        model.handle(DictationIndicatorEvent::JobCompleted { job_id: 1 });
        assert_eq!(model.pattern(false), LedPattern::Blink);
        model.handle(DictationIndicatorEvent::JobFailed {
            job_id: 2,
            message: None,
        });
        assert_eq!(model.pattern(false), LedPattern::Off);
    }

    #[test]
    fn an_older_jobs_stage_event_never_interrupts_a_newer_capture() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Submitted { job_id: 1 });
        assert_eq!(model.pattern(false), LedPattern::Blink);

        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Meter {
            average: 0.1,
            peak: 0.2,
        });
        assert_eq!(model.pattern(false), LedPattern::Solid);
        model.handle(DictationIndicatorEvent::Transcribing { job_id: 1 });
        model.handle(DictationIndicatorEvent::Processing { job_id: 1 });
        assert_eq!(
            model.pattern(false),
            LedPattern::Solid,
            "an older job's stage must not clear recording or blink"
        );

        model.handle(DictationIndicatorEvent::JobCompleted { job_id: 1 });
        assert_eq!(model.pattern(false), LedPattern::Solid);
    }

    #[test]
    fn a_stage_event_for_an_unknown_job_changes_nothing() {
        let mut model = LedModel::default();
        model.handle(DictationIndicatorEvent::Transcribing { job_id: 42 });
        model.handle(DictationIndicatorEvent::Processing { job_id: 42 });
        assert_eq!(model.pattern(false), LedPattern::Off);

        model.handle(DictationIndicatorEvent::Started);
        model.handle(DictationIndicatorEvent::Meter {
            average: 0.1,
            peak: 0.2,
        });
        model.handle(DictationIndicatorEvent::Transcribing { job_id: 42 });
        assert_eq!(model.pattern(false), LedPattern::Solid);
    }

    #[test]
    fn cancelling_or_failing_a_capture_turns_the_light_off() {
        for event in [
            DictationIndicatorEvent::Discarded,
            DictationIndicatorEvent::Cancelled,
            DictationIndicatorEvent::Failed { message: None },
        ] {
            let mut model = LedModel::default();
            model.handle(DictationIndicatorEvent::Started);
            model.handle(DictationIndicatorEvent::Meter {
                average: 0.1,
                peak: 0.2,
            });
            assert_eq!(model.pattern(false), LedPattern::Solid);
            model.handle(event.clone());
            assert_eq!(model.pattern(false), LedPattern::Off, "{event:?}");
        }
    }

    #[test]
    fn a_hid_press_matches_an_edge_timestamp_from_before_the_callback() {
        let pressed = CaptureInstant::from_nanos(1_000_000_000);
        let edge = CaptureInstant::from_nanos(995_000_000);
        let mut gate = PressGate::new();
        assert_eq!(gate.hid_press(pressed), None);
        assert_eq!(
            gate.alpha_shift(true, true, edge),
            Some(pressed),
            "the device timestamp is earlier than the callback, and the press keeps it"
        );
        assert_eq!(
            gate.alpha_shift(true, false, edge + Duration::from_millis(40)),
            None,
            "the reset edge after a confirmed press must not press again"
        );
    }

    #[test]
    fn an_edge_that_arrives_before_the_hid_press_still_matches() {
        let pressed = CaptureInstant::from_nanos(2_000_000_000);
        let edge = CaptureInstant::from_nanos(1_997_000_000);
        let mut gate = PressGate::new();
        assert_eq!(gate.alpha_shift(true, true, edge), None);
        assert_eq!(gate.hid_press(pressed), Some(pressed));
    }

    #[test]
    fn a_fast_double_press_yields_two_presses() {
        let base = CaptureInstant::from_nanos(3_000_000_000);
        let first_press = base + Duration::from_millis(100);
        let first_edge = first_press + Duration::from_millis(2);
        let second_press = base + Duration::from_millis(220);
        let second_edge = second_press + Duration::from_millis(2);
        let mut gate = PressGate::new();

        assert_eq!(gate.hid_press(first_press), None);
        assert_eq!(gate.alpha_shift(true, true, first_edge), Some(first_press));
        assert_eq!(gate.hid_press(second_press), None);
        assert_eq!(
            gate.alpha_shift(true, true, second_edge),
            Some(second_press)
        );
    }

    #[test]
    fn a_hid_press_without_an_alpha_shift_edge_never_presses() {
        let pressed = CaptureInstant::from_nanos(4_000_000_000);
        let mut gate = PressGate::new();
        assert_eq!(gate.hid_press(pressed), None);
        // A remapped Caps Lock never changes AlphaShift, so nothing confirms
        // this press. No call to the gate can turn it into one.
        assert_eq!(
            gate.alpha_shift(
                true,
                true,
                pressed + PRESS_CONFIRM_WINDOW + Duration::from_millis(1)
            ),
            None
        );
    }

    #[test]
    fn without_the_hid_tracker_an_on_edge_is_the_fallback_press() {
        let at = CaptureInstant::from_nanos(5_000_000_000);
        let mut gate = PressGate::new();
        assert_eq!(gate.alpha_shift(false, false, at), None);
        assert_eq!(gate.alpha_shift(false, true, at), Some(at));
        assert_eq!(gate.alpha_shift(false, false, at), None);
    }

    #[test]
    fn the_light_pattern_codes_round_trip() {
        for pattern in [
            LedPattern::Off,
            LedPattern::Solid,
            LedPattern::Blink,
            LedPattern::SlowBlink,
        ] {
            assert_eq!(LedPattern::from_code(pattern.code()), pattern);
        }
        assert_eq!(LedPattern::from_code(99), LedPattern::Off);
    }
}
