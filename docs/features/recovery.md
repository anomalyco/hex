# Recovery

Recovery means the next fresh capture succeeds, including its expected output.
A cleared error or a live process alone is not proof.

This page owns interruption and failure transitions. [Dictation](dictation.md)
owns normal capture behavior; the [index](README.md) covers optional processing,
platforms, and dependencies.

## Sub-features

- [Cancel without losing other work](#cancel-without-losing-other-work).
- [Rearm without a phantom capture](#rearm-without-a-phantom-capture).
- [Reopen with fresh audio](#reopen-with-fresh-audio).
- [Fence late capture notifications](#fence-late-capture-notifications).
- [Degrade only the failing dependency](#degrade-only-the-failing-dependency).
- [Keep Linux listeners managed](#keep-linux-listeners-managed).

## How To Get To It

Start with normal dictation in a foreground app. Escape cancels capture or
cancellable pending output; after an interruption, try a fresh shortcut hold.
On Linux, Settings also exposes Start/Stop and Retry/Dismiss.

```ts
cancel / interruption / dependency failure
  -> affected operation stops, falls back, or reports failure
  -> automatic recovery OR explicit retry              // depends on the failure
  -> fresh hold -> new audio -> release -> expected output // recovery endpoint
```
These are paths to recognize when a failure occurs, not instructions to disrupt
a running app or its devices. The checks below do not prove this entire journey.

## Existing Checks

This is a map of existing checks, not a verified end-to-end recovery driver.
Unless an executed result is dated below, checks are locators rather than passing
runs. Capture evidence is macOS-specific unless stated otherwise; each sketch
puts its source and proof limit beside it.

### Cancel Without Losing Other Work

`recover.cancel`: Escape targets the active capture first, not an older job
whose output happens to be pending.

```ts
Escape
  ├── active capture -> cancel that capture          // accepted jobs keep ownership
  └── no active capture -> newest cancellable job

paste preparation -> still cancellable
first paste mutation -> committed                    // cancellation cannot undo output

cancelled job -> no paste / History / last-result update
older unaffected job + fresh capture -> can both complete
```
Checks in [parakeet.rs](../../src/parakeet.rs):
`cancellation_replaces_waiting_output_and_ignores_late_completion`,
`output_stays_cancellable_through_preparation_but_not_after_mutation`.
Proof limit: controlled ordered output, paste commitment, and retained text;
not target-app clipboard consumption.

### Rearm Without A Phantom Capture

`recover.shortcut`: an ordinary chord or missed release can leave stale shortcut
bookkeeping. Repairing that bookkeeping must not manufacture a recording.

```ts
stale shortcut state
  -> no active/pending gesture + no pending input     // required gates
  -> full key scan + >= 100 ms sampled neutrality
  -> rearm bookkeeping only                          // emits NO capture actions
  -> fresh hold -> release -> expected output

delayed pre-recovery input -> fenced                  // cannot corrupt the fresh hold
```
Checks in [suppression.rs](../../src/suppression.rs):
`neutral_keyboard_rearms_after_a_missing_modifier_release`,
`delayed_neutral_callback_after_recovery_preserves_the_next_hold`,
`stale_key_recovery_requires_complete_neutrality_and_no_intervening_input`,
`stale_key_recovery_never_polls_active_or_pending_gestures`.
Proof limit: synthetic events and sampled-state fixtures, not every physical
keyboard or remapping.

Fn metadata needs a narrower rule than "ignore Fn."

```ts
SecondaryFn navigation metadata
  ├── timestamped modifier-change evidence says Fn is up
  │     -> may ignore that metadata for neutrality    // full scan + delayed fences remain
  └── Fn held or unknown -> keep Fn as a blocker

strip Fn globally from shortcut matching              // UNSAFE: not the recovery rule
```
Checks in [suppression.rs](../../src/suppression.rs):
`arrow_function_metadata_does_not_block_the_next_option_hold`,
`function_recovery_preserves_unknown_and_flags_only_fn_holds`.
Proof limit: supplied modifier evidence, not universal native Fn behavior.

`recover.tap-interruption`: a disabled event tap breaks trust in the input
history, not just the current modifier flags.

```ts
event tap disabled
  -> cancel active hotkey gesture                     // no stuck recording
  -> invalidate uncertain modifier evidence           // no inferred release
  -> fresh input -> next valid gesture -> expected output
```
Checks in [suppression.rs](../../src/suppression.rs):
`tap_failure_cancels_recording_after_live_opt_in`,
`blind_input_periods_invalidate_function_modifier_evidence`.
Proof limit: reducer response to a supplied interruption, not macOS disabling
and re-enabling a real tap.

The [2.1.11 release notes](../releases/2.1.11.md) describe the shipped navigation
and missed-release fix; they are historical context, not a current-tree test.

### Reopen With Fresh Audio

`recover.microphone`: missing input at startup and an interrupted stream use
bounded recovery. "Keep ready" keeps retrying without a settings change or
listener restart, even after cancellation.

```ts
initial open fails OR stream interrupts
  -> cancel incomplete capture + discard stale audio  // interrupted audio cannot return
  -> bounded backoff -> reopen selected input         // warm microphone retries
  -> replacement delivers new audio
  -> fresh capture -> release -> expected output
```
Checks in [audio.rs](../../src/audio.rs):
`microphone_recovery_retries_with_bounded_backoff_and_resets`,
`startup_retry_recovers_metadata_and_audio_without_restarting`;
[dictation_audio.rs](../../src/dictation_audio.rs):
`recovered_owner_delivers_audio_and_accepts_a_new_capture`.
Proof limit: retry policy and channel-backed replacement audio, not a physical
device reconnect or full native mic-to-target result.

`recover.cold-open`: "Release when idle" opens for a gesture, not an idle retry
loop. Each attempt must own its opening result.

```ts
press A -> asynchronous open A
  -> release/cancel before ready -> abandon A
press B -> asynchronous open B
  ├── late ready A -> reject                          // belongs to abandoned open
  └── ready B -> capture B -> finish -> expected output
                   └── capture idle -> close device   // accepted jobs do not keep it open

release-when-idle enabled + capture idle -> stop retries
warm mode enabled during pending capture -> preserve capture; retry after cancellation
```
Checks in [dictation_audio.rs](../../src/dictation_audio.rs):
`cold_capture_restart_rejects_late_open_results_and_closes_after_finish`,
`enabling_warm_microphone_preserves_pending_capture_and_recovers_after_cancellation`.
Proof limit: controlled asynchronous opener and capture ownership, not onset
latency or the macOS microphone indicator. Cold Voice Action has a separate gap
below.

### Fence Late Capture Notifications

`recover.late-notification`: keeping a failure visible is correct until it
would be attributed to a different capture.

```ts
capture A fails -> finish/cancel A -> preserve A's terminal failure
start capture B -> new generation                     // fences obsolete notifications
  ├── late failure/ready A -> cannot finish, fail, or activate B
  └── B's own audio -> finish B -> expected output
```
Checks in [dictation_audio.rs](../../src/dictation_audio.rs):
`capture_failures_survive_terminal_controls_but_not_a_new_capture`,
`boundary_drain_preserves_finish_failure_but_does_not_fail_a_new_capture`.
Proof limit: error delivery across capture boundaries, not arbitrary
cross-thread or native event ordering.

### Degrade Only The Failing Dependency

Command-model preparation must not make hotkey dictation wait or fail with it.

```ts
command model loads in background
  ├── slow/failing loader -> command preparation unavailable
  └── listener remains responsive -> fresh hotkey capture -> expected output
```
Check in [recognition.rs](../../src/recognition.rs):
`command_preparation_does_not_block_the_listener_or_propagate_failure`.
Proof limit: controlled loader responsiveness, not a full model download or
microphone session. See the [index](README.md) for dependency recovery actions.

`recover.command-pressure`: command recognition is best-effort; dictation
audio is not.

```ts
microphone timeline
  ├── active dictation -> retain continuous audio -> expected output
  └── command backlog exceeds bound
        -> invalidate stale generation + reset recognition
        -> discard stale audio/updates                // no stale command may execute

command recovery -> next fresh dictation still works  // not just an emptied queue
```
Sources: [dictation_audio.rs](../../src/dictation_audio.rs),
`overflow_recognition` and `forward_recognition`;
[recognition.rs](../../src/recognition.rs), discontinuity handling.
Proof limit: source-backed contract only. A whole-path native stall check must
still establish lossless dictation and stale-command rejection together.

`recover.optional-processing`: ordinary dictation falls back to usable text;
Voice Action does not substitute a transcript for a failed action.

```ts
ordinary dictation -> corrections -> optional rewrite -> transformations -> paste
                                      │                  │
                                      v                  v
                             rewrite fails       chain fails
                             -> corrected text   -> chain's input // discard partial results

Voice Action fails -> paste nothing                    // different contract, no fallback
either path -> later request -> expected output         // recovery still needs observation
```
Stage-specific sources, checks, and the known host failure are in the
[index](README.md); late native replies are also covered below.
Proof limit: fallback behavior alone does not establish that the next request
succeeds.

### Keep Linux Listeners Managed

`recover.linux-listener`: Settings exposes listener status, Start/Stop, and
Retry/Dismiss. Hiding a failure is not restarting its worker.

```ts
listener worker fails -> reap worker -> show failure
  ├── Retry -> new managed listener -> fresh capture -> expected output
  └── Dismiss -> clear error                            // not evidence of recovery

close Settings
  ├── X11 + usable tray -> hide window; listener stays managed
  └── Wayland OR unusable tray -> stop listener -> drain workers -> quit
                                                       // never strand a background mic
```
Sources/checks: [linux_app.rs](../../src/linux_app.rs),
[Linux CI](../../.github/workflows/check-linux.yml),
[test-wayland-paste.sh](../../scripts/test-wayland-paste.sh).
Proof limit: source and available headless checks, not current supported-host
behavior. Native compositor/device validation remains separate; the smoke's
isolation gap is documented in the [index](README.md).

### Keyboard Layout Resolution

```ts
GUI startup -> require OS main thread -> initialize_layout // recover.keyboard-layout
  -> publish complete snapshot BEFORE settings/workers/GPUI
     ├── cached character -> keycode
     └── missing character -> error                    // no worker-side TIS/TSM query

Headless lookup without snapshot -> shared native lock -> current layout query
Concurrent snapshot construction -> same lock          // serialized, not a separate race
```
Sources: [keyboard.rs](../../src/keyboard.rs),
[meeting_watcher.rs](../../src/meeting_watcher.rs).
The lock alone is not GUI thread safety: successful main-thread prewarming and
pure cached misses prevent later GUI workers from querying native layout APIs.
The GUI snapshot remains fixed until restart. CLI lookups without a snapshot
still query the current layout; this fix does not remove their live resolution.

**Executed September 1, 2026:** [keyboard_layout.rs](../../tests/keyboard_layout.rs)
includes the production module and runs four scenarios in fresh child processes,
three times each, with eight workers per scenario. Before the fix, cold lookups,
mixed initialization/lookups, and warmed misses aborted in all nine affected
runs; warm hits did not abort. After the fix, all twelve child runs passed in
both debug and optimized release profiles.

```ts
Retained regression
├── cold-lookups -> native queries serialized; headless lookups remain live
├── cold-mixed -> initialization and lookups coordinate through the same lock
├── warm-hits -> exactly one native snapshot acquisition
└── warm-misses -> exactly one native snapshot acquisition, not just "no crash"

After explicit initialization -> further hits AND misses add zero native queries
```

Run `cargo test --locked --test keyboard_layout` on macOS with the repository's
native build dependencies; add `--release` for the optimized run. These runs used
macOS 26.5.1 (25F80), arm64, Rust 1.95.0,
and the locally installed CMake executable via `CMAKE`. The harness has a
20-second child deadline and confines a native abort to its child process.
It posts no key events and does not launch the installed app or capture audio.

**Local startup check, September 1, 2026:** Developer ID-signed version 2.1.11,
local build `20111.1`, was installed and launched from `/Applications/Hex.app`.
Bundle identity/signature and the installed executable's match to the signed
build were validated. The new process reported resolved keyboard layout,
loaded transcription model, and Listening, with no startup errors or panic.
The settings file's SHA-256 was unchanged after restart. This local build was
not notarized or published; the previous notarized app was retained for rollback.

Proof limit: these are native layout-query and local app-startup checks, not
physical shortcut/paste acceptance or validation of a distributed update.
Changing layouts during a running GUI session still requires a restart to
refresh its snapshot; live GUI layout refresh is not part of this fix.

## Gaps And Constraints

These five known gaps still limit recovery claims; they are not proposed features.

### Remapped Control

```ts
remapped keyboard -> Quartz modifier event -> InputEvent::Flags
                                              └── keycode discarded
side-less Control fixture -> does not prove remapped-key identity
listen-only native metadata -> establish distinguishability before choosing a fix
```
Source/check: [suppression.rs](../../src/suppression.rs),
`remapped_control_does_not_start_or_hold_right_control_dictation`;
issue [#37](https://github.com/anomalyco/hex/issues/37).
Missing evidence: affected-keyboard metadata showing whether Quartz preserves
enough identity to distinguish the remapped key.

### Native Muting And Missed Holds

```ts
reported mute failure / missed hold
  -> record build + permissions + mic mode + exact devices
  -> observe hold -> capture -> release -> result       // missing native evidence

ownership/timing tests -> device-specific root cause    // UNSAFE inference
```
Reports: [#53](https://github.com/anomalyco/hex/issues/53),
[#54](https://github.com/anomalyco/hex/issues/54).
Proof limit: existing ownership/timing tests do not establish these native
failure causes; a current hold-to-result sequence is still needed.

### Cold Voice Action

```ts
Command-first -> start_pending_voice_action -> intentional threshold -> open mic
Option-first  -> already-started capture -> promote to Voice Action

ordinary cold-dictation checks -> both Voice Action orders // NOT established
```
Source: [recognition.rs](../../src/recognition.rs), `start_pending_voice_action`;
issue [#23](https://github.com/anomalyco/hex/issues/23).
Missing evidence: both modifier orders with a controlled opener, followed by
native onset evidence.

### Late Native Tool Reply

```ts
tool invocation interrupted -> host.ts removes correlation
  -> late native reply -> unmatched -> rejected
invocation completes with outstanding tools -> personal_commands.rs rejects completion

timeout + late reply -> unaffected concurrent work -> later successful command
                                                      // missing lifecycle regression
```
Sources: [host.ts](../../sdk/commands/src/host.ts),
[personal_commands.rs](../../src/personal_commands.rs);
issue [#20](https://github.com/anomalyco/hex/issues/20).
Proof limit: rejection paths do not prove host recovery. Retain a Rust/TypeScript
regression covering that complete lifecycle.

### Slow Paste Consumer

```ts
publish clipboard -> post Paste
  ├── settle for 100 ms -> allow next output
  └── schedule restore after 500 ms -> restore only if still owned
target consumes later?                                // no consumption acknowledgment

controlled delayed consumer -> output A -> output B -> restoration
                                                      // missing isolated evidence
```
Source: [paste.rs](../../src/paste.rs);
issue [#24](https://github.com/anomalyco/hex/issues/24).
Missing evidence: both outputs and restoration with a controlled delayed
consumer, without using the user's clipboard.

Do not reset settings or loosen shortcut matching to make a recovery check pass.
Keep automatic recovery, explicit retry, and defects that prevent recovery
distinct. Test cleanup must preserve failure evidence.
