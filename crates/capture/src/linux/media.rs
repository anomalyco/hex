//! Movies on Linux through the `ffmpeg` and `ffprobe` executables: muxing the hardware
//! encoder's H.264 into `recording.mp4`, the software encoder (x264) when the GPU can't encode,
//! and reading frames, sizes and audio back for the pipeline. Nothing here links FFmpeg.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

use anyhow::{Context, Result, bail, ensure};

use crate::helper::{FrameTimes, Probe};
use crate::model::RgbaFrame;

/// The recorder's frame rate: a constant-rate movie, the newest screen frame repeated.
pub const FPS: u32 = 30;

fn ffmpeg() -> Command {
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-nostats", "-loglevel", "error"]);
    command
}

fn spawn(mut command: Command) -> Result<Child> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Linux captures need ffmpeg (install the ffmpeg package)")
}

/// A running `ffmpeg` that writes a movie from what its stdin receives.
pub struct Writer {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Writer {
    /// Muxes an H.264 Annex B stream at [`FPS`] into `movie`, without re-encoding.
    pub fn h264(movie: &Path) -> Result<Self> {
        let mut command = ffmpeg();
        command
            .args(["-f", "h264", "-framerate", &FPS.to_string(), "-i", "pipe:0"])
            .args(["-c", "copy", "-movflags", "+faststart", "-y"])
            .arg(movie);
        Self::start(command)
    }

    /// Encodes raw 32-bit frames (`bgr0`, `rgb0`, …) of `size` with x264, scaled to `out`.
    pub fn x264(movie: &Path, pixel: &str, size: (u32, u32), out: (u32, u32)) -> Result<Self> {
        let mut command = ffmpeg();
        command
            .args(["-f", "rawvideo", "-pix_fmt", pixel])
            .args(["-video_size", &format!("{}x{}", size.0, size.1)])
            .args(["-framerate", &FPS.to_string(), "-i", "pipe:0"])
            .args([
                "-vf",
                &format!("scale={}:{}:flags=area,format=yuv420p", out.0, out.1),
            ])
            .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "24"])
            .args(["-movflags", "+faststart", "-y"])
            .arg(movie);
        Self::start(command)
    }

    fn start(command: Command) -> Result<Self> {
        let mut child = spawn(command)?;
        let stdin = child.stdin.take();
        Ok(Self { child, stdin })
    }

    /// Closes the input and waits for the movie to be written.
    pub fn finish(mut self) -> Result<()> {
        drop(self.stdin.take());
        let mut errors = String::new();
        if let Some(mut stderr) = self.child.stderr.take() {
            let _ = stderr.read_to_string(&mut errors);
        }
        let status = self.child.wait()?;
        ensure!(status.success(), "ffmpeg failed: {}", errors.trim());
        Ok(())
    }
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match &mut self.stdin {
            Some(stdin) => stdin.write(bytes),
            None => Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stdin.as_mut().map_or(Ok(()), Write::flush)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn output(command: &mut Command) -> Result<Vec<u8>> {
    let out = command
        .stdin(Stdio::null())
        .output()
        .context("Linux captures need ffmpeg (install the ffmpeg package)")?;
    ensure!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(out.stdout)
}

pub fn probe(movie: &Path) -> Result<Probe> {
    #[derive(serde::Deserialize)]
    struct Stream {
        codec_type: String,
        #[serde(default)]
        width: u32,
        #[serde(default)]
        height: u32,
        #[serde(default)]
        avg_frame_rate: String,
    }
    #[derive(serde::Deserialize)]
    struct Format {
        #[serde(default)]
        duration: String,
    }
    #[derive(serde::Deserialize)]
    struct Probed {
        streams: Vec<Stream>,
        format: Format,
    }
    let json = output(
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_streams",
                "-show_format",
            ])
            .arg(movie),
    )?;
    let probed: Probed = serde_json::from_slice(&json)?;
    let video = probed.streams.iter().find(|s| s.codec_type == "video");
    let fps = video
        .and_then(|v| v.avg_frame_rate.split_once('/'))
        .and_then(|(n, d)| {
            Some(n.parse::<f64>().ok()? / d.parse::<f64>().ok().filter(|d| *d > 0.0)?)
        })
        .unwrap_or(0.0);
    Ok(Probe {
        duration_ms: (probed.format.duration.parse::<f64>().unwrap_or(0.0) * 1000.0) as u64,
        width: video.map_or(0, |v| v.width),
        height: video.map_or(0, |v| v.height),
        audio_tracks: probed
            .streams
            .iter()
            .filter(|s| s.codec_type == "audio")
            .count() as u32,
        fps,
    })
}

/// The size `max_edge` gives `(width, height)` (0 keeps it), even on both sides.
fn fit(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let long = width.max(height).max(1);
    if max_edge == 0 || long <= max_edge {
        return (width & !1, height & !1);
    }
    let scale = f64::from(max_edge) / f64::from(long);
    (
        ((f64::from(width) * scale).round() as u32).max(2) & !1,
        ((f64::from(height) * scale).round() as u32).max(2) & !1,
    )
}

fn frame_at(movie: &Path, ms: u64, size: (u32, u32)) -> Result<RgbaFrame> {
    let rgba = output(
        ffmpeg()
            .args(["-ss", &format!("{:.3}", ms as f64 / 1000.0), "-i"])
            .arg(movie)
            .args([
                "-frames:v",
                "1",
                "-vf",
                &format!("scale={}:{}", size.0, size.1),
            ])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]),
    )?;
    ensure!(
        rgba.len() == size.0 as usize * size.1 as usize * 4,
        "no frame at {ms} ms"
    );
    Ok(RgbaFrame::new(ms, size.0, size.1, rgba))
}

pub fn frames(movie: &Path, times: FrameTimes, max_edge: u32) -> Result<Vec<RgbaFrame>> {
    let probed = probe(movie)?;
    ensure!(
        probed.width > 0 && probed.height > 0,
        "the movie has no video"
    );
    let size = fit(probed.width, probed.height, max_edge);
    let times: Vec<u64> = match times {
        FrameTimes::At(times) => times.to_vec(),
        FrameTimes::Every(ms) => (0..probed.duration_ms)
            .step_by(ms.max(1) as usize)
            .collect(),
    };
    std::thread::scope(|scope| {
        let workers: Vec<_> = times
            .chunks(times.len().div_ceil(4).max(1))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|&ms| {
                            frame_at(movie, ms.min(probed.duration_ms.saturating_sub(1)), size)
                        })
                        .collect::<Result<Vec<_>>>()
                })
            })
            .collect();
        let mut out = Vec::with_capacity(times.len());
        for worker in workers {
            match worker.join() {
                Ok(frames) => out.extend(frames?),
                Err(_) => bail!("a frame reader panicked"),
            }
        }
        for (frame, &ms) in out.iter_mut().zip(&times) {
            frame.t_ms = ms;
        }
        Ok(out)
    })
}

pub fn shrink(movie: &Path, out: &Path, max_edge: u32) -> Result<crate::retention::Shrunk> {
    let before = std::fs::metadata(movie)?.len();
    let probed = probe(movie)?;
    let (width, height) = fit(probed.width, probed.height, max_edge);
    output(
        ffmpeg()
            .arg("-i")
            .arg(movie)
            .args(["-vf", &format!("scale={width}:{height}"), "-c:v", "libx264"])
            .args(["-preset", "veryfast", "-crf", "26", "-c:a", "copy", "-y"])
            .arg(out),
    )?;
    Ok(crate::retention::Shrunk {
        before,
        after: std::fs::metadata(out)?.len(),
        width,
        height,
        ms: probed.duration_ms,
    })
}

pub fn export_audio(movie: &Path, wav: &Path) -> Result<()> {
    output(
        ffmpeg()
            .arg("-i")
            .arg(movie)
            .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", "-y"])
            .arg(wav),
    )?;
    Ok(())
}
