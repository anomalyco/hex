//! `HEX_AUDIO_FIXTURE`: a WAV file that stands in for every microphone, so
//! benchmarks and tests drive the real capture, stream, and output pipeline
//! without recording anyone. The file plays at real-time pace in 10 ms
//! chunks, from its start at each open, and loops. Each chunk is stamped
//! with when its last sample was due on the clock the shortcuts use
//! ([`CaptureInstant::now`]), as a device stamps its samples' own time: a
//! late wake on a busy machine delivers the chunk late but never moves it.
//! Device enumeration is skipped: the fixture is the only input.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};

use super::{AudioInput, CaptureInstant, STREAM_STALL_TIMEOUT, Selection};

/// What the fixture input is called in the log and Settings.
pub(super) const NAME: &str = "Audio fixture";

/// Chunks per second, as a device's 10 ms period delivers them.
const CHUNKS_PER_SECOND: u32 = 100;

/// The fixture's path, read once: `HEX_AUDIO_FIXTURE`.
pub(super) fn path() -> Option<&'static Path> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::var_os("HEX_AUDIO_FIXTURE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    })
    .as_deref()
}

/// The fixture's mono samples and sample rate, decoded once.
fn clip(path: &Path) -> Result<(Arc<Vec<f32>>, u32)> {
    type Decoded = (PathBuf, Arc<Vec<f32>>, u32);
    static CLIP: Mutex<Option<Decoded>> = Mutex::new(None);
    let mut cached = CLIP.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((_, samples, rate)) = cached.as_ref().filter(|(at, ..)| at == path) {
        return Ok((samples.clone(), *rate));
    }
    let (samples, rate) = read(path)?;
    let samples = Arc::new(samples);
    *cached = Some((path.to_path_buf(), samples.clone(), rate));
    Ok((samples, rate))
}

fn read(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path)
        .wrap_err_with(|| format!("could not open the audio fixture {}", path.display()))?;
    let spec = reader.spec();
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1_i64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|sample| sample as f32 / scale))
                .collect::<std::result::Result<Vec<_>, _>>()?
        }
    };
    let channels = usize::from(spec.channels.max(1));
    let mono = super::mono(&samples, channels, |sample| *sample);
    if mono.len() < (spec.sample_rate / CHUNKS_PER_SECOND) as usize {
        return Err(eyre!("the audio fixture {} holds no audio", path.display()));
    }
    Ok((mono, spec.sample_rate))
}

/// Opens the fixture as a microphone. Dropping the input stops its thread.
pub(super) fn open(path: &Path) -> Result<AudioInput> {
    let (samples, sample_rate) = clip(path)?;
    let (sender, chunks) = mpsc::channel();
    let (_, stream_errors) = mpsc::sync_channel(1);
    let frames = (sample_rate / CHUNKS_PER_SECOND) as usize;
    let period = Duration::from_secs(1) / CHUNKS_PER_SECOND;
    thread::Builder::new()
        .name("audio-fixture".into())
        .spawn(move || {
            let started = Instant::now();
            let opened = CaptureInstant::now();
            let mut at = 0;
            for chunk in 1_u32.. {
                // Paced against the start, so a late wake never drifts.
                let due = started + period * chunk;
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    thread::sleep(wait);
                }
                let mut out = Vec::with_capacity(frames);
                while out.len() < frames {
                    let take = (frames - out.len()).min(samples.len() - at);
                    out.extend_from_slice(&samples[at..at + take]);
                    at = (at + take) % samples.len();
                }
                if sender.send((out, opened + period * chunk)).is_err() {
                    return;
                }
            }
        })
        .wrap_err("could not start the audio fixture")?;
    tracing::warn!(
        path = %path.display(),
        sample_rate,
        "HEX_AUDIO_FIXTURE replaces the microphone"
    );
    Ok(AudioInput {
        _stream: None,
        chunks,
        sample_rate,
        device_name: NAME.into(),
        uid: None,
        selection: Selection::InUse,
        stream_errors,
        watch_service_restarts: false,
        restart_generation: super::audio_service_restart_generation(),
        hex_microphone: crate::audio::hex_microphone::capture_feed(sample_rate).1,
        stall_after: Some(STREAM_STALL_TIMEOUT),
        last_delivery: Mutex::new(Instant::now()),
        reported_period: AtomicBool::new(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixture_plays_its_file_from_the_start_in_ten_ms_chunks() {
        let directory = std::env::temp_dir().join(format!("hex-fixture-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("fixture.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for frame in 0..120_i16 {
            writer.write_sample(frame * 100).unwrap();
            writer.write_sample(frame * 100).unwrap();
        }
        writer.finalize().unwrap();

        let (samples, rate) = read(&path).unwrap();
        assert_eq!(rate, 8_000);
        assert_eq!(samples.len(), 120);

        let input = open(&path).unwrap();
        let mut heard = Vec::new();
        let mut last = CaptureInstant::ZERO;
        while heard.len() < 160 {
            let super::super::AudioInputEvent::Chunk {
                samples,
                captured_through,
            } = input.recv_timeout(Duration::from_secs(1))
            else {
                panic!("the fixture stopped delivering");
            };
            assert_eq!(samples.len(), 80);
            assert!(captured_through > last);
            last = captured_through;
            heard.extend(samples);
        }
        // It loops: the 121st sample is the first again.
        assert_eq!(heard[0], heard[120]);
        assert!((heard[1] - 100.0 / 32_768.0).abs() < 1e-6);
        drop(input);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
