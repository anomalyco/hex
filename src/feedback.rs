use std::io::Cursor;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, DeviceSinkBuilder, Source};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tone {
    #[cfg(target_os = "macos")]
    Wake,
    #[cfg(target_os = "macos")]
    Sleep,
    #[cfg(target_os = "macos")]
    Error,
    DictationStart,
    DictationStop,
    Cancel,
}

static DICTATION_PLAYER: OnceLock<SyncSender<Tone>> = OnceLock::new();
static LOADER_STARTED: OnceLock<()> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(true);
static VOLUME: AtomicU32 = AtomicU32::new(0.5_f32.to_bits());

#[cfg(target_os = "macos")]
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_volume(volume: f32) {
    VOLUME.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

fn volume() -> f32 {
    f32::from_bits(VOLUME.load(Ordering::Relaxed))
}

/// Maps the admission wait outcome to an error. A timeout does not stop the
/// loader thread: the audio stack may simply be cold, and the loader keeps
/// running so tones recover once the default output finally opens.
fn admission_error(outcome: Result<Result<()>, mpsc::RecvTimeoutError>) -> Result<()> {
    match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(eyre!("timed out preloading feedback audio")),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(eyre!("feedback audio loader exited before initializing"))
        }
    }
}

pub fn preload() -> Result<()> {
    if DICTATION_PLAYER.get().is_some() {
        return Ok(());
    }
    if LOADER_STARTED.set(()).is_err() {
        // A loader is already running from an earlier admission; it registers
        // the player itself, so report the same slow-audio outcome.
        return Err(eyre!("timed out preloading feedback audio"));
    }
    let (sender, receiver) = mpsc::sync_channel(8);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    // The loader thread owns its clone; the admission path registers the same
    // sender only if it observed readiness within the timeout below.
    let loader_sender = sender.clone();
    thread::spawn(move || {
        let result = (|| -> Result<_> {
            let output = DeviceSinkBuilder::open_default_sink()
                .wrap_err("could not open the audio output")?;
            let start = decode(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/audio/startRecording.mp3"
            )))?;
            let stop = decode(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/audio/stopRecording.mp3"
            )))?;
            let cancel = decode(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/audio/cancel.mp3"
            )))?;
            Ok((output, start, stop, cancel))
        })();
        let Ok((output, start, stop, cancel)) = result else {
            let _ = ready_sender.send(result.map(|_| ()));
            return;
        };
        let _ = ready_sender.send(Ok(()));
        // Register late so a cold audio stack that merely needs more than the
        // admission timeout still serves every later tone in this session.
        let _ = DICTATION_PLAYER.set(loader_sender);
        while let Ok(tone) = receiver.recv() {
            let sound = match tone {
                Tone::DictationStart => &start,
                Tone::DictationStop => &stop,
                Tone::Cancel => &cancel,
                #[cfg(target_os = "macos")]
                Tone::Wake | Tone::Sleep | Tone::Error => continue,
            };
            output.mixer().add(sound.clone().amplify(volume()));
        }
    });
    let outcome = ready_receiver.recv_timeout(Duration::from_secs(2));
    admission_error(outcome)?;
    let _ = DICTATION_PLAYER.set(sender);
    Ok(())
}

pub fn play(tone: Tone) {
    if !ENABLED.load(Ordering::Relaxed) || volume() <= 0.0 {
        return;
    }
    match tone {
        Tone::DictationStart | Tone::DictationStop | Tone::Cancel => {
            enqueue(DICTATION_PLAYER.get(), tone);
        }
        #[cfg(target_os = "macos")]
        Tone::Wake | Tone::Sleep | Tone::Error => play_system_sound(tone),
    }
}

fn enqueue(player: Option<&SyncSender<Tone>>, tone: Tone) {
    if let Some(player) = player {
        let _ = player.try_send(tone);
    }
}

#[cfg(target_os = "macos")]
fn play_system_sound(tone: Tone) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_recording_sounds_decode_without_an_audio_device() {
        for bytes in [
            include_bytes!("../resources/audio/startRecording.mp3").as_slice(),
            include_bytes!("../resources/audio/stopRecording.mp3").as_slice(),
            include_bytes!("../resources/audio/cancel.mp3").as_slice(),
        ] {
            let mut sound = decode(bytes).unwrap();
            assert!(sound.total_duration().unwrap() > Duration::ZERO);
            assert!(sound.any(|sample| sample.abs() > 0.0));
        }
    }

    #[test]
    fn feedback_admission_never_waits_for_playback() {
        let (sender, receiver) = mpsc::sync_channel(2);
        enqueue(Some(&sender), Tone::DictationStart);
        enqueue(Some(&sender), Tone::DictationStop);
        enqueue(Some(&sender), Tone::Cancel);
        assert_eq!(receiver.try_recv().unwrap(), Tone::DictationStart);
        assert_eq!(receiver.try_recv().unwrap(), Tone::DictationStop);
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        enqueue(Some(&sender), Tone::Cancel);
        enqueue(None, Tone::DictationStart);
    }

    #[test]
    fn timeout_and_loader_exit_report_distinct_errors() {
        // A silent channel models a loader still warming the audio stack; a
        // dropped one models a loader that exited before loading.
        assert_eq!(
            admission_error(Err(mpsc::RecvTimeoutError::Timeout))
                .unwrap_err()
                .to_string(),
            "timed out preloading feedback audio"
        );
        assert_eq!(
            admission_error(Err(mpsc::RecvTimeoutError::Disconnected))
                .unwrap_err()
                .to_string(),
            "feedback audio loader exited before initializing"
        );

        // Real channels exercise the same admission wait the loader drives.
        let (slow_sender, slow_receiver) = mpsc::sync_channel::<Result<()>>(1);
        std::mem::forget(slow_sender);
        assert!(admission_error(slow_receiver.recv_timeout(Duration::ZERO)).is_err());

        let (done_sender, done_receiver) = mpsc::sync_channel::<Result<()>>(1);
        done_sender.send(Ok(())).unwrap();
        assert!(admission_error(done_receiver.recv_timeout(Duration::ZERO)).is_ok());

        let (failed_sender, failed_receiver) = mpsc::sync_channel::<Result<()>>(1);
        failed_sender
            .send(Err(eyre!("could not open the audio output")))
            .unwrap();
        assert_eq!(
            admission_error(failed_receiver.recv_timeout(Duration::ZERO))
                .unwrap_err()
                .to_string(),
            "could not open the audio output"
        );
    }
}
