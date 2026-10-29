# capture-core

Screen recording for macOS 15+ on ScreenCaptureKit. A host describes a recording as a `Plan`;
the core records each display, window or display region to its own H.264 MP4, places everything
it writes on one clock (Mach host time in nanoseconds), and describes the result in a versioned
`capture.json` manifest.

The crate compiles only on macOS. In this repository it sits outside the workspace and is built
only through the `capture` feature (`scripts/check` lints and tests it as `-p capture-core`).
Linux, Windows, Nix and release builds never compile it.

## API

```rust
use capture_core::*;

let (input, input_rx) = host_channel(1024, KeyPolicy::Chords);
let (frames, frame_rx) = FrameTap::new(8, Duration::from_millis(500), 640);

let mut recording = Recorder::start(Plan {
    sources: vec![
        SourcePlan { source: Source::Display(display_id), long_edge: Some(2560) },
        SourcePlan { source: Source::Window(window_id), long_edge: Some(1280) },
    ],
    cursor: CursorDrawing::Hidden,
    audio: AudioPlan { mic: Mic::External(ExternalMic::default()), system: false },
    input: Input::Host(input_rx),
    frames: Some(frames),
    exclude: Exclusions { windows: vec![my_overlay_window], apps: vec![] },
    fps: 60,
    dir: bundle_dir,
    names: Names {
        videos: vec!["recording.mp4".into(), "window.mp4".into()],
        microphone: Some("audio.wav".into()),
        cursor: Some("cursor.json".into()),
        input: Some("input.json".into()),
        ..Names::default()
    },
})?;

let mut mic = recording.external_audio().unwrap();       // ExternalAudioSink
mic.offer(&pcm_16k_mono, HostTime::from_nanos(core_audio_ns)); // never blocks
mic.gap(from, through);                                   // the host lost audio
input.send(HostTime::now(), event);                       // never blocks

let finishing = recording.stop();   // returns at once
// ... frame_rx already holds the last frame of each source
let finished = finishing.wait()?;   // or poll with finishing.try_wait()
```

- `Recorder::start(Plan) -> Result<Recording, Error>`: validates the plan, refuses names that
  already exist in `dir`, and starts sources `1..n` before source `0`, which owns audio.
- `Source`: `Display(id)`, `Window(id)`, or `Region { display, rect }`, a rect in the display's
  own points from its top-left corner (the stream's `sourceRect`), trimmed to the display.
- `SourcePlan::long_edge`: the video's longest side in pixels, keeping the aspect and never above
  the source's own pixels. `None` records at full resolution. Both sides are rounded to even.
- `screenshot(&Source, &Exclusions, long_edge) -> Result<Image, Error>`: one RGBA still through
  `SCScreenshotManager`, with the same filter and size a recording of that source would use.
- `Recording`: `stop()` and `cancel()` (which removes every file) return a `Finishing` at once.
  `health()` reports output failures and system stream stops as they happen. `external_audio()`
  hands out the microphone sink once. `click_status()` reports the optional tap. Dropping a
  `Recording` stops it and keeps its files.
- `Finishing`: `wait()` blocks; `try_wait()` and `is_finished()` poll. Per-source problems are in
  the result, not in the `Err`: a partly failed recording still returns every file that survived.
- `Finished`: the `Manifest`, per-source cursor telemetry, the input log, and any errors outside
  a single source.

### Audio

- `Mic::None`: no microphone. ScreenCaptureKit's microphone capture stays off.
- `Mic::Device { id, warmup }`: ScreenCaptureKit opens the Core Audio device and records it as a
  track in the first source's MP4, attached `warmup` after the stream starts (usually
  `MIC_WARMUP`, 1.5 s) so both tracks share an origin. `start` blocks for the warm-up; a host
  that cues playback on the start, like a voiceover, passes zero.
- `Mic::External(ExternalMic)`: the host owns the microphone and pushes mono PCM (16 kHz by
  default) through an `ExternalAudioSink`. The sink is bounded: when the queue is full, the audio
  is dropped and recorded as a gap instead of blocking. Gaps are padded with silence, so sample 0
  of the WAV is the clock anchor and every sample stays at its true time. The manifest lists
  every gap. The core never opens a microphone in this mode.
- `AudioPlan::system`: system audio, captured on the first source's stream. It is always a track
  in that source's MP4 (ScreenCaptureKit's recording output records what the stream captures,
  mixing system audio and a `Mic::Device` microphone into one AAC track).
  When `Names::system_audio` is set, it is also written as a separate AAC file, cut at the stop.
  If no system audio arrives, that file is not left behind and the manifest does not list it.

### Input

- `Input::Host(receiver)`: the host forwards events from its own tap through `host_channel`.
  Sending never blocks; a full queue drops events and counts them.
- `Input::Tap`: the core runs its own listen-only `CGEventTap` for mouse buttons. It needs Input
  Monitoring; without it the manifest lists `input_monitoring` under `missing_permissions`.
- `Input::None`: no input log.
- Keys are kept only as `⌘`/`⌃` chords by default (`KeyPolicy::Chords`), by name, never as typed
  text. Every key event is dropped while secure event input is on (a password field has focus).
- Naming `Names::cursor` turns on a pointer sampler (~240 Hz on the host clock) that places the
  cursor in each source's frames. Button events from either input path become click telemetry.

### Frame tap

`FrameTap::new(capacity, interval, max_width)` sends at most one downscaled RGBA frame per source
per `interval` through a bounded channel, dropping frames when it is full. The newest skipped
frame is sent the moment the recording stops, so the host can build a contact sheet right away.

## The clock

Every timestamp the core produces is Mach host time (`mach_absolute_time`) in nanoseconds: the
clock of Core Audio host times, `CGEvent` timestamps and ScreenCaptureKit frame display times.
That covers stream starts, audio file starts and gaps, cursor samples and clicks (`host_ns`, next
to their `source_time` in the video), input events (`host_ns`) and tapped frames
(`Frame::host_time`). A host on the same clock lines its own timestamps up without converting.
`HostTime::from_nanos` and `HostTime::as_nanos` cross the API boundary.

## The bundle

The host picks every file name. The core adds only the manifest, `capture.json` unless
`Names::manifest` names it (a host that keeps several recordings in one directory names it per
recording):

```json
{
  "version": 1,
  "clock": "mach_host_ns",
  "anchor_ns": 81234000000000,
  "sources": [{
    "source": { "kind": "region", "display": 1, "rect": { "x": 100.0, "y": 80.0, "width": 640.0, "height": 400.0 } },
    "file": "recording.mp4",
    "start_ns": 81234041000000, "duration_ms": 5000, "width": 1280, "height": 800,
    "frame": { "x": 100.0, "y": 80.0, "width": 640.0, "height": 400.0 },
    "audio_tracks": []
  }],
  "files": [
    { "name": "recording.mp4", "kind": { "type": "video", "source": 0 }, "start_ns": 81234041000000 },
    { "name": "audio.wav", "kind": { "type": "microphone", "sample_rate": 16000 }, "start_ns": 81234000000000 }
  ],
  "audio_gaps": [{ "start_ns": 81235200000000, "end_ns": 81235450000000 }],
  "missing_permissions": []
}
```

`anchor_ns` is taken just before the first source starts; sample 0 of the microphone WAV sits
there. A media file's `start_ns` is where its time zero sits; the cursor and input logs carry
`host_ns` per entry instead. A source's `frame` is the captured area in global points, and its
`error` is set when it did not finish cleanly. `Manifest::read` (or `Manifest::read_file` for a named
one) refuses a manifest newer than it understands.

## Seams

- The host chooses sources, exclusions, file names, and where audio and input come from. The core
  excludes nothing on its own, not even its host's windows.
- `ExternalAudioSink`, `HostInputSender` and `FrameTap` are bounded queues between the host's
  threads and the core's. None of them blocks the sender.
- Inside the crate, each source's native stream sits behind a small trait so the stop sequence is
  tested against a fake stream.

## Invariants

- No global state, no servers, no shortcuts. Threads start with a recording and end with its
  finishing, with one exception: a stream stop that never returns (below) leaves its thread
  blocked inside ScreenCaptureKit.
- `stop()` returns at once (under 10 ms, tested). Finishing runs on the recording's own thread.
- Stopping cuts every source before waiting on any of them. A source's stream keeps running
  until its detached recording output reports that it finished or failed: stopping a stream
  while ScreenCaptureKit is still exporting a removed output can make the export run twice and
  abort, losing the file. Only then is the stream stopped, and that stop is bounded at 10 s,
  because the capture service can stop answering.
- A system stream stop is not an output failure. If the output still finishes, the MP4 is kept.
- ScreenCaptureKit writes a frame only when the screen changes, so a video track can end at the
  last change while audio runs to the stop. The final frame is held to the recorded end by
  lengthening its sample in place, so every reader sees the full length. When B-frames decode
  the final frame before another, a skipped H.264 picture that copies it exactly is appended
  instead; no existing sample is re-encoded or moved. The video's end is where its frames stop,
  even when the edit list claims more, as it can for a video-only source. If either fails, the
  original file is kept.
- Logging goes through `tracing`.

## Tests

`scripts/check` runs them with everything else. For this crate alone:

```sh
cargo nextest run -p "$(cargo pkgid)" -p capture-core --features capture -E 'not package(hex)'
```

The final-frame tests write their fixtures with AVAssetWriter and `ffmpeg`, and check them with
AVFoundation and `ffprobe` (both must be on `PATH`). Tests that need Screen Recording permission
and a live display are `#[ignore]`d.
