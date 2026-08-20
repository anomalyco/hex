#[cfg(not(target_os = "windows"))]
use std::io::Cursor;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};
#[cfg(not(target_os = "windows"))]
use rodio::Decoder;
use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Source};

#[derive(Clone, Copy)]
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub enum Tone {
    Wake,
    Sleep,
    Error,
    DictationStart,
    DictationStop,
    Cancel,
}

static DICTATION_PLAYER: OnceLock<SyncSender<Tone>> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(true);
static VOLUME: AtomicU32 = AtomicU32::new(0.5_f32.to_bits());

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_volume(volume: f32) {
    VOLUME.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

fn volume() -> f32 {
    f32::from_bits(VOLUME.load(Ordering::Relaxed))
}

pub fn preload() -> Result<()> {
    if DICTATION_PLAYER.get().is_some() {
        return Ok(());
    }
    let (sender, receiver) = mpsc::sync_channel(8);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = (|| -> Result<_> {
            let output = open_feedback_output()?;
            #[cfg(target_os = "windows")]
            let (start, stop, cancel) = (
                synthesize_cue(Cue::Start),
                synthesize_cue(Cue::Stop),
                synthesize_cue(Cue::Cancel),
            );
            #[cfg(not(target_os = "windows"))]
            let (start, stop, cancel) = (
                decode(include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/resources/audio/startRecording.mp3"
                )))?,
                decode(include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/resources/audio/stopRecording.mp3"
                )))?,
                decode(include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/resources/audio/cancel.mp3"
                )))?,
            );
            Ok((output, start, stop, cancel))
        })();
        let Ok((output, start, stop, cancel)) = result else {
            let _ = ready_sender.send(result.map(|_| ()));
            return;
        };
        let _ = ready_sender.send(Ok(()));
        while let Ok(tone) = receiver.recv() {
            let sound = match tone {
                Tone::DictationStart => &start,
                Tone::DictationStop => &stop,
                Tone::Cancel => &cancel,
                Tone::Wake | Tone::Sleep | Tone::Error => continue,
            };
            output.mixer().add(sound.clone().amplify(volume()));
        }
    });
    ready_receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| eyre!("timed out preloading feedback audio"))??;
    let _ = DICTATION_PLAYER.set(sender);
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_feedback_output() -> Result<MixerDeviceSink> {
    const LOW_LATENCY_BUFFER_FRAMES: u32 = 512;
    match DeviceSinkBuilder::from_default_device().and_then(|builder| {
        builder
            .with_buffer_size(rodio::cpal::BufferSize::Fixed(LOW_LATENCY_BUFFER_FRAMES))
            .open_stream()
    }) {
        Ok(output) => Ok(output),
        Err(error) => {
            tracing::warn!(%error, "low-latency feedback output is unavailable; using the default buffer");
            DeviceSinkBuilder::open_default_sink().wrap_err("could not open the audio output")
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn open_feedback_output() -> Result<MixerDeviceSink> {
    DeviceSinkBuilder::open_default_sink().wrap_err("could not open the audio output")
}

pub fn play(tone: Tone) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    match tone {
        Tone::DictationStart | Tone::DictationStop | Tone::Cancel => {
            if let Some(player) = DICTATION_PLAYER.get() {
                let _ = player.try_send(tone);
            }
        }
        Tone::Wake | Tone::Sleep | Tone::Error => {
            #[cfg(target_os = "macos")]
            play_system_tone(tone);
        }
    }
}

#[cfg(target_os = "macos")]
fn play_system_tone(tone: Tone) {
    let sound = match tone {
        Tone::Wake => "Pop",
        Tone::Sleep => "Tink",
        Tone::Error => "Basso",
        Tone::DictationStart | Tone::DictationStop | Tone::Cancel => return,
    };
    let volume = volume().to_string();
    let child = Command::new("/usr/bin/afplay")
        .args([
            "-v",
            &volume,
            &format!("/System/Library/Sounds/{sound}.aiff"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
enum Cue {
    Start,
    Stop,
    Cancel,
}

/// Soft synthesized cues instead of the sampled tones: a short upward chirp
/// to start, a downward chirp to stop, and a low blip on cancel, each with a
/// fast exponential attack and a ~90 ms decay. The envelope peaks at 0.15 so
/// the default 50% feedback volume reproduces the reference loudness.
#[cfg(target_os = "windows")]
fn synthesize_cue(cue: Cue) -> SamplesBuffer {
    const SAMPLE_RATE: u32 = 48_000;
    const DURATION: f32 = 0.095;
    const ATTACK: f32 = 0.008;
    const RELEASE_END: f32 = 0.09;
    const PEAK: f32 = 0.15;
    const FLOOR: f32 = 0.000_2;
    let (start_hz, end_hz, ramp) = match cue {
        Cue::Start => (660.0_f32, 820.0_f32, 0.065_f32),
        Cue::Stop => (520.0, 390.0, 0.075),
        Cue::Cancel => (230.0, 230.0, DURATION),
    };
    let count = (SAMPLE_RATE as f32 * DURATION) as usize;
    let mut samples = Vec::with_capacity(count);
    let mut phase = 0.0_f32;
    for index in 0..count {
        let time = index as f32 / SAMPLE_RATE as f32;
        let frequency = if time < ramp {
            start_hz * (end_hz / start_hz).powf(time / ramp)
        } else {
            end_hz
        };
        let gain = if time < ATTACK {
            FLOOR * (PEAK / FLOOR).powf(time / ATTACK)
        } else if time < RELEASE_END {
            PEAK * (FLOOR / PEAK).powf((time - ATTACK) / (RELEASE_END - ATTACK))
        } else {
            0.0
        };
        phase += std::f32::consts::TAU * frequency / SAMPLE_RATE as f32;
        samples.push(phase.sin() * gain);
    }
    SamplesBuffer::new(
        std::num::NonZero::new(1).expect("one channel is non-zero"),
        std::num::NonZero::new(SAMPLE_RATE).expect("the sample rate is non-zero"),
        samples,
    )
}

#[cfg(not(target_os = "windows"))]
fn decode(bytes: &'static [u8]) -> Result<SamplesBuffer> {
    let decoder = Decoder::new(Cursor::new(bytes)).wrap_err("could not decode feedback audio")?;
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    Ok(SamplesBuffer::new(
        channels,
        sample_rate,
        decoder.collect::<Vec<_>>(),
    ))
}
