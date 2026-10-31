//! "Lower audio" while dictating: a relative, channel-preserving volume
//! ramp, adapted from Scrawl's playback ramp, over each OS's volume adapter
//! (CoreAudio output channels on macOS, the default sink's channels on
//! Linux, other apps' audio sessions on Windows). Only the ramp's own thread
//! waits for restoration; capture and the UI never do.
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(16);
const EPSILON: f32 = 0.002;

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
    /// Lowers what `open` returns, opened on the ramp's own thread.
    pub(crate) fn start<V: Volume>(
        factor: f32,
        open: impl FnOnce() -> Option<V> + Send + 'static,
    ) -> Option<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("output-ducking".into())
            .spawn(move || {
                if let Some(output) = open()
                    && let Some(mut ramp) = Ramp::new(output)
                {
                    run(&mut ramp, factor, receiver);
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

fn run<V: Volume>(ramp: &mut Ramp<V>, factor: f32, receiver: Receiver<()>) {
    ramp.retarget(Some(factor));
    let started = Instant::now();
    loop {
        match receiver.recv_timeout(TICK) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => ramp.tick(started.elapsed().as_secs_f32()),
        }
        if ramp.finished {
            return;
        }
    }
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
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < EPSILON)
}

impl<V: Volume> Ramp<V> {
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

    fn tick(&mut self, seconds: f32) {
        if self.finished || self.settled {
            return;
        }
        let t = seconds.clamp(0.0, 1.0);
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
        if t >= 1.0 && (close(&self.accepted, &self.target) || seconds >= 1.5) {
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
        for original in [&[0.3, 0.8][..], &[0.0, 0.8], &[0.5]] {
            for factor in [0.0, 0.2, 1.0] {
                let mut r = ramp(original);
                r.retarget(Some(factor));
                r.tick(0.5);
                assert!(close(
                    &r.output.values,
                    &original
                        .iter()
                        .map(|v| v * (1.0 + factor) / 2.0)
                        .collect::<Vec<_>>()
                ));
                r.tick(1.0);
                r.retarget(None);
                r.tick(1.0);
                assert!(close(&r.output.values, original));
                assert!(r.finished);
            }
        }
    }
    #[test]
    fn ducking_respects_manual_changes_and_restores_interrupted_ramps() {
        let mut r = ramp(&[0.3, 0.8]);
        r.retarget(Some(0.2));
        r.tick(1.0);
        r.output.values = vec![0.5, 0.5];
        r.retarget(None);
        r.tick(1.0);
        assert_eq!(r.output.values, vec![0.5, 0.5]);
        let mut r = ramp(&[0.3, 0.8]);
        r.retarget(Some(0.0));
        r.tick(0.1);
        r.retarget(None);
        r.tick(1.0);
        assert!(close(&r.output.values, &[0.3, 0.8]));
    }
    #[test]
    fn ducking_rolls_back_every_channel_on_partial_failure() {
        let mut r = ramp(&[0.3, 0.8]);
        r.output.fail = true;
        r.retarget(Some(0.2));
        r.tick(0.5);
        assert!(r.finished);
        assert!(close(&r.output.values, &[0.3, 0.8]));
    }
}
