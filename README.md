# HEX

HEX is a private local dictation app for macOS, built in Rust with strict-Metal
transcription. Experimental voice commands are available as an opt-in, while
meetings remain available only in development builds.

## Linux X11 Preview

Linux currently supports the GPUI shell, configurable global hold/release
dictation, Parakeet transcription through Vulkan with CPU fallback, X11
clipboard insertion, Escape cancellation, double-tap lock, and NDJSON
diagnostics. Voice commands, application context, recording-environment
controls, meetings, and packaging remain in progress.

On x86_64 Linux with ALSA or PipeWire's ALSA compatibility layer:

```sh
./scripts/setup.sh
cargo run -- model install
cargo run -- model check
cargo run -- devices
cargo run -- app
```

The X11 shell starts the dictation service and displays completed transcripts.
When the desktop provides a StatusNotifier tray host, closing the window hides
it while dictation continues; the tray can show the window, start or stop
listening, or quit HEX. Without a tray host, close exits normally instead of
leaving an unreachable background process. The window advertises the standard
EWMH dialog type so compliant tiling window managers can float it.
Hold the configured shortcut, `Alt+Space` by default, and release it to
transcribe and paste. Click the shortcut in the UI while stopped to replace it.
Press it twice within 300 ms to lock recording, press it again to finish, or
press Escape to cancel.

`cargo run -- dictate` runs the same global dictation service without the UI.
`cargo run -- listen` remains available for continuous Moonshine transcription.

Select a microphone with a case-insensitive device-name fragment:

```sh
cargo run -- dictate --device "Revelator IO 44"
cargo run -- status --lines 20
```

## Try It

Install and launch the signed GPUI desktop app with:

```sh
./scripts/install-app.sh
```

On first launch, HEX guides you through Microphone, Input Monitoring, and
Accessibility access, then installs the selected checksum-pinned dictation
model. Dictation starts only after those required capabilities are ready.

The current coworker build is available from the stable download URL:

<https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/releases/HEX-latest-arm64.dmg>

Open the DMG and drag HEX into Applications. The download is Apple-silicon-only
and requires macOS 15 or newer.

The distributed app starts with command recognition and meeting detection off.
Open **Commands** in the sidebar to enable experimental voice commands; the
setting persists and applies without restarting HEX. Meetings remain limited to
debug builds. Start the complete developer app with:

```sh
cargo run -- app
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

The development setup scripts require `uv`, `curl`, and `tar`.
Building the app bundle requires Xcode 26 to compile its Icon Composer source.

Dictation uses curated local GGUF models through
`transcribe.cpp` and strict Metal. Open **Settings > Local transcription** to
choose a language; HEX recommends speed, accuracy, and recognition-hint options,
then downloads, verifies, and prewarms the selected model before activating it.
Parakeet v2 Q8 remains the default English model. For development or offline
bootstrap, install that default directly with:

```sh
./scripts/setup-parakeet.sh
```

Hold the dictation shortcut, speak, then release to transcribe and paste into the
foreground application. The shortcut defaults to Option and can be recorded in
Settings as a modifier chord, modifier-plus-key chord, or standalone function
key. Holds shorter than 300 ms are discarded, and 450 ms of warm pre-roll
protects the first phoneme.
Double-tap the shortcut within 300 ms to lock hands-free dictation, then press it
again to finish or Escape to cancel. Settings can also disable double-tap lock,
control other audio while dictating, choose a microphone, prevent idle sleep
during recordings, launch HEX at login, adjust tone volume, and show or hide HEX
in the Dock. Microphone changes apply as soon as the active capture ends. If a
saved microphone is unavailable, HEX falls back to its automatic preference
order instead of stopping dictation. Warm capture remains enabled so the
in-memory pre-roll can protect speech onset. The language picker affects local
dictation.
Press Option-Shift-V to paste the last successful transcript again.

To edit existing text, select it, hold Option-Command, describe the change, and
release. HEX transcribes the instruction, asks OpenCode for replacement text,
and replaces the selection only if the same application and exact text remain
selected when processing finishes. The edit shortcut is configurable. Missing
or changed selections and processing failures leave the document unchanged.

The desktop app shows a click-through dictation indicator at the top center of
the display under the pointer. A deliberate shortcut hold reveals a red capsule
whose inner energy follows live RMS and peak microphone levels. On release it
contracts without overshoot into a fixed blue orb with a clipped processing
sweep, then exits on completion, cancellation, failure, or a brief discarded
tap.

Preview the complete HUD sequence without microphone input:

```sh
cargo run -- app --preview-dictation
```

For deterministic UI development, launch the production shell without
recognition, meeting detection, persisted settings, or real downloads:

```sh
cargo run -- preview settings
cargo run -- preview onboarding
cargo run -- preview transcription-picker --language zh --model-state installed
cargo run -- preview transcription-picker --language en --model-state downloading
cargo run -- preview transcription-picker --language zh --model-state error
```

Capture only the preview window without Apple Events or Accessibility access:

```sh
./scripts/capture-preview.sh /tmp/hex-picker.png transcription-picker \
  --language zh --model-state installed
```

The installed app checks for and downloads signed Sparkle updates automatically,
then installs them in the background or when HEX next quits. Use **HEX > Check
for Updates…** or **Settings > Software updates** to check immediately. Every
update is verified with both Developer ID and Sparkle EdDSA signatures before
installation.

Release builds are signed, notarized, stapled, Gatekeeper-assessed, and prepared
locally before publication. Publishing is an explicit second step:

```sh
HEX_BUILD_NUMBER=20001 \
HEX_RELEASE_NOTES=release-notes/2.0.1.md \
./scripts/release-app.sh prepare

./scripts/release-app.sh publish
```

## Experimental Commands

The command catalog is visible in every build, but recognition is disabled by
default. Enable it from the **Commands** pane. The first enablement installs the
checksum-pinned local Moonshine model in the background.

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

Meetings are an unfinished developer feature and are disabled in distributed
release builds.

Record a local Granola-style meeting with separate system and microphone tracks:

```sh
cargo run -- meeting record --title "Design sync"
```

Press Ctrl-C to stop. While recording, one Moonshine model maintains independent
Computer and You streams and appends a recoverable `transcript.live.ndjson`
draft. Live inference is bounded and may skip draft packets under load without
affecting WAV capture. HEX then retranscribes both complete WAV tracks locally
in 30-second chunks with the selected local model, merges available timestamped
segments, and atomically publishes source-labeled `transcript.ndjson` and
`transcript.md` files under
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

The installed HEX app lives at `/Applications/HEX.app` with bundle ID
`com.kitlangton.voice-control.agent`. Microphone, Automation, and Screen & System
Audio Recording permissions attach to that stable signed identity. The GPUI
panel itself needs no notification permission. Use **Settings > Launch at login**
to register the signed main app through macOS Service Management. If macOS has
revoked approval, HEX links to **System Settings > General > Login Items**.

Run the terminal dashboard in another terminal:

```sh
cargo run -- status
```

The dashboard polls
`~/Library/Application Support/voice-control/logs/live.ndjson`, shows listening and dictation states,
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

The executable is native Rust. Model and macOS unsafe code stays inside focused
adapters:

- `moonshine` dynamically loads Moonshine's stable C interface and presents a
  safe streaming recognizer.
- `suppression` owns the bounded event tap, configurable hotkey state machine,
  and reserved paste shortcuts.
- `keyboard` resolves logical keys through the active layout and posts balanced
  native shortcuts.
- `recording_environment` owns CoreAudio output muting, media pause/resume, and
  parent-bound idle-sleep prevention.
- `parakeet` owns a bounded background `transcribe.cpp` Metal worker.
- `dictation` owns warm pre-roll, duration limits, and band-limited resampling.
- `dictation_processor` owns optional context-aware OpenCode rewriting and raw
  transcript fallback plus explicit selected-text editing; `selected_text` owns
  bounded selection capture and destination validation; `paste` owns clipboard
  insertion and restoration.
- `command_grammar` compiles literal and typed patterns into the registry that
  powers pure resolution and the generated catalog.
- `meeting` owns explicit ScreenCaptureKit capture, private durable artifacts,
  bounded source-separated live transcription, atomic final transcription, and
  source-aware transcript projections.
- `dictation_indicator` owns the click-through GPUI hold-to-talk HUD, audio
  metering projection, and recording/transcription outcome animation.
- `app_settings` owns atomic persistence and live projection of microphone,
  hotkey, transcription, recording, feedback, and Dock settings; `login_item`
  keeps macOS `SMAppService` state authoritative for launch at login.
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
