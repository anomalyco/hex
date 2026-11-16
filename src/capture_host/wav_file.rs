//! A capture's audio: 16 kHz mono PCM16 on the capture's clock, written to
//! `audio.wav.partial` while it records so memory does not grow with it, then
//! given its final `audio.wav` name once its build writes `unfinished.json`.

use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use scrawl_capture::audio::pcm16;

use super::SAMPLE_RATE;

/// Samples written per silence block: 256 ms at 16 kHz, so a gap of hours
/// costs no more memory than a gap of milliseconds.
const SILENCE_BLOCK: usize = 4_096;
/// How often the header is updated while recording, so a crash leaves a WAV
/// that reads up to there instead of one whose header says it holds nothing.
const HEADER_EVERY: Duration = Duration::from_secs(5);

/// `audio.wav.partial`, open for the whole capture.
pub(super) struct WavFile {
    path: PathBuf,
    writer: hound::WavWriter<BufWriter<File>>,
    /// Samples written, leading silence included.
    written: usize,
    flushed: Instant,
}

impl WavFile {
    /// Makes the file owner-only and writes its header; no samples yet.
    pub(super) fn create(path: &Path) -> io::Result<Self> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::new(BufWriter::new(private(path)?), spec).map_err(broken)?;
        Ok(Self {
            path: path.to_path_buf(),
            writer,
            written: 0,
            flushed: Instant::now(),
        })
    }

    pub(super) fn written(&self) -> usize {
        self.written
    }

    pub(super) fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        if samples.is_empty() {
            return Ok(());
        }
        let mut block = self.writer.get_i16_writer(samples.len() as u32);
        for &sample in samples {
            block.write_sample(pcm16(sample));
        }
        block.flush().map_err(broken)?;
        self.written += samples.len();
        self.update_header_if_due()
    }

    /// Appends `samples` of digital silence in bounded blocks.
    pub(super) fn write_silence(&mut self, mut samples: usize) -> io::Result<()> {
        while samples > 0 {
            let block = samples.min(SILENCE_BLOCK);
            let mut writer = self.writer.get_i16_writer(block as u32);
            for _ in 0..block {
                writer.write_sample(0_i16);
            }
            writer.flush().map_err(broken)?;
            self.written += block;
            samples -= block;
        }
        self.update_header_if_due()
    }

    /// Finalises the header, drops everything past `keep` samples, and returns
    /// how many samples the file really holds.
    pub(super) fn finish(self, keep: usize) -> io::Result<usize> {
        let Self {
            path,
            writer,
            written,
            ..
        } = self;
        writer.finalize().map_err(broken)?;
        let keep = keep.min(written);
        if keep < written {
            trim(&path, keep)?;
        }
        Ok(keep)
    }

    /// Writes the header every few seconds, so a crash leaves a WAV that reads
    /// up to there. A failed flush is a write failure: the capture's collector
    /// records the rest as a gap rather than trusting a file it cannot update.
    fn update_header_if_due(&mut self) -> io::Result<()> {
        if self.flushed.elapsed() < HEADER_EVERY {
            return Ok(());
        }
        self.flushed = Instant::now();
        self.writer.flush().map_err(broken)
    }
}

#[cfg(unix)]
fn private(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private(path: &Path) -> io::Result<File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

fn broken(error: hound::Error) -> io::Error {
    io::Error::other(error)
}

/// Drops the file's samples past `keep`, rewriting the RIFF and data lengths.
fn trim(path: &Path, keep: usize) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let data = data_bytes(&mut file)?;
    let len = (keep as u64) * 2;
    if len > u64::from(u32::MAX) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the capture's audio is too long for a WAV file",
        ));
    }
    let end = data + len;
    file.set_len(end)?;
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&(end as u32 - 8).to_le_bytes())?;
    file.seek(SeekFrom::Start(data - 4))?;
    file.write_all(&(len as u32).to_le_bytes())?;
    file.flush()
}

/// Where the `data` chunk's first byte sits, found the way a reader finds it.
fn data_bytes(file: &mut File) -> io::Result<u64> {
    let mut head = [0_u8; 12];
    file.read_exact(&mut head)?;
    if &head[..4] != b"RIFF" || &head[8..] != b"WAVE" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the capture's audio is not a WAV file",
        ));
    }
    let mut at = 12_u64;
    loop {
        let mut chunk = [0_u8; 8];
        file.seek(SeekFrom::Start(at))?;
        file.read_exact(&mut chunk)?;
        let len = u64::from(u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
        if &chunk[..4] == b"data" {
            return Ok(at + 8);
        }
        at += 8 + len + (len & 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hex-capture-wav-{name}-{}.wav", std::process::id()))
    }

    #[test]
    fn a_finished_file_reads_back_and_can_be_trimmed() {
        let path = temp("trim");
        let mut file = WavFile::create(&path).unwrap();
        file.write(&[0.5; 160]).unwrap();
        file.write_silence(320).unwrap();
        file.write(&[0.25; 160]).unwrap();
        assert_eq!(file.written(), 640);
        assert_eq!(file.finish(480).unwrap(), 480);

        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.spec().bits_per_sample, 16);
        let samples = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples.len(), 480);
        assert_eq!(samples[0], pcm16(0.5));
        assert!(samples[160..480].iter().all(|&sample| sample == 0));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_keep_past_the_written_samples_keeps_them_all() {
        let path = temp("keep");
        let mut file = WavFile::create(&path).unwrap();
        file.write(&[0.5; 160]).unwrap();
        assert_eq!(file.finish(16_000).unwrap(), 160);
        assert_eq!(
            hound::WavReader::open(&path).unwrap().duration(),
            160,
            "the file holds what was written, not the requested length"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp("private");
        let file = WavFile::create(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        drop(file);
        let _ = std::fs::remove_file(&path);
        assert_eq!(mode & 0o777, 0o600);
    }
}
