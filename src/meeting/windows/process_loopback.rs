//! WASAPI process loopback: one app's playback for a meeting's system track
//! (Labs › Choose meeting audio).
//!
//! ActivateAudioInterfaceAsync on VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK opens
//! a loopback of a process tree's audio, on whatever outputs it plays,
//! without hearing anything else. Windows 10 version 2004 (build 19041) added
//! it; before that the activation fails, which is logged once and recorded as
//! silence rather than falling back to all computer audio.
//!
//! One capture thread per process-tree root owns its COM objects from
//! CoInitializeEx through drop, so a closed app releases them where they were
//! made, and fills a bounded channel with its audio. The source mixes the
//! roots' audio and re-lists the app's processes on the same 2 s cadence as
//! the rest of the Windows capture, reopening when that set changes. An app
//! that is not running records silence, and is picked up when it starts. A
//! root whose stream fails or ends is retried on its own with the same
//! bounded backoff while the other roots keep recording; only an open where
//! no root could start fails, so the supervisor retries the whole source.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use windows::Win32::Foundation::{CloseHandle, E_FAIL, HANDLE, WAIT_FAILED};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVE_FORMAT_PCM, WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, IAgileObject, IAgileObject_Impl,
    StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0},
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{HRESULT, IUnknown, Interface, PCWSTR, Ref, implement};

use super::super::SAMPLE_RATE;
use super::super::audio_source::{SourceChoice, effective};
use super::super::capture::{LiveSource, MIX_HOLD, POLL_INTERVAL, PacketSink, Restart, To16k, mix};

/// Audio buffered for the source, however many roots the app has.
const QUEUE_CAPACITY: usize = 512;
/// How often the source checks that the app's processes have not changed.
const ROUTE_CHECK: Duration = Duration::from_secs(2);
/// How long one ActivateAudioInterfaceAsync may take to complete.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(3);
/// How long opening one root, activation included, may take.
const OPEN_TIMEOUT: Duration = Duration::from_secs(6);
/// The stream's buffer, and shared mode's periodicity.
const BUFFER_HNS: i64 = 200_000;
const PERIODICITY_HNS: i64 = 0;
/// How long the capture thread waits for a packet before checking its stop.
const EVENT_WAIT_MS: u32 = 100;
const CHANNELS: u16 = 2;
const BITS_PER_SAMPLE: u16 = 16;
/// The rates requested, in order: process loopback has no mix format to read,
/// so the stream is opened with a plain 16-bit stereo format the engine
/// converts into, and 44.1 kHz stands in for a device that refuses 48 kHz.
const RATES: [u32; 2] = [48_000, 44_100];

/// The system track's audio from one app, through WASAPI process loopback.
pub(super) struct AppSource {
    /// The app's executable file name, lowercased such as ms-teams.exe.
    id: String,
    /// The choice this source opened with, so a different one reopens it:
    /// a change applies mid-recording.
    choice: SourceChoice,
    /// The process-tree roots this source opened, as listed.
    roots: Vec<u32>,
    /// One entry per root, in listing order, so a root's audio keeps its
    /// place in the mix across restarts.
    captures: Vec<Root>,
    /// Kept while no capture runs, so the channel never disconnects.
    sender: SyncSender<(usize, Vec<f32>)>,
    buffers: Receiver<(usize, Vec<f32>)>,
    /// Kept so a restarted root's thread can report its failure.
    error_sender: SyncSender<(usize, String)>,
    errors: Receiver<(usize, String)>,
    /// Restarts in flight, one per root being reopened: each opens on its own
    /// thread so a slow activation never holds up the roots still recording.
    pending: Vec<PendingRestart>,
    route_checked: Instant,
}

/// One root's restart in flight: the thread opening it and the slot it will
/// fill. Its result is adopted on a later poll.
struct PendingRestart {
    index: usize,
    started: Instant,
    result: Receiver<Result<Capture>>,
}

impl PendingRestart {
    /// Whether the supervisor stops waiting for this restart: a start that
    /// outlives opening a root is treated as failed, so the root retries at
    /// its next due time. Nothing leaks: the retry thread ends within its own
    /// [`OPEN_TIMEOUT`], and a stream it returns finds its receiver gone and is
    /// dropped, which ends its worker.
    fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) > OPEN_TIMEOUT * 2
    }
}

impl AppSource {
    /// Opens one process loopback per root of the chosen app, or silence when
    /// it is not running. A root that would not open keeps its place with a
    /// due time, so it alone is retried with bounded backoff; only an open
    /// where no root could start fails, so the supervisor retries the whole
    /// source; the choice never widens to every output either way.
    pub(super) fn start(choice: SourceChoice) -> Result<Self> {
        let Some(id) = choice.app_id().map(str::to_owned) else {
            return Err(eyre!("the meeting audio choice is not an app"));
        };
        let roots = root_processes(&id).unwrap_or_default();
        let (sender, buffers) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (error_sender, errors) = mpsc::sync_channel(roots.len().max(1));
        let mut captures = Vec::with_capacity(roots.len());
        let mut first_error = None;
        let now = Instant::now();
        for &pid in &roots {
            let index = captures.len();
            let (stream, retry) =
                match Capture::start(pid, index, sender.clone(), error_sender.clone()) {
                    Ok(capture) => (Some(capture), Restart::running(now)),
                    Err(error) => {
                        tracing::debug!(
                            pid,
                            error = %format!("{error:#}"),
                            "could not hear one process of the chosen app; trying again"
                        );
                        first_error.get_or_insert(error);
                        let mut retry = Restart::running(now);
                        retry.exited(now);
                        (None, retry)
                    }
                };
            captures.push(Root { pid, stream, retry });
        }
        if roots.is_empty() {
            tracing::info!(
                app = id,
                "the chosen app isn't running; recording silence until it starts"
            );
        } else if captures.iter().all(|root| root.stream.is_none()) {
            // Windows before 10 2004 refuses the activation, and any other
            // failure fails this open: the supervisor retries it on its
            // bounded backoff, and the choice never widens to every output.
            let error = first_error
                .map(|error| format!("{error:#}"))
                .unwrap_or_else(|| "no process could be captured".into());
            // The supervisor logs this once per distinct failure, so a
            // retry that fails the same way again stays out of the log.
            tracing::debug!(app = id, %error, "could not capture the chosen app's audio");
            return Err(eyre!("could not capture {id}: {error}"));
        } else {
            tracing::info!(
                app = id,
                pids = ?captures
                    .iter()
                    .filter(|root| root.stream.is_some())
                    .map(|root| root.pid)
                    .collect::<Vec<_>>(),
                "the chosen app's audio loopback started"
            );
        }
        Ok(Self {
            id,
            choice,
            roots,
            captures,
            sender,
            buffers,
            error_sender,
            errors,
            pending: Vec::new(),
            route_checked: Instant::now(),
        })
    }

    /// Why this source should reopen on a route check: the app's process-tree
    /// roots changed. A listing that cannot be read keeps them.
    fn route_change(&self) -> Option<&'static str> {
        let roots = root_processes(&self.id)?;
        (roots != self.roots).then_some("the app's processes changed")
    }

    fn take(&mut self, (index, samples): (usize, Vec<f32>)) {
        if let Some(root) = self.captures.get_mut(index)
            && let Some(stream) = root.stream.as_mut()
        {
            let resampled = stream.resample.push(&samples);
            stream.pending.extend(resampled);
        }
    }

    /// Starts again every root whose stream failed or ended and whose own
    /// backoff is due, so one root that stopped is retried while the others
    /// keep recording.
    /// Ends the stream a worker error named and schedules the root's retry.
    fn stop_root(&mut self, index: usize, error: String) {
        let Some(root) = self.captures.get_mut(index) else {
            return;
        };
        let Some(mut stream) = root.stream.take() else {
            return;
        };
        tracing::warn!(
            pid = root.pid,
            %error,
            "the app's audio loopback stopped; trying that process again"
        );
        stream.end();
        root.retry.exited(Instant::now());
    }

    /// Starts each due root's retry on its own thread and adopts the ones
    /// that finished, so a slow activation never stops the roots that are
    /// still recording.
    fn restart_stopped(&mut self, deferred: &mut Vec<(usize, String)>) {
        let now = Instant::now();
        let mut pending = std::mem::take(&mut self.pending);
        pending.retain(|restart| match restart.result.try_recv() {
            Ok(Ok(stream)) => {
                let failure = adoption_failure(deferred, restart.index, stream.finished());
                if let Some(error) = failure {
                    let root = &mut self.captures[restart.index];
                    root.retry.exited(now);
                    tracing::debug!(
                        pid = root.pid,
                        error = %error,
                        "a restarted app audio loopback failed at once; trying again"
                    );
                    return false;
                }
                let root = &mut self.captures[restart.index];
                root.stream = Some(stream);
                root.retry.started(now);
                tracing::info!(pid = root.pid, "the app's audio loopback restarted");
                false
            }
            Ok(Err(error)) => {
                let root = &mut self.captures[restart.index];
                root.retry.exited(now);
                tracing::debug!(
                    pid = root.pid,
                    error = %format!("{error:#}"),
                    "could not restart one process's app audio loopback; trying again"
                );
                false
            }
            // A start still inside its own timeout keeps waiting; one that
            // outlived it is abandoned and the root keeps its next due time.
            Err(mpsc::TryRecvError::Empty) if !restart.expired(now) => true,
            Err(_) => {
                self.captures[restart.index].retry.exited(now);
                false
            }
        });
        for index in retry_due(&self.captures, now) {
            if pending.iter().any(|restart| restart.index == index) {
                continue;
            }
            let pid = self.captures[index].pid;
            let (finished, result) = mpsc::channel();
            let sender = self.sender.clone();
            let errors = self.error_sender.clone();
            let spawned = thread::Builder::new()
                .name("meeting-app-loopback-retry".into())
                .spawn(move || {
                    let _ = finished.send(Capture::start(pid, index, sender, errors));
                });
            if spawned.is_err() {
                self.captures[index].retry.exited(now);
                continue;
            }
            pending.push(PendingRestart {
                index,
                started: now,
                result,
            });
        }
        self.pending = pending;
        // An error held for a restart that was abandoned names a stream that
        // was never adopted; one the root still has ends it here.
        for (index, error) in std::mem::take(deferred) {
            self.stop_root(index, error);
        }
    }
}

/// Why a restarted stream must not be adopted: an error its worker reported
/// while the restart was in flight, or a worker that already finished. Both
/// would leave a dead stream that never retries.
fn adoption_failure(
    deferred: &[(usize, String)],
    index: usize,
    worker_finished: bool,
) -> Option<String> {
    deferred
        .iter()
        .find(|(deferred_index, _)| *deferred_index == index)
        .map(|(_, error)| error.clone())
        .or_else(|| worker_finished.then(|| "the loopback ended right after it started".to_owned()))
}

/// One process-tree root: its running stream while one runs, and the backoff
/// before the next attempt after it failed or ended.
struct Root {
    pid: u32,
    stream: Option<Capture>,
    retry: Restart,
}

/// The roots to try again: a stream that failed or ended, and whose own
/// backoff is due.
fn retry_due(captures: &[Root], now: Instant) -> Vec<usize> {
    captures
        .iter()
        .enumerate()
        .filter(|(_, root)| root.stream.is_none() && root.retry.is_due(now))
        .map(|(index, _)| index)
        .collect()
}

impl LiveSource for AppSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        // A failing root stops alone and starts again after its own backoff;
        // the others keep the track going. Only when no root is left does
        // the source reopen, so the supervisor retries the whole app.
        //
        // An error for a root whose restart is still in flight names the
        // stream that restart is opening, so it is held until the restart is
        // adopted: applying it now would find no stream and be lost.
        let mut deferred: Vec<(usize, String)> = Vec::new();
        while let Ok((index, error)) = self.errors.try_recv() {
            if self.pending.iter().any(|restart| restart.index == index) {
                deferred.push((index, error));
            } else {
                self.stop_root(index, error);
            }
        }
        self.restart_stopped(&mut deferred);
        // A restart still in flight says nothing yet: only a root left with
        // no stream and no retry means every loopback really stopped.
        if !self.roots.is_empty()
            && self.pending.is_empty()
            && self.captures.iter().all(|root| root.stream.is_none())
        {
            return Err("every app audio loopback stopped".into());
        }
        if self.route_checked.elapsed() >= ROUTE_CHECK {
            self.route_checked = Instant::now();
            if effective() != self.choice {
                return Err("the meeting audio choice changed".into());
            }
            if let Some(reason) = self.route_change() {
                return Err(reason.into());
            }
        }
        match self.buffers.recv_timeout(POLL_INTERVAL) {
            Ok(first) => {
                self.take(first);
                while let Ok(next) = self.buffers.try_recv() {
                    self.take(next);
                }
                let mut pending = self
                    .captures
                    .iter_mut()
                    .filter_map(|root| root.stream.as_mut())
                    .map(|stream| &mut stream.pending)
                    .collect::<Vec<_>>();
                sink.send(SAMPLE_RATE, mix(&mut pending, MIX_HOLD), None);
                Ok(())
            }
            // The app is not running, or plays nothing: silence, padded by
            // the track writer like any other silent output.
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("the app's audio loopback disconnected".into())
            }
        }
    }

    fn describe(&self) -> &'static str {
        "wasapi-process-loopback"
    }
}

/// One root's running process loopback stream and the audio it has not been
/// mixed.
struct Capture {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    resample: To16k,
    pending: Vec<f32>,
}

impl Capture {
    /// Whether its worker has already ended, so the stream cannot deliver: an
    /// error it reported while its restart was still in flight may not have
    /// been seen yet, and a stream adopted dead would never be retried.
    fn finished(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
    }
}

impl Capture {
    /// Opens the pid's process loopback on its own thread, returning it once
    /// the stream runs.
    fn start(
        pid: u32,
        index: usize,
        sender: SyncSender<(usize, Vec<f32>)>,
        errors: SyncSender<(usize, String)>,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("meeting-app-loopback".into())
            .spawn(move || capture(pid, index, sender, errors, &thread_stop, ready))
            .wrap_err("could not start the app loopback thread")?;
        let rate = match started.recv_timeout(OPEN_TIMEOUT) {
            Ok(Ok(rate)) => rate,
            Ok(Err(error)) => {
                stop.store(true, Ordering::Relaxed);
                return Err(eyre!(error));
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                return Err(eyre!("the app's loopback stream did not start in time"));
            }
        };
        let resample = match To16k::new(rate) {
            Ok(resample) => resample,
            Err(error) => {
                stop.store(true, Ordering::Relaxed);
                return Err(error);
            }
        };
        Ok(Self {
            stop,
            worker: Some(worker),
            resample,
            pending: Vec::new(),
        })
    }

    /// Ends this capture, joining its thread so nothing outlives the source.
    fn end(&mut self) {
        self.pending.clear();
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::error!("app audio loopback thread panicked");
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.end();
    }
}

/// Runs one root's process loopback: activation, initialization, and its
/// packets, all in this thread's COM apartment.
fn capture(
    pid: u32,
    index: usize,
    sender: SyncSender<(usize, Vec<f32>)>,
    errors: SyncSender<(usize, String)>,
    stop: &AtomicBool,
    ready: SyncSender<std::result::Result<u32, String>>,
) {
    // SAFETY: Joins this thread to the multithreaded apartment. A thread
    // already in it gets S_FALSE, which still needs the CoUninitialize below.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    run_capture(pid, index, &sender, &errors, stop, &ready);
    // Every COM object this thread made is dropped by now.
    if initialized {
        // SAFETY: Balances the successful CoInitializeEx above on this thread,
        // and this thread keeps running nothing else.
        unsafe { CoUninitialize() };
    }
}

/// Records one root until it stops or fails, dropping its COM objects before
/// it returns.
fn run_capture(
    pid: u32,
    index: usize,
    sender: &SyncSender<(usize, Vec<f32>)>,
    errors: &SyncSender<(usize, String)>,
    stop: &AtomicBool,
    ready: &SyncSender<std::result::Result<u32, String>>,
) {
    let opened = match open(pid) {
        Ok(opened) => opened,
        Err(error) => {
            let _ = ready.try_send(Err(error));
            return;
        }
    };
    if ready.send(Ok(opened.format.nSamplesPerSec)).is_err() {
        return;
    }
    if let Err(error) = pump(
        &opened.capture,
        &opened.format,
        opened.event.0,
        index,
        sender,
        stop,
    ) {
        let _ = errors.try_send((index, error));
    }
    // SAFETY: Stops this thread's own initialized stream before it drops.
    let _ = unsafe { opened.client.Stop() };
}

/// A root's open stream: its client, its capture service, the event its
/// packets are waited on, and the format they arrive in.
struct Opened {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: OwnedEvent,
    format: WAVEFORMATEX,
}

/// Opens process loopback of the pid and starts it, trying each requested
/// rate.
fn open(pid: u32) -> std::result::Result<Opened, String> {
    let mut last = "the process could not be captured".to_owned();
    for rate in RATES {
        let format = pcm_format(rate);
        let client = match activate(pid) {
            Ok(client) => client,
            Err(error) => {
                last = error;
                continue;
            }
        };
        // SAFETY: The format is a valid WAVEFORMATEX for the call, and the
        // client is this thread's own, uninitialized one.
        let initialized = unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    // The app may play a rate or channel count the requested
                    // format does not name: the engine converts it instead of
                    // refusing the stream.
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                BUFFER_HNS,
                PERIODICITY_HNS,
                &format,
                None,
            )
        };
        if let Err(error) = initialized {
            last = hresult("Initialize", &error);
            continue;
        }
        // SAFETY: A fresh event owned by OwnedEvent, closed on drop.
        let event = match unsafe { CreateEventW(None, false, false, PCWSTR::null()) } {
            Ok(event) => OwnedEvent(event),
            Err(error) => {
                last = hresult("CreateEvent", &error);
                continue;
            }
        };
        // SAFETY: The event outlives the stream, and no second handle is set.
        if let Err(error) = unsafe { client.SetEventHandle(event.0) } {
            last = hresult("SetEventHandle", &error);
            continue;
        }
        // SAFETY: The client was initialized, so it serves a capture client.
        let capture = match unsafe { client.GetService::<IAudioCaptureClient>() } {
            Ok(capture) => capture,
            Err(error) => {
                last = hresult("GetService", &error);
                continue;
            }
        };
        // SAFETY: The stream was initialized, so it can start.
        if let Err(error) = unsafe { client.Start() } {
            last = hresult("Start", &error);
            continue;
        }
        return Ok(Opened {
            client,
            capture,
            event,
            format,
        });
    }
    Err(last)
}

/// Delivers the stream's packets to the sender until stop or a failure.
fn pump(
    capture: &IAudioCaptureClient,
    format: &WAVEFORMATEX,
    event: HANDLE,
    index: usize,
    sender: &SyncSender<(usize, Vec<f32>)>,
    stop: &AtomicBool,
) -> std::result::Result<(), String> {
    let block = usize::from(format.nBlockAlign);
    let channels = usize::from(format.nChannels);
    while !stop.load(Ordering::Relaxed) {
        // SAFETY: Waits on this stream's own event, with a bounded timeout so
        // a stop is noticed without one.
        if unsafe { WaitForSingleObject(event, EVENT_WAIT_MS) } == WAIT_FAILED {
            return Err("the app's audio loopback event failed".into());
        }
        loop {
            // SAFETY: The client belongs to this thread; the sizes it returns
            // are used as given.
            let packet = unsafe { capture.GetNextPacketSize() }
                .map_err(|error| hresult("GetNextPacketSize", &error))?;
            if packet == 0 {
                break;
            }
            let mut data = std::ptr::null_mut();
            let mut frames = 0_u32;
            let mut flags = 0_u32;
            unsafe { capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) }
                .map_err(|error| hresult("GetBuffer", &error))?;
            let frames_count = frames as usize;
            let samples = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                vec![0.0; frames_count]
            } else {
                // SAFETY: GetBuffer returned that many frames of block bytes.
                let bytes = unsafe { std::slice::from_raw_parts(data, frames_count * block) };
                mono_i16(bytes, channels)
            };
            unsafe { capture.ReleaseBuffer(frames) }
                .map_err(|error| hresult("ReleaseBuffer", &error))?;
            if samples.is_empty() {
                continue;
            }
            // A full queue drops this root's buffer rather than growing
            // without bound; the mix keeps the others.
            let _ = sender.try_send((index, samples));
        }
    }
    Ok(())
}

/// The WAVEFORMATEX process loopback is opened with: a plain 16-bit stereo
/// PCM format, which the audio engine converts the app's audio into.
fn pcm_format(rate: u32) -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: CHANNELS,
        nSamplesPerSec: rate,
        nAvgBytesPerSec: rate * u32::from(CHANNELS) * u32::from(BITS_PER_SAMPLE / 8),
        nBlockAlign: CHANNELS * (BITS_PER_SAMPLE / 8),
        wBitsPerSample: BITS_PER_SAMPLE,
        cbSize: 0,
    }
}

/// Interleaved 16-bit little-endian frames, averaged to the mono f32 the
/// meeting tracks carry. A trailing partial frame is dropped.
fn mono_i16(bytes: &[u8], channels: usize) -> Vec<f32> {
    let channels = channels.max(1);
    let (samples, _) = bytes.as_chunks::<2>();
    samples
        .chunks_exact(channels)
        .map(|frame| {
            frame
                .iter()
                .map(|sample| f32::from(i16::from_le_bytes(*sample)) / f32::from(i16::MAX))
                .sum::<f32>()
                / frame.len() as f32
        })
        .collect()
}

/// Activates a process loopback IAudioClient for the pid and waits for it.
fn activate(pid: u32) -> std::result::Result<IAudioClient, String> {
    let (sender, done) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler = ActivationDone(sender).into();
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    let propvariant = activation_propvariant(&mut params);
    // SAFETY: The parameters outlive the call and the API copies the value
    // before returning; the pointer is never used after it.
    let activation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*propvariant),
            &handler,
        )
    };
    activation.map_err(|error| hresult("ActivateAudioInterfaceAsync", &error))?;
    // The parameters stay in this frame until activation completes or times
    // out, though the API has already copied them.
    done.recv_timeout(ACTIVATION_TIMEOUT)
        .map_err(|_| "the app's loopback activation did not complete in time".to_owned())?
}

/// The VT_BLOB PROPVARIANT naming the activation parameters, which
/// ActivateAudioInterfaceAsync copies before returning.
///
/// It is never dropped: PROPVARIANT's Drop runs PropVariantClear, which
/// frees a VT_BLOB's data with CoTaskMemFree, and this blob points at the
/// caller's stack. Freeing it corrupted the heap and crashed Hex every time a
/// meeting recorded one app (#2815). The value owns nothing else to release.
fn activation_propvariant(
    params: &mut AUDIOCLIENT_ACTIVATION_PARAMS,
) -> std::mem::ManuallyDrop<PROPVARIANT> {
    let blob = BLOB {
        cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
        pBlobData: std::ptr::from_mut(params).cast::<u8>(),
    };
    std::mem::ManuallyDrop::new(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 { blob },
            }),
        },
    })
}

/// Process loopback's activation completion, which the API calls from its
/// own thread; it is agile because it owns nothing but a channel.
#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct ActivationDone(mpsc::Sender<std::result::Result<IAudioClient, String>>);

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationDone_Impl {
    fn ActivateCompleted(
        &self,
        operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let result = operation
            .ok()
            .and_then(|operation| {
                let mut status = HRESULT::default();
                let mut interface: Option<IUnknown> = None;
                // SAFETY: The operation fills the status and the interface it
                // activated; both are this call's own locals.
                unsafe { operation.GetActivateResult(&mut status, &mut interface) }?;
                status.ok()?;
                interface
                    .ok_or_else(|| windows::core::Error::from(E_FAIL))?
                    .cast::<IAudioClient>()
            })
            .map_err(|error| hresult("GetActivateResult", &error));
        let _ = self.0.send(result);
        Ok(())
    }
}

impl IAgileObject_Impl for ActivationDone_Impl {}

/// The capture event SetEventHandle signals, closed on drop.
struct OwnedEvent(HANDLE);

impl Drop for OwnedEvent {
    fn drop(&mut self) {
        // SAFETY: The handle is this struct's own, and closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The process-tree roots of the app now: its processes whose parent is not
/// one of its own, so a helper and its children are captured once, and never
/// Hex's own process.
fn root_processes(id: &str) -> Option<Vec<u32>> {
    // Roots need each process's parent, never its start time: attribution
    // does, and only the pane's app listing asks for it.
    let processes = crate::microphone_activity::running_processes(false)?;
    Some(root_pids(id, &processes, std::process::id()))
}

/// The tree roots among `processes` whose executable is the id and whose
/// parent's executable is not, sorted: one loopback of a root already covers
/// its children. Hex's own pid is never a root, whatever it is named.
fn root_pids(
    id: &str,
    processes: &[crate::microphone_activity::RunningProcess],
    own_pid: u32,
) -> Vec<u32> {
    let is_id = |pid: u32| {
        processes
            .iter()
            .any(|process| process.pid == pid && process.name.eq_ignore_ascii_case(id))
    };
    let mut roots = processes
        .iter()
        .filter(|process| {
            process.name.eq_ignore_ascii_case(id)
                && process.pid != 0
                && process.parent != process.pid
                && process.pid != own_pid
                && !is_id(process.parent)
        })
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    roots.sort_unstable();
    roots.dedup();
    roots
}

fn hresult(step: &str, error: &windows::core::Error) -> String {
    format!(
        "{step} failed: {} ({:#010x})",
        error.message(),
        error.code().0 as u32
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::capture::{RETRY_INITIAL, RETRY_MAX};

    fn process(pid: u32, parent: u32, name: &str) -> crate::microphone_activity::RunningProcess {
        crate::microphone_activity::RunningProcess {
            pid,
            parent,
            name: name.into(),
            started: Some(1),
        }
    }

    /// A root whose stream failed or ended is retried after its own backoff
    /// while another root keeps recording; it never waits for every root or
    /// for the app's process list to change.
    #[test]
    fn each_failed_root_retries_on_its_own_backoff() {
        let now = Instant::now();
        let mut failed = Restart::running(now);
        failed.exited(now);
        let running = Capture {
            stop: Arc::new(AtomicBool::new(false)),
            worker: Some(thread::spawn(|| {})),
            resample: To16k::new(48_000).expect("a 48 kHz resampler"),
            pending: Vec::new(),
        };
        let mut captures = [
            Root {
                pid: 200,
                stream: None,
                retry: failed,
            },
            Root {
                pid: 201,
                stream: Some(running),
                retry: Restart::running(now),
            },
        ];
        assert!(retry_due(&captures, now).is_empty(), "not due yet");
        assert_eq!(
            retry_due(&captures, now + RETRY_INITIAL),
            [0],
            "only the failed root is due, and the running root is not"
        );
        assert_eq!(
            retry_due(&captures, now + RETRY_MAX),
            [0],
            "the failed root waits for nothing but its own backoff; the running root is never due"
        );
        // The restart claims the root it starts, so the backoff it already
        // spent is not due a second time.
        captures[0].retry.started(now + RETRY_INITIAL);
        assert!(
            retry_due(&captures, now + RETRY_MAX).is_empty(),
            "a retry that ran is not due again"
        );
    }

    /// A restart that outlives opening a root is abandoned, so one stuck
    /// activation cannot keep a root pending forever.
    #[test]
    fn a_restart_that_outlives_its_open_is_abandoned() {
        let now = Instant::now();
        let (_finished, result) = mpsc::channel::<Result<Capture>>();
        let restart = PendingRestart {
            index: 0,
            started: now,
            result,
        };
        assert!(!restart.expired(now));
        assert!(!restart.expired(now + OPEN_TIMEOUT));
        assert!(restart.expired(now + OPEN_TIMEOUT * 2 + Duration::from_millis(1)));
    }

    /// A restarted stream whose worker reported an error before it was
    /// adopted, or whose worker already ended, is never adopted: it would
    /// leave a dead stream that never retries.
    #[test]
    fn a_restarted_stream_that_already_failed_is_not_adopted() {
        let deferred = [(3, "the loopback stopped".to_owned())];
        assert_eq!(
            adoption_failure(&deferred, 3, false).as_deref(),
            Some("the loopback stopped")
        );
        assert!(
            adoption_failure(&deferred, 4, true).is_some(),
            "a worker that already finished"
        );
        assert!(adoption_failure(&deferred, 4, false).is_none());
    }

    #[test]
    fn the_roots_of_an_app_are_its_processes_whose_parent_is_not_its_own() {
        let processes = [
            process(100, 4, "explorer.exe"),
            process(200, 4, "ms-teams.exe"),
            process(201, 200, "ms-teams.exe"),
            process(202, 201, "ms-teams.exe"),
            process(300, 4, "ms-teams.exe"),
            process(400, 200, "msedgewebview2.exe"),
        ];
        assert_eq!(root_pids("ms-teams.exe", &processes, 9_000), vec![200, 300]);
    }

    #[test]
    fn the_executable_matches_regardless_of_case_and_only_exactly() {
        let processes = [
            process(10, 4, "MS-Teams.EXE"),
            process(11, 10, "ms-teams.exe"),
            process(12, 4, "ms-teams-helper.exe"),
            process(13, 4, "teams.exe"),
        ];
        assert_eq!(root_pids("ms-teams.exe", &processes, 9_000), vec![10]);
        assert_eq!(root_pids("slack.exe", &processes, 9_000), Vec::<u32>::new());
    }

    #[test]
    fn an_orphan_whose_parent_has_exited_is_a_root() {
        let processes = [process(50, 999, "zoom.exe"), process(51, 50, "zoom.exe")];
        assert_eq!(root_pids("zoom.exe", &processes, 9_000), vec![50]);
    }

    /// Hex's own pid is never a capture root, whatever it is named, so a
    /// chosen app's tree can never include Hex.
    #[test]
    fn hex_is_never_a_part_of_a_capture_target() {
        let processes = [
            process(60, 4, "chrome.exe"),
            process(61, 60, "chrome.exe"),
            process(70, 60, "hex.exe"),
        ];
        assert_eq!(
            root_pids("chrome.exe", &processes, 60),
            Vec::<u32>::new(),
            "the only root is Hex's own pid"
        );
        assert_eq!(root_pids("hex.exe", &processes, 70), Vec::<u32>::new());
        assert_eq!(root_pids("hex.exe", &processes, 9_000), vec![70]);
    }

    #[test]
    fn process_loopback_asks_for_16_bit_stereo_at_the_requested_rate() {
        // WAVEFORMATEX is packed, so each field is copied out before it is
        // compared; a reference to a packed field is unaligned.
        let format = pcm_format(48_000);
        let tag = format.wFormatTag;
        let channels = format.nChannels;
        let rate = format.nSamplesPerSec;
        let bits = format.wBitsPerSample;
        let block = format.nBlockAlign;
        let bytes = format.nAvgBytesPerSec;
        let extra = format.cbSize;
        assert_eq!(tag, WAVE_FORMAT_PCM as u16);
        assert_eq!(channels, 2);
        assert_eq!(rate, 48_000);
        assert_eq!(bits, 16);
        assert_eq!(block, 4);
        assert_eq!(bytes, 48_000 * 4);
        assert_eq!(extra, 0);
    }

    #[test]
    fn interleaved_16_bit_frames_become_mono_float() {
        // Two stereo frames: (0, i16::MAX) and (-i16::MAX, 0).
        let bytes = [0x00, 0x00, 0xFF, 0x7F, 0x01, 0x80, 0x00, 0x00];
        assert_eq!(mono_i16(&bytes, 2), vec![0.5, -0.5]);
        // Mono audio passes through, and a trailing partial frame is dropped.
        let mono = [0xFF, 0x7F, 0x00, 0x00, 0x01];
        assert_eq!(mono_i16(&mono, 1), vec![1.0, 0.0]);
    }
}
