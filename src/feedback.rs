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

pub fn preload() -> Result<()> {
    if DICTATION_PLAYER.get().is_some() {
        return Ok(());
    }
    let (sender, receiver) = mpsc::sync_channel(8);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
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
    ready_receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| eyre!("timed out preloading feedback audio"))??;
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
}
