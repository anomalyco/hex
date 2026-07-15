use std::io::Cursor;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, DeviceSinkBuilder, Source};

#[derive(Clone, Copy)]
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

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
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
                Tone::Wake | Tone::Sleep | Tone::Error => continue,
            };
            output.mixer().add(sound.clone().amplify(0.45));
        }
    });
    ready_receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| eyre!("timed out preloading feedback audio"))??;
    let _ = DICTATION_PLAYER.set(sender);
    Ok(())
}

pub fn play(tone: Tone) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if matches!(
        tone,
        Tone::DictationStart | Tone::DictationStop | Tone::Cancel
    ) {
        if let Some(player) = DICTATION_PLAYER.get() {
            let _ = player.try_send(tone);
        }
        return;
    }

    let sound = match tone {
        Tone::Wake => "Pop",
        Tone::Sleep => "Tink",
        Tone::Error => "Basso",
        Tone::DictationStart | Tone::DictationStop | Tone::Cancel => return,
    };
    let child = Command::new("/usr/bin/afplay")
        .args([
            "-v",
            "0.45",
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
