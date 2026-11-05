//! The CoreAudio adapter for "Muffle": a private process tap on every
//! other app's output, muted at the device while tapped, replayed through
//! `audio::muffle` onto the default output by a private aggregate device
//! that holds the tap and that output. Hex's own tones are left out of the
//! tap, so they play as they are. Needs System Audio Recording (macOS 14.2+).
use std::mem::size_of;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::AnyThread;
use objc2_core_audio::{
    AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceStart, AudioDeviceStop, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap, AudioObjectID,
    AudioObjectPropertyAddress, CATapDescription, CATapMuteBehavior, kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioTimeStamp, kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsNonInterleaved,
};
use objc2_foundation::{NSArray, NSNumber, NSString};

use crate::audio::muffle::Muffler;

/// How long the door may take to open before the tap goes regardless.
const OPEN_DEADLINE: Duration = Duration::from_millis(400);

/// Holds other audio muffled until restored or dropped.
pub(super) struct Guard {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    io_proc: AudioDeviceIOProcID,
    /// Kept so the IOProc's block outlives its registration.
    _block: Option<IoBlock>,
    opening: Arc<AtomicBool>,
    opened: Arc<AtomicBool>,
    /// IO cycles run and the loudest tapped sample, for the log at restore.
    cycles: Arc<AtomicU64>,
    peak: Arc<AtomicU32>,
}

type IoBlock = RcBlock<
    dyn Fn(
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
    ),
>;

/// The IOProc's state: Core Audio calls it one cycle at a time, so the
/// lock is never contended.
struct Replay {
    muffler: Muffler,
    planes: Vec<Vec<f32>>,
    interleaved_input: bool,
}

pub(super) fn start() -> Option<Guard> {
    match Guard::start() {
        Ok(guard) => {
            tracing::info!("muffling other audio while recording");
            Some(guard)
        }
        Err(error) => {
            tracing::warn!(%error, "could not muffle other audio; lowering it instead");
            None
        }
    }
}

impl Guard {
    fn start() -> Result<Self, String> {
        let output = super::default_output_device().ok_or("no default output")?;
        let output_uid =
            crate::microphone_activity::property_cf_string(output, kAudioDevicePropertyDeviceUID)
                .ok_or("the default output has no UID")?;
        let excluded = crate::meeting::own_process_object()
            .map(|object| vec![NSNumber::new_u32(object)])
            .unwrap_or_default();
        let excluded = NSArray::from_retained_slice(&excluded);
        // SAFETY: Standard CATapDescription initialization and setters.
        let description = unsafe {
            let description = CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &excluded,
            );
            description.setName(&NSString::from_str("Hex Muffle"));
            description.setPrivate(true);
            description.setMuteBehavior(CATapMuteBehavior::MutedWhenTapped);
            description
        };
        let mut tap: AudioObjectID = 0;
        // SAFETY: `description` is a valid tap description; `tap` is writable.
        check(
            unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap) },
            "could not create the muffle tap",
        )?;
        let mut guard = Self {
            tap,
            aggregate: 0,
            io_proc: None,
            _block: None,
            opening: Arc::new(AtomicBool::new(false)),
            opened: Arc::new(AtomicBool::new(false)),
            cycles: Arc::new(AtomicU64::new(0)),
            peak: Arc::new(AtomicU32::new(0)),
        };
        let format = crate::meeting::tap_format(tap).map_err(|error| error.to_string())?;
        if format.mFormatFlags & kAudioFormatFlagIsFloat == 0 || format.mBitsPerChannel != 32 {
            return Err(format!(
                "unsupported muffle tap format (flags {:#x}, {} bits)",
                format.mFormatFlags, format.mBitsPerChannel
            ));
        }
        // SAFETY: Reading the UUID of a live description.
        let tap_uid = unsafe { description.UUID() }.UUIDString();
        guard.aggregate = crate::meeting::create_tap_aggregate(
            &tap_uid,
            "Hex Muffle",
            &format!("anomaly.hex.muffle.{}", std::process::id()),
            Some(&output_uid),
        )
        .map_err(|error| error.to_string())?;
        let rate = super::read_property::<f64>(
            guard.aggregate,
            AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyNominalSampleRate,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            },
        )
        .filter(|rate| *rate > 0.0)
        .unwrap_or(format.mSampleRate);
        let channels = format.mChannelsPerFrame.clamp(1, 2) as usize;
        let replay = Mutex::new(Replay {
            muffler: Muffler::new(rate as f32, channels),
            planes: vec![Vec::new(); channels],
            interleaved_input: format.mFormatFlags & kAudioFormatFlagIsNonInterleaved == 0,
        });
        let opening = guard.opening.clone();
        let opened = guard.opened.clone();
        let cycles = guard.cycles.clone();
        let peak = guard.peak.clone();
        let block: IoBlock = RcBlock::new(
            move |_now: NonNull<AudioTimeStamp>,
                  input: NonNull<AudioBufferList>,
                  _input_time: NonNull<AudioTimeStamp>,
                  output: NonNull<AudioBufferList>,
                  _output_time: NonNull<AudioTimeStamp>| {
                let mut replay = replay
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if opening.load(Ordering::Acquire) {
                    replay.muffler.open();
                }
                // SAFETY: Core Audio passes valid float buffer lists for this cycle.
                let loudest = unsafe { replay.cycle(input, output) };
                cycles.fetch_add(1, Ordering::Relaxed);
                peak.fetch_max(loudest.to_bits(), Ordering::Relaxed);
                if replay.muffler.is_open() {
                    opened.store(true, Ordering::Release);
                }
            },
        );
        let mut io_proc: AudioDeviceIOProcID = None;
        // SAFETY: The aggregate exists; Core Audio copies the block.
        check(
            unsafe {
                AudioDeviceCreateIOProcIDWithBlock(
                    NonNull::from(&mut io_proc),
                    guard.aggregate,
                    None,
                    RcBlock::as_ptr(&block),
                )
            },
            "could not attach to the muffle tap",
        )?;
        guard.io_proc = io_proc;
        guard._block = Some(block);
        // SAFETY: Starting the IOProc created above.
        check(
            unsafe { AudioDeviceStart(guard.aggregate, guard.io_proc) },
            "could not start the muffle tap",
        )?;
        tracing::debug!(rate, channels, "muffle tap started");
        Ok(guard)
    }

    /// Opens the door (the filter sweeps back up) and returns once the
    /// output is dry again, or after [`OPEN_DEADLINE`].
    pub(super) fn restore(&mut self) {
        if self.aggregate == 0 || self.opening.swap(true, Ordering::AcqRel) {
            return;
        }
        let started = Instant::now();
        while !self.opened.load(Ordering::Acquire) && started.elapsed() < OPEN_DEADLINE {
            thread::sleep(Duration::from_millis(5));
        }
        self.teardown();
        tracing::info!(
            cycles = self.cycles.load(Ordering::Relaxed),
            peak = f32::from_bits(self.peak.load(Ordering::Relaxed)),
            "restored other audio after muffling it"
        );
    }

    fn teardown(&mut self) {
        // SAFETY: Each identifier was created by this guard and is torn down
        // once, in reverse order of creation.
        unsafe {
            if self.aggregate != 0 {
                if self.io_proc.is_some() {
                    AudioDeviceStop(self.aggregate, self.io_proc);
                    AudioDeviceDestroyIOProcID(self.aggregate, self.io_proc);
                    self.io_proc = None;
                }
                AudioHardwareDestroyAggregateDevice(self.aggregate);
                self.aggregate = 0;
            }
            if self.tap != 0 {
                AudioHardwareDestroyProcessTap(self.tap);
                self.tap = 0;
            }
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.restore();
        self.teardown();
    }
}

impl Replay {
    /// One IO cycle: the tap's channels into planes, muffled, into every
    /// output channel (an output with more channels than the tap repeats
    /// them). Returns the loudest tapped sample.
    ///
    /// # Safety
    /// `input` and `output` must point at valid `AudioBufferList`s of 32-bit
    /// float samples for the current cycle.
    unsafe fn cycle(
        &mut self,
        input: NonNull<AudioBufferList>,
        output: NonNull<AudioBufferList>,
    ) -> f32 {
        let inputs = unsafe { buffers(input) };
        let outputs = unsafe { buffers(output) };
        let frames = inputs
            .iter()
            .map(frames_in)
            .min()
            .unwrap_or(0)
            .min(outputs.iter().map(frames_in).min().unwrap_or(0));
        if frames == 0 {
            return 0.0;
        }
        for plane in &mut self.planes {
            plane.clear();
            plane.resize(frames, 0.0);
        }
        if self.interleaved_input {
            let Some(buffer) = inputs.first() else {
                return 0.0;
            };
            let channels = (buffer.mNumberChannels as usize).max(1);
            let samples = unsafe { samples_in(buffer) };
            for (plane_index, plane) in self.planes.iter_mut().enumerate() {
                let channel = plane_index.min(channels - 1);
                for (frame, sample) in plane.iter_mut().enumerate() {
                    *sample = samples[frame * channels + channel];
                }
            }
        } else {
            for (plane_index, plane) in self.planes.iter_mut().enumerate() {
                let samples = unsafe { samples_in(&inputs[plane_index.min(inputs.len() - 1)]) };
                plane.copy_from_slice(&samples[..frames]);
            }
        }
        let loudest = self
            .planes
            .iter()
            .flatten()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        let mut planes: Vec<&mut [f32]> = self.planes.iter_mut().map(Vec::as_mut_slice).collect();
        self.muffler.process(&mut planes);
        let mut next_channel = 0;
        for buffer in outputs {
            let channels = (buffer.mNumberChannels as usize).max(1);
            let samples = unsafe { samples_out(buffer) };
            for frame in 0..frames {
                for channel in 0..channels {
                    let plane = &self.planes[(next_channel + channel) % self.planes.len()];
                    samples[frame * channels + channel] = plane[frame];
                }
            }
            next_channel += channels;
        }
        loudest
    }
}

/// # Safety
/// `list` must point at a valid `AudioBufferList`.
unsafe fn buffers<'a>(list: NonNull<AudioBufferList>) -> &'a [AudioBuffer] {
    let list = unsafe { list.as_ref() };
    unsafe { std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize) }
}

fn frames_in(buffer: &AudioBuffer) -> usize {
    if buffer.mData.is_null() {
        return 0;
    }
    buffer.mDataByteSize as usize / size_of::<f32>() / (buffer.mNumberChannels as usize).max(1)
}

/// # Safety
/// `buffer` must hold 32-bit float samples (checked against the tap format).
unsafe fn samples_in<'a>(buffer: &AudioBuffer) -> &'a [f32] {
    if buffer.mData.is_null() {
        return &[];
    }
    unsafe {
        std::slice::from_raw_parts(
            buffer.mData.cast::<f32>(),
            buffer.mDataByteSize as usize / size_of::<f32>(),
        )
    }
}

/// # Safety
/// `buffer` must be an output buffer of 32-bit float samples for this cycle.
unsafe fn samples_out<'a>(buffer: &AudioBuffer) -> &'a mut [f32] {
    if buffer.mData.is_null() {
        return &mut [];
    }
    unsafe {
        std::slice::from_raw_parts_mut(
            buffer.mData.cast::<f32>(),
            buffer.mDataByteSize as usize / size_of::<f32>(),
        )
    }
}

fn check(status: i32, context: &str) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("{context} (OSStatus {status})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Muffles this Mac's output for three seconds. Needs System Audio
    /// Recording for the test binary and something playing to hear it.
    #[test]
    #[ignore = "taps and replays this Mac's live output"]
    fn muffles_the_default_output_for_a_while() {
        let mut guard = Guard::start().unwrap();
        thread::sleep(Duration::from_secs(3));
        let cycles = guard.cycles.load(Ordering::Relaxed);
        let peak = f32::from_bits(guard.peak.load(Ordering::Relaxed));
        guard.restore();
        eprintln!("{cycles} cycles, peak input {peak}");
        assert!(cycles > 0, "the tap delivered no cycles");
        assert!(
            guard.opened.load(Ordering::Relaxed),
            "the door opened before teardown"
        );
    }
}
