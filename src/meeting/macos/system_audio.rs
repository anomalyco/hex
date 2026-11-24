//! A meeting's system track through a private Core Audio process tap: every
//! output minus Hex's own sounds (the default), one app's processes, or one
//! output device (Labs › Choose meeting audio).
//!
//! A chosen app or output is looked for on the same 2 s route check the
//! default output uses. While it isn't present the source records nothing —
//! it never carries audio, and the track writer pads the gap once it returns
//! — and it is never widened to every output. A chosen source never falls
//! back to ScreenCaptureKit either, which records the whole display's audio;
//! only every-output capture has that fallback, in `open_system_audio`. The
//! route check also rereads the choice: another one reopens the source, so a
//! choice applies mid-recording.
//!
//! Every tap is a private aggregate device with no display dependency. An
//! output tap names its device as the aggregate's main and clock device, so
//! the tap is clocked by the device it records instead of the default output.

use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use block2::RcBlock;
use color_eyre::eyre::{Result, eyre};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_core_audio::{
    AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceStart, AudioDeviceStop, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap,
    AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress, CATapDescription,
    CATapMuteBehavior, kAudioHardwarePropertyTranslatePIDToProcessObject,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    kAudioTapPropertyFormat,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved,
};
use objc2_foundation::{NSArray, NSNumber, NSString};

use super::check;
use crate::audio::output_route::default_output_uid;
use crate::meeting::SAMPLE_RATE;
use crate::meeting::audio_source::{SourceChoice, effective};
use crate::meeting::capture::{
    LiveSource, POLL_INTERVAL, PacketSink, STALL_TIMEOUT, To16k, output_changed,
};

/// How often a tap re-reads what it records: the default output for every
/// output, the chosen app's processes, or whether the chosen output device is
/// listed. A global tap's aggregate names no output, so it keeps the device
/// it was created on and is reopened when that changes.
const ROUTE_CHECK: Duration = Duration::from_secs(2);
const TAP_QUEUE_CAPACITY: usize = 512;
/// How long a chosen tap that never delivered an IO cycle waits before it is
/// rebuilt: an idle app or output may legitimately deliver nothing, so this
/// is a bounded repair rather than a stall. Each rebuild that still delivers
/// nothing doubles the wait up to [`NEVER_DELIVERED_REBUILD_MAX`], so a tap
/// that cannot work never reopens in a tight loop.
const NEVER_DELIVERED_REBUILD: Duration = Duration::from_secs(30);
const NEVER_DELIVERED_REBUILD_MAX: Duration = Duration::from_secs(300);

/// The wait before the next rebuild of a tap that still never delivered:
/// doubled, capped.
fn next_rebuild_wait(wait: Duration) -> Duration {
    (wait * 2).clamp(NEVER_DELIVERED_REBUILD, NEVER_DELIVERED_REBUILD_MAX)
}

/// The scope a meeting's system track records, read from Labs › Choose
/// meeting audio as the source opens.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Scope {
    /// Every output, minus Hex's own sounds: the default, and the shape
    /// meetings recorded before the lab existed.
    All,
    /// One app's processes, by the canonical bundle id the Meetings pane
    /// lists.
    App(String),
    /// One output device, by the Core Audio UID the pane lists.
    Output(String),
}

impl Scope {
    /// The scope a saved choice names.
    pub(super) fn of(choice: &SourceChoice) -> Self {
        match (choice.app_id(), choice.output_id()) {
            (Some(id), _) => Self::App(id.to_owned()),
            (None, Some(id)) => Self::Output(id.to_owned()),
            (None, None) => Self::All,
        }
    }
}

/// What an open tap records, so a re-listing can tell when it changed.
#[derive(Clone, Debug)]
enum Recorded {
    /// Every output.
    Everything,
    /// One app's process objects, whether they play now or not.
    Processes(Vec<u32>),
    /// One output device, by Core Audio UID.
    Device(String),
}

impl Recorded {
    /// Whether `other` records the same source: an app's processes compare as
    /// a set, so the order Core Audio listed them in is not a change.
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Everything, Self::Everything) => true,
            (Self::Device(left), Self::Device(right)) => left == right,
            (Self::Processes(left), Self::Processes(right)) => {
                let objects = |objects: &Vec<u32>| {
                    let mut objects = objects.clone();
                    objects.sort_unstable();
                    objects.dedup();
                    objects
                };
                objects(left) == objects(right)
            }
            _ => false,
        }
    }
}

/// Whether a tap holding `open` must be rebuilt to record `listed`: nothing
/// open always starts one, and a different app process set or another device
/// replaces the tap that records the old one.
fn must_rebuild(open: Option<&Recorded>, listed: &Recorded) -> bool {
    open.is_none_or(|open| !open.same(listed))
}

/// What Core Audio reports for a scope on one route check.
enum Listing {
    /// The scope records this now.
    Present(Recorded),
    /// The chosen app isn't running or the chosen device isn't listed, so the
    /// track records nothing until it returns.
    Absent,
    /// The read failed, which says nothing about what is there.
    Unreadable,
}

/// One live tap on a private aggregate device, torn down with it.
struct Tap {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    io_proc: AudioDeviceIOProcID,
}

impl Tap {
    /// Starts a tap that records `recorded` and sends each IO cycle's mono
    /// samples, returning it with the tap's sample rate.
    fn start(
        recorded: &Recorded,
        sender: SyncSender<Vec<f32>>,
        callbacks: Arc<AtomicU64>,
        format_error: Arc<AtomicBool>,
    ) -> Result<(Self, u32)> {
        let description = tap_description(recorded);
        let mut tap = Self {
            tap: 0,
            aggregate: 0,
            io_proc: None,
        };
        // SAFETY: `description` is a valid tap description; `tap.tap` is
        // writable.
        check(
            unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap.tap) },
            "could not create the system audio tap",
        )?;
        let format = tap_format(tap.tap)?;
        if format.mFormatFlags & kAudioFormatFlagIsFloat == 0 || format.mBitsPerChannel != 32 {
            return Err(eyre!(
                "unsupported system audio tap format (flags {:#x}, {} bits)",
                format.mFormatFlags,
                format.mBitsPerChannel
            ));
        }
        let rate = format.mSampleRate.round() as u32;
        // SAFETY: Reading the UUID of a live description.
        let tap_uid = unsafe { description.UUID() }.UUIDString();
        tap.aggregate = create_aggregate(&tap_uid, recorded)?;

        let channels = format.mChannelsPerFrame.max(1) as usize;
        let non_interleaved = format.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0;
        let block = RcBlock::new(
            move |_now: NonNull<AudioTimeStamp>,
                  input: NonNull<AudioBufferList>,
                  _input_time: NonNull<AudioTimeStamp>,
                  _output: NonNull<AudioBufferList>,
                  _output_time: NonNull<AudioTimeStamp>| {
                callbacks.fetch_add(1, Ordering::Relaxed);
                // SAFETY: Core Audio passes a valid buffer list for this cycle.
                match unsafe { mono_samples(input, channels, non_interleaved) } {
                    Some(samples) => {
                        let _ = sender.try_send(samples);
                    }
                    None => format_error.store(true, Ordering::Relaxed),
                }
            },
        );
        let mut io_proc: AudioDeviceIOProcID = None;
        // SAFETY: The aggregate exists; Core Audio copies the block.
        check(
            unsafe {
                AudioDeviceCreateIOProcIDWithBlock(
                    NonNull::from(&mut io_proc),
                    tap.aggregate,
                    None,
                    RcBlock::as_ptr(&block),
                )
            },
            "could not attach to the system audio tap",
        )?;
        tap.io_proc = io_proc;
        // SAFETY: Starting the IOProc created above.
        check(
            unsafe { AudioDeviceStart(tap.aggregate, tap.io_proc) },
            "could not start the system audio tap",
        )?;
        Ok((tap, rate))
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        // SAFETY: Each identifier was created by this tap and is torn down
        // once, in reverse order of creation.
        unsafe {
            if self.aggregate != 0 {
                if self.io_proc.is_some() {
                    AudioDeviceStop(self.aggregate, self.io_proc);
                    AudioDeviceDestroyIOProcID(self.aggregate, self.io_proc);
                }
                AudioHardwareDestroyAggregateDevice(self.aggregate);
            }
            if self.tap != 0 {
                AudioHardwareDestroyProcessTap(self.tap);
            }
        }
    }
}

/// The tap description for what a meeting records: every output minus Hex,
/// one app's process objects, or one output device's stream 0 minus Hex. Hex
/// is never among an app's objects (`app_process_objects` leaves it out),
/// and never heard through a device tap.
fn tap_description(recorded: &Recorded) -> Retained<CATapDescription> {
    let excluded = || {
        let excluded = own_process_object()
            .map(|object| vec![NSNumber::new_u32(object)])
            .unwrap_or_default();
        NSArray::from_retained_slice(&excluded)
    };
    // SAFETY: Standard CATapDescription initialization and setters.
    unsafe {
        let description = match recorded {
            Recorded::Everything => CATapDescription::initMonoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &excluded(),
            ),
            Recorded::Processes(objects) => {
                let objects = objects
                    .iter()
                    .map(|object| NSNumber::new_u32(*object))
                    .collect::<Vec<_>>();
                let objects = NSArray::from_retained_slice(&objects);
                CATapDescription::initMonoMixdownOfProcesses(CATapDescription::alloc(), &objects)
            }
            Recorded::Device(uid) => {
                CATapDescription::initExcludingProcesses_andDeviceUID_withStream(
                    CATapDescription::alloc(),
                    &excluded(),
                    &NSString::from_str(uid),
                    0,
                )
            }
        };
        description.setName(&NSString::from_str("Hex meeting system audio"));
        description.setPrivate(true);
        description.setMuteBehavior(CATapMuteBehavior::Unmuted);
        description
    }
}

/// A private aggregate that reads one tap. An output tap names its device as
/// the aggregate's main and clock device, so the tap is clocked by the device
/// it records; the device is not a sub-device, which made USB interfaces stop
/// delivering to the IOProc. A tap-only aggregate is used if that is refused,
/// since the tap itself is already pinned to the device.
fn create_aggregate(tap_uid: &NSString, recorded: &Recorded) -> Result<AudioObjectID> {
    let name = "Hex Meeting System Audio";
    match recorded {
        Recorded::Device(device_uid) => {
            let device_uid = NSString::from_str(device_uid);
            let at_device = super::create_tap_aggregate_at(
                tap_uid,
                Some(&*device_uid),
                name,
                &next_aggregate_uid(),
            );
            match at_device {
                Ok(aggregate) => Ok(aggregate),
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "could not clock the system audio tap by its chosen output; using the tap alone"
                    );
                    super::create_tap_aggregate(tap_uid, name, &next_aggregate_uid())
                }
            }
        }
        Recorded::Everything | Recorded::Processes(_) => {
            super::create_tap_aggregate(tap_uid, name, &next_aggregate_uid())
        }
    }
}

/// A fresh aggregate UID: a rebuilt tap must not reuse the one it replaced.
fn next_aggregate_uid() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "anomaly.hex.meeting-tap.{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// A meeting's system track: a tap of `scope`, rebuilt when what it records
/// changes, and nothing at all while a chosen source isn't present.
pub(super) struct ProcessTapSource {
    scope: Scope,
    /// The open tap, or `None` while the chosen app or output isn't present
    /// and the track records nothing.
    tap: Option<Tap>,
    /// What the open tap records.
    recorded: Option<Recorded>,
    /// The default output's UID when a global tap was created; a different
    /// one means the tap must be rebuilt.
    output_uid: Option<String>,
    sender: SyncSender<Vec<f32>>,
    buffers: Receiver<Vec<f32>>,
    callbacks: Arc<AtomicU64>,
    format_error: Arc<AtomicBool>,
    seen_callbacks: u64,
    last_callback: Instant,
    /// Whether the open tap delivered any IO cycle at all: one that hasn't is
    /// a quiet source, not a stopped tap.
    delivered: bool,
    /// When the open tap was created, so a tap that never delivers an IO
    /// cycle is rebuilt on a bounded schedule.
    opened: Instant,
    /// How long the next never-delivered rebuild waits: doubled by each
    /// rebuild that still delivers nothing, and reset once a tap delivers.
    rebuild_wait: Duration,
    /// What the chosen app or output was last seen doing, read at most once
    /// per stall window so a stalled tap does not hammer Core Audio.
    playback_checked: Instant,
    playback_running: bool,
    route_checked: Instant,
    /// The open tap's resampler, at its own rate.
    resample: Option<To16k>,
}

// SAFETY: The Core Audio object IDs and IOProc handle are plain identifiers
// used only from the owning supervisor thread.
unsafe impl Send for ProcessTapSource {}

impl ProcessTapSource {
    /// Opens the source for `scope`. A chosen app or output that isn't
    /// present opens no tap and records nothing; the route check picks it up
    /// when it appears.
    pub(super) fn start(scope: Scope) -> Result<Self> {
        let (sender, buffers) = mpsc::sync_channel(TAP_QUEUE_CAPACITY);
        let mut source = Self {
            // The tap hears the output that is default when it is created, so
            // the baseline is read first; a change after this reopens the tap.
            output_uid: (scope == Scope::All).then(default_output_uid).flatten(),
            scope,
            tap: None,
            recorded: None,
            sender,
            buffers,
            callbacks: Arc::new(AtomicU64::new(0)),
            format_error: Arc::new(AtomicBool::new(false)),
            seen_callbacks: 0,
            last_callback: Instant::now(),
            delivered: false,
            opened: Instant::now(),
            rebuild_wait: NEVER_DELIVERED_REBUILD,
            playback_checked: Instant::now(),
            playback_running: false,
            route_checked: Instant::now(),
            resample: None,
        };
        match source.list() {
            Listing::Present(recorded) => source.open(recorded)?,
            Listing::Absent => tracing::info!(
                scope = ?source.scope,
                "the chosen meeting audio isn't present; the system track records silence until it returns"
            ),
            Listing::Unreadable => tracing::debug!(
                scope = ?source.scope,
                "could not read what the meeting records; trying again on the route check"
            ),
        }
        Ok(source)
    }

    /// What Core Audio reports the scope records now.
    fn list(&self) -> Listing {
        match &self.scope {
            Scope::All => Listing::Present(Recorded::Everything),
            Scope::App(id) => match crate::microphone_activity::output_process_objects() {
                Ok(processes) => {
                    let objects = crate::microphone_activity::app_process_objects(id, &processes);
                    if objects.is_empty() {
                        Listing::Absent
                    } else {
                        Listing::Present(Recorded::Processes(objects))
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, "could not read the meeting app's audio processes");
                    Listing::Unreadable
                }
            },
            Scope::Output(uid) => match crate::audio::output_route::output_device_listed(uid) {
                Some(true) => Listing::Present(Recorded::Device(uid.clone())),
                Some(false) => Listing::Absent,
                None => Listing::Unreadable,
            },
        }
    }

    /// Starts a tap that records `recorded`, replacing any open one.
    fn open(&mut self, recorded: Recorded) -> Result<()> {
        self.close();
        self.format_error.store(false, Ordering::Relaxed);
        let (tap, rate) = Tap::start(
            &recorded,
            self.sender.clone(),
            self.callbacks.clone(),
            self.format_error.clone(),
        )?;
        self.resample = Some(To16k::new(rate)?);
        self.tap = Some(tap);
        self.recorded = Some(recorded);
        self.seen_callbacks = self.callbacks.load(Ordering::Relaxed);
        self.last_callback = Instant::now();
        self.delivered = false;
        self.opened = Instant::now();
        tracing::info!(scope = ?self.scope, rate, "system audio tap started");
        Ok(())
    }

    /// Drops the open tap, so the track records nothing until one opens
    /// again.
    fn close(&mut self) {
        self.tap = None;
        self.recorded = None;
        self.resample = None;
        // A dropped tap may have left audio in the queue; it belongs to the
        // source that just stopped and never replays after it.
        while self.buffers.try_recv().is_ok() {}
    }

    /// Forwards the audio the open tap already queued, resampled by that same
    /// tap, before it is replaced: a rebuild loses nothing it recorded.
    fn flush(&mut self, sink: &PacketSink) {
        let mut samples = Vec::new();
        while let Ok(next) = self.buffers.try_recv() {
            samples.extend(next);
        }
        if samples.is_empty() {
            return;
        }
        let Some(resample) = self.resample.as_mut() else {
            return;
        };
        sink.send(SAMPLE_RATE, resample.push(&samples), None);
    }

    /// Replaces the open tap with one that records `recorded`, keeping what
    /// the old one already recorded.
    fn rebuild(&mut self, recorded: Recorded, sink: &PacketSink) -> Result<()> {
        self.flush(sink);
        self.open(recorded)
    }

    /// Whether silence past [`STALL_TIMEOUT`] means this tap stopped: every
    /// output runs while Hex records, so its silence is a stall, while a
    /// chosen app's or output's tap may sit quiet from the moment it opens (an
    /// app that isn't playing, an output nothing plays to) until its first
    /// IO cycle.
    fn silence_is_a_stall(&self) -> bool {
        self.scope == Scope::All || self.delivered
    }

    /// Whether the chosen source should be producing audio now: an app any of
    /// whose processes runs output, or an output device that is running
    /// somewhere. A chosen tap that should play but never delivers an IO cycle
    /// is stalled. Read at most once per stall window, so a stalled tap never
    /// hammers Core Audio.
    fn playing_now(&mut self) -> bool {
        if self.playback_checked.elapsed() < STALL_TIMEOUT {
            return self.playback_running;
        }
        self.playback_checked = Instant::now();
        self.playback_running = match &self.scope {
            Scope::All => true,
            Scope::App(id) => crate::microphone_activity::app_output_running(id).unwrap_or(false),
            Scope::Output(uid) => {
                crate::audio::output_route::output_device_running(uid).unwrap_or(false)
            }
        };
        self.playback_running
    }

    /// Applies one route check. Every output still reopens the whole source
    /// when the default output changes, as it always has; a chosen app's
    /// processes or a chosen device's presence is answered here, so the tap
    /// is rebuilt without waiting for the supervisor. Another choice reopens
    /// the source, which applies it mid-recording.
    fn check_route(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        // With the lab off the choice is every output, so this never fires
        // for a source that opened on every output.
        if Scope::of(&effective()) != self.scope {
            return Err("the meeting audio choice changed".into());
        }
        if self.scope == Scope::All {
            let current = default_output_uid();
            if self.output_uid.is_none() {
                // A read that failed when the tap was created adopts the
                // first that works, instead of never checking this stream.
                self.output_uid = current;
            } else if output_changed(self.output_uid.as_deref(), current.as_deref()) {
                return Err(format!(
                    "the default output changed from {} to {}",
                    self.output_uid.as_deref().unwrap_or("unknown"),
                    current.as_deref().unwrap_or("unknown"),
                ));
            }
            return Ok(());
        }
        match self.list() {
            Listing::Unreadable => {}
            Listing::Absent => {
                if self.tap.is_some() {
                    tracing::info!(
                        scope = ?self.scope,
                        "the chosen meeting audio stopped; the system track records silence until it returns"
                    );
                    self.close();
                }
            }
            Listing::Present(recorded) => {
                if must_rebuild(self.recorded.as_ref(), &recorded) {
                    self.rebuild(recorded, sink)
                        .map_err(|error| format!("{error:#}"))?;
                }
            }
        }
        Ok(())
    }
}

impl LiveSource for ProcessTapSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        if self.route_checked.elapsed() >= ROUTE_CHECK {
            self.route_checked = Instant::now();
            self.check_route(sink)?;
        }
        // A chosen source that isn't present records nothing: the track
        // writer pads the gap once it returns, so the system track keeps its
        // place on the meeting's timeline.
        if self.tap.is_none() {
            thread::sleep(POLL_INTERVAL);
            return Ok(());
        }
        let mut samples = Vec::new();
        // The source holds its own sender, so the queue only ever times out
        // with nothing new; the tap's stall check below covers a stopped one.
        if let Ok(first) = self.buffers.recv_timeout(POLL_INTERVAL) {
            samples.extend(first);
            while let Ok(next) = self.buffers.try_recv() {
                samples.extend(next);
            }
        }
        if !samples.is_empty() {
            let Some(resample) = self.resample.as_mut() else {
                return Err("system audio tap has no resampler".into());
            };
            sink.send(SAMPLE_RATE, resample.push(&samples), None);
        }
        if self.format_error.load(Ordering::Relaxed) {
            return Err("system audio tap delivered an unsupported buffer".into());
        }
        let callbacks = self.callbacks.load(Ordering::Relaxed);
        if callbacks != self.seen_callbacks {
            self.seen_callbacks = callbacks;
            self.delivered = true;
            self.last_callback = Instant::now();
            // A tap that delivers is healthy again: a later tap that never
            // does starts its rebuilds over at the first wait.
            self.rebuild_wait = NEVER_DELIVERED_REBUILD;
        } else if self.last_callback.elapsed() > STALL_TIMEOUT {
            // A chosen tap that should be playing and never delivers is as
            // stopped as one that went quiet, so both reopen the source.
            if self.silence_is_a_stall() || self.playing_now() {
                return Err("system audio tap stopped delivering audio".into());
            }
            // Otherwise the app or output is simply idle. A tap that has
            // never delivered at all may still be wedged, so it is rebuilt on
            // a schedule that backs off while it still delivers nothing.
            if !self.delivered && self.opened.elapsed() >= self.rebuild_wait {
                tracing::info!(
                    scope = ?self.scope,
                    wait = ?self.rebuild_wait,
                    "the system audio tap never delivered an IO cycle; rebuilding it"
                );
                if let Some(recorded) = self.recorded.clone() {
                    self.rebuild(recorded, sink)
                        .map_err(|error| format!("{error:#}"))?;
                    self.rebuild_wait = next_rebuild_wait(self.rebuild_wait);
                }
            }
        }
        Ok(())
    }

    fn describe(&self) -> &'static str {
        "core-audio-tap"
    }
}

/// Hex's own Core Audio process object, so a tap never records Hex's sounds.
fn own_process_object() -> Option<AudioObjectID> {
    let pid = std::process::id() as i32;
    let mut object: AudioObjectID = 0;
    let mut size = size_of::<AudioObjectID>() as u32;
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyTranslatePIDToProcessObject,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    // SAFETY: The qualifier is a pid and the output is one AudioObjectID.
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&address),
            size_of::<i32>() as u32,
            (&pid as *const i32).cast(),
            NonNull::from(&mut size),
            NonNull::from(&mut object).cast(),
        )
    };
    (status == 0 && object != 0).then_some(object)
}

fn tap_format(tap: AudioObjectID) -> Result<AudioStreamBasicDescription> {
    let mut format = AudioStreamBasicDescription {
        mSampleRate: 0.0,
        mFormatID: 0,
        mFormatFlags: 0,
        mBytesPerPacket: 0,
        mFramesPerPacket: 0,
        mBytesPerFrame: 0,
        mChannelsPerFrame: 0,
        mBitsPerChannel: 0,
        mReserved: 0,
    };
    let mut size = size_of::<AudioStreamBasicDescription>() as u32;
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioTapPropertyFormat,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    // SAFETY: The output buffer is one AudioStreamBasicDescription.
    check(
        unsafe {
            AudioObjectGetPropertyData(
                tap,
                NonNull::from(&address),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut format).cast(),
            )
        },
        "could not read the system audio tap format",
    )?;
    if format.mSampleRate <= 0.0 {
        return Err(eyre!("system audio tap reported no sample rate"));
    }
    Ok(format)
}

/// Mixes one IO cycle's float buffers to mono.
///
/// # Safety
/// `list` must point at a valid `AudioBufferList` of 32-bit float samples.
unsafe fn mono_samples(
    list: NonNull<AudioBufferList>,
    channels: usize,
    non_interleaved: bool,
) -> Option<Vec<f32>> {
    let list = unsafe { list.as_ref() };
    let count = list.mNumberBuffers as usize;
    let buffers: &[AudioBuffer] =
        unsafe { std::slice::from_raw_parts(list.mBuffers.as_ptr(), count) };
    let float_slice = |buffer: &AudioBuffer| -> Option<&[f32]> {
        if buffer.mData.is_null() {
            return Some(&[]);
        }
        let len = buffer.mDataByteSize as usize / size_of::<f32>();
        Some(unsafe { std::slice::from_raw_parts(buffer.mData.cast::<f32>(), len) })
    };
    match buffers {
        [] => Some(Vec::new()),
        [buffer] => {
            let interleaved = (buffer.mNumberChannels as usize).max(1);
            let samples = float_slice(buffer)?;
            if interleaved == 1 {
                return Some(samples.to_vec());
            }
            Some(
                samples
                    .chunks_exact(interleaved)
                    .map(|frame| frame.iter().sum::<f32>() / interleaved as f32)
                    .collect(),
            )
        }
        many if non_interleaved || many.len() == channels => {
            let planes: Vec<&[f32]> = many.iter().map(float_slice).collect::<Option<_>>()?;
            let frames = planes.iter().map(|plane| plane.len()).min().unwrap_or(0);
            Some(
                (0..frames)
                    .map(|frame| {
                        planes.iter().map(|plane| plane[frame]).sum::<f32>() / planes.len() as f32
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_names_the_scope_it_records() {
        assert_eq!(Scope::of(&SourceChoice::All), Scope::All);
        assert_eq!(
            Scope::of(&SourceChoice::App {
                id: "com.google.Chrome".into(),
                name: "Google Chrome".into(),
            }),
            Scope::App("com.google.Chrome".into())
        );
        assert_eq!(
            Scope::of(&SourceChoice::Output {
                id: "BuiltInSpeakerDevice".into(),
                name: "MacBook Pro Speakers".into(),
            }),
            Scope::Output("BuiltInSpeakerDevice".into())
        );
    }

    /// A tap that never delivers is rebuilt less and less often, so a tap
    /// that cannot work never reopens in a tight loop.
    #[test]
    fn a_never_delivered_tap_backs_off_its_rebuild() {
        let mut waits = Vec::new();
        let mut wait = NEVER_DELIVERED_REBUILD;
        for _ in 0..6 {
            waits.push(wait);
            wait = next_rebuild_wait(wait);
        }
        assert_eq!(waits, [30, 60, 120, 240, 300, 300].map(Duration::from_secs));
        assert_eq!(next_rebuild_wait(Duration::ZERO), NEVER_DELIVERED_REBUILD);
    }

    #[test]
    fn a_tap_is_rebuilt_only_for_a_different_source() {
        // The same app, listed in another order, is the same tap.
        assert!(!must_rebuild(
            Some(&Recorded::Processes(vec![7, 9])),
            &Recorded::Processes(vec![9, 7])
        ));
        assert!(!must_rebuild(
            Some(&Recorded::Device("BuiltInSpeakerDevice".into())),
            &Recorded::Device("BuiltInSpeakerDevice".into())
        ));
        assert!(!must_rebuild(
            Some(&Recorded::Everything),
            &Recorded::Everything
        ));
        // A helper that opened or closed, or another device, rebuilds it.
        assert!(must_rebuild(
            Some(&Recorded::Processes(vec![7])),
            &Recorded::Processes(vec![7, 9])
        ));
        assert!(must_rebuild(
            Some(&Recorded::Device("BuiltInSpeakerDevice".into())),
            &Recorded::Device("USB Audio".into())
        ));
        // Nothing open records as soon as the chosen source appears.
        assert!(must_rebuild(None, &Recorded::Processes(vec![7])));
        assert!(must_rebuild(None, &Recorded::Device("USB Audio".into())));
    }
}
