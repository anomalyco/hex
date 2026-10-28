//! Pause a browser that is audibly playing while you dictate, and play it
//! again only when Hex saw it go silent right after its own pause.
//!
//! macOS no longer tells other apps what is playing, so Hex listens to the
//! browsers' own output: the level says whether the system pause reached a
//! browser, and only that proof allows the play at the end.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use block2::RcBlock;
use color_eyre::eyre::{Result, eyre};
use objc2::AnyThread;
use objc2_core_audio::{
    AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceStart, AudioDeviceStop, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap, AudioObjectID,
    CATapDescription, CATapMuteBehavior,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp};
use objc2_foundation::{NSArray, NSNumber, NSString};

/// Sound: a peak above this over the first listen.
const SOUND_DBFS: f32 = -60.0;
/// Silence: a peak below this in one window after the pause.
const SILENCE_DBFS: f32 = -90.0;
const LISTEN: Duration = Duration::from_millis(300);
const CONFIRM_WINDOW: Duration = Duration::from_millis(100);
const CONFIRM_WINDOWS: usize = 10;

/// The explicit MediaRemote commands. Never the toggle: it can start music.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum MediaCommand {
    Play = 0,
    Pause = 1,
}

pub(crate) trait BrowserAudio {
    /// A browser is using the microphone: probably a call.
    fn in_call(&mut self) -> bool;
    /// Peak of the browsers' output over `window`, in dBFS; None when it cannot listen.
    fn peak_dbfs(&mut self, window: Duration) -> Option<f32>;
    fn send(&mut self, command: MediaCommand) -> bool;
}

/// Proof that Hex paused a sounding browser; only this allows play.
#[derive(Debug)]
pub(crate) struct PausedBrowser(());

#[cfg(test)]
impl PausedBrowser {
    pub(crate) fn for_test() -> Self {
        Self(())
    }
}

pub(crate) fn pause(audio: &mut impl BrowserAudio) -> Option<PausedBrowser> {
    if audio.in_call() {
        tracing::info!("left the browser alone: it is using the microphone");
        return None;
    }
    match audio.peak_dbfs(LISTEN) {
        Some(peak) if peak > SOUND_DBFS => {}
        Some(_) => {
            tracing::info!("left the browser alone: it is silent");
            return None;
        }
        None => {
            tracing::info!("left the browser alone: cannot hear it");
            return None;
        }
    }
    if !audio.send(MediaCommand::Pause) {
        tracing::info!("the system refused the pause command");
        return None;
    }
    for _ in 0..CONFIRM_WINDOWS {
        if audio
            .peak_dbfs(CONFIRM_WINDOW)
            .is_some_and(|peak| peak < SILENCE_DBFS)
        {
            tracing::info!("paused the browser");
            return Some(PausedBrowser(()));
        }
    }
    tracing::info!("sent pause but the browser still plays; it will not get play");
    None
}

pub(crate) fn resume(audio: &mut impl BrowserAudio, _paused: PausedBrowser) {
    if audio
        .peak_dbfs(LISTEN)
        .is_some_and(|peak| peak > SOUND_DBFS)
    {
        tracing::info!("the browser already plays again; sent no play");
        return;
    }
    if audio.send(MediaCommand::Play) {
        tracing::info!("resumed the browser");
    } else {
        tracing::info!("the system refused the play command");
    }
}

/// The browsers' own output, heard through a private Core Audio process tap,
/// and the system media commands. Build one per pause or resume: the tap
/// covers the browser processes running when it first listens.
pub(crate) struct NativeBrowserAudio {
    tap: Option<BrowserTap>,
    started: bool,
}

impl NativeBrowserAudio {
    pub(crate) fn new() -> Self {
        Self {
            tap: None,
            started: false,
        }
    }
}

impl BrowserAudio for NativeBrowserAudio {
    fn in_call(&mut self) -> bool {
        crate::microphone_activity::active_microphone_applications()
            .map(|applications| applications.iter().any(|app| app.is_browser()))
            .unwrap_or(true)
    }

    fn peak_dbfs(&mut self, window: Duration) -> Option<f32> {
        if !self.started {
            self.started = true;
            self.tap = BrowserTap::start()
                .inspect_err(|error| tracing::warn!(%error, "could not listen to the browser"))
                .ok()
                .flatten();
        }
        self.tap.as_ref()?.peak_dbfs(window)
    }

    fn send(&mut self, command: MediaCommand) -> bool {
        send_media_command(command)
    }
}

struct BrowserTap {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    io_proc: AudioDeviceIOProcID,
    /// The largest |sample| since the last reset, as f32 bits. Non-negative
    /// floats order the same as their bits, so `fetch_max` keeps the peak.
    peak: Arc<AtomicU32>,
    callbacks: Arc<AtomicU64>,
}

impl BrowserTap {
    /// None when no browser is running.
    fn start() -> Result<Option<Self>> {
        let processes = crate::microphone_activity::browser_process_objects()?;
        if processes.is_empty() {
            return Ok(None);
        }
        let processes: Vec<_> = processes.into_iter().map(NSNumber::new_u32).collect();
        let processes = NSArray::from_retained_slice(&processes);
        // SAFETY: Standard CATapDescription initialization and setters.
        let description = unsafe {
            let description =
                CATapDescription::initMonoMixdownOfProcesses(CATapDescription::alloc(), &processes);
            description.setName(&NSString::from_str("Hex browser level"));
            description.setPrivate(true);
            description.setMuteBehavior(CATapMuteBehavior::Unmuted);
            description
        };
        let mut tap: AudioObjectID = 0;
        // SAFETY: `description` is a valid tap description; `tap` is writable.
        check(
            unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap) },
            "could not create the browser audio tap",
        )?;
        let mut source = Self {
            tap,
            aggregate: 0,
            io_proc: None,
            peak: Arc::new(AtomicU32::new(0)),
            callbacks: Arc::new(AtomicU64::new(0)),
        };
        // SAFETY: Reading the UUID of a live description.
        let tap_uid = unsafe { description.UUID() }.UUIDString();
        source.aggregate = crate::meeting::create_tap_aggregate(
            &tap_uid,
            "Hex Browser Level",
            &format!("anomaly.hex.browser-tap.{}", std::process::id()),
        )?;
        let peak = source.peak.clone();
        let callbacks = source.callbacks.clone();
        let block = RcBlock::new(
            move |_now: NonNull<AudioTimeStamp>,
                  input: NonNull<AudioBufferList>,
                  _input_time: NonNull<AudioTimeStamp>,
                  _output: NonNull<AudioBufferList>,
                  _output_time: NonNull<AudioTimeStamp>| {
                // SAFETY: Core Audio passes a valid list of float buffers for this cycle.
                let cycle_peak = unsafe { buffer_peak(input) };
                peak.fetch_max(cycle_peak.to_bits(), Ordering::Relaxed);
                callbacks.fetch_add(1, Ordering::Relaxed);
            },
        );
        let mut io_proc: AudioDeviceIOProcID = None;
        // SAFETY: The aggregate exists; Core Audio copies the block.
        check(
            unsafe {
                AudioDeviceCreateIOProcIDWithBlock(
                    NonNull::from(&mut io_proc),
                    source.aggregate,
                    None,
                    RcBlock::as_ptr(&block),
                )
            },
            "could not attach to the browser audio tap",
        )?;
        source.io_proc = io_proc;
        // SAFETY: Starting the IOProc created above.
        check(
            unsafe { AudioDeviceStart(source.aggregate, source.io_proc) },
            "could not start the browser audio tap",
        )?;
        Ok(Some(source))
    }

    /// None when the tap delivered nothing during `window`.
    fn peak_dbfs(&self, window: Duration) -> Option<f32> {
        let callbacks = self.callbacks.load(Ordering::Relaxed);
        self.peak.store(0, Ordering::Relaxed);
        std::thread::sleep(window);
        if self.callbacks.load(Ordering::Relaxed) == callbacks {
            return None;
        }
        let peak = f32::from_bits(self.peak.load(Ordering::Relaxed));
        Some(if peak > 0.0 {
            20.0 * peak.log10()
        } else {
            -200.0
        })
    }
}

impl Drop for BrowserTap {
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

/// The largest |sample| in one IO cycle.
///
/// # Safety
/// `list` must point at a valid `AudioBufferList` of 32-bit float samples.
unsafe fn buffer_peak(list: NonNull<AudioBufferList>) -> f32 {
    let list = unsafe { list.as_ref() };
    let buffers: &[AudioBuffer] =
        unsafe { std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize) };
    buffers
        .iter()
        .filter(|buffer| !buffer.mData.is_null())
        .flat_map(|buffer| {
            let len = buffer.mDataByteSize as usize / size_of::<f32>();
            unsafe { std::slice::from_raw_parts(buffer.mData.cast::<f32>(), len) }
        })
        .fold(0.0, |peak: f32, sample| peak.max(sample.abs()))
}

fn check(status: i32, context: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(eyre!("{context} (OSStatus {status})"))
    }
}

type SendCommand = unsafe extern "C" fn(u32, *const c_void) -> bool;

/// `MRMediaRemoteSendCommand` from the private MediaRemote framework: the
/// same pause and play as the media keys, sent to the app macOS has as Now Playing.
fn send_media_command(command: MediaCommand) -> bool {
    static SEND: OnceLock<Option<SendCommand>> = OnceLock::new();
    let send = SEND.get_or_init(|| {
        // SAFETY: dlopen and dlsym with static C strings; the symbol has
        // this signature on every macOS that ships the framework.
        unsafe {
            let handle = libc::dlopen(
                c"/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote".as_ptr(),
                libc::RTLD_LAZY,
            );
            if handle.is_null() {
                return None;
            }
            let symbol = libc::dlsym(handle, c"MRMediaRemoteSendCommand".as_ptr());
            (!symbol.is_null()).then(|| std::mem::transmute::<*mut c_void, SendCommand>(symbol))
        }
    });
    let Some(send) = send else {
        tracing::warn!("MediaRemote is not available");
        return false;
    };
    // SAFETY: A command number and no options dictionary.
    unsafe { send(command as u32, std::ptr::null()) }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use super::*;

    struct Fake {
        in_call: bool,
        peaks: VecDeque<Option<f32>>,
        sent: Vec<MediaCommand>,
        send_ok: bool,
        listened: usize,
    }

    impl Fake {
        fn new(peaks: impl IntoIterator<Item = Option<f32>>) -> Self {
            Self {
                in_call: false,
                peaks: peaks.into_iter().collect(),
                sent: Vec::new(),
                send_ok: true,
                listened: 0,
            }
        }
    }

    impl BrowserAudio for Fake {
        fn in_call(&mut self) -> bool {
            self.in_call
        }

        fn peak_dbfs(&mut self, _window: Duration) -> Option<f32> {
            self.listened += 1;
            self.peaks.pop_front().flatten()
        }

        fn send(&mut self, command: MediaCommand) -> bool {
            self.sent.push(command);
            self.send_ok
        }
    }

    #[test]
    #[ignore = "needs a browser playing audio and System Audio Recording"]
    fn native_tap_hears_a_playing_browser() {
        let peak = NativeBrowserAudio::new().peak_dbfs(Duration::from_millis(300));
        eprintln!("browser peak: {peak:?} dBFS");
        assert!(peak.is_some_and(|peak| peak > SOUND_DBFS));
    }

    #[test]
    fn a_silent_browser_is_left_alone() {
        let mut audio = Fake::new([Some(-120.0)]);
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, []);
    }

    #[test]
    fn a_call_is_never_paused() {
        let mut audio = Fake::new([Some(-20.0)]);
        audio.in_call = true;
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, []);
        assert_eq!(audio.listened, 0);
    }

    #[test]
    fn a_confirmed_pause_is_resumed() {
        let mut audio = Fake::new([Some(-20.0), Some(-30.0), Some(-200.0), Some(-200.0)]);
        let paused = pause(&mut audio).expect("the pause is confirmed");
        resume(&mut audio, paused);
        assert_eq!(audio.sent, [MediaCommand::Pause, MediaCommand::Play]);
    }

    #[test]
    fn two_sounding_tabs_never_get_play() {
        let mut audio =
            Fake::new(std::iter::once(Some(-20.0)).chain(std::iter::repeat_n(Some(-25.0), 10)));
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }

    #[test]
    fn no_level_sends_nothing() {
        let mut audio = Fake::new([None]);
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, []);
    }

    #[test]
    fn a_browser_already_playing_gets_no_play() {
        let mut audio = Fake::new([Some(-20.0), Some(-200.0)]);
        let paused = pause(&mut audio).expect("the pause is confirmed");
        audio.peaks = VecDeque::from([Some(-20.0)]);
        resume(&mut audio, paused);
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }

    #[test]
    fn a_refused_command_is_not_confirmed() {
        let mut audio = Fake::new([Some(-20.0), Some(-200.0)]);
        audio.send_ok = false;
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }
}
