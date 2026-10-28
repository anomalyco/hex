//! Fixtures are written in-test with `AVAssetWriter` and `ffmpeg`; checks use `ffmpeg`/`ffprobe`
//! from `PATH`.

use std::process::Command;
use std::time::Duration;

use super::*;
use crate::av_writer::AvWriter;
use crate::av_writer::fixtures::{bgra, cm_time, solid_bgra, tone};

const WIDTH: usize = 64;
const HEIGHT: usize = 48;
const AUDIO_RATE: u64 = 48_000;

struct TempMp4 {
    _dir: crate::test_dir::TempDir,
    path: PathBuf,
}

impl TempMp4 {
    fn new() -> Self {
        let dir = crate::test_dir::tempdir().expect("temp dir");
        let path = dir.path().join("recording.mp4");
        Self { _dir: dir, path }
    }
}

#[test]
fn holds_avfoundation_still_screen_video_until_audio_ends() {
    let file = TempMp4::new();
    write_still_recording(&file.path, Some(time(5, 1)));
    let before = probe(&file.path).expect("probe fixture");
    assert_eq!(
        video_end(&file.path),
        time(1, 15),
        "fixture reproduces the short video"
    );
    assert!(frames_resampled_at_10_fps(&file.path) <= 2);
    assert!(!decodes_frame_at(&file.path, Duration::from_millis(4_500)));
    let audio_before = audio_packets(&file.path);
    let brand_before = major_brand(&file.path);

    let hold = hold_final_video_frame(&file.path, time(5, 1)).expect("hold final frame");

    assert_eq!(
        hold,
        FinalFrameHold::Held {
            video_end: time(1, 15),
            held_end: time(5, 1)
        }
    );
    assert_held(&file.path, 5.0);
    assert_eq!(
        probe(&file.path).expect("probe held").audio(),
        before.audio()
    );
    assert_eq!(audio_packets(&file.path), audio_before);
    assert_eq!(major_brand(&file.path), brand_before);
    assert_no_staging_left(&file.path);
}

#[test]
fn holds_leading_movie_header_without_moving_samples() {
    let file = TempMp4::new();
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=64x48:rate=30:duration=0.05",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=5",
        "-c:v",
        "libx264",
        "-bf",
        "0",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-movflags",
        "+faststart",
        file.path.to_str().expect("UTF-8 path"),
    ]);
    let audio_before = audio_packets(&file.path);

    let hold = hold_final_video_frame(&file.path, time(5, 1)).expect("hold final frame");

    assert_eq!(hold.end(), time(5, 1));
    assert_held(&file.path, 5.0);
    assert_eq!(audio_packets(&file.path), audio_before);
}

#[test]
fn holds_video_only_recording_until_recorded_end() {
    let file = TempMp4::new();
    write_still_recording(&file.path, None);

    let hold = hold_final_video_frame(&file.path, time(3, 1)).expect("hold final frame");

    assert_eq!(hold.end(), time(3, 1));
    assert_eq!(video_end(&file.path), time(3, 1));
    assert!(decodes_frame_at(&file.path, Duration::from_millis(2_900)));
}

#[test]
fn leaves_video_that_already_covers_audio_untouched() {
    let file = TempMp4::new();
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=64x48:rate=30:duration=5",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=3",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        file.path.to_str().expect("UTF-8 path"),
    ]);
    let bytes = fs::read(&file.path).expect("read fixture");

    let hold = hold_final_video_frame(&file.path, time(5, 1)).expect("inspect covered video");

    assert!(matches!(hold, FinalFrameHold::Covered { .. }), "{hold:?}");
    assert_eq!(fs::read(&file.path).expect("read fixture"), bytes);
    assert_no_staging_left(&file.path);
}

#[test]
fn holds_reordered_cabac_video_with_a_skipped_picture() {
    let file = TempMp4::new();
    write_reordered_x264(&file.path, &[]);
    assert_holds_reordered(&file.path);
}

#[test]
fn holds_reordered_cavlc_video_with_a_skipped_picture() {
    let file = TempMp4::new();
    write_reordered_x264(&file.path, &["-coder", "0"]);
    assert_holds_reordered(&file.path);
}

#[test]
fn holds_reordered_video_with_a_leading_movie_header() {
    let file = TempMp4::new();
    write_reordered_x264(&file.path, &["-movflags", "+faststart"]);
    assert_holds_reordered(&file.path);
}

#[test]
fn holds_reordered_avfoundation_video_with_a_skipped_picture() {
    let file = TempMp4::new();
    let writer = AvWriter::mp4_reordered(&file.path, WIDTH, HEIGHT, true);
    for frame in 0..20_u8 {
        let image = bgra(WIDTH, HEIGHT, |x, y| {
            let x = u8::try_from(x).expect("x");
            let y = u8::try_from(y).expect("y");
            [
                x.wrapping_mul(4).wrapping_add(frame.wrapping_mul(9)),
                y.wrapping_mul(5).wrapping_sub(frame.wrapping_mul(3)),
                (x ^ y).wrapping_mul(frame | 1),
                255,
            ]
        });
        writer.append_frame(&image, time(i64::from(frame), 30));
    }
    let total = 5 * AUDIO_RATE;
    let mut start = 0;
    while start < total {
        let frames = (total - start).min(4_800);
        writer.append_pcm(&tone(start, frames), start);
        start += frames;
    }
    writer
        .finish(Some(cm_time(time(5, 1))), Duration::from_secs(30))
        .expect("finish writer");
    assert_holds_reordered(&file.path);
}

#[test]
fn refuses_reordered_hevc_without_changing_it() {
    let file = TempMp4::new();
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=64x48:rate=30:duration=1",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=5",
        "-c:v",
        "libx265",
        "-x265-params",
        "bframes=2:log-level=error",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        file.path.to_str().expect("UTF-8 path"),
    ]);
    assert!(!final_packet_is_presented_last(&file.path));
    let bytes = fs::read(&file.path).expect("read fixture");

    let refused = hold_final_video_frame(&file.path, time(5, 1));

    assert!(
        matches!(refused, Err(FinalFrameHoldError::Unsupported(_))),
        "{refused:?}"
    );
    assert_eq!(fs::read(&file.path).expect("read fixture"), bytes);
    assert_no_staging_left(&file.path);
}

#[test]
fn refuses_reordered_interlaced_video_without_changing_it() {
    let file = TempMp4::new();
    write_reordered_x264(&file.path, &["-flags", "+ildct+ilme"]);
    let bytes = fs::read(&file.path).expect("read fixture");

    let refused = hold_final_video_frame(&file.path, time(5, 1));

    assert!(
        matches!(
            refused,
            Err(FinalFrameHoldError::Unsupported("interlaced video"))
        ),
        "{refused:?}"
    );
    assert_eq!(fs::read(&file.path).expect("read fixture"), bytes);
    assert_no_staging_left(&file.path);
}

/// A second of moving `libx264` video with B-frames (decoded out of presentation order) and five
/// seconds of audio.
fn write_reordered_x264(path: &Path, extra: &[&str]) {
    let mut arguments = vec![
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=64x48:rate=30:duration=1",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=5",
        "-c:v",
        "libx264",
        "-bf",
        "2",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
    ];
    arguments.extend_from_slice(extra);
    arguments.push(path.to_str().expect("UTF-8 path"));
    ffmpeg(&arguments);
}

/// Holds a reordered recording and checks the tail repeats the final frame, pixel for pixel,
/// while every original frame decodes unchanged.
fn assert_holds_reordered(path: &Path) {
    assert!(
        !final_packet_is_presented_last(path),
        "fixture decodes its final frame before another"
    );
    let frames_before = frame_hashes(path);
    let final_frame = ffmpeg_frames(path, &["-fps_mode", "passthrough"])
        .pop()
        .expect("final frame");
    let final_time = last_frame_time(path);
    let final_image = avfoundation_image(path, final_time);
    let audio_before = audio_packets(path);
    let before = probe(path).expect("probe fixture");

    let hold = hold_final_video_frame(path, time(5, 1)).expect("hold final frame");

    assert_eq!(hold.end(), time(5, 1));
    assert_held(path, 5.0);
    assert_eq!(audio_packets(path), audio_before);
    assert_eq!(probe(path).expect("probe held").audio(), before.audio());
    assert_no_staging_left(path);

    let frames_after = frame_hashes(path);
    assert_eq!(frames_after.len(), frames_before.len() + 1);
    assert_eq!(frames_after[..frames_before.len()], frames_before[..]);
    // Resampled at 10 fps, the last frame is the one shown at 4.9 s.
    assert!(
        ffmpeg_frames(path, &["-vf", "fps=10"]).last() == Some(&final_frame),
        "ffmpeg shows the final frame at the end"
    );
    assert!(
        avfoundation_image(path, Duration::from_millis(4_900)) == final_image,
        "AVFoundation shows the final frame at the end"
    );
}

fn final_packet_is_presented_last(path: &Path) -> bool {
    let times: Vec<i64> = ffprobe(
        path,
        &[
            "-select_streams",
            "v:0",
            "-show_entries",
            "packet=pts",
            "-of",
            "csv=p=0",
        ],
    )
    .lines()
    .map(|line| line.trim().parse().expect("packet pts"))
    .collect();
    times.last() == times.iter().max()
}

/// A time a few milliseconds into the final video frame, clear of `ffprobe`'s rounding.
fn last_frame_time(path: &Path) -> Duration {
    let times = ffprobe(
        path,
        &[
            "-select_streams",
            "v:0",
            "-show_entries",
            "frame=pts_time",
            "-of",
            "csv=p=0",
        ],
    );
    let seconds: f64 = times
        .lines()
        .last()
        .expect("frames")
        .trim()
        .parse()
        .expect("frame time");
    Duration::from_secs_f64(seconds) + Duration::from_millis(5)
}

/// The MD5 of each decoded video frame, in presentation order.
fn frame_hashes(path: &Path) -> Vec<String> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-map",
            "0:v",
            "-fps_mode",
            "passthrough",
            "-f",
            "framemd5",
            "-",
        ])
        .output()
        .expect("run ffmpeg");
    assert!(output.status.success(), "framemd5: {output:?}");
    String::from_utf8(output.stdout)
        .expect("UTF-8 framemd5")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.rsplit(',').next().expect("hash").trim().to_owned())
        .collect()
}

/// Decoded video frames as raw YUV, from `ffmpeg` with `options` choosing which frames.
fn ffmpeg_frames(path: &Path, options: &[&str]) -> Vec<Vec<u8>> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:v"])
        .args(options)
        .args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
        .output()
        .expect("run ffmpeg");
    assert!(output.status.success(), "decode: {output:?}");
    output
        .stdout
        .chunks(WIDTH * HEIGHT * 3 / 2)
        .map(<[u8]>::to_vec)
        .collect()
}

/// The pixels `AVFoundation` shows exactly at `at`.
fn avfoundation_image(path: &Path, at: Duration) -> Vec<u8> {
    use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
    use objc2_core_graphics::{CGDataProvider, CGImage};
    use objc2_foundation::{NSString, NSURL};

    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().expect("UTF-8 path")));
    let micros = i64::try_from(at.as_micros()).expect("micros");
    // SAFETY: A local file asset and a valid, positive-timescale time.
    let image = unsafe {
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
        generator.setRequestedTimeToleranceBefore(objc2_core_media::kCMTimeZero);
        generator.setRequestedTimeToleranceAfter(objc2_core_media::kCMTimeZero);
        let mut actual = objc2_core_media::kCMTimeZero;
        #[allow(deprecated)]
        generator
            .copyCGImageAtTime_actualTime_error(
                objc2_core_media::CMTime::new(micros, 1_000_000),
                &raw mut actual,
            )
            .expect("AVFoundation image")
    };
    let data =
        CGDataProvider::data(CGImage::data_provider(Some(&image)).as_deref()).expect("image data");
    data.to_vec()
}

fn assert_held(path: &Path, seconds: f64) {
    let probed = probe(path).expect("probe held file");
    assert_eq!(
        probed.video_end(),
        probed.audio().and_then(crate::probe::Track::end),
        "AVFoundation video ends with audio"
    );
    assert!(
        decodes_frame_at(path, Duration::from_millis(4_500)),
        "a frame decodes at 4.5 s"
    );

    let duration: f64 = ffprobe(
        path,
        &[
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=duration",
            "-of",
            "csv=p=0",
        ],
    )
    .trim()
    .parse()
    .expect("ffprobe video duration");
    assert!(
        (duration - seconds).abs() < 0.01,
        "ffprobe video duration {duration}"
    );
    let format = ffprobe(
        path,
        &["-show_entries", "format=format_name", "-of", "csv=p=0"],
    );
    assert!(format.contains("mp4"), "{format}");

    let full = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .output()
        .expect("run ffmpeg");
    assert!(
        full.status.success() && full.stderr.is_empty(),
        "full decode: {full:?}"
    );
    let frames = frames_resampled_at_10_fps(path);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let expected = (seconds * 10.0).round() as usize;
    assert!(
        frames.abs_diff(expected) <= 1,
        "ffmpeg resampled {frames} frames, expected {expected}"
    );
}

/// Whether `ffmpeg` decodes a video frame at `at` (seeking in the output, so a short video track
/// yields nothing).
/// Whether `AVFoundation` decodes a frame exactly at `at`, as players and editors read it.
fn decodes_frame_at(path: &Path, at: Duration) -> bool {
    use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
    use objc2_foundation::{NSString, NSURL};

    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().expect("UTF-8 path")));
    let millis = i64::try_from(at.as_millis()).expect("millis");
    // SAFETY: A local file asset and a valid, positive-timescale time.
    unsafe {
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
        generator.setRequestedTimeToleranceBefore(objc2_core_media::kCMTimeZero);
        generator.setRequestedTimeToleranceAfter(objc2_core_media::kCMTimeZero);
        let mut actual = objc2_core_media::kCMTimeZero;
        #[allow(deprecated)]
        let image = generator.copyCGImageAtTime_actualTime_error(
            objc2_core_media::CMTime::new(millis, 1_000),
            &raw mut actual,
        );
        image.is_ok()
    }
}

/// A constant-rate resample repeats each frame for its stored duration, so a held final frame
/// fills the whole tail.
fn frames_resampled_at_10_fps(path: &Path) -> usize {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-map", "0:v", "-vf", "fps=10", "-f", "rawvideo", "-pix_fmt", "gray", "-",
        ])
        .output()
        .expect("run ffmpeg");
    assert!(output.status.success(), "resample: {output:?}");
    output.stdout.len() / (WIDTH * HEIGHT)
}

fn assert_no_staging_left(path: &Path) {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("name");
    let leftovers = fs::read_dir(path.parent().expect("parent"))
        .expect("read temp dir")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|entry| entry.starts_with(&format!(".{name}.hold-")))
        })
        .count();
    assert_eq!(leftovers, 0);
}

/// Writes two frames plus optional AAC through `AVAssetWriter`, matching a still-screen capture
/// whose video track ends at the final delivered frame.
fn write_still_recording(path: &Path, audio_end: Option<MediaTime>) {
    let writer = AvWriter::mp4(path, WIDTH, HEIGHT, audio_end.is_some());
    writer.append_frame(
        &solid_bgra(WIDTH, HEIGHT, [30, 60, 200, 255]),
        MediaTime::ZERO,
    );
    writer.append_frame(&solid_bgra(WIDTH, HEIGHT, [200, 60, 30, 255]), time(1, 30));
    let end = if let Some(end) = audio_end {
        let total = u64::try_from(end.value()).expect("end") * AUDIO_RATE
            / u64::try_from(end.timescale()).expect("timescale");
        let mut start = 0;
        while start < total {
            let frames = (total - start).min(4_800);
            writer.append_pcm(&tone(start, frames), start);
            start += frames;
        }
        end
    } else {
        time(1, 15)
    };
    writer
        .finish(Some(cm_time(end)), Duration::from_secs(30))
        .expect("finish writer");
}

fn video_end(path: &Path) -> MediaTime {
    probe(path).expect("probe").video_end().expect("video end")
}

fn audio_packets(path: &Path) -> Vec<u8> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a", "-c", "copy", "-f", "data", "-"])
        .output()
        .expect("run ffmpeg");
    assert!(output.status.success(), "ffmpeg failed: {output:?}");
    assert!(!output.stdout.is_empty());
    output.stdout
}

fn major_brand(path: &Path) -> Vec<u8> {
    fs::read(path).expect("read MP4")[4..12].to_vec()
}

fn ffmpeg(arguments: &[&str]) {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(arguments)
        .output()
        .expect("run ffmpeg");
    assert!(output.status.success(), "ffmpeg failed: {output:?}");
}

fn ffprobe(path: &Path, arguments: &[&str]) -> String {
    let output = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(arguments)
        .arg(path)
        .output()
        .expect("run ffprobe");
    assert!(output.status.success(), "ffprobe failed: {output:?}");
    String::from_utf8(output.stdout).expect("UTF-8 ffprobe output")
}

fn time(value: i64, timescale: i32) -> MediaTime {
    MediaTime::new(value, timescale).expect("valid media time")
}
