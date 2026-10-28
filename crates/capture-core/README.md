# capture-core

Screen recording for macOS 15+ on ScreenCaptureKit. A host describes a recording as a `Plan`;
the core records each display or window to its own H.264 MP4, places every file it writes on one
host clock, and describes the result in a versioned `capture.json` manifest.

The crate compiles only on macOS. In this repository it sits outside the workspace and is built
only through the `capture` feature (`scripts/check` lints and tests it as `-p capture-core`).
Linux, Windows, Nix and release builds never compile it.

## API

```rust
use capture_core::*;

let (input, input_rx) = host_channel(1024, KeyPolicy::Chords);
let (frames, frame_rx) = FrameTap::new(8, Duration::from_millis(500), 640);

let mut recording = Recorder::start(Plan {
    sources: vec![Source::Display(display_id), Source::Window(window_id)],
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

let mut mic = recording.external_audio().unwrap(); // ExternalAudioSink
mic.offer(&pcm_16k_mono, captured_through);         // never blocks
mic.gap(from, through);                             // the host lost audio
input.send(HostTime::now(), event);                 // never blocks

let finishing = recording.stop();   // returns at once
// ... frame_rx already holds the last frame of each source
let finished = finishing.wait()?;   // or poll with finishing.try_wait()
```

- `Recorder::start(Plan) -> Result<Recording, Error>`: validates the plan, refuses names that
  already exist in `dir`, and starts sources `1..n` before source `0`, which owns audio.
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
- `Mic::Device(id)`: ScreenCaptureKit opens the Core Audio device and records it as a track in
  the first source's MP4, attached after a 1.5 s warm-up so both tracks share an origin.
- `Mic::External(ExternalMic)`: the host owns the microphone and pushes mono PCM (16 kHz by
  default) through an `ExternalAudioSink`. The sink is bounded: when the queue is full, the audio
  is dropped and recorded as a gap instead of blocking. Gaps are padded with silence, so sample 0
  of the WAV is the clock anchor and every sample stays at its true time. The manifest lists
  every gap. The core never opens a microphone in this mode.
- `AudioPlan::system`: system audio, captured on the first source's stream. It is always a track
  in that source's MP4 (ScreenCaptureKit's recording output records what the stream captures).
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

## The bundle

The host picks every file name. The core adds only `capture.json`:

```json
{
  "version": 1,
  "clock": { "anchor_host_time": 123456789, "timebase": { "numer": 125, "denom": 3 } },
  "sources": [{
    "source": { "kind": "display", "id": 1 },
    "file": "recording.mp4",
    "start_ms": 41, "duration_ms": 5000, "width": 3024, "height": 1964,
    "frame": { "x": 0.0, "y": 0.0, "width": 1512.0, "height": 982.0 },
    "audio_tracks": []
  }],
  "files": [
    { "name": "recording.mp4", "kind": { "type": "video", "source": 0 }, "start_ms": 41 },
    { "name": "audio.wav", "kind": { "type": "microphone", "sample_rate": 16000 }, "start_ms": 0 }
  ],
  "audio_gaps": [{ "start_ms": 1200, "end_ms": 1450 }],
  "missing_permissions": []
}
```

Every `*_ms` counts from `clock.anchor_host_time`, a Mach host time taken just before the first
source starts. A source's `error` is set when it did not finish cleanly. `Manifest::read` refuses
a manifest newer than it understands.

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
  lengthening its sample in place, so every reader sees the full length. If that fails, the
  original file is kept.
- Logging goes through `tracing`.

## Tests

`scripts/check` runs them with everything else. For this crate alone:

```sh
cargo nextest run -p voice-control -p capture-core --features capture -E 'not package(voice-control)'
```

The final-frame tests write their fixtures with AVAssetWriter and `ffmpeg`, and check them with
AVFoundation and `ffprobe` (both must be on `PATH`). Tests that need Screen Recording permission
and a live display are `#[ignore]`d.
