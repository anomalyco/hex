//! Pause a browser that is audibly playing while you dictate, and play it
//! again only when Hex saw it go silent right after its own pause.
//!
//! macOS no longer tells other apps what is playing, so Hex listens to the
//! browsers' own output: the level says whether the pause reached a
//! browser, and only that proof allows the play at the end. Both commands go
//! to the browsers by name: the system's Now Playing app is whichever app
//! started last, often a music player Hex resumed after the browser (#2825).

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

use crate::audio::CaptureInstant;
use crate::microphone_activity::BrowserProcesses;

/// Sound: a peak above this in every listen window.
const SOUND_DBFS: f32 = -60.0;
/// Silence: a peak below this in consecutive windows soon after the pause.
const SILENCE_DBFS: f32 = -90.0;
const WINDOW: Duration = Duration::from_millis(100);
const LISTEN_WINDOWS: usize = 3;
/// The silence must start in the first windows after the pause: a sound
/// that ends later on its own is not the pause reaching the browser.
const CONFIRM_WINDOWS: usize = 5;
const SILENT_WINDOWS: usize = 2;
/// The last listen before play.
const RESUME_LISTEN: Duration = Duration::from_millis(300);
/// After play, the browser must sound within this many windows (2 s). If it
/// does not, the play reached something else, so Hex pauses it again.
const PLAY_CHECK_WINDOWS: usize = 20;
/// A browser that closed its output opens it again within this many windows
/// of play (about 200 ms on Brave). If none does, the play reached another
/// app, so Hex pauses it again sooner.
const OPEN_CHECK_WINDOWS: usize = 10;
/// How long a new tap may take to deliver its first cycle.
const TAP_WARM_UP: Duration = Duration::from_millis(300);

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
    /// The browsers with open output, by canonical bundle ID.
    fn playing_browsers(&mut self) -> Vec<&'static str>;
    /// Sends `command` to `browsers`, or to the system's Now Playing app
    /// when there are none or the system cannot address an app by name.
    fn send(&mut self, command: MediaCommand, browsers: &[&'static str]) -> bool;
    /// A browser process runs and every one has closed its audio output.
    fn output_closed(&mut self) -> bool;
}

/// Proof that Hex paused a sounding browser; only this allows play, and
/// only to the browsers it names.
#[derive(Debug)]
pub(crate) struct PausedBrowser {
    browsers: Vec<&'static str>,
}

#[cfg(test)]
impl PausedBrowser {
    pub(crate) fn for_test() -> Self {
        Self {
            browsers: Vec::new(),
        }
    }
}

/// Pauses a sounding browser and returns the proof, with the instant the
/// pause command was sent: the confirmation windows after it are not part of
/// when the browser actually stopped.
pub(crate) fn pause(audio: &mut impl BrowserAudio) -> Option<(PausedBrowser, CaptureInstant)> {
    if audio.in_call() {
        tracing::info!("left the browser alone: it is using the microphone");
        return None;
    }
    for _ in 0..LISTEN_WINDOWS {
        match audio.peak_dbfs(WINDOW) {
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
    }
    let browsers = audio.playing_browsers();
    let sent_at = CaptureInstant::now();
    if !audio.send(MediaCommand::Pause, &browsers) {
        tracing::info!("the system refused the pause command");
        return None;
    }
    let mut silent = 0;
    for _ in 0..CONFIRM_WINDOWS {
        if audio
            .peak_dbfs(WINDOW)
            .is_some_and(|peak| peak < SILENCE_DBFS)
        {
            silent += 1;
            if silent == SILENT_WINDOWS {
                tracing::info!("paused the browser");
                return Some((PausedBrowser { browsers }, sent_at));
            }
        } else {
            silent = 0;
        }
    }
    tracing::info!("sent pause but the browser still plays; it will not get play");
    None
}

pub(crate) fn resume(audio: &mut impl BrowserAudio, paused: PausedBrowser) {
    let mut heard = true;
    match audio.peak_dbfs(RESUME_LISTEN) {
        Some(peak) if peak <= SOUND_DBFS => {}
        Some(_) => {
            tracing::info!("the browser already plays again; sent no play");
            return;
        }
        // A pause longer than about 10 s closes the browser's output, and a
        // tap on closed outputs delivers nothing: that browser is still paused.
        None if audio.output_closed() => heard = false,
        None => {
            tracing::info!("cannot hear the browser; sent no play");
            return;
        }
    }
    if !audio.send(MediaCommand::Play, &paused.browsers) {
        tracing::info!("the system refused the play command");
        return;
    }
    // The tap delivers again once play opens the output, about 300 ms later.
    for window in 1..=PLAY_CHECK_WINDOWS {
        match audio.peak_dbfs(WINDOW) {
            Some(peak) if peak > SOUND_DBFS => {
                tracing::info!("resumed the browser");
                return;
            }
            Some(_) => heard = true,
            None if !heard => {
                if window == OPEN_CHECK_WINDOWS && audio.output_closed() {
                    break;
                }
            }
            None => {
                tracing::info!("sent play but cannot hear the browser any more");
                return;
            }
        }
    }
    if !heard && !audio.output_closed() {
        tracing::info!("sent play but cannot hear the browser");
        return;
    }
    // A closed or reloaded tab stays silent, and a play sent without a
    // browser's name went to whatever macOS now treats as playing: pause again.
    tracing::info!("the browser stayed silent after play; sent pause again");
    audio.send(MediaCommand::Pause, &paused.browsers);
}

/// The browsers' own output, heard through a private Core Audio process tap,
/// and the system media commands. Build one per pause or resume: the tap
/// covers the browser processes running when it first listens.
pub(crate) struct NativeBrowserAudio {
    processes: BrowserProcesses,
    tap: Option<BrowserTap>,
    started: bool,
}

impl NativeBrowserAudio {
    /// Hears only browsers with open output: a closed output is silent, so
    /// most dictations open no tap at all.
    pub(crate) fn for_pause() -> Self {
        Self::new(BrowserProcesses::Playing)
    }

    /// Hears every browser: a paused one closes its output a few seconds later.
    pub(crate) fn for_resume() -> Self {
        Self::new(BrowserProcesses::All)
    }

    fn new(processes: BrowserProcesses) -> Self {
        Self {
            processes,
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
            self.tap = BrowserTap::start(self.processes)
                .inspect_err(|error| tracing::warn!(%error, "could not listen to the browser"))
                .ok()
                .flatten();
        }
        self.tap.as_ref()?.peak_dbfs(window)
    }

    fn playing_browsers(&mut self) -> Vec<&'static str> {
        crate::microphone_activity::browser_applications(BrowserProcesses::Playing)
            .inspect_err(|error| tracing::warn!(%error, "could not list the playing browsers"))
            .unwrap_or_default()
    }

    fn send(&mut self, command: MediaCommand, browsers: &[&'static str]) -> bool {
        let mut by_name = None;
        for &browser in browsers {
            if let Some(sent) = send_media_command_to_app(command, browser) {
                by_name = Some(by_name.unwrap_or(false) | sent);
            }
        }
        match by_name {
            Some(true) => true,
            // A play by name the browsers refused goes nowhere else: the
            // system one could start an app Hex never paused.
            Some(false) if command == MediaCommand::Play => false,
            Some(false) => {
                tracing::info!(?browsers, "no browser took the pause; sent it to the system");
                send_media_command(command)
            }
            None => send_media_command(command),
        }
    }

    fn output_closed(&mut self) -> bool {
        use crate::microphone_activity::browser_process_objects;
        // A quit browser is not a paused one: play would reach another app.
        browser_process_objects(BrowserProcesses::All).is_ok_and(|all| !all.is_empty())
            && browser_process_objects(BrowserProcesses::Playing).is_ok_and(|open| open.is_empty())
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
    /// None when no browser process matches.
    fn start(processes: BrowserProcesses) -> Result<Option<Self>> {
        let processes = crate::microphone_activity::browser_process_objects(processes)?;
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
        let warm_up = std::time::Instant::now();
        while source.callbacks.load(Ordering::Relaxed) == 0 && warm_up.elapsed() < TAP_WARM_UP {
            std::thread::sleep(Duration::from_millis(10));
        }
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
/// `MRMediaRemoteSendCommandToApp(command, options, origin, bundleID,
/// flags, queue, completion)`; the completion gets an error code and the
/// handler statuses.
type SendCommandToApp = unsafe extern "C" fn(
    u32,
    *const c_void,
    *const c_void,
    *const c_void,
    u32,
    *mut c_void,
    *mut c_void,
);
type GetLocalOrigin = unsafe extern "C" fn() -> *const c_void;

/// How long a browser has to answer a command sent by name.
const SEND_TO_APP_TIMEOUT: Duration = Duration::from_millis(500);

unsafe extern "C" {
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    fn CFGetTypeID(value: *const c_void) -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: *const c_void, number_type: isize, value: *mut c_void) -> bool;
}

const CF_NUMBER_SINT64_TYPE: isize = 4;

/// The handler statuses MediaRemote reports for a command sent by name;
/// non-numbers are skipped.
///
/// # Safety
/// `statuses` must be null or a live CF object for the duration of the call.
unsafe fn handler_statuses(statuses: *const c_void) -> Vec<i64> {
    // SAFETY: The caller keeps `statuses` alive; every value is type-checked
    // before it is read.
    unsafe {
        if statuses.is_null() || CFGetTypeID(statuses) != CFArrayGetTypeID() {
            return Vec::new();
        }
        (0..CFArrayGetCount(statuses))
            .filter_map(|index| {
                let value = CFArrayGetValueAtIndex(statuses, index);
                if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
                    return None;
                }
                let mut status = 0_i64;
                CFNumberGetValue(value, CF_NUMBER_SINT64_TYPE, (&mut status as *mut i64).cast())
                    .then_some(status)
            })
            .collect()
    }
}

/// A command sent by name reached the app only when MediaRemote reports no
/// error and at least one handler succeeded (status 0): an error of 0 with
/// no handler, or only failing ones (no such content, command failed, no
/// actionable item), means nothing acted on it.
fn handled(error: u32, statuses: &[i64]) -> bool {
    error == 0 && statuses.contains(&0)
}

/// A symbol of the private MediaRemote framework, null when it is missing.
fn media_remote_symbol(name: &std::ffi::CStr) -> *mut c_void {
    static HANDLE: OnceLock<usize> = OnceLock::new();
    let handle = *HANDLE.get_or_init(|| {
        // SAFETY: dlopen with a static C string; the handle is never closed.
        unsafe {
            libc::dlopen(
                c"/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote".as_ptr(),
                libc::RTLD_LAZY,
            ) as usize
        }
    });
    if handle == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: dlsym on a live handle with a C string.
    unsafe { libc::dlsym(handle as *mut c_void, name.as_ptr()) }
}

/// `MRMediaRemoteSendCommandToApp`: the command reaches the app named by
/// `bundle_id` whichever app macOS has as Now Playing. None when this macOS
/// cannot address an app by name; otherwise whether the app took it.
fn send_media_command_to_app(command: MediaCommand, bundle_id: &str) -> Option<bool> {
    static SEND: OnceLock<Option<(SendCommandToApp, Option<GetLocalOrigin>)>> = OnceLock::new();
    let (send, origin) = (*SEND.get_or_init(|| {
        let send = media_remote_symbol(c"MRMediaRemoteSendCommandToApp");
        let origin = media_remote_symbol(c"MRMediaRemoteGetLocalOrigin");
        if send.is_null() {
            return None;
        }
        // SAFETY: Both symbols have these signatures where they exist.
        let send = unsafe { std::mem::transmute::<*mut c_void, SendCommandToApp>(send) };
        let origin = (!origin.is_null())
            .then(|| unsafe { std::mem::transmute::<*mut c_void, GetLocalOrigin>(origin) });
        Some((send, origin))
    }))?;
    let (done, answer) = std::sync::mpsc::sync_channel::<(u32, Vec<i64>)>(1);
    let completion = RcBlock::new(move |error: u32, statuses: *const c_void| {
        // SAFETY: MediaRemote keeps the array alive for the block's call.
        let statuses = unsafe { handler_statuses(statuses) };
        let _ = done.try_send((error, statuses));
    });
    let name = objc2_core_foundation::CFString::from_str(bundle_id);
    // SAFETY: The origin is the local device's (or null for it), `name` is
    // a live CFString, the queue is a global one, and MediaRemote
    // copies the block before returning.
    unsafe {
        let origin = origin.map_or(std::ptr::null(), |origin| origin());
        send(
            command as u32,
            std::ptr::null(),
            origin,
            (&*name as *const objc2_core_foundation::CFString).cast(),
            0,
            dispatch_get_global_queue(0, 0),
            RcBlock::as_ptr(&completion).cast(),
        );
    }
    let sent = match answer.recv_timeout(SEND_TO_APP_TIMEOUT) {
        Ok((error, statuses)) if handled(error, &statuses) => true,
        Ok((error, statuses)) => {
            tracing::info!(
                bundle_id,
                error,
                ?statuses,
                "the browser did not act on the media command"
            );
            false
        }
        Err(_) => {
            tracing::info!(bundle_id, "the browser did not answer the media command");
            false
        }
    };
    Some(sent)
}

/// `MRMediaRemoteSendCommand` from the private MediaRemote framework: the
/// same pause and play as the media keys, sent to the app macOS has as Now Playing.
fn send_media_command(command: MediaCommand) -> bool {
    static SEND: OnceLock<Option<SendCommand>> = OnceLock::new();
    let send = SEND.get_or_init(|| {
        let symbol = media_remote_symbol(c"MRMediaRemoteSendCommand");
        // SAFETY: The symbol has this signature on every macOS that ships
        // the framework.
        (!symbol.is_null())
            .then(|| unsafe { std::mem::transmute::<*mut c_void, SendCommand>(symbol) })
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
        sent_to: Vec<Vec<&'static str>>,
        browsers: Vec<&'static str>,
        send_ok: bool,
        listened: usize,
        output_closed: bool,
    }

    impl Fake {
        fn new(peaks: impl IntoIterator<Item = Option<f32>>) -> Self {
            Self {
                in_call: false,
                peaks: peaks.into_iter().collect(),
                sent: Vec::new(),
                sent_to: Vec::new(),
                browsers: vec!["net.imput.helium"],
                send_ok: true,
                listened: 0,
                output_closed: false,
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

        fn playing_browsers(&mut self) -> Vec<&'static str> {
            self.browsers.clone()
        }

        fn send(&mut self, command: MediaCommand, browsers: &[&'static str]) -> bool {
            self.sent.push(command);
            self.sent_to.push(browsers.to_vec());
            self.send_ok
        }

        fn output_closed(&mut self) -> bool {
            self.output_closed
        }
    }

    /// The three listen windows of a browser that plays.
    fn playing() -> [Option<f32>; 3] {
        [Some(-20.0); 3]
    }

    #[test]
    #[ignore = "needs a browser playing audio and System Audio Recording"]
    fn native_tap_hears_a_playing_browser() {
        let peak = NativeBrowserAudio::for_pause().peak_dbfs(WINDOW);
        eprintln!("browser peak: {peak:?} dBFS");
        assert!(peak.is_some_and(|peak| peak > SOUND_DBFS));
    }

    #[test]
    #[ignore = "needs a browser playing audio for 15 s and System Audio Recording"]
    fn native_resume_after_a_long_pause_plays_the_browser() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let (paused, _) = pause(&mut NativeBrowserAudio::for_pause()).expect("paused the browser");
        std::thread::sleep(Duration::from_secs(15));
        let mut audio = NativeBrowserAudio::for_resume();
        eprintln!("output closed after 15 s: {}", audio.output_closed());
        resume(&mut audio, paused);
        // One browser tap per process at a time: its aggregate UID is per pid.
        drop(audio);
        std::thread::sleep(Duration::from_millis(500));
        let peak = NativeBrowserAudio::for_pause().peak_dbfs(WINDOW);
        eprintln!("browser peak after resume: {peak:?} dBFS");
        assert!(peak.is_some_and(|peak| peak > SOUND_DBFS));
    }

    #[test]
    fn a_silent_browser_is_left_alone() {
        let mut audio = Fake::new([Some(-120.0)]);
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, []);
    }

    #[test]
    fn a_short_sound_is_not_a_playing_browser() {
        for listen in [
            [Some(-20.0), Some(-120.0), Some(-120.0)],
            [Some(-120.0), Some(-20.0), Some(-20.0)],
            [Some(-20.0), Some(-20.0), None],
        ] {
            let mut audio = Fake::new(listen.into_iter().chain([Some(-200.0); 5]));
            assert!(pause(&mut audio).is_none(), "{listen:?}");
            assert_eq!(audio.sent, [], "{listen:?}");
        }
    }

    #[test]
    fn a_call_is_never_paused() {
        let mut audio = Fake::new(playing());
        audio.in_call = true;
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, []);
        assert_eq!(audio.listened, 0);
    }

    #[test]
    fn a_confirmed_pause_is_resumed() {
        let mut audio = Fake::new(playing().into_iter().chain([
            Some(-30.0),
            Some(-200.0),
            Some(-200.0),
            Some(-200.0),
            Some(-200.0),
            Some(-25.0),
        ]));
        let (paused, _) = pause(&mut audio).expect("the pause is confirmed");
        resume(&mut audio, paused);
        assert_eq!(audio.sent, [MediaCommand::Pause, MediaCommand::Play]);
    }

    #[test]
    fn a_command_by_name_counts_only_when_a_handler_succeeded() {
        assert!(handled(0, &[0]));
        assert!(handled(0, &[2, 0]));
        assert!(!handled(0, &[]));
        assert!(!handled(0, &[1, 2, 3]));
        assert!(!handled(5, &[0]));
    }

    #[test]
    fn play_goes_only_to_the_browsers_hex_paused() {
        // Another app is Now Playing after Hex resumes it (#2825): both
        // commands name the browsers that were playing.
        let mut audio = Fake::new(playing().into_iter().chain([
            Some(-200.0),
            Some(-200.0),
            Some(-200.0),
            Some(-25.0),
        ]));
        audio.browsers = vec!["com.google.Chrome", "net.imput.helium"];
        let (paused, _) = pause(&mut audio).expect("the pause is confirmed");
        audio.browsers = vec!["com.apple.Safari"];
        resume(&mut audio, paused);
        assert_eq!(audio.sent, [MediaCommand::Pause, MediaCommand::Play]);
        assert_eq!(audio.sent_to, [["com.google.Chrome", "net.imput.helium"]; 2]);
    }

    #[test]
    fn a_play_the_browser_never_takes_is_paused_again() {
        let mut audio = Fake::new(std::iter::repeat_n(Some(-200.0), 40));
        resume(&mut audio, PausedBrowser::for_test());
        assert_eq!(audio.sent, [MediaCommand::Play, MediaCommand::Pause]);
    }

    #[test]
    fn a_browser_that_plays_late_in_the_check_keeps_playing() {
        let mut audio = Fake::new(std::iter::repeat_n(Some(-200.0), 19).chain([Some(-25.0)]));
        resume(&mut audio, PausedBrowser::for_test());
        assert_eq!(audio.sent, [MediaCommand::Play]);
    }

    #[test]
    fn a_browser_it_stops_hearing_after_play_gets_nothing_more() {
        let mut audio = Fake::new([Some(-200.0), Some(-200.0)]);
        resume(&mut audio, PausedBrowser::for_test());
        assert_eq!(audio.sent, [MediaCommand::Play]);
    }

    #[test]
    fn two_sounding_tabs_never_get_play() {
        let mut audio = Fake::new(
            playing()
                .into_iter()
                .chain(std::iter::repeat_n(Some(-25.0), 10)),
        );
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }

    #[test]
    fn a_late_silence_is_not_a_confirmed_pause() {
        let mut audio = Fake::new(
            playing()
                .into_iter()
                .chain([Some(-25.0); 4])
                .chain([Some(-200.0); 6]),
        );
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }

    #[test]
    fn one_quiet_window_is_not_a_confirmed_pause() {
        let mut audio = Fake::new(playing().into_iter().chain([
            Some(-200.0),
            Some(-25.0),
            Some(-200.0),
            Some(-25.0),
            Some(-25.0),
            Some(-200.0),
            Some(-200.0),
        ]));
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
        let mut audio = Fake::new(playing().into_iter().chain([Some(-200.0), Some(-200.0)]));
        let (paused, _) = pause(&mut audio).expect("the pause is confirmed");
        audio.peaks = VecDeque::from([Some(-20.0)]);
        resume(&mut audio, paused);
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }

    #[test]
    fn a_browser_it_cannot_hear_gets_no_play() {
        let mut audio = Fake::new([None]);
        resume(&mut audio, PausedBrowser::for_test());
        assert_eq!(audio.sent, []);
    }

    #[test]
    fn a_browser_heard_before_play_that_goes_quiet_is_left_alone() {
        let mut audio = Fake::new([Some(-200.0), None]);
        audio.output_closed = true;
        resume(&mut audio, PausedBrowser::for_test());
        assert_eq!(audio.sent, [MediaCommand::Play]);
        assert_eq!(audio.listened, 2);
    }

    #[test]
    fn a_browser_that_closed_its_output_gets_play() {
        // A pause longer than about 10 s: the browser closed its output, so
        // the tap delivers nothing until play opens it again.
        let mut audio = Fake::new([None, None, None, None, Some(-25.0)]);
        audio.output_closed = true;
        resume(&mut audio, PausedBrowser::for_test());
        assert_eq!(audio.sent, [MediaCommand::Play]);
    }

    #[test]
    fn a_closed_browser_that_never_opens_after_play_is_paused_again() {
        let mut audio = Fake::new(std::iter::repeat_n(None, 40));
        audio.output_closed = true;
        resume(&mut audio, PausedBrowser::for_test());
        assert_eq!(audio.sent, [MediaCommand::Play, MediaCommand::Pause]);
        // A closed tab sends the play to another app: stop it after 1 s, not 2.
        assert_eq!(audio.listened, 1 + 10);
    }

    #[test]
    fn a_refused_command_is_not_confirmed() {
        let mut audio = Fake::new(playing().into_iter().chain([Some(-200.0); 5]));
        audio.send_ok = false;
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }
}
