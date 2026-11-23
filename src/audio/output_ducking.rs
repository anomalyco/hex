//! "Lower audio" while dictating: a relative, channel-preserving volume
//! ramp, adapted from Scrawl's playback ramp, over each OS's volume adapter
//! (CoreAudio output channels on macOS, the default sink's channels on
//! Linux, other apps' audio sessions on Windows). Only the ramp's own thread
//! waits for restoration; capture and the UI never do.
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::audio::CaptureInstant;

const TICK: Duration = Duration::from_millis(16);
const EPSILON: f32 = 0.002;
/// Lowering starts after the 300 ms hold threshold, so it must finish before
/// the speech that follows; a full second left the start of a dictation loud.
const LOWER: Duration = Duration::from_millis(120);
/// Restoring eases back over the original second so playback never jumps.
const RESTORE: Duration = Duration::from_secs(1);
/// Bluetooth may acknowledge channels after the ramp ends; wait this much
/// longer, then settle on whatever the output reports.
const SETTLE: Duration = Duration::from_millis(500);

/// A set of volumes lowered and restored together, such as an output's
/// channels or a set of audio sessions.
pub(crate) trait Volume {
    fn read(&self) -> Option<Vec<f32>>;
    fn write(&mut self, values: &[f32]) -> bool;
}

/// Holds the output lowered to `factor` of its volume until restored or
/// dropped.
pub(crate) struct Guard {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Guard {
    /// Lowers what `open` returns, opened on the ramp's own thread. macOS
    /// reports the lowering through [`Self::start_reporting`]; the Linux and
    /// Windows suppressors start without a report.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) fn start<V: Volume>(
        factor: f32,
        open: impl FnOnce() -> Option<V> + Send + 'static,
    ) -> Option<Self> {
        Self::start_reporting(factor, open, None)
    }

    /// As [`Self::start`], sending the moment the lowering first writes its
    /// full target through `report`, from the ramp's own thread. Nothing is
    /// sent when there is no output to lower, the output is already silent,
    /// the ramp fails, or the guard is restored before the target is written.
    pub(crate) fn start_reporting<V: Volume>(
        factor: f32,
        open: impl FnOnce() -> Option<V> + Send + 'static,
        report: Option<Sender<CaptureInstant>>,
    ) -> Option<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("output-ducking".into())
            .spawn(move || {
                if let Some(mut ramp) = open().and_then(Ramp::new) {
                    run(&mut ramp, factor, receiver, report);
                }
            })
            .ok()?;
        Some(Self {
            stop,
            worker: Some(worker),
        })
    }

    /// Ramps the volume back and returns once it is restored.
    pub(crate) fn restore(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.restore();
    }
}

fn run<V: Volume>(
    ramp: &mut Ramp<V>,
    factor: f32,
    receiver: Receiver<()>,
    mut report: Option<Sender<CaptureInstant>>,
) {
    if !ramp.would_change(factor) {
        report = None;
    }
    ramp.retarget(Some(factor));
    let started = Instant::now();
    loop {
        match receiver.recv_timeout(TICK) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                ramp.tick(started.elapsed().as_secs_f32());
                if ramp.lowered
                    && let Some(report) = report.take()
                {
                    // The target was written; Bluetooth may take longer to
                    // report it back, and that is not when the audio dropped.
                    let _ = report.send(CaptureInstant::now());
                }
            }
        }
        if ramp.finished {
            return;
        }
    }
    // Restored before the target was written: nothing to report.
    drop(report);
    ramp.retarget(None);
    let started = Instant::now();
    while !ramp.finished {
        ramp.tick(started.elapsed().as_secs_f32());
        if !ramp.finished {
            thread::sleep(TICK);
        }
    }
}

struct Ramp<V: Volume> {
    output: V,
    original: Vec<f32>,
    accepted: Vec<f32>,
    from: Vec<f32>,
    target: Vec<f32>,
    restoring: bool,
    settled: bool,
    finished: bool,
    /// The full lowering target was written at least once. This is when the
    /// output actually dropped, whatever the readback says afterwards.
    lowered: bool,
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < EPSILON)
}

impl<V: Volume> Ramp<V> {
    /// Whether lowering to `factor` would change a running output: some
    /// channel has a volume, and the target is below it.
    fn would_change(&self, factor: f32) -> bool {
        factor < 1.0 && self.original.iter().any(|value| *value > EPSILON)
    }

    fn new(output: V) -> Option<Self> {
        let original = output.read().filter(|values| !values.is_empty())?;
        Some(Self {
            output,
            accepted: original.clone(),
            from: original.clone(),
            target: original.clone(),
            original,
            restoring: false,
            settled: false,
            finished: false,
            lowered: false,
        })
    }

    fn retarget(&mut self, factor: Option<f32>) {
        if self.finished {
            return;
        }
        if self.settled
            && !self
                .output
                .read()
                .is_some_and(|v| close(&v, &self.accepted))
        {
            self.finished = true; // Respect manual changes after the hardware has settled.
            return;
        }
        self.from = self.output.read().unwrap_or_else(|| self.accepted.clone());
        self.target = self
            .original
            .iter()
            .map(|v| v * factor.unwrap_or(1.0))
            .collect();
        self.restoring = factor.is_none();
        self.settled = false;
    }

    fn duration(&self) -> Duration {
        if self.restoring { RESTORE } else { LOWER }
    }

    fn tick(&mut self, seconds: f32) {
        if self.finished || self.settled {
            return;
        }
        let duration = self.duration().as_secs_f32();
        let t = (seconds / duration).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        let values: Vec<_> = self
            .from
            .iter()
            .zip(&self.target)
            .map(|(a, b)| a + (b - a) * eased)
            .collect();
        if !self.output.write(&values) {
            self.rollback();
            return;
        }
        let Some(actual) = self.output.read() else {
            self.rollback();
            return;
        };
        // Bluetooth may acknowledge channels late; use a bounded settling period.
        self.accepted = actual;
        // The ramp wrote its full target: the output dropped now, even if a
        // late Bluetooth readback still disagrees and delays the settle.
        if t >= 1.0 && !self.restoring {
            self.lowered = true;
        }
        if t >= 1.0
            && (close(&self.accepted, &self.target) || seconds >= duration + SETTLE.as_secs_f32())
        {
            self.settled = true;
            self.finished = self.restoring;
        }
    }

    fn rollback(&mut self) {
        self.output.write(&self.original);
        self.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        values: Vec<f32>,
        fail: bool,
    }
    impl Volume for Fake {
        fn read(&self) -> Option<Vec<f32>> {
            Some(self.values.clone())
        }
        fn write(&mut self, values: &[f32]) -> bool {
            self.values[0] = values[0];
            if std::mem::take(&mut self.fail) {
                return false;
            }
            self.values = values
                .iter()
                .map(|v| (v * 1000.0).round() / 1000.0)
                .collect();
            true
        }
    }
    fn ramp(values: &[f32]) -> Ramp<Fake> {
        Ramp::new(Fake {
            values: values.to_vec(),
            fail: false,
        })
        .unwrap()
    }
    #[test]
    fn ducking_preserves_balance_silence_and_restores() {
        let lower_midpoint = LOWER.as_secs_f32() / 2.0;
        let restore_midpoint = RESTORE.as_secs_f32() / 2.0;
        for original in [&[0.3, 0.8][..], &[0.0, 0.8], &[0.5]] {
            for factor in [0.0, 0.2, 1.0] {
                let midpoint = original
                    .iter()
                    .map(|v| v * (1.0 + factor) / 2.0)
                    .collect::<Vec<_>>();
                let mut r = ramp(original);
                r.retarget(Some(factor));
                r.tick(lower_midpoint);
                assert!(close(&r.output.values, &midpoint));
                r.tick(LOWER.as_secs_f32());
                r.retarget(None);
                r.tick(restore_midpoint);
                assert!(close(&r.output.values, &midpoint));
                r.tick(RESTORE.as_secs_f32());
                assert!(close(&r.output.values, original));
                assert!(r.finished);
            }
        }
    }
    #[test]
    fn lowering_finishes_at_its_own_duration_while_restoring_is_still_ramping() {
        let elapsed = LOWER.as_secs_f32();
        let mut lowered = ramp(&[0.3, 0.8]);
        lowered.retarget(Some(0.2));
        lowered.tick(elapsed);
        assert!(close(&lowered.output.values, &[0.06, 0.16]));
        assert!(lowered.settled);

        let mut restoring = ramp(&[0.3, 0.8]);
        restoring.retarget(Some(0.2));
        restoring.tick(elapsed);
        restoring.retarget(None);
        restoring.tick(elapsed);
        assert!(!close(&restoring.output.values, &[0.3, 0.8]));
        assert!(restoring.output.values[0] < (0.06 + 0.3) / 2.0);
    }
    #[test]
    fn ducking_respects_manual_changes_and_restores_interrupted_ramps() {
        let mut r = ramp(&[0.3, 0.8]);
        r.retarget(Some(0.2));
        r.tick(LOWER.as_secs_f32());
        r.output.values = vec![0.5, 0.5];
        r.retarget(None);
        r.tick(RESTORE.as_secs_f32());
        assert_eq!(r.output.values, vec![0.5, 0.5]);
        let mut r = ramp(&[0.3, 0.8]);
        r.retarget(Some(0.0));
        r.tick(LOWER.as_secs_f32() / 2.0);
        r.retarget(None);
        r.tick(RESTORE.as_secs_f32());
        assert!(close(&r.output.values, &[0.3, 0.8]));
    }
    #[test]
    fn ducking_rolls_back_every_channel_on_partial_failure() {
        let mut r = ramp(&[0.3, 0.8]);
        r.output.fail = true;
        r.retarget(Some(0.2));
        r.tick(LOWER.as_secs_f32() / 2.0);
        assert!(r.finished);
        assert!(close(&r.output.values, &[0.3, 0.8]));
    }

    /// Reads back a rounded value below 0.5, as Bluetooth may: the settling
    /// check never sees its target, so only the write itself can time the
    /// lowering.
    struct Rounded {
        values: Vec<f32>,
    }

    impl Volume for Rounded {
        fn read(&self) -> Option<Vec<f32>> {
            Some(
                self.values
                    .iter()
                    .map(|value| if *value < 0.5 { value + 0.003 } else { *value })
                    .collect(),
            )
        }

        fn write(&mut self, values: &[f32]) -> bool {
            self.values = values.to_vec();
            true
        }
    }

    #[test]
    fn a_guard_that_changes_nothing_reports_no_moment() {
        // An already silent output has nothing to lower, so a capture hearing
        // its microphone has not missed anything Hex suppressed.
        let (report, outcome) = mpsc::channel();
        let guard = Guard::start_reporting(
            0.2,
            || {
                Some(Fake {
                    values: vec![0.0],
                    fail: false,
                })
            },
            Some(report),
        )
        .unwrap();
        assert!(
            outcome
                .recv_timeout(LOWER + Duration::from_millis(80))
                .is_err()
        );
        drop(guard);
    }

    #[test]
    fn a_guard_reports_when_the_target_is_written_not_when_the_readback_settles() {
        let (report, outcome) = mpsc::channel();
        let started = Instant::now();
        let guard =
            Guard::start_reporting(0.2, || Some(Rounded { values: vec![0.8] }), Some(report))
                .unwrap();
        outcome
            .recv_timeout(LOWER + Duration::from_millis(80))
            .expect("the lowering reports when it writes its target");
        assert!(
            started.elapsed() < LOWER + Duration::from_millis(80),
            "reported at the settle fallback: {:?}",
            started.elapsed()
        );
        drop(guard);
    }
}
