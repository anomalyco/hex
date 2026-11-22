//! One open microphone stream: the chunks it delivers with their capture
//! clock, the failure it reports, and the one place its overloads reach the
//! audio owner.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use color_eyre::eyre::eyre;
use color_eyre::eyre::{Result, WrapErr};
use cpal::traits::DeviceTrait;
#[cfg(target_os = "windows")]
use cpal::traits::HostTrait;
use cpal::{Device, SizedSample, Stream, StreamConfig};

use super::instant::{CaptureInstant, duration_for_frames};
#[cfg(target_os = "windows")]
use super::open::{
    OpenRequest, SelectedFallback, device_name, input_devices_of, open_configured_input,
};
use super::routes::audio_service_restart_generation;
#[cfg(target_os = "windows")]
use super::routes::notify_input_route_changed;
#[cfg(target_os = "macos")]
use super::routes::watch_input_devices;
#[cfg(target_os = "macos")]
use super::uses_fixture;
use super::{input_format, interruption, overloads};

/// Why a stream stopped, told apart where the reopen leaves voice processing
/// on rather than counting it as a failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum StreamError {
    /// The stream failed or stopped delivering audio.
    Failed(String),
    /// AVAudioEngine reconfigured. Reopen it, but voice processing is not to
    /// blame, so the fallback memory stays clear.
    VoiceProcessingConfigurationChanged,
}

pub struct AudioInput {
    // Tests can deliver PCM through channels without opening a native microphone.
    pub(super) _stream: Option<input_format::InputStream>,
    pub(super) chunks: Receiver<(Vec<f32>, CaptureInstant)>,
    pub sample_rate: u32,
    pub device_name: String,
    /// The device's stable OS identifier, when it has one, which is what a
    /// call app's reported inputs are compared by.
    pub uid: Option<String>,
    /// Whether this input stands in for the saved microphone until it can
    /// be opened again.
    pub selection: Selection,
    /// Whether this input was asked to open through Apple's voice processing
    /// (Labs › Voice isolation), whether or not it could; the reconcile
    /// comparison reads this requested path, not the one that opened.
    pub voice_isolation: bool,
    /// Whether Apple's voice processing actually opened this input; false
    /// when the raw cpal stream stood in.
    pub voice_processing_active: bool,
    pub(super) stream_errors: Receiver<StreamError>,
    pub(super) watch_service_restarts: bool,
    pub(super) restart_generation: u64,
    pub(super) hex_microphone: crate::audio::hex_microphone::CaptureFeedSwitch,
    /// How long the stream may go without delivering audio before it counts
    /// as failed; `None` for test inputs fed by hand.
    pub(super) stall_after: Option<Duration>,
    /// When the stream opened or last delivered a chunk.
    pub(super) last_delivery: Mutex<Instant>,
    /// Whether this stream's first chunk has reported how often it delivers
    /// audio.
    pub(super) reported_period: AtomicBool,
}

/// A running stream delivers audio every few milliseconds, even silence. One
/// that has delivered nothing for this long has stopped without reporting an
/// error (seen after a burst of overruns, #1206) and is reopened.
pub(crate) const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(4);

/// A stream that has delivered chunks for this long since its first one has
/// proven that it works: its failure reopens it at once. One that fails
/// sooner never proved steady delivery, so its reopen backs off instead of
/// looping every stall timeout (#1681).
pub(super) const STREAM_PROVEN_UPTIME: Duration = STREAM_STALL_TIMEOUT.saturating_mul(2);

pub enum AudioInputEvent {
    Chunk {
        samples: Vec<f32>,
        captured_through: CaptureInstant,
    },
    Timeout,
    StreamFailed(String),
    /// AVAudioEngine reconfigured; reopen the stream, still through voice
    /// processing when it is on, rather than treating it as a failure.
    StreamReconfigured,
}

/// Whether an input stands in for the saved microphone, and why.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Selection {
    /// Nothing is saved, or the input is the saved microphone.
    #[default]
    InUse,
    /// The saved microphone is not listed.
    Unlisted,
    /// The saved microphone is listed but would not open.
    Unopenable,
    /// The saved microphone is built in and off while the lid is closed.
    LidClosed,
}

/// What resolving and opening the configured input found.
// One is made per stream open, so the Windows stream's size doesn't matter.
#[allow(clippy::large_enum_variant)]
pub enum Opened {
    Input(Box<AudioInput>),
    /// The input resolved to the device already open, which was kept.
    Unchanged {
        selection: Selection,
    },
}

impl AudioInput {
    #[cfg(test)]
    pub fn channel_for_test() -> (Self, Sender<(Vec<f32>, CaptureInstant)>) {
        let (sender, chunks) = mpsc::channel();
        let (_, stream_errors) = mpsc::channel();
        (
            Self {
                _stream: None,
                chunks,
                stream_errors,
                sample_rate: 48_000,
                device_name: "Test microphone".into(),
                uid: None,
                selection: Selection::InUse,
                voice_isolation: false,
                voice_processing_active: false,
                watch_service_restarts: false,
                restart_generation: audio_service_restart_generation(),
                hex_microphone: crate::audio::hex_microphone::capture_feed(48_000).1,
                stall_after: None,
                last_delivery: Mutex::new(Instant::now()),
                reported_period: AtomicBool::new(false),
            },
            sender,
        )
    }

    /// Gives a test its own error channel, so it can report a configuration
    /// change or failure through the real `recv_timeout` path.
    #[cfg(test)]
    pub(super) fn stream_errors_for_test(&mut self) -> SyncSender<StreamError> {
        let (sender, errors) = mpsc::sync_channel(1);
        self.stream_errors = errors;
        sender
    }

    #[cfg(test)]
    pub(crate) fn watch_stalls_for_test(&mut self, after: Duration) {
        self.stall_after = Some(after);
        *self
            .last_delivery
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Instant::now();
    }

    #[cfg(test)]
    pub fn watch_service_restarts_for_test(&mut self) {
        self.watch_service_restarts = true;
        self.restart_generation = audio_service_restart_generation();
    }

    /// The OS's default input, or a real one standing in when the default is
    /// Hex Microphone or missing.
    #[cfg(target_os = "windows")]
    pub fn open_system_default() -> Result<Self> {
        match open_configured_input(&OpenRequest {
            device_override: None,
            selected_device: None,
            fallback: SelectedFallback::Immediate,
            keep: None,
            avoids_bluetooth: true,
            call_input: None,
            renegotiate_rate: false,
            voice_processing: false,
            voice_processing_fallback: false,
        })? {
            Opened::Input(input) => Ok(*input),
            Opened::Unchanged { .. } => unreachable!("nothing to keep"),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn open_matching(query: &str) -> Result<Self> {
        let host = cpal::default_host();
        let (devices, names) = input_devices_of(&host)?;
        let query_lower = query.to_lowercase();
        let index = names
            .iter()
            .position(|name| name.to_lowercase().contains(&query_lower))
            .ok_or_else(|| eyre!("microphone is unavailable: {query}"))?;
        Self::open_device(
            devices[index].clone(),
            names[index].clone(),
            false,
            false,
            false,
        )
    }

    /// Opens `device`. `follows_default` marks cpal's default-input handle,
    /// whose Windows stream reports a default change as invalidation while
    /// it keeps running on the old device. `renegotiate_rate` makes the
    /// device change its sample rate before the stream opens (#1575).
    /// `voice_processing` opens it through Apple's voice processing instead
    /// (Labs › Voice isolation), falling back to the raw stream when the
    /// engine refuses.
    pub(super) fn open_device(
        device: Device,
        device_name: String,
        follows_default: bool,
        renegotiate_rate: bool,
        voice_processing: bool,
        voice_processing_fallback: bool,
    ) -> Result<Self> {
        #[cfg(target_os = "macos")]
        watch_input_devices();
        let uid = device.id().ok().map(|id| id.id().to_owned());
        let restart_generation = audio_service_restart_generation();
        // Taken before the stream runs, so CoreAudio's real-time thread reads
        // an initialized clock.
        overloads::start_clock();
        #[cfg(target_os = "macos")]
        let voice_processed = if voice_processing && !voice_processing_fallback {
            match uid.as_deref() {
                Some(uid) => Some(input_format::start_voice_processing(uid, &device_name)),
                None => {
                    tracing::warn!(
                        device = %device_name,
                        "voice processing needs the microphone's uid; opening it normally"
                    );
                    None
                }
            }
        } else {
            None
        };
        #[cfg(not(target_os = "macos"))]
        let voice_processed = {
            let _ = (voice_processing, voice_processing_fallback);
            None::<Result<input_format::StartedStream>>
        };
        let open_raw = || {
            input_format::start_stream_with_fallback(
                &device,
                &device_name,
                follows_default.then(|| device_name.clone()),
                renegotiate_rate,
            )
        };
        let mut voice_processing_active = false;
        let started = match voice_processed {
            Some(Ok(started)) => {
                voice_processing_active = true;
                started
            }
            Some(Err(error)) => {
                tracing::warn!(
                    error = format!("{error:#}"),
                    device = %device_name,
                    "voice processing would not start; opening the microphone normally"
                );
                open_raw()?
            }
            None => open_raw()?,
        };

        Ok(Self {
            _stream: Some(started.stream),
            chunks: started.chunks,
            sample_rate: started.sample_rate,
            device_name,
            uid,
            selection: Selection::InUse,
            voice_isolation: voice_processing,
            voice_processing_active,
            stream_errors: started.stream_errors,
            watch_service_restarts: true,
            restart_generation,
            hex_microphone: started.hex_microphone,
            stall_after: Some(STREAM_STALL_TIMEOUT),
            last_delivery: Mutex::new(Instant::now()),
            reported_period: AtomicBool::new(false),
        })
    }

    /// Whether this stream's audio feeds Hex Microphone. Only the dictation
    /// listener's current input does; a meeting's second stream on the same
    /// microphone must not.
    pub fn feed_hex_microphone(&self, on: bool) {
        self.hex_microphone.set(on);
    }

    pub fn recv_timeout(&self, timeout: Duration) -> AudioInputEvent {
        // The one place a stream's overloads reach the log, whoever reported
        // them: the error callback only counts them, and this poll writes the
        // count.
        overloads::report();
        if self.watch_service_restarts
            && audio_service_restart_generation() != self.restart_generation
        {
            return AudioInputEvent::StreamFailed("CoreAudio service restarted".into());
        }
        // A stream failure reported before or during the wait outranks any
        // chunk delivered alongside it.
        if let Some(event) = self.take_stream_error() {
            return event;
        }
        let received = self.chunks.recv_timeout(timeout);
        if self.watch_service_restarts
            && audio_service_restart_generation() != self.restart_generation
        {
            return AudioInputEvent::StreamFailed("CoreAudio service restarted".into());
        }
        if let Some(event) = self.take_stream_error() {
            return event;
        }
        match received {
            Ok((samples, captured_through)) => {
                self.report_period(samples.len());
                if self.stall_after.is_some() {
                    *self
                        .last_delivery
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Instant::now();
                }
                AudioInputEvent::Chunk {
                    samples,
                    captured_through,
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if interruption::stalled(self.stall_after, &self.last_delivery) {
                    AudioInputEvent::StreamFailed("microphone stopped delivering audio".into())
                } else {
                    AudioInputEvent::Timeout
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                AudioInputEvent::StreamFailed("microphone audio channel disconnected".into())
            }
        }
    }

    /// The stream error waiting, if any, as the event the audio owner acts
    /// on. A configuration change is its own event so a reopen through voice
    /// processing isn't mistaken for a failure.
    fn take_stream_error(&self) -> Option<AudioInputEvent> {
        match self.stream_errors.try_recv() {
            Ok(StreamError::Failed(error)) => Some(AudioInputEvent::StreamFailed(error)),
            Ok(StreamError::VoiceProcessingConfigurationChanged) => {
                Some(AudioInputEvent::StreamReconfigured)
            }
            Err(_) => None,
        }
    }

    /// Reports how much audio the stream's callbacks deliver, once per open.
    /// cpal exposes no readback of the period it picks for `BufferSize::Default`
    /// and overloads arrive sooner on a smaller one, so the first chunk says
    /// what the device actually does. A fixture input's chunks are its WAV
    /// file's, not a device's, so it says nothing.
    fn report_period(&self, frames: usize) {
        let first = !self.reported_period.swap(true, Ordering::Relaxed);
        #[cfg(target_os = "macos")]
        if first && !uses_fixture() {
            tracing::info!(
                device = %self.device_name,
                sample_rate = self.sample_rate,
                voice_processing = self.voice_processing_active,
                frames,
                period_us = duration_for_frames(frames, self.sample_rate).as_micros() as u64,
                "microphone stream opened"
            );
        }
        // Only macOS logs the period; every platform keeps the same open state.
        #[cfg(not(target_os = "macos"))]
        let _ = (frames, first);
    }
}

impl Drop for AudioInput {
    fn drop(&mut self) {
        // Stop the stream before the count is taken, so a closing stream's last
        // burst is written here instead of beside the next open's lines.
        self._stream = None;
        overloads::flush();
    }
}

pub(super) fn mono<T>(samples: &[T], channels: usize, convert: impl Fn(&T) -> f32) -> Vec<f32> {
    samples
        .chunks(channels)
        .map(|frame| frame.iter().map(&convert).sum::<f32>() / channels as f32)
        .collect()
}

/// Where a stream reports errors to the audio owner.
#[derive(Clone)]
pub(super) struct StreamErrorReporter {
    pub(super) sender: SyncSender<StreamError>,
    /// The device's name when the stream was opened on cpal's default-input
    /// handle.
    pub(super) default_name: Option<String>,
}

impl StreamErrorReporter {
    fn report(&self, error: cpal::Error) {
        // WASAPI reports a default-input change as invalidation, yet the
        // stream keeps capturing the old device. Hex switches once capture is
        // idle rather than failing a recording.
        #[cfg(target_os = "windows")]
        if error.kind() == cpal::ErrorKind::StreamInvalidated
            && let Some(name) = &self.default_name
            && cpal::default_host()
                .default_input_device()
                .and_then(|device| device_name(&device))
                .is_some_and(|current| current != *name)
        {
            tracing::info!(from = %name, "the default microphone changed");
            notify_input_route_changed();
            return;
        }
        #[cfg(not(target_os = "windows"))]
        let _ = &self.default_name;
        report_stream_error(&self.sender, error);
    }
}

fn report_stream_error(sender: &SyncSender<StreamError>, error: cpal::Error) {
    match error.kind() {
        // An overload or a refused real-time promotion is a glitch on a stream
        // that keeps running, common while a cold device starts. Some hosts
        // report it from the thread that runs the stream, where a log line or a
        // lock blocks an already-late cycle, so only the counters move here and
        // the audio owner writes the burst. Real sample gaps still surface as
        // capture discontinuities.
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied => overloads::record(),
        _ => {
            tracing::error!(%error, "microphone stream failed");
            let _ = sender.try_send(StreamError::Failed(error.to_string()));
        }
    }
}

pub(super) fn build_stream<T: SizedSample>(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    sender: Sender<(Vec<f32>, CaptureInstant)>,
    reporter: StreamErrorReporter,
    mut feed: crate::audio::hex_microphone::CaptureFeed,
    convert: impl Fn(T) -> f32 + Send + 'static,
) -> Result<Stream> {
    let sample_rate = config.sample_rate;
    device
        .build_input_stream(
            *config,
            move |data: &[T], info| {
                let captured_through = captured_through(info, data.len() / channels, sample_rate);
                let mono_samples = mono(data, channels, |sample| convert(*sample));
                feed.push(&mono_samples);
                let _ = sender.send((mono_samples, captured_through));
            },
            move |error| reporter.report(error),
            None,
        )
        .wrap_err_with(|| format!("could not open {} microphone stream", T::FORMAT))
}

fn captured_through(
    info: &cpal::InputCallbackInfo,
    frames: usize,
    sample_rate: u32,
) -> CaptureInstant {
    let captured_at = CaptureInstant::from_stream(info.timestamp().capture);
    let duration = duration_for_frames(frames, sample_rate);
    captured_at.checked_add(duration).unwrap_or(captured_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::mpsc;

    use crate::audio::since_last_stream_glitch;

    #[test]
    fn stream_failures_are_visible_to_the_audio_owner() {
        let (sender, errors) = mpsc::sync_channel(1);

        report_stream_error(
            &sender,
            cpal::Error::with_message(
                cpal::ErrorKind::StreamInvalidated,
                "Device sample rate changed",
            ),
        );

        assert_eq!(
            errors.try_recv().unwrap(),
            StreamError::Failed("Device sample rate changed".into())
        );
    }

    #[test]
    fn overload_glitches_do_not_interrupt_the_stream() {
        let (sender, errors) = mpsc::sync_channel(1);

        report_stream_error(&sender, cpal::ErrorKind::Xrun.into());
        report_stream_error(&sender, cpal::ErrorKind::RealtimeDenied.into());
        assert!(errors.try_recv().is_err());
        assert!(since_last_stream_glitch().is_some());

        report_stream_error(&sender, cpal::ErrorKind::DeviceNotAvailable.into());
        assert!(errors.try_recv().is_ok());
    }
}
