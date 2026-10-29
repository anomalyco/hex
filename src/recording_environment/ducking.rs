//! Relative, channel-preserving output ducking, adapted from Scrawl's playback ramp.
//! Only the environment worker waits for restoration; capture and the UI never do.
use std::mem::{offset_of, size_of};
use std::ptr::NonNull;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectIsPropertySettable,
    AudioObjectPropertyAddress, AudioObjectSetPropertyData, kAudioObjectPropertyScopeOutput,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};

const TICK: Duration = Duration::from_millis(16);
const EPSILON: f32 = 0.002;

pub(super) struct Guard {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Guard {
    pub(super) fn start(factor: f32) -> Option<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("output-ducking".into())
            .spawn(move || {
                if let Some(output) = Output::current()
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

    pub(super) fn restore(&mut self) {
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

fn run(ramp: &mut Ramp<Output>, factor: f32, receiver: Receiver<()>) {
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

trait Volume {
    fn read(&self) -> Option<Vec<f32>>;
    fn write(&mut self, values: &[f32]) -> bool;
}

struct Output {
    device: u32,
    elements: Vec<u32>,
}

fn address(selector: [u8; 4], element: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: u32::from_be_bytes(selector),
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: element,
    }
}

impl Output {
    fn current() -> Option<Self> {
        let device = super::default_output_device()?;
        let mut property = address(*b"slay", 0); // stream configuration
        let mut bytes = 0;
        // SAFETY: The property address and size point to initialized stack values.
        if unsafe {
            AudioObjectGetPropertyDataSize(
                device,
                NonNull::from(&mut property),
                0,
                std::ptr::null(),
                NonNull::from(&mut bytes),
            )
        } != 0
            || !(size_of::<AudioBufferList>() as u32..=65536).contains(&bytes)
        {
            return None;
        }
        // u64 storage gives AudioBufferList and its pointer fields their required alignment.
        let mut storage = vec![0u64; (bytes as usize).div_ceil(8)];
        // SAFETY: Storage is aligned and has at least the size reported by CoreAudio.
        if unsafe {
            AudioObjectGetPropertyData(
                device,
                NonNull::from(&mut property),
                0,
                std::ptr::null(),
                NonNull::from(&mut bytes),
                NonNull::new(storage.as_mut_ptr()).unwrap().cast(),
            )
        } != 0
        {
            return None;
        }
        if bytes as usize > storage.len() * size_of::<u64>()
            || (bytes as usize) < size_of::<AudioBufferList>()
        {
            return None;
        }
        // SAFETY: The successful property read initialized the header within storage.
        let count =
            unsafe { (*(storage.as_ptr().cast::<AudioBufferList>())).mNumberBuffers } as usize;
        let offset = offset_of!(AudioBufferList, mBuffers);
        if offset + count.checked_mul(size_of::<AudioBuffer>())? > bytes as usize {
            return None;
        }
        // SAFETY: The bounds above cover every buffer, and offset preserves alignment.
        let buffers = unsafe {
            std::slice::from_raw_parts(
                storage
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<AudioBuffer>(),
                count,
            )
        };
        let channels = buffers
            .iter()
            .try_fold(0u32, |sum, b| sum.checked_add(b.mNumberChannels))?;
        if channels > 256 {
            return None;
        }
        let separate = Self {
            device,
            elements: (1..=channels).collect(),
        };
        if channels > 0 && separate.writable() && separate.read().is_some() {
            return Some(separate);
        }
        // A master write must not destroy a readable balance we cannot restore individually.
        if separate.elements.iter().any(|&element| {
            super::read_property::<f32>(device, address(*b"volm", element)).is_some()
        }) {
            return None;
        }
        let master = Self {
            device,
            elements: vec![0],
        };
        (master.writable() && master.read().is_some()).then_some(master)
    }

    fn writable(&self) -> bool {
        self.elements.iter().all(|&element| {
            let mut property = address(*b"volm", element);
            let mut settable = 0;
            // SAFETY: Both output pointers refer to initialized, correctly typed values.
            unsafe {
                AudioObjectIsPropertySettable(
                    self.device,
                    NonNull::from(&mut property),
                    NonNull::from(&mut settable),
                ) == 0
                    && settable != 0
            }
        })
    }
}

impl Volume for Output {
    fn read(&self) -> Option<Vec<f32>> {
        self.elements
            .iter()
            .map(|&element| {
                super::read_property::<f32>(self.device, address(*b"volm", element))
                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
            })
            .collect()
    }

    fn write(&mut self, values: &[f32]) -> bool {
        let mut success = true;
        for (&element, &value) in self.elements.iter().zip(values) {
            let mut property = address(*b"volm", element);
            let mut value = value;
            // SAFETY: The scalar and property are initialized and correctly sized.
            success &= unsafe {
                AudioObjectSetPropertyData(
                    self.device,
                    NonNull::from(&mut property),
                    0,
                    std::ptr::null(),
                    size_of::<f32>() as u32,
                    NonNull::from(&mut value).cast(),
                ) == 0
            };
        }
        success
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
