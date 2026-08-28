use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::audio::{AudioInput, AudioInputEvent, CaptureInstant};
use crate::dictation::{DictationCapture, Finish};
use crate::events::{DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms};
use crate::linux_desktop::LinuxIndicator;
use crate::linux_input::{HotkeyEvent, LinuxHotkeyMonitor};
use crate::linux_paste::LinuxPaster;
use crate::linux_transcriber::LinuxTranscriber;

const UPDATE_INTERVAL: Duration = Duration::from_millis(20);

struct Job {
    samples: Vec<f32>,
    audio_ms: u64,
}

struct OutputWorker {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for OutputWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn run(event_path: &Path, device: Option<&str>, shutdown: &AtomicBool) -> Result<()> {
    let settings = crate::linux_settings::LinuxSettings::load()?;
    let transcriber = LinuxTranscriber::load(&settings.transcription)?;
    run_with_settings(event_path, device, shutdown, settings, transcriber)
}

pub fn run_with_transcriber(
    event_path: &Path,
    device: Option<&str>,
    shutdown: &AtomicBool,
    transcriber: LinuxTranscriber,
) -> Result<()> {
    let settings = crate::linux_settings::LinuxSettings::load()?;
    run_with_settings(event_path, device, shutdown, settings, transcriber)
}

fn run_with_settings(
    event_path: &Path,
    device: Option<&str>,
    shutdown: &AtomicBool,
    settings: crate::linux_settings::LinuxSettings,
    transcriber: LinuxTranscriber,
) -> Result<()> {
    // Drop audio/hotkeys and close the job queue before joining slow output on
    // every exit, including failures during startup or recording.
    let mut output = OutputWorker {
        stop: Arc::new(AtomicBool::new(false)),
        worker: None,
    };
    let input = match device {
        Some(name) => AudioInput::open_matching(name)?,
        None => AudioInput::open(&[])?,
    };
    let hotkey_label = settings.dictation_hotkey.label();
    let hotkey = LinuxHotkeyMonitor::start(settings.dictation_hotkey, settings.double_tap_lock)?;
    let indicator = LinuxIndicator::new();
    let (jobs, job_receiver) = mpsc::sync_channel::<Job>(2);
    let (result_sender, results) = mpsc::channel();
    let worker_stop = output.stop.clone();
    output.worker = Some(thread::spawn(move || {
        let mut transcriber = transcriber;
        let mut paster = LinuxPaster::new(worker_stop.clone(), settings.paste_with_shift);
        while let Ok(job) = job_receiver.recv() {
            if worker_stop.load(Ordering::Acquire) {
                break;
            }
            let started = Instant::now();
            let result = transcriber
                .transcribe(&job.samples)
                .map_err(|error| format!("{error:#}"))
                .and_then(|text| {
                    let text = text.trim().to_string();
                    if text.is_empty() {
                        return Err("transcription was empty".into());
                    }
                    let paste_result = paster
                        .as_mut()
                        .map_err(|error| format!("{error:#}"))?
                        .paste(&text)
                        .map_err(|error| format!("{error:#}"));
                    paste_result.map(|()| text)
                });
            tracing::info!(
                audio_ms = job.audio_ms,
                elapsed_ms = started.elapsed().as_millis(),
                "completed Linux dictation job"
            );
            if result_sender.send(result).is_err() {
                break;
            }
        }
    }));

    let mut events = EventLog::create(event_path)?;
    let mut capture = DictationCapture::new(input.sample_rate);
    let mut recording = false;
    let mut captured_through = CaptureInstant::ZERO;
    let mut pending = 0_usize;
    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    emit_state(&mut events, VoiceState::Listening, &input.device_name)?;
    println!(
        "HEX dictation is ready on {}. Hold {} to dictate; Ctrl-C stops.",
        input.device_name, hotkey_label
    );

    while !shutdown.load(Ordering::Relaxed) {
        if let Ok(error) = hotkey.errors.try_recv() {
            return Err(eyre!("Linux hotkey monitor stopped: {error}"));
        }
        while let Ok(action) = hotkey.events.try_recv() {
            match action {
                HotkeyEvent::Start if !recording => {
                    capture.start(captured_through);
                    recording = true;
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, VoiceState::Dictating, &input.device_name)?;
                }
                HotkeyEvent::Finish if recording => {
                    recording = false;
                    submit_capture(
                        &mut capture,
                        captured_through,
                        &jobs,
                        &mut events,
                        &mut pending,
                    )?;
                    emit_state(
                        &mut events,
                        active_state(recording, pending),
                        &input.device_name,
                    )?;
                }
                HotkeyEvent::Cancel if recording => {
                    capture.cancel();
                    recording = false;
                    events.dictation(DictationPhase::Cancelled, "")?;
                    emit_state(
                        &mut events,
                        active_state(recording, pending),
                        &input.device_name,
                    )?;
                }
                _ => {}
            }
        }
        while let Ok(result) = results.try_recv() {
            pending = pending.saturating_sub(1);
            match result {
                Ok(text) => {
                    events.emit(&VoiceEvent::Transcript {
                        timestamp_ms: now_ms(),
                        phase: TranscriptPhase::Completed,
                        latency_ms: 0,
                        text: text.clone(),
                    })?;
                    events.dictation(DictationPhase::Pasted, text)?;
                }
                Err(error) => events.dictation(DictationPhase::Failed(error), "")?,
            }
            emit_state(
                &mut events,
                active_state(recording, pending),
                &input.device_name,
            )?;
        }

        indicator.update(recording, pending);
        let chunk = match input.recv_timeout(UPDATE_INTERVAL) {
            AudioInputEvent::Chunk {
                samples,
                captured_through: chunk_captured_through,
            } => {
                captured_through = chunk_captured_through;
                samples
            }
            AudioInputEvent::Timeout => continue,
            AudioInputEvent::StreamFailed(error) => {
                return Err(eyre!("microphone stream stopped: {error}"));
            }
        };
        if recording {
            capture.ingest(&chunk, captured_through);
            capture.become_intentional(captured_through);
        } else {
            capture.keep_warm(&chunk);
        }
    }

    indicator.update(false, pending);
    emit_state(&mut events, VoiceState::Stopping, &input.device_name)?;
    output.stop.store(true, Ordering::Release);
    drop(hotkey);
    drop(input);
    drop(jobs);
    if output
        .worker
        .take()
        .expect("dictation output worker exists")
        .join()
        .is_err()
    {
        return Err(eyre!("Linux dictation worker panicked"));
    }
    println!("Stopped.");
    Ok(())
}

fn submit_capture(
    capture: &mut DictationCapture,
    ended_at: CaptureInstant,
    jobs: &SyncSender<Job>,
    events: &mut EventLog,
    pending: &mut usize,
) -> Result<()> {
    match capture.finish(ended_at) {
        Finish::Discard => events.dictation(DictationPhase::Discarded, "")?,
        Finish::Transcribe(clip) => {
            let audio_ms = clip.duration_ms();
            let job = Job {
                samples: clip.into_parakeet_samples(),
                audio_ms,
            };
            match jobs.try_send(job) {
                Ok(()) => {
                    *pending += 1;
                    events.dictation(DictationPhase::Transcribing, "")?;
                }
                Err(TrySendError::Full(_)) => events
                    .dictation(DictationPhase::Failed("dictation queue is full".into()), "")?,
                Err(TrySendError::Disconnected(_)) => {
                    return Err(eyre!("dictation worker is unavailable"));
                }
            }
        }
    }
    Ok(())
}

fn active_state(recording: bool, pending: usize) -> VoiceState {
    if recording {
        VoiceState::Dictating
    } else if pending > 0 {
        VoiceState::Transcribing
    } else {
        VoiceState::Listening
    }
}

fn emit_state(events: &mut EventLog, state: VoiceState, device: &str) -> Result<()> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state,
        device: device.into(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_capture_keeps_pending_output_visible() {
        assert_eq!(active_state(true, 2), VoiceState::Dictating);
        assert_eq!(active_state(false, 2), VoiceState::Transcribing);
        assert_eq!(active_state(false, 1), VoiceState::Transcribing);
        assert_eq!(active_state(false, 0), VoiceState::Listening);
        assert_eq!(active_state(true, 0), VoiceState::Dictating);
    }

    #[test]
    fn output_guard_cancels_and_joins_even_on_listener_error() {
        let stopped = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let result: Result<()> = {
            let mut output = OutputWorker {
                stop: stopped.clone(),
                worker: None,
            };
            let (_jobs, receiver) = mpsc::channel::<()>();
            let stop = stopped.clone();
            let completed = finished.clone();
            output.worker = Some(thread::spawn(move || {
                // The sender must close before the guard joins this worker.
                assert!(receiver.recv().is_err());
                while !stop.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                completed.store(true, Ordering::Release);
            }));
            Err(eyre!("listener failure"))
        };
        assert!(result.is_err());
        assert!(stopped.load(Ordering::Acquire));
        assert!(finished.load(Ordering::Acquire));
    }
}
