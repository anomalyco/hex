//! Labs › Caps Lock dictation (macOS): a Caps Lock press starts a
//! hands-free dictation, the next press finishes it, and the key's light
//! shows Hex's state. The platform-independent decisions (press → start or
//! finish, dictation events → light pattern, duplicate press evidence) live
//! here; [`macos`] owns the IOKit work.
//!
//! A press is observed twice: the raw HID tracker sees the physical key
//! below macOS's keyboard filter, and the event tap sees the AlphaShift
//! flag change. The two paths are deduplicated, so the feature still works
//! when the HID tracker is inactive (Input Monitoring is not granted).
//!
//! The light needs the same Input Monitoring permission: macOS exposes
//! keyboard LED output elements only to a process it lets read HID
//! keyboards. Without it, dictation still works but the light stays under
//! macOS's control. The system Caps Lock state is reset off through the
//! `IOHIDSystem` parameter connection on every press, so a press never
//! uppercases what follows.

use std::collections::BTreeSet;
#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "macos")]
use std::sync::mpsc::SyncSender;
#[cfg(target_os = "macos")]
use std::sync::{LazyLock, Mutex};
#[cfg(target_os = "macos")]
use std::time::Instant;

use crate::dictation::indicator::DictationIndicatorEvent;

/// `kCGEventFlagMaskAlphaShift`: Caps Lock's software state in event flags.
pub(crate) const ALPHA_SHIFT_MASK: u64 = 1 << 16;

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
            DictationIndicatorEvent::Submitted { job_id }
            | DictationIndicatorEvent::Transcribing { job_id }
            | DictationIndicatorEvent::Processing { job_id } => {
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

/// How long after one press the other observation path's copy of the same
/// press is ignored.
pub(crate) const PRESS_DEDUP: std::time::Duration = std::time::Duration::from_millis(80);

/// Turns the two press observations into one press each. The raw HID path
/// sees the key earlier than the AlphaShift flag change, but either can be
/// the only one that arrives; the first one wins.
#[derive(Debug, Default)]
pub(crate) struct PressDetector {
    last_shift: bool,
    last_press: Option<std::time::Instant>,
}

impl PressDetector {
    /// A physical Caps Lock key-down from the HID tracker.
    pub(crate) fn physical(&mut self, now: std::time::Instant) -> bool {
        self.press(now)
    }

    /// An event tap flags change. Only an AlphaShift off → on edge counts;
    /// Hex's own reset produces the off edge.
    pub(crate) fn flags(&mut self, shift: bool, now: std::time::Instant) -> bool {
        let rising = shift && !self.last_shift;
        self.last_shift = shift;
        rising && self.press(now)
    }

    fn press(&mut self, now: std::time::Instant) -> bool {
        if self
            .last_press
            .is_some_and(|last| now.saturating_duration_since(last) < PRESS_DEDUP)
        {
            return false;
        }
        self.last_press = Some(now);
        true
    }
}

/// Whether the lab is on, mirroring [`crate::labs::caps_lock_dictation`] for
/// the event tap and HID threads, which cannot read settings.
#[cfg(target_os = "macos")]
pub(crate) static ENABLED: AtomicBool = AtomicBool::new(false);

/// The worker commands the event tap and HID tracker can reach.
#[cfg(target_os = "macos")]
static COMMANDS: LazyLock<Mutex<Option<SyncSender<macos::Command>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Sends a press or AlphaShift change from an event-tap or HID callback. The
/// worker does every IOKit call; a full queue drops evidence rather than
/// blocking either callback.
#[cfg(target_os = "macos")]
fn send(command: macos::Command) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    if let Some(sender) = COMMANDS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
    {
        let _ = sender.try_send(command);
    }
}

/// An event tap flags change, including the AlphaShift change a Caps Lock
/// press and Hex's own lock reset both produce.
#[cfg(target_os = "macos")]
pub(crate) fn flags_changed(flags: u64) {
    send(macos::Command::Flags {
        shift: flags & ALPHA_SHIFT_MASK != 0,
        at: Instant::now(),
    });
}

/// A physical Caps Lock key-down from the raw HID tracker.
#[cfg(target_os = "macos")]
pub(crate) fn physical_press() {
    send(macos::Command::Press { at: Instant::now() });
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use self::macos::CapsLockDictation;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn a_press_starts_when_idle_and_finishes_its_own_recording() {
        assert_eq!(press_action(CapsState::Idle), CapsAction::Start);
        assert_eq!(press_action(CapsState::Recording), CapsAction::Finish);
        assert_eq!(press_action(CapsState::Busy), CapsAction::Ignore);
    }

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
    fn both_press_paths_report_one_press() {
        let start = Instant::now();
        let mut detector = PressDetector::default();
        assert!(detector.physical(start));
        assert!(
            !detector.flags(true, start + Duration::from_millis(2)),
            "the flag path's copy of the same press is ignored"
        );
        assert!(!detector.flags(true, start + Duration::from_millis(4)));
        assert!(!detector.flags(false, start + Duration::from_millis(6)));
        assert!(
            detector.flags(true, start + Duration::from_millis(200)),
            "a later AlphaShift edge is a new press"
        );
    }
}
