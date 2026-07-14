# Voice Control

A local, observable voice command system for macOS, built in Rust on top of
Moonshine Voice.

## Try It

Moonshine `0.0.68` and the English Medium Streaming model are installed locally.
Start the listener with:

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

For hands-free dictation, say `dictate start`, speak, then say `dictate stop` to
transcribe and paste or `dictate send` to paste and press Enter. Say
`dictate cancel` to discard the capture. While voice dictation is active,
ordinary voice commands are ignored.
You may continue directly after the activation phrase, as in
`dictate start this is my message`; the activation prefix is removed.

Run the terminal dashboard in another terminal:

```sh
cargo run -- status
```

The dashboard tails `logs/live.ndjson`, shows listening and dictation states,
highlights partial and completed recognition, and displays inference latency.
It opens on the contextually available command catalog. Press `1` for commands,
`2` for the activity log, and `q` or Escape to close the dashboard without
stopping the listener.

Voice Control starts listening. Say one of:

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
