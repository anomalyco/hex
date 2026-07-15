# HEX

HEX is the second-generation successor to Voice Control: a local, observable
voice command system for macOS, built in Rust on top of Moonshine Voice.

## Try It

Moonshine `0.0.68` and the English Medium Streaming model are installed locally.
Install and launch the signed GPUI desktop app with:

```sh
./scripts/install-app.sh
```

The app starts command listening and meeting detection together. For CLI-only
development, start the listener with:

```sh
cargo run -- listen
```

The listener prefers `Universal Audio Thunderbolt`, falls back to
`Studio Display Microphone`, then falls back to the macOS default input. Override
that order for one run with:

```sh
cargo run -- listen --device "MacBook Pro Microphone"
```

Press Ctrl-C to stop. To recreate the Python environment and model cache:

```sh
./scripts/setup.sh
```

The first run requires macOS Microphone, Input Monitoring, and Accessibility
permission. The setup scripts require `uv`, `curl`, and `tar`.

Option dictation uses Parakeet v2 through the same ONNX engine as Handy. Install
the model once with:

```sh
./scripts/setup-parakeet.sh
```

Hold Option, speak, then release to transcribe and paste into the foreground
application. Holds shorter than 300 ms are discarded, and 450 ms of warm
pre-roll protects the first phoneme. Pressing or releasing Option resets
Moonshine's stream so dictation audio cannot leak into a command.
Press Option-Shift-V to paste the last successful transcript again.

The desktop app shows a click-through dictation indicator at the top center of
the display under the pointer. A deliberate Option hold reveals a red capsule
whose inner energy follows live RMS and peak microphone levels. On release it
contracts without overshoot into a fixed blue orb with a clipped processing
sweep, then exits on completion, cancellation, failure, or a brief discarded
tap.

Preview the complete HUD sequence without microphone input:

```sh
cargo run -- app --preview-dictation
```

For hands-free dictation, say `dictate start`, speak, then say `dictate stop` to
transcribe and paste or `dictate send` to paste and press Enter. Say
`dictate cancel` to discard the capture. While voice dictation is active,
ordinary voice commands are ignored.
You may continue directly after the activation phrase, as in
`dictate start this is my message`; the activation prefix is removed.

Say `captain's log`, speak an entry, then say `captain's log end` to transcribe
it into today's Organizer journal through the canonical `log add` command.
Set `VOICE_CONTROL_LOG_CLI` only when the Organizer executable is not installed
at `~/.bun/bin/log`.

## Meetings

Record a local Granola-style meeting with separate system and microphone tracks:

```sh
cargo run -- meeting record --title "Design sync"
```

Press Ctrl-C to stop. While recording, one Moonshine model maintains independent
Computer and You streams and appends a recoverable `transcript.live.ndjson`
draft. Live inference is bounded and may skip draft packets under load without
affecting WAV capture. HEX then retranscribes both complete WAV tracks locally
in 30-second Parakeet chunks, merges the timestamped segments, and atomically
publishes source-labeled `transcript.ndjson` and `transcript.md` files under
`~/Library/Application Support/voice-control/meetings/`.

```sh
cargo run -- meeting list
cargo run -- meeting show <meeting-id>
```

Meeting capture requires macOS 15 or newer plus Screen Recording and Microphone
permission. It excludes HEX's own audio and does not make network
calls. The transcript labels the local microphone as `You` and mixed computer
audio as `Computer`; it does not yet identify individual remote speakers or
remove loudspeaker echo. Headphones provide the cleanest source separation.

### Automatic Meeting Offers

Install the signed background agent once:

```sh
./scripts/install-app.sh
```

The HEX desktop app observes CoreAudio process metadata, not microphone
samples, to find supported applications that have actually activated microphone
input. It recognizes Zoom, Microsoft Teams, Slack, FaceTime, Chrome, Brave,
Safari, Firefox, and Edge process families. A Brave tab at a joined Google Meet
URL is labeled as Google Meet; other browser microphone sessions remain
conservatively labeled `Browser call`.

After two seconds of stable activity, HEX opens a non-activating
floating panel with `Record Locally` and `Not Now` actions. It appears across
Spaces and beside fullscreen applications without requiring Notification Center
permission. Recording never begins from detection alone. Selecting
`Record Locally` starts the existing two-track recorder. Recording state, timer,
`Stop & Transcribe`, live draft, finalization, and the final transcript remain
inside the Meetings pane rather than a floating status window. The pane updates
automatically throughout the lifecycle and falls back to the live draft if final
transcription fails. One offer is shown per microphone activation; a call must
be inactive for five seconds before it can prompt again.

Recording can also be started explicitly from the Meetings pane or by saying
`start a meeting` / `record this meeting`. Say `stop meeting` to save the tracks
and begin final transcription. These direct actions count as explicit approval;
automatic detection remains offer-only.

Inspect the permission-light signal without launching the agent:

```sh
cargo run -- meeting probe
cargo run -- meeting watch --preview
```

The installed HEX app lives at `~/Applications/HEX.app` with bundle ID
`com.kitlangton.voice-control.agent`. Microphone, Automation, and Screen & System
Audio Recording permissions attach to that stable signed identity. The GPUI
panel itself needs no notification permission. Add the app under System Settings
> General > Login Items to run it after login.

Run the terminal dashboard in another terminal:

```sh
cargo run -- status
```

The dashboard tails `logs/live.ndjson`, shows listening and dictation states,
highlights partial and completed recognition, and displays inference latency.
It opens on the contextually available command catalog. Press `1` for commands,
`2` for the activity log, `3` for meetings, and `j`/`k` or the arrow keys to
select a meeting transcript. Press `q` or Escape to close the dashboard without
stopping the listener.

HEX starts listening. Say one of:

```text
open Brave
open Slack
go to X
command one
go to sleep
```

After `go to sleep`, say `voice control` or `wake up` to resume listening.

When the foreground Brave tab is on `x.com`, these contextual commands are
also active:

```text
go to notifications
go to chat
go home
```

When Slack is foreground, these contextual commands are also active:

```text
go to threads
next unread
previous unread
go to console
```

The command model depends on browser host rather than Brave itself. Brave
AppleScript is only the first browser adapter.

## Architecture

The executable is native Rust. Unsafe code is isolated in three adapters:

- `moonshine` dynamically loads Moonshine's stable C interface and presents a
  safe streaming recognizer.
- `suppression` reads the system-wide macOS Option modifier state.
- `keyboard` resolves logical keys through the active layout and posts balanced
  native shortcuts.
- `parakeet` owns a bounded background ONNX transcription worker.
- `dictation` owns warm pre-roll, duration limits, and band-limited resampling.
- `meeting` owns explicit ScreenCaptureKit capture, private durable artifacts,
  bounded source-separated live transcription, atomic final transcription, and
  source-aware transcript projections.
- `dictation_indicator` owns the click-through GPUI hold-to-talk HUD, audio
  metering projection, and recording/transcription outcome animation.
- `microphone_activity` observes CoreAudio process metadata without capturing
  audio; `meeting_detection` debounces supported app families; the bundled
  GPUI desktop runtime owns meeting offers and inline recording state.

The remaining modules are safe Rust:

- `audio` owns CoreAudio capture and converts input to mono float PCM.
- `recognition` owns command and dictation capture lifecycles.
- `events` owns the append-only NDJSON observation format.
- `dashboard` is a read-only terminal projection of those events.

Rust and platform diagnostics are appended to `logs/process.log`; structured
voice events are appended to `logs/live.ndjson`.

The intended runtime states are:

```text
sleep (wake-word only) -> command -> dictation
```

No captured audio should be persisted by default.
