//! Annotate 15 s rolling screen replay (`Labs › Annotate replay`, macOS 27+).
//!
//! While `labs.annotate_replay` and `capture.enabled` are both on (and
//! `SCClipBufferingOutput` exists on the running macOS), a single background
//! `capture-replay` worker keeps one `SCStream` + `SCClipBufferingOutput`
//! open on the focused/main display with Hex's own overlays excluded. While
//! the Lab is off, no worker thread, `SCStream`, or display listener exists.
//!
//! `hex capture replay` exports the last 15 s into `<bundle>/recording.mp4`
//! and feeds it through the normal Annotate bundle pipeline (`capture.json`,
//! `scrawl.json`, keyframes, `contact-sheet.png`, clipboard copy, and router).
//! Holding `capture.replay_hotkey` freezes the 15 s clip at the moment of the
//! press and records voice narration until release.

#[cfg(target_os = "macos")]
mod sck;

#[cfg(any(target_os = "macos", test))]
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
#[cfg(target_os = "macos")]
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
#[cfg(target_os = "macos")]
use std::thread::{self, JoinHandle};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(any(target_os = "macos", test))]
use anyhow::Context as _;
#[cfg(target_os = "macos")]
use anyhow::bail;
#[cfg(any(target_os = "macos", test))]
use scrawl_capture::audio::{GapSpan, RecordedAudio};
#[cfg(target_os = "macos")]
use scrawl_capture::capture::{Movie, RouteContext, ScreenRecording};
#[cfg(target_os = "macos")]
use scrawl_capture::helper::{self, FrameTimes};
#[cfg(target_os = "macos")]
use scrawl_capture::model::{CaptureMode, TargetFrame, TargetInfo};
#[cfg(any(target_os = "macos", test))]
use serde::Serialize;

use super::{AudioTimeline, FinishJob, SharedOutcomes};
#[cfg(target_os = "macos")]
use super::{
    CaptureAudio, ChosenTranscriber, End, Sealed, Stage, TargetKind, Written, hex_engine_config,
    lock, new_bundle_dir, remove_bundle, routing,
};
use crate::audio::CaptureInstant;
use crate::control::ControlReply;

/// Maximum duration `SCClipBufferingOutput` supports (`SCClipBufferingOutput.h`).
#[cfg(target_os = "macos")]
const DEFAULT_REPLAY_SECS: f64 = 15.0;

/// Minimum hold on `capture.replay_hotkey` to keep spoken narration; shorter
/// taps export a silent replay clip without a key-click WAV.
#[cfg(target_os = "macos")]
const MIN_NARRATION_HOLD: Duration = Duration::from_millis(300);

#[cfg(target_os = "macos")]
const EXPORT_WAIT: Duration = Duration::from_secs(8);
#[cfg(target_os = "macos")]
const CANDIDATE_SAMPLE_MS: u64 = 220;
#[cfg(target_os = "macos")]
const CANDIDATE_MAX_EDGE: u32 = 640;

/// Whether the running OS supports `SCClipBufferingOutput` (macOS 27+).
pub fn available() -> bool {
    #[cfg(target_os = "macos")]
    {
        sck::available()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Host-side replay state owned by [`super::CaptureHost`]. Default state
/// allocates nothing and spawns no threads while `labs.annotate_replay` is off.
#[derive(Default)]
pub(super) struct Controller {
    #[cfg(target_os = "macos")]
    worker: Option<WorkerHandle>,
    #[cfg(target_os = "macos")]
    narrating: Option<NarratingReplay>,
    #[cfg(target_os = "macos")]
    last_exclusions: Option<([Option<u32>; 3], bool)>,
}

#[cfg(target_os = "macos")]
struct WorkerHandle {
    commands: SyncSender<WorkerCmd>,
    thread: JoinHandle<()>,
}

#[cfg(target_os = "macos")]
enum WorkerCmd {
    UpdateExclusions {
        window_ids: Vec<u32>,
        exclude_own_chrome: bool,
    },
    Export {
        output_mp4: PathBuf,
        duration_secs: f64,
        reply: SyncSender<Result<ExportTicket, String>>,
    },
    Stop,
}

#[cfg(target_os = "macos")]
struct ExportTicket {
    done: Receiver<Result<(), String>>,
    display: sck::BufferDisplay,
    buffered_for: Duration,
}

#[cfg(target_os = "macos")]
struct NarratingReplay {
    bundle: PathBuf,
    press_at: CaptureInstant,
    anchor_ns: u64,
    started_at_iso: String,
    chunks: Receiver<CaptureAudio>,
    ticket: ExportTicket,
    route_context: RouteContext,
    frontmost: Option<(i32, String, Option<String>)>,
    prepared_look: Option<routing::Look>,
    _quiet: Option<super::Quiet>,
}

impl Controller {
    pub fn is_narrating(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.narrating.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    pub fn narrating_since(&self) -> Option<CaptureInstant> {
        #[cfg(target_os = "macos")]
        {
            self.narrating.as_ref().map(|active| active.press_at)
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    pub fn narrating_bundle(&self) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            self.narrating.as_ref().map(|active| active.bundle.clone())
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    pub fn reconcile(&mut self, audio: &impl AudioTimeline, outcomes: &SharedOutcomes) {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (audio, outcomes);
        }
        #[cfg(target_os = "macos")]
        {
            let want = available()
                && crate::labs::annotate_replay()
                && crate::app_settings::capture_on()
                && !crate::app_settings::scrawl_app_running();
            match (self.worker.is_some(), want) {
                (false, false) => {}
                (false, true) => match start_worker() {
                    Ok(handle) => {
                        let snap = current_exclusions();
                        self.last_exclusions = Some(snap);
                        self.worker = Some(handle);
                        tracing::info!("Annotate 15 s replay buffer enabled");
                    }
                    Err(error) => {
                        tracing::warn!(%error, "could not start Annotate replay worker");
                    }
                },
                (true, false) => {
                    self.cancel_narration(CaptureInstant::now(), audio, outcomes);
                    self.stop_worker();
                    tracing::info!("Annotate 15 s replay buffer stopped");
                }
                (true, true) => {
                    let snap = current_exclusions();
                    if self.last_exclusions != Some(snap) {
                        self.last_exclusions = Some(snap);
                        if let Some(worker) = &self.worker {
                            let window_ids = snap.0.into_iter().flatten().collect();
                            let _ = worker.commands.try_send(WorkerCmd::UpdateExclusions {
                                window_ids,
                                exclude_own_chrome: !snap.1,
                            });
                        }
                    }
                }
            }
        }
    }

    pub fn shutdown(
        &mut self,
        now: CaptureInstant,
        audio: &impl AudioTimeline,
        outcomes: &SharedOutcomes,
    ) {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (now, audio, outcomes);
        }
        #[cfg(target_os = "macos")]
        {
            self.cancel_narration(now, audio, outcomes);
            self.stop_worker();
        }
    }

    #[cfg(target_os = "macos")]
    fn stop_worker(&mut self) {
        self.last_exclusions = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.commands.send(WorkerCmd::Stop);
            super::join_by(
                worker.thread,
                Instant::now() + Duration::from_secs(2),
                "capture-replay",
            );
        }
    }

    /// `hex capture replay`: exports the last 15 s immediately with no audio,
    /// returning the queued bundle path on success or a `ControlReply` refusal.
    pub fn export_cli(
        &mut self,
        now: CaptureInstant,
        active_recording: bool,
        refusal: Option<ControlReply>,
        captures_dir: Option<&Path>,
        finisher: Option<&SyncSender<FinishJob>>,
        outcomes: &SharedOutcomes,
    ) -> Result<PathBuf, ControlReply> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (
                now,
                active_recording,
                refusal,
                captures_dir,
                finisher,
                outcomes,
            );
            Err(ControlReply::error(
                "unsupported",
                "Annotate replay requires macOS 27 or later",
            ))
        }
        #[cfg(target_os = "macos")]
        {
            if !available() {
                return Err(ControlReply::error(
                    "unsupported",
                    "Annotate replay requires macOS 27 or later",
                ));
            }
            if !crate::labs::annotate_replay() {
                return Err(ControlReply::error(
                    "feature-off",
                    "Annotate replay is off; turn it on with: hex labs on annotate-replay",
                ));
            }
            if active_recording || self.narrating.is_some() {
                return Err(ControlReply::error(
                    "busy",
                    "Another capture is currently recording",
                ));
            }
            if let Some(refused) = refusal {
                return Err(refused);
            }
            if let Some(denied) = check_screen_permission() {
                return Err(denied);
            }
            let Some(jobs) = finisher else {
                return Err(ControlReply::error("busy", "Hex is stopping"));
            };
            if self.worker.is_none() {
                match start_worker() {
                    Ok(handle) => self.worker = Some(handle),
                    Err(err) => return Err(ControlReply::error("failed", err)),
                }
            }
            let dir_root = captures_dir
                .map(Path::to_path_buf)
                .unwrap_or_else(|| crate::app_settings::capture_settings().captures_dir());
            let bundle = match new_bundle_dir(&dir_root) {
                Ok(dir) => dir,
                Err(err) => return Err(ControlReply::error("failed", format!("{err:#}"))),
            };
            let duration_secs = requested_replay_duration_secs();
            let mp4 = bundle.join("recording.mp4");
            let ticket = match self.request_export(&mp4, duration_secs) {
                Ok(ticket) => ticket,
                Err(err) => {
                    remove_bundle(&bundle);
                    return Err(ControlReply::error("failed", err));
                }
            };
            // Wait for `SCClipBufferingOutput` to write `recording.mp4` so CLI
            // callers immediately learn of invalid durations or empty buffers,
            // while keyframe extraction and routing run on `capture-finish`.
            let ExportTicket {
                done,
                display,
                buffered_for,
            } = ticket;
            match done.recv_timeout(EXPORT_WAIT) {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    remove_bundle(&bundle);
                    return Err(ControlReply::error(
                        "failed",
                        format!("replay export failed: {err}"),
                    ));
                }
                Err(_) => {
                    remove_bundle(&bundle);
                    return Err(ControlReply::error(
                        "failed",
                        "timed out exporting 15 s replay clip",
                    ));
                }
            }

            let clip_ms = (duration_secs.clamp(0.1, DEFAULT_REPLAY_SECS) * 1_000.0) as u64;
            let actual_clip_ms = clip_ms.min(buffered_for.as_millis() as u64).max(100);
            let anchor_ns = now
                .as_nanos()
                .saturating_sub(actual_clip_ms.saturating_mul(1_000_000));
            let started_at_iso = iso8601_utc_minus(Duration::from_millis(actual_clip_ms));
            let (frontmost, route_context) = capture_frontmost_context();
            let prepared_look = routing::look(route_context.clone());

            {
                let mut out = lock(outcomes);
                out.held = true;
                out.started(bundle.clone(), now, TargetKind::Display);
                out.stopped(&bundle, End::Finish);
                out.release();
            }

            let (already_tx, already_rx) = mpsc::sync_channel(1);
            let _ = already_tx.send(Ok(()));
            let sealed = Box::new(SealedReplay {
                bundle: bundle.clone(),
                anchor_ns,
                started_at_iso,
                export_done: already_rx,
                display,
                route_context,
                frontmost,
            });
            let (give_look, look) = mpsc::sync_channel(1);
            let job = FinishJob {
                bundle: bundle.clone(),
                capture: sealed,
                audio: RecordedAudio::default(),
                look,
            };
            match jobs.try_send(job) {
                Ok(()) => {
                    if let Some(look) = prepared_look {
                        let _ = give_look.send(look);
                    }
                    tracing::info!(bundle = %bundle.display(), "15 s replay queued for finishing");
                    Ok(bundle)
                }
                Err(mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job)) => {
                    job.capture.cancel();
                    lock(outcomes).settle(bundle, Stage::Cancelled, None);
                    Err(ControlReply::error(
                        "busy",
                        "Capture finish queue is full; try again shortly",
                    ))
                }
            }
        }
    }

    /// Starts a shortcut-triggered replay: freezes and exports the last 15 s
    /// immediately while opening the dictation timeline to record narration
    /// for as long as the shortcut stays held.
    #[cfg(target_os = "macos")]
    #[expect(
        clippy::too_many_arguments,
        reason = "borrows host fields without coupling"
    )]
    pub fn begin_shortcut(
        &mut self,
        press_at: CaptureInstant,
        active_recording: bool,
        refusal: Option<ControlReply>,
        captures_dir: Option<&Path>,
        audio: &impl AudioTimeline,
        quiet: Option<super::Quiet>,
        outcomes: &SharedOutcomes,
    ) {
        if !available() || !crate::labs::annotate_replay() {
            return;
        }
        if active_recording || self.narrating.is_some() || refusal.is_some() {
            return;
        }
        if check_screen_permission().is_some() {
            return;
        }
        if self.worker.is_none() {
            match start_worker() {
                Ok(handle) => self.worker = Some(handle),
                Err(err) => {
                    tracing::warn!(error = %err, "could not start replay worker for shortcut");
                    return;
                }
            }
        }
        let dir_root = captures_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| crate::app_settings::capture_settings().captures_dir());
        let Ok(bundle) = new_bundle_dir(&dir_root) else {
            return;
        };
        let duration_secs = requested_replay_duration_secs();
        let mp4 = bundle.join("recording.mp4");
        let ticket = match self.request_export(&mp4, duration_secs) {
            Ok(ticket) => ticket,
            Err(err) => {
                tracing::warn!(error = %err, "replay shortcut export could not start");
                remove_bundle(&bundle);
                return;
            }
        };
        let (sink, chunks) = mpsc::sync_channel(super::SINK_CAPACITY);
        if let Err(error) = audio.open_sink(sink, press_at) {
            tracing::warn!(%error, "could not open audio sink for narrated replay");
            remove_bundle(&bundle);
            return;
        }
        let clip_ms = (duration_secs.clamp(0.1, DEFAULT_REPLAY_SECS) * 1_000.0) as u64;
        let actual_clip_ms = clip_ms.min(ticket.buffered_for.as_millis() as u64).max(100);
        let anchor_ns = press_at
            .as_nanos()
            .saturating_sub(actual_clip_ms.saturating_mul(1_000_000));
        let started_at_iso = iso8601_utc_minus(Duration::from_millis(actual_clip_ms));
        let (frontmost, route_context) = capture_frontmost_context();
        let prepared_look = routing::look(route_context.clone());

        {
            let mut out = lock(outcomes);
            out.held = true;
            out.started(bundle.clone(), press_at, TargetKind::Display);
            out.progress(&bundle, Stage::Recording);
            out.release();
        }
        tracing::info!(bundle = %bundle.display(), "replay shortcut pressed; exporting 15 s clip and listening for narration");
        self.narrating = Some(NarratingReplay {
            bundle,
            press_at,
            anchor_ns,
            started_at_iso,
            chunks,
            ticket,
            route_context,
            frontmost,
            prepared_look,
            _quiet: quiet,
        });
    }

    #[cfg(target_os = "macos")]
    pub fn finish_shortcut(
        &mut self,
        release_at: CaptureInstant,
        audio: &impl AudioTimeline,
        finisher: Option<&SyncSender<FinishJob>>,
        outcomes: &SharedOutcomes,
    ) {
        let Some(active) = self.narrating.take() else {
            return;
        };
        let closed = audio.close_sink(release_at).unwrap_or_else(|error| {
            tracing::warn!(%error, "could not close replay narration sink");
            None
        });
        let mut timeline = super::Timeline::new(active.press_at);
        while let Ok(chunk) = active.chunks.try_recv() {
            match chunk {
                CaptureAudio::Samples {
                    samples,
                    sample_rate,
                    captured_through,
                } => timeline.push(&samples, sample_rate, captured_through),
                CaptureAudio::Gap { from, through } => timeline.gap(from, through),
            }
        }
        let held = release_at.duration_since(active.press_at);
        let recorded = if held >= MIN_NARRATION_HOLD {
            timeline.finish(release_at, closed)
        } else {
            RecordedAudio::default()
        };
        lock(outcomes).stopped(&active.bundle, End::Finish);
        let Some(jobs) = finisher else {
            remove_bundle(&active.bundle);
            lock(outcomes).settle(active.bundle, Stage::Cancelled, None);
            return;
        };
        let sealed = Box::new(SealedReplay {
            bundle: active.bundle.clone(),
            anchor_ns: active.anchor_ns,
            started_at_iso: active.started_at_iso,
            export_done: active.ticket.done,
            display: active.ticket.display,
            route_context: active.route_context,
            frontmost: active.frontmost,
        });
        let (give_look, look) = mpsc::sync_channel(1);
        let job = FinishJob {
            bundle: active.bundle.clone(),
            capture: sealed,
            audio: recorded,
            look,
        };
        match jobs.try_send(job) {
            Ok(()) => {
                if let Some(look) = active.prepared_look {
                    let _ = give_look.send(look);
                }
            }
            Err(mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job)) => {
                job.capture.cancel();
                lock(outcomes).settle(active.bundle, Stage::Cancelled, None);
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn cancel_narration(
        &mut self,
        now: CaptureInstant,
        audio: &impl AudioTimeline,
        outcomes: &SharedOutcomes,
    ) {
        self.cancel_narration_with(now, End::Cancel, audio, outcomes);
    }

    #[cfg(target_os = "macos")]
    pub fn cancel_narration_with(
        &mut self,
        now: CaptureInstant,
        end: End,
        audio: &impl AudioTimeline,
        outcomes: &SharedOutcomes,
    ) {
        let Some(active) = self.narrating.take() else {
            return;
        };
        let _ = audio.close_sink(now);
        lock(outcomes).stopped(&active.bundle, end);
        remove_bundle(&active.bundle);
        lock(outcomes).settle(active.bundle, Stage::Cancelled, None);
    }

    #[cfg(target_os = "macos")]
    fn request_export(&self, mp4: &Path, duration_secs: f64) -> Result<ExportTicket, String> {
        let worker = self
            .worker
            .as_ref()
            .ok_or_else(|| "Annotate replay worker is not running".to_string())?;
        let (tx, rx) = mpsc::sync_channel(1);
        worker
            .commands
            .send(WorkerCmd::Export {
                output_mp4: mp4.to_path_buf(),
                duration_secs,
                reply: tx,
            })
            .map_err(|_| "Annotate replay worker stopped".to_string())?;
        rx.recv_timeout(EXPORT_WAIT)
            .map_err(|_| "timed out waiting for Annotate replay buffer".to_string())?
    }
}

#[cfg(target_os = "macos")]
fn requested_replay_duration_secs() -> f64 {
    if crate::dev_build::is_dev_build()
        && let Ok(raw) = std::env::var("HEX_REPLAY_DURATION_SECS")
        && let Ok(parsed) = raw.parse::<f64>()
    {
        return parsed;
    }
    DEFAULT_REPLAY_SECS
}

#[cfg(target_os = "macos")]
fn check_screen_permission() -> Option<ControlReply> {
    if cfg!(test) {
        return None;
    }
    match super::screen_access::screen_access_now() {
        super::screen_access::ScreenAccess::Allowed => None,
        super::screen_access::ScreenAccess::NeedsRestart => Some(ControlReply::error(
            "permission",
            "Screen Recording turns on after Hex restarts",
        )),
        super::screen_access::ScreenAccess::Off => {
            super::permissions::report_capture(
                super::permissions::CaptureNeed::ScreenRecording,
                false,
            );
            Some(ControlReply::error(
                "permission",
                "Screen Recording is off for Hex",
            ))
        }
    }
}

#[cfg(target_os = "macos")]
fn current_exclusions() -> ([Option<u32>; 3], bool) {
    let windows = *super::OWN_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let recorded = super::own_windows_recorded();
    (windows, recorded)
}

#[cfg(target_os = "macos")]
fn capture_frontmost_context() -> (Option<(i32, String, Option<String>)>, RouteContext) {
    let front = scrawl_capture::capture::frontmost_app_info();
    let route = RouteContext {
        front_bundle_id: front.as_ref().and_then(|(_, _, b)| b.clone()),
        captured_bundle_id: front.as_ref().and_then(|(_, _, b)| b.clone()),
        app_pid: front.as_ref().map(|(pid, _, _)| *pid),
    };
    (front, route)
}

#[cfg(target_os = "macos")]
fn start_worker() -> Result<WorkerHandle, String> {
    let (commands_tx, commands_rx) = mpsc::sync_channel(8);
    let thread = thread::Builder::new()
        .name("capture-replay".into())
        .spawn(move || run_worker(commands_rx))
        .map_err(|err| format!("could not spawn capture-replay thread: {err}"))?;
    Ok(WorkerHandle {
        commands: commands_tx,
        thread,
    })
}

#[cfg(target_os = "macos")]
fn run_worker(commands: Receiver<WorkerCmd>) {
    let (sig_tx, sig_rx) = mpsc::sync_channel::<sck::StreamSignal>(16);
    let (reconfig_tx, reconfig_rx) = mpsc::sync_channel::<()>(2);
    let _display_watch = sck::DisplayReconfigWatch::register(reconfig_tx);

    let (initial_windows, initial_recorded) = current_exclusions();
    let mut window_ids: Vec<u32> = initial_windows.into_iter().flatten().collect();
    let mut exclude_own_chrome = !initial_recorded;

    let mut session: Option<sck::ClipBufferSession> = None;
    let mut retry_delay = Duration::from_millis(500);
    let mut next_open_attempt = Instant::now();

    loop {
        // Open the stream only once Screen Recording is already granted, so
        // flipping the Labs switch never triggers a surprise TCC dialog.
        if session.is_none() && Instant::now() >= next_open_attempt {
            if super::screen_access::screen_access_now()
                == super::screen_access::ScreenAccess::Allowed
            {
                match sck::ClipBufferSession::open(&window_ids, exclude_own_chrome, sig_tx.clone())
                {
                    Ok(opened) => {
                        tracing::info!(
                            display_id = opened.display.display_id,
                            width = opened.display.width,
                            height = opened.display.height,
                            fps = sck::BUFFER_FPS,
                            "SCClipBufferingOutput rolling 15 s buffer active"
                        );
                        session = Some(opened);
                        retry_delay = Duration::from_millis(500);
                    }
                    Err(err) => {
                        tracing::warn!(
                            error = %err,
                            retry_ms = retry_delay.as_millis() as u64,
                            "could not open SCClipBufferingOutput stream; will retry"
                        );
                        next_open_attempt = Instant::now() + retry_delay;
                        retry_delay = (retry_delay * 2).min(Duration::from_secs(5));
                    }
                }
            } else {
                next_open_attempt = Instant::now() + Duration::from_secs(5);
            }
        }

        // Drain async delegate & display-reconfiguration signals before parking.
        let mut reopen = false;
        while let Ok(signal) = sig_rx.try_recv() {
            match signal {
                sck::StreamSignal::BufferingStarted => {
                    tracing::info!("SCClipBufferingOutput delegate confirmed buffering started");
                }
                sck::StreamSignal::BufferingFailed(err) => {
                    tracing::warn!(error = %err, "SCClipBufferingOutput failed; reopening");
                    reopen = true;
                }
                sck::StreamSignal::BufferingStopped => {
                    tracing::debug!("SCClipBufferingOutput stopped");
                }
                sck::StreamSignal::StreamStopped(err) => {
                    tracing::warn!(error = %err, "replay SCStream stopped; reopening");
                    reopen = true;
                }
            }
        }
        while reconfig_rx.try_recv().is_ok() {
            tracing::info!("display configuration changed; refreshing replay SCStream");
            reopen = true;
        }
        if reopen {
            session = None;
            next_open_attempt = Instant::now() + Duration::from_millis(300);
            continue;
        }

        // When the buffer is healthy, block indefinitely until a command
        // arrives (or check display/delegate queues if woken). Only use a
        // bounded wait when recovering or waiting on permissions, or to notice
        // async delegate signals cleanly (2 s idle poll costs 0.00% CPU).
        let wait = if session.is_some() {
            Duration::from_secs(2)
        } else {
            next_open_attempt.saturating_duration_since(Instant::now())
        };

        match commands.recv_timeout(wait) {
            Ok(WorkerCmd::Stop) | Err(RecvTimeoutError::Disconnected) => {
                drop(session);
                return;
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Ok(WorkerCmd::UpdateExclusions {
                window_ids: next_ids,
                exclude_own_chrome: next_own,
            }) => {
                window_ids = next_ids;
                exclude_own_chrome = next_own;
                if let Some(active) = &session
                    && let Err(err) = active.update_exclusions(&window_ids, exclude_own_chrome)
                {
                    tracing::debug!(error = %err, "could not update replay exclusions in place; reopening");
                    session = None;
                    next_open_attempt = Instant::now();
                }
            }
            Ok(WorkerCmd::Export {
                output_mp4,
                duration_secs,
                reply,
            }) => {
                if session.is_none() {
                    match sck::ClipBufferSession::open(
                        &window_ids,
                        exclude_own_chrome,
                        sig_tx.clone(),
                    ) {
                        Ok(opened) => session = Some(opened),
                        Err(err) => {
                            let _ = reply.send(Err(err));
                            continue;
                        }
                    }
                }
                if let Some(active) = &session {
                    let done = active.export(&output_mp4, duration_secs);
                    let _ = reply.send(Ok(ExportTicket {
                        done,
                        display: active.display,
                        buffered_for: active.started_at.elapsed(),
                    }));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bundle finalization (`Sealed` implementation for `capture-finish`).

#[cfg(target_os = "macos")]
struct SealedReplay {
    bundle: PathBuf,
    anchor_ns: u64,
    started_at_iso: String,
    export_done: Receiver<Result<(), String>>,
    display: sck::BufferDisplay,
    route_context: RouteContext,
    frontmost: Option<(i32, String, Option<String>)>,
}

#[cfg(target_os = "macos")]
impl Sealed for SealedReplay {
    fn cancel(self: Box<Self>) {
        let _ = self.export_done.recv_timeout(Duration::from_secs(2));
        remove_bundle(&self.bundle);
    }

    fn finish(self: Box<Self>, audio: RecordedAudio) -> anyhow::Result<Written> {
        let result = self.finish_inner(audio);
        if result.is_err() {
            remove_bundle(&self.bundle);
        }
        result
    }

    fn route_context(&self) -> Option<RouteContext> {
        Some(self.route_context.clone())
    }
}

#[cfg(target_os = "macos")]
impl SealedReplay {
    fn finish_inner(&self, audio: RecordedAudio) -> anyhow::Result<Written> {
        match self.export_done.recv_timeout(EXPORT_WAIT) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => bail!("SCClipBufferingOutput export failed: {err}"),
            Err(_) => bail!("timed out waiting for SCClipBufferingOutput clip export"),
        }

        let video_path = self.bundle.join("recording.mp4");
        let probed = helper::probe(&video_path)
            .with_context(|| format!("could not probe exported replay {}", video_path.display()))?;
        let video_duration_ms = probed.duration_ms.max(1);
        let width = if probed.width > 0 {
            probed.width
        } else {
            self.display.width
        };
        let height = if probed.height > 0 {
            probed.height
        } else {
            self.display.height
        };

        let candidates = helper::frames(
            &video_path,
            FrameTimes::Every(CANDIDATE_SAMPLE_MS),
            CANDIDATE_MAX_EDGE,
        )
        .with_context(|| {
            format!(
                "could not decode candidate frames from {}",
                video_path.display()
            )
        })?;
        if candidates.is_empty() {
            bail!("exported replay movie contained no decodable video frames");
        }

        let (recorded_audio, total_duration_ms) = shift_audio_after_video(audio, video_duration_ms);
        let gaps = recorded_audio.as_ref().map_or(0, |a| a.gaps.len());
        let had_audio_samples = recorded_audio.is_some();

        write_capture_manifest(
            &self.bundle,
            &ManifestParams {
                anchor_ns: self.anchor_ns,
                started_at: Some(self.started_at_iso.clone()),
                display_id: self.display.display_id,
                frame: self.display.frame,
                scale: self.display.scale,
                width,
                height,
                video_duration_ms,
                audio: recorded_audio.as_ref(),
            },
        )?;

        let recording = ScreenRecording {
            frames: candidates,
            video: Some(Movie::written(video_path, 0)),
            duration_ms: total_duration_ms,
            no_screen: None,
        };
        let target = TargetInfo {
            mode: CaptureMode::Display,
            app: self.frontmost.as_ref().map(|(_, name, _)| name.clone()),
            bundle_id: self.frontmost.as_ref().and_then(|(_, _, bid)| bid.clone()),
            window_title: None,
            window_id: None,
            display_id: self.display.display_id,
            frame: TargetFrame {
                x: self.display.frame[0],
                y: self.display.frame[1],
                width: self.display.frame[2],
                height: self.display.frame[3],
                scale: self.display.scale,
            },
        };

        let engine_cfg = hex_engine_config();
        let transcriber = ChosenTranscriber;
        let (dir, bundle, summary) = scrawl_capture::pipeline::finalize_capture_bundle_for_config(
            &self.bundle,
            target,
            recording,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            recorded_audio,
            None,
            Default::default(),
            Some(&transcriber),
            Some(&engine_cfg),
        )?;

        let words = bundle.transcript.words.len();
        let missing_transcript = bundle
            .transcript
            .missing
            .as_ref()
            .map(|m| m.reason.clone())
            .or_else(|| {
                (bundle.transcript.text.trim().is_empty()
                    && bundle.audio.is_none()
                    && !had_audio_samples)
                    .then(|| "no_audio".to_owned())
            });

        tracing::info!(
            bundle = %dir.display(),
            video_duration_ms,
            total_duration_ms,
            words,
            summary = %summary,
            "15 s replay capture written"
        );
        Ok(Written {
            words,
            gaps,
            missing_transcript,
        })
    }
}

/// Aligns optional post-press voice narration after the frozen replay video on
/// the bundle clock, returning `None` when no narration samples were captured
/// so silent replays never write a 44-byte empty `audio.wav`.
#[cfg(any(target_os = "macos", test))]
fn shift_audio_after_video(
    audio: RecordedAudio,
    video_duration_ms: u64,
) -> (Option<RecordedAudio>, u64) {
    if audio.sample_count() == 0 {
        return (None, video_duration_ms);
    }
    let shifted_start = video_duration_ms.saturating_add(audio.start_offset_ms);
    let shifted_gaps = audio
        .gaps
        .into_iter()
        .map(|g| GapSpan {
            start_ms: video_duration_ms.saturating_add(g.start_ms),
            end_ms: video_duration_ms.saturating_add(g.end_ms),
        })
        .collect();
    let shifted = RecordedAudio::new(
        audio.samples,
        audio.sample_rate,
        shifted_start,
        shifted_gaps,
    );
    let total_ms = video_duration_ms.max(shifted.total_duration_ms());
    (Some(shifted), total_ms)
}

// ---------------------------------------------------------------------------
// `capture.json` v1 manifest writer (matches `crates/capture-core/src/manifest.rs`).

#[cfg(any(target_os = "macos", test))]
struct ManifestParams<'a> {
    anchor_ns: u64,
    started_at: Option<String>,
    display_id: u32,
    frame: [f64; 4],
    scale: f64,
    width: u32,
    height: u32,
    video_duration_ms: u64,
    audio: Option<&'a RecordedAudio>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Serialize)]
struct ReplayManifest {
    version: u32,
    clock: &'static str,
    anchor_ns: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_at: Option<String>,
    sources: Vec<ManifestSource>,
    files: Vec<ManifestFile>,
    audio_gaps: Vec<ManifestAudioGap>,
    missing_permissions: Vec<&'static str>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Serialize)]
struct ManifestSource {
    source: DisplaySourceTag,
    file: &'static str,
    start_ns: u64,
    duration_ms: u64,
    width: u32,
    height: u32,
    scale: Option<f64>,
    frame: ManifestRect,
    audio_tracks: Vec<&'static str>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Serialize)]
struct DisplaySourceTag {
    #[serde(rename = "type")]
    kind: &'static str,
    id: u32,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Serialize)]
struct ManifestRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Serialize)]
struct ManifestFile {
    name: &'static str,
    #[serde(flatten)]
    kind: ManifestFileKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_ns: Option<u64>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ManifestFileKind {
    Video { source: usize },
    Microphone { sample_rate: u32 },
}

#[cfg(any(target_os = "macos", test))]
#[derive(Serialize)]
struct ManifestAudioGap {
    start_ns: u64,
    duration_ns: u64,
}

#[cfg(any(target_os = "macos", test))]
fn write_capture_manifest(bundle_dir: &Path, params: &ManifestParams<'_>) -> anyhow::Result<()> {
    let mut files = vec![ManifestFile {
        name: "recording.mp4",
        kind: ManifestFileKind::Video { source: 0 },
        start_ns: Some(params.anchor_ns),
    }];
    let mut audio_gaps = Vec::new();
    if let Some(audio) = params.audio {
        let audio_start_ns = params
            .anchor_ns
            .saturating_add(audio.start_offset_ms.saturating_mul(1_000_000));
        files.push(ManifestFile {
            name: "audio.wav",
            kind: ManifestFileKind::Microphone {
                sample_rate: audio.sample_rate,
            },
            start_ns: Some(audio_start_ns),
        });
        for gap in &audio.gaps {
            let start_ns = params
                .anchor_ns
                .saturating_add(gap.start_ms.saturating_mul(1_000_000));
            let duration_ns = gap
                .end_ms
                .saturating_sub(gap.start_ms)
                .saturating_mul(1_000_000);
            audio_gaps.push(ManifestAudioGap {
                start_ns,
                duration_ns,
            });
        }
    }
    let manifest = ReplayManifest {
        version: 1,
        clock: "mach_host_ns",
        anchor_ns: params.anchor_ns,
        started_at: params.started_at.clone(),
        sources: vec![ManifestSource {
            source: DisplaySourceTag {
                kind: "display",
                id: params.display_id,
            },
            file: "recording.mp4",
            start_ns: params.anchor_ns,
            duration_ms: params.video_duration_ms,
            width: params.width,
            height: params.height,
            scale: (params.scale.is_finite() && params.scale > 0.0).then_some(params.scale),
            frame: ManifestRect {
                x: params.frame[0],
                y: params.frame[1],
                width: params.frame[2],
                height: params.frame[3],
            },
            audio_tracks: Vec::new(),
        }],
        files,
        audio_gaps,
        missing_permissions: Vec::new(),
    };
    let mut json = serde_json::to_vec_pretty(&manifest)?;
    json.push(b'\n');
    let path = bundle_dir.join("capture.json");
    let tmp = bundle_dir.join(".capture.json.tmp");
    fs::write(&tmp, &json).with_context(|| format!("could not write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("could not rename {}", path.display()))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn iso8601_utc_minus(rewind: Duration) -> String {
    let total_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .saturating_sub(rewind)
        .as_millis() as i64;
    let secs = total_ms.div_euclid(1_000);
    let millis = total_ms.rem_euclid(1_000);
    let days = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400);
    let hour = day_secs / 3_600;
    let minute = (day_secs % 3_600) / 60;
    let second = day_secs % 60;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

// ---------------------------------------------------------------------------
// `capture.replay_hotkey` state machine (macOS).

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayShortcutAction {
    None,
    Begin,
    Finish,
    Cancel,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct ReplayShortcut {
    held: Option<(u16, crate::app_settings::RuntimeHotkey)>,
}

#[cfg(target_os = "macos")]
impl ReplayShortcut {
    pub fn reset(&mut self) {
        self.held = None;
    }

    pub fn takes_escape(&self) -> bool {
        self.held.is_some()
    }

    pub fn input(
        &mut self,
        event: crate::suppression::InputEvent,
        hotkeys: crate::app_settings::RuntimeHotkeys,
        free: bool,
    ) -> ReplayShortcutAction {
        use crate::suppression::InputEvent;
        if self.held.is_some() && event.is_escape_down() {
            self.held = None;
            return ReplayShortcutAction::Cancel;
        }
        match event {
            InputEvent::Key {
                code,
                down: true,
                flags,
            } => {
                if self.held.is_some() || !free {
                    return ReplayShortcutAction::None;
                }
                let Some(binding) = hotkeys
                    .capture_replay
                    .filter(|binding| binding.matches_key_press(code, flags))
                else {
                    return ReplayShortcutAction::None;
                };
                self.held = Some((code, binding));
                ReplayShortcutAction::Begin
            }
            InputEvent::Key {
                code,
                down: false,
                flags,
            } => {
                let Some((held_code, binding)) = self.held else {
                    return ReplayShortcutAction::None;
                };
                if code == held_code
                    || (!binding.modifiers.is_empty() && !binding.required_modifiers_down(flags))
                {
                    self.held = None;
                    ReplayShortcutAction::Finish
                } else {
                    ReplayShortcutAction::None
                }
            }
            InputEvent::Flags(flags) => {
                let Some((_, binding)) = self.held else {
                    return ReplayShortcutAction::None;
                };
                if !binding.modifiers.is_empty() && !binding.required_modifiers_down(flags) {
                    self.held = None;
                    ReplayShortcutAction::Finish
                } else {
                    ReplayShortcutAction::None
                }
            }
            InputEvent::TapDisabled => {
                if self.held.take().is_some() {
                    ReplayShortcutAction::Cancel
                } else {
                    ReplayShortcutAction::None
                }
            }
            _ => ReplayShortcutAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_audio_produces_no_wav_and_narrated_audio_shifts_after_video() {
        let (silent, total) = shift_audio_after_video(RecordedAudio::default(), 15_000);
        assert!(silent.is_none());
        assert_eq!(total, 15_000);

        let samples = vec![0.1_f32; 32_000]; // 2,000 ms @ 16 kHz
        let raw = RecordedAudio::new(
            samples,
            16_000,
            120,
            vec![GapSpan {
                start_ms: 200,
                end_ms: 350,
            }],
        );
        let (shifted, total) = shift_audio_after_video(raw, 15_000);
        let shifted = shifted.expect("narrated audio preserved");
        assert_eq!(shifted.start_offset_ms, 15_120);
        assert_eq!(
            shifted.gaps,
            vec![GapSpan {
                start_ms: 15_200,
                end_ms: 15_350
            }]
        );
        assert_eq!(total, 17_120);
    }

    #[test]
    fn capture_manifest_matches_v1_schema_with_and_without_audio() {
        let dir =
            std::env::temp_dir().join(format!("hex-replay-manifest-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        write_capture_manifest(
            &dir,
            &ManifestParams {
                anchor_ns: 1_000_000_000,
                started_at: Some("2026-11-04T12:00:00.000Z".into()),
                display_id: 1,
                frame: [0.0, 0.0, 1512.0, 982.0],
                scale: 2.0,
                width: 2560,
                height: 1662,
                video_duration_ms: 15_000,
                audio: None,
            },
        )
        .unwrap();
        let parsed: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("capture.json")).unwrap()).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["clock"], "mach_host_ns");
        assert_eq!(parsed["sources"][0]["source"]["type"], "display");
        assert_eq!(parsed["sources"][0]["duration_ms"], 15_000);
        assert_eq!(parsed["files"].as_array().unwrap().len(), 1);

        let narrated = RecordedAudio::new(
            vec![0.0; 16_000],
            16_000,
            15_000,
            vec![GapSpan {
                start_ms: 15_100,
                end_ms: 15_250,
            }],
        );
        write_capture_manifest(
            &dir,
            &ManifestParams {
                anchor_ns: 1_000_000_000,
                started_at: None,
                display_id: 1,
                frame: [0.0, 0.0, 1512.0, 982.0],
                scale: 2.0,
                width: 2560,
                height: 1662,
                video_duration_ms: 15_000,
                audio: Some(&narrated),
            },
        )
        .unwrap();
        let parsed: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("capture.json")).unwrap()).unwrap();
        assert_eq!(parsed["files"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["files"][1]["name"], "audio.wav");
        assert_eq!(parsed["files"][1]["type"], "microphone");
        assert_eq!(parsed["audio_gaps"][0]["duration_ns"], 150_000_000);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn encoded_dimensions_stay_even_and_capped_at_2560() {
        assert_eq!(
            sck::encoded_dimensions(1512.0, 982.0, 2.0).unwrap(),
            (2560, 1662)
        );
        assert_eq!(
            sck::encoded_dimensions(1280.0, 720.0, 1.0).unwrap(),
            (1280, 720)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn replay_shortcut_begins_on_press_finishes_on_release_and_cancels_on_escape() {
        use crate::app_settings::{
            HotkeyBinding, HotkeyKey, HotkeyModifiers, ModifierSide, OPTION_KEY_MASK,
            RuntimeHotkeys, SHIFT_KEY_MASK,
        };
        use crate::suppression::InputEvent;

        let binding = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: Some(HotkeyKey {
                code: 15,
                label: "R".into(),
            }),
        };
        let hotkeys = RuntimeHotkeys {
            capture_replay: Some(binding.runtime()),
            ..Default::default()
        };
        let flags = OPTION_KEY_MASK | SHIFT_KEY_MASK;

        let mut sc = ReplayShortcut::default();
        assert_eq!(
            sc.input(
                InputEvent::Key {
                    code: 15,
                    down: true,
                    flags
                },
                hotkeys,
                false,
            ),
            ReplayShortcutAction::None,
            "busy capture blocks replay shortcut"
        );
        assert_eq!(
            sc.input(
                InputEvent::Key {
                    code: 15,
                    down: true,
                    flags
                },
                hotkeys,
                true,
            ),
            ReplayShortcutAction::Begin
        );
        assert!(sc.takes_escape());
        // Key repeat while held does nothing.
        assert_eq!(
            sc.input(
                InputEvent::Key {
                    code: 15,
                    down: true,
                    flags
                },
                hotkeys,
                true,
            ),
            ReplayShortcutAction::None
        );
        assert_eq!(
            sc.input(
                InputEvent::Key {
                    code: 15,
                    down: false,
                    flags
                },
                hotkeys,
                true,
            ),
            ReplayShortcutAction::Finish
        );
        assert!(!sc.takes_escape());

        // Escape while held cancels.
        assert_eq!(
            sc.input(
                InputEvent::Key {
                    code: 15,
                    down: true,
                    flags
                },
                hotkeys,
                true,
            ),
            ReplayShortcutAction::Begin
        );
        assert_eq!(
            sc.input(
                InputEvent::Key {
                    code: 53,
                    down: true,
                    flags: 0
                },
                hotkeys,
                true,
            ),
            ReplayShortcutAction::Cancel
        );
    }
}
