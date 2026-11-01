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
- [Load settings with invalid fields](#load-settings-with-invalid-fields).

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

### Load Settings With Invalid Fields

`recover.settings`: a settings file written by another build, or edited by
hand, cannot prevent the app from opening.

```ts
Launch -> read settings.json
├── Unknown keys -> ignored
├── Invalid field -> log its key and reason -> that field's default
│   └── Valid siblings and unrelated sections stay
├── Invalid JSON / unreadable file -> log why -> defaults in memory
└── Save over invalid settings -> preserve exact original first
    ├── settings.json.unreadable-<sha256>, owner-only; same bytes backed up once
    └── Backup fails / original cannot be read -> save fails, original stays
```

Loading repaired settings does not itself rewrite the file. A later settings
save (including a migration save) takes the backup before replacing it.
Backups are never automatically removed. Unchosen `meetings.retention` is
written as an absent key, so older builds with a non-optional retention still
read the file; existing `null` values also load in the current build.

Source and checks: [settings persistence](../../src/app_settings/persistence.rs)
and its [tests](../../src/app_settings/persistence/tests.rs): old-shape retention,
invalid enums, nested types, list entries, unknown keys, invalid JSON,
exactly-once private backups, interrupted saves, and backup failure. These
checks use temporary files; they do not exercise a real app launch.

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
  -> no hold/pending gesture + no pending input       // required gates
  -> full key scan + >= 100 ms sampled neutrality
  -> rearm bookkeeping only                          // emits NO capture actions
  -> fresh hold -> release -> expected output

LOCKED + key release missed while hands-free          // would block the finish press
  -> same gates, scan, and neutrality
  -> clear key tracking, stay LOCKED                  // emits NO capture actions
  -> fresh shortcut press -> Finish

delayed pre-recovery input -> fenced                  // cannot corrupt the fresh hold
```
Checks in [suppression.rs](../../src/suppression.rs):
`neutral_keyboard_rearms_after_a_missing_modifier_release`,
`delayed_neutral_callback_after_recovery_preserves_the_next_hold`,
`stale_key_recovery_requires_complete_neutrality_and_no_intervening_input`,
`stale_key_recovery_never_polls_holds_or_pending_gestures`,
`locked_dictation_finishes_after_a_missing_key_release`.
Proof limit: synthetic events and sampled-state fixtures, not every physical
keyboard or remapping.

`recover.shortcut-press`: Command-Tab swallows Tab's release, and Kit often
presses Option within 100 ms of letting go of Command. The press itself can
resynchronize instead of waiting for the poll. Switching apps with Command-Tab
while a Toggle or double-tap capture is locked left Tab tracked too, and every
press that should have finished the lock logged `locked with stale tracked
keys` and did nothing; the finishing press now resynchronizes the same way.

```ts
Command-Tab -> Tab still tracked (IDLE, DIRTY, or LOCKED)
  -> typing, Return, Shift …                          // don't matter
  -> binding press: the binding's own down edge
     ├── keyboard reported no modifiers after each tracked key's own press
     ├── only the binding's modifiers went down since
     └── tracked keys + every other modifier read up now  // same source as recovery
        -> clear tracked keys, fence input stamped before the press
           -> fresh press -> Start (IDLE, DIRTY) or Finish (LOCKED)
              ├── chord at the same tick -> Discard     // not fenced
              └── older delayed release of the key -> ignored
  -> any check fails -> press ignored (logged with reason + key_codes)
                     -> 100 ms poll as before
LOCKED + Escape -> Cancel, stale keys or not
fenced edges, edges before a disabled tap, suspension -> no neutral evidence
```
Checks in [suppression.rs](../../src/suppression.rs):
`option_soon_after_command_tab_records`,
`shortcut_press_after_command_tab_from_dirty_records`,
`typing_after_command_tab_does_not_block_the_next_press`,
`a_delayed_release_cannot_untrack_a_resynchronized_key_press`,
`a_chord_at_the_resynchronized_press_still_discards_it`,
`fenced_and_pre_reset_modifier_edges_are_not_neutral_evidence`,
`stale_press_waits_for_recovery_while_anything_else_is_held`,
`stale_press_requires_a_neutral_keyboard_after_the_tracked_keys`,
`stale_press_finishes_a_lock_and_leaves_escape_unchanged`,
`toggle_mode_finishes_after_command_tab_while_recording`. Unit tests read a keyboard
that is all up unless a test sets one, so every shortcut test can take this
path.
A binding press ignored while stale or DIRTY logs `shortcut press ignored` to
`process.log` with its reason; a resynchronized one logs `resynchronized stale
input tracking on a shortcut press`. Proof limit: synthetic events and injected
keyboard state; the live state is read when the listener processes the press,
not when it happened, so a tracked key released in between reads as up (the
neutral-keyboard check covers the common Tab-then-Option chord). No
physical-keyboard run yet.

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

`recover.tap-health`: macOS can disable a tap (callback timeout, user input)
or silently disable or invalidate it while Hex idles, after sleep, secure
input, or an Accessibility change. The tap thread repairs all three taps (key,
modifier, HID) and logs each repair to `process.log`.

```ts
disable event (timeout | user input)
  -> re-enable every tap                    // "macOS disabled the event tap; re-enabled it"
  -> recover.tap-interruption
every 5 s on the tap thread
  ├── tap disabled without notice -> re-enable  // "event tap was disabled without notice"
  ├── tap invalidated or re-enable fails -> recreate  // "recreated event tap"
  └── creation fails -> retry next check    // "could not recreate event tap", logged once
  -> key or modifier gap -> recover.tap-interruption
tap thread holds a user-initiated activity  // App Nap cannot throttle it; idle sleep allowed
```
Source: `TapSet::check`, `reenable_disabled_taps`, and `AppNapOptOut` in
[suppression.rs](../../src/suppression.rs). No check: it needs macOS to disable
a real tap. Proof limit: reported field failure (#612), not reproduced.

The [2.1.11 release notes](../releases/2.1.11.md) describe the shipped navigation
and missed-release fix; they are historical context, not a current-tree test.

### Reopen With Fresh Audio

`recover.microphone`: missing input at startup and an interrupted stream use
bounded recovery. "Keep ready" keeps retrying without a settings change or
listener restart, even after cancellation.

```ts
initial open fails OR stream interrupts
  -> cancel incomplete capture + discard stale audio  // interrupted audio cannot return
  -> bounded backoff -> reopen the resolved input     // warm microphone retries
  -> replacement delivers new audio
  -> fresh capture -> release -> expected output
no input device at all (macOS)
  -> devices whose name can't be read are skipped     // audio::device_name, never Display
  -> bounded recovery; `hex status` listener: "No microphone"
listener worker returns an error or panics (macOS)
  -> status: running false, "Stopped", capture unknown // never "Enabled"
saved microphone missing or will not open
  -> system default stands in, setting unchanged      // audio::Selection
  ├── listed but will not open -> look again with bounded backoff
  └── not listed -> look again on a device-list change // macOS reports arrivals
                    └── Windows, Linux: bounded backoff, logged only on a switch
  -> found -> switch back once capture is idle        // not during a pending start or tail
MacBook lid closed (macOS)                            // built-in mic is off but still listed
  -> built-in mic is passed over like Hex Microphone  // AppleClamshellState, read per resolve
  ├── headphone-jack mic (BuiltInHeadphoneInputDevice UID, or the built-in
  │   input's emic/line data source) -> stays usable: listed, selectable,
  │   followed as the default, and can stand in (#712)
  ├── default or saved mic is built in -> first other real input stands in
  │     -> Settings: "… is off while the lid is closed, so Hex listens to …"
  ├── no other input -> "the built-in microphone is off while the lid is closed"
  └── lid opens or closes -> re-resolve like a device-list change // IOKit clamshell notification
        -> switch once capture is idle
Bluetooth default input (macOS, avoid_bluetooth_microphone on) // transport type, read per resolve
  -> built-in mic stands in, else the first other real input
  ├── no other usable input -> the Bluetooth default
  └── setting changes -> re-resolve like a device-list change -> switch once idle

programmatic dictation (local API) when the stream interrupts or gaps
  ├── audio delivered before the loss -> kept under its id // recover.programmatic-loss
  │     -> /finish transcribes it; /cancel, heartbeats, and the lease still apply
  │     -> also when /finish arrives before the loss is read (take_loss)
  └── no audio since the start (open failed, or pre-roll only)
        -> /finish answers 409 microphone-lost
```
Checks in [audio.rs](../../src/audio.rs):
`an_unreadable_device_name_is_skipped_instead_of_panicking`,
`microphone_recovery_retries_with_bounded_backoff_and_resets`,
`startup_retry_recovers_metadata_and_audio_without_restarting`,
`a_missing_selection_uses_the_default_until_it_returns`,
`a_closed_lid_passes_over_the_built_in_microphone`,
`only_the_internal_microphone_is_switched_off_by_the_lid`,
`a_headphone_jack_microphone_is_used_with_the_lid_closed`,
`where_the_os_reports_new_devices_only_a_failed_open_is_retried`,
`only_an_idle_capture_lets_the_microphone_switch`;
[dictation/audio.rs](../../src/dictation/audio.rs):
`recovered_owner_delivers_audio_and_accepts_a_new_capture`,
`microphone_loss_keeps_only_a_programmatic_captures_delivered_audio`,
`microphone_loss_with_only_pre_roll_keeps_nothing`,
`finish_after_an_unread_microphone_loss_takes_the_kept_clip`;
[recognition.rs](../../src/recognition.rs):
`microphone_loss_keeps_a_programmatic_clip_for_finish` (the hold, not a `/finish`
through the running listener).
[rune_ui/tests.rs](../../src/rune_ui/tests.rs):
`a_closed_lid_names_the_input_standing_in_for_the_built_in_microphone`.
Proof limit: retry policy and channel-backed replacement audio, not a physical
device reconnect or full native mic-to-target result. The closed-lid cases are
pure resolution and a fixture scene; the IOKit lid read and its notification
have not been observed on a real MacBook closing and opening its lid, and the
headphone-jack UID and data-source codes come from CoreAudio's documented
values, not a jack microphone observed with the lid closed.

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

cold open -> selected microphone fails to open
  -> retry it twice, 100 ms apart -> then the system default (logged)
ready stream fails while the press is held
  ├── before the 300 ms hold threshold -> back to pending, reopen for the same press
  │     ├── release before ready -> discard
  │     └── ready -> capture from the original press -> expected output
  └── after the threshold -> fail with "Microphone capture was interrupted"
CoreAudio processor overload (Xrun) -> warn and keep the stream // not a failure
gap in the microphone timeline (any OS, warm or cold)
  ├── 50–250 ms -> filled with silence, capture continues // logged, #619
  └── over 250 ms -> fail as a capture discontinuity
```
Checks in [dictation/audio.rs](../../src/dictation/audio.rs):
`cold_capture_restart_rejects_late_open_results_and_closes_after_finish`,
`enabling_warm_microphone_preserves_pending_capture_and_recovers_after_cancellation`,
`cold_stream_failure_before_hold_threshold_reopens_for_the_same_press`,
`cold_stream_failure_after_hold_threshold_still_fails_the_capture`,
`a_short_microphone_gap_is_filled_with_silence_instead_of_failing_the_capture`; in
[audio.rs](../../src/audio.rs): `overload_glitches_do_not_interrupt_the_stream`,
`cold_open_retries_the_selected_microphone_before_falling_back`.

**Fixed after 2.1.25 ([#130](https://github.com/anomalyco/hex/issues/130)):** the
first hold after minutes of idle failed with the error tone. The reporter's logs
show the cold stream reporting "A buffer underrun or overrun occurred" 20–76 ms
after opening. cpal 0.18 reports that as `ErrorKind::Xrun` from CoreAudio's
`kAudioDeviceProcessorOverload` listener; the stream keeps running, but Hex
treated every error callback as fatal. In release-when-idle mode the owner then
closed the device without retrying, and recognition suspended the held gesture.
Overloads and refused real-time promotion are now logged and ignored; actual
sample gaps over 50 ms still fail the capture as a discontinuity. An interruption
before the hold threshold re-pends the same press instead of failing it. The same
logs show a cold open falling back from the selected microphone to automatic
selection; whether that device was missing or failed its first open is unknown,
so the cold open retries the selection before the existing fallback.
Not verified: the reporter's hardware, whether 200 ms of retries is long enough
for their device, or a physical cold open after idle.
Proof limit: controlled asynchronous opener and capture ownership, not onset
latency or the macOS microphone indicator.

**Fixed after dev ([#619](https://github.com/anomalyco/hex/issues/619)):** a
Bluetooth headset's stream skipped about 130–145 ms of audio soon after it
started, and every gap over 50 ms failed the held capture with the error tone.
Gaps up to 250 ms are now filled with silence on the timeline, so the capture
(and its pre-roll ring) keeps going; longer gaps still fail it. The Annotate
sink still records the gap itself. Not verified on the reporter's headset; the
report that a Bluetooth device was used with `avoid_bluetooth_microphone` on
was not reproduced (a saved Bluetooth microphone, or no other usable input,
keeps it by design).

### Fence Late Capture Notifications

`recover.late-notification`: keeping a failure visible is correct until it
would be attributed to a different capture.

```ts
capture A fails -> finish/cancel A -> preserve A's terminal failure
start capture B -> new generation                     // fences obsolete notifications
  ├── late failure/ready A -> cannot finish, fail, or activate B
  └── B's own audio -> finish B -> expected output
```
Checks in [dictation/audio.rs](../../src/dictation/audio.rs):
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
Sources: [dictation/audio.rs](../../src/dictation/audio.rs),
`overflow_recognition` and `forward_recognition`;
[recognition.rs](../../src/recognition.rs), discontinuity handling.
Proof limit: source-backed contract only. A whole-path native stall check must
still establish lossless dictation and stale-command rejection together.

`recover.optional-processing`: ordinary dictation falls back to usable text.

```ts
ordinary dictation -> corrections -> optional rewrite -> transformations -> paste
                                      │                  │
                                      v                  v
                             rewrite fails       chain fails
                             -> corrected text   -> chain's input // discard partial results

later request -> expected output                       // recovery still needs observation
```
Stage-specific sources and checks are in the [index](README.md); runaway
scripts are covered below.
Proof limit: fallback behavior alone does not establish that the next request
succeeds.

### Keep Linux Listeners Managed

`recover.linux-listener`: Settings exposes listener status, Start/Stop, and
Retry/Dismiss. Hiding a failure is not restarting its worker.

```ts
listener worker fails -> reap worker -> show failure
  ├── Retry -> new managed listener -> fresh capture -> expected output
  └── Dismiss -> clear error                            // not evidence of recovery

close the window -> Hex keeps running
  └── uncommitted shortcut capture -> cancel edit -> restore prior listening

Hex crashes -> systemd restart-on-failure (no window) OR explicit hex start
  -> hex / hex status -> live state, not stale log state

hex quit / Quit Hex -> drain accepted work -> release microphone and hotkeys -> exit 0
```
Sources/checks: [linux/app.rs](../../src/linux/app.rs),
[Linux CI](../../.github/workflows/check-linux.yml),
[test-wayland-paste.sh](../../scripts/test-wayland-paste.sh), and
[test-linux-service.py](../../scripts/test-linux-service.py).
Proof limit: source and available headless checks, not current supported-host
behavior. Native compositor/device validation remains separate. The
single-instance socket is same-user only; bounded I/O and queues keep client
stalls off the audio loop.

**Fixed after 2.1.24:** on X11 the Escape-cancel grab is best-effort.

```ts
capture starts -> grab Escape with AnyModifier        // recover.linux-escape
  ├── success -> Escape cancels
  └── BadAccess -> try bare Escape + Caps/Num Lock variants
        ├── success -> plain Escape cancels; modified shortcuts stay with their owner
        └── BadAccess -> keep dictating without Escape-cancel
              -> Settings + hex status show a non-fatal listener warning
              -> log once per conflict episode

next capture -> retry grab
  └── success -> clear warning and rearm episode logging
other grab error -> stop listener and report failure
```

The warning persists while idle until a later successful grab, or until that
listener stops. It does not become an `operation_error` or hide other failures.
Release the hold shortcut, or press the locked shortcut, to finish when Escape
is unavailable. With the bare-Escape fallback, release other modifiers before
pressing Escape. A failed fallback releases any partial grabs.

On Wayland, Escape stays with applications. A GlobalShortcuts binding is held
for as long as it is bound and consumed before the focused app sees it (KDE's
kglobalaccel, GNOME Shell's accelerator grabs, Hyprland's `global` binds), and
desktops keep one shortcut set per app, so it can't be bound only while
recording:

```ts
listener starts -> one session, one BindShortcuts: Hex's shortcuts + Cancel dictation
  └── Cancel dictation has no suggested trigger -> the user assigns a key
user's cancel key
  ├── recording or Annotate drawing -> cancel, like Escape   // recover.linux-portal-cancel
  └── idle -> nothing
no key assigned / no portal -> `hex dictation cancel`, or finish with the shortcut
desktop closes the shortcuts session -> cancel active capture
  -> listener stops with an error -> Retry / hex start binds again
```

**Fixed after dev-249** ([#482](https://github.com/anomalyco/hex/issues/482),
reported by ohlmann on KDE Plasma): Hex bound Escape in a second portal session
when recording started. KDE keys shortcuts by app, so that session's
BindShortcuts asked again for "Cancel dictation (Escape)" and, once approved,
removed Hex's other shortcuts, so recording never started. Hex now binds every
shortcut once in one session and never binds Escape there. The cancel id is new
(`cancel-dictation`), so KDE drops the old `cancel` Escape binding instead of
restoring it. Verified against xdg-desktop-portal-kde's `BindShortcuts` and
`setActions` and xdg-desktop-portal-gnome's grab/ungrab source, not on a live
Plasma session.


Checks in [linux/input.rs](../../src/linux/input.rs):
`escape_is_wanted_only_while_something_records`,
`escape_grab_conflict_is_not_fatal`, `non_conflict_grab_errors_stay_fatal`,
and `escape_warning_is_logged_once_per_episode_and_clears_on_recovery`.
`listener_warning_is_nonfatal_and_round_trips_in_status` in
[linux/app.rs](../../src/linux/app.rs) checks status publication, warning
recovery, and listener shutdown. The ignored X11 checks
`conflicted_escape_still_dictates_without_cancel`,
`modified_escape_conflict_keeps_plain_escape_cancel`, and
`conflicted_escape_fallback_releases_partial_grabs` cover real server conflicts,
fresh-capture recovery, fallback cancellation, and partial-grab cleanup on an
isolated display. These checks do not prove microphone-to-target output or
native Settings rendering.

**Executed October 3, 2026:** on x86_64 Arch Linux with Rust 1.97.0, rebased onto
`faa2192`, the working tree passed 157 Rust tests (ten opt-in tests ignored),
formatting, strict all-target/all-feature Clippy, and whitespace checks. The
macOS-only keyboard-layout harness skipped. All six ignored X11 input tests
passed separately on Xvfb. The service IPC/lifecycle smoke passed. An ad hoc
Escape-conflict-to-dictation probe also passed using a separate D-Bus session,
virtual microphone, CPU Parakeet inference, software Vulkan for Settings, and
an isolated GTK paste target. That probe is not retained as a repository test.
No physical microphone, user's clipboard, installed-service restart, or visual
warning check was exercised.

Historical contributor probe before these additions: holding Escape/ANY from
a second client on i3 reproduced server-side BadAccess (error 10, GrabKey opcode
33); `hex status` still showed `Listening` without an `operation_error`. The
151-test binary suite and `test-linux-service.py` passed on that host. That probe
did not verify the new visible warning or bare-Escape fallback.

### Keyboard Layout Resolution

```ts
GUI startup -> require OS main thread -> initialize_layout // recover.keyboard-layout
  -> publish complete snapshot BEFORE settings/workers/Rune
     ├── cached character -> keycode
     ├── missing ASCII character -> filled from ASCII-capable layout
     └── Command layer (UCKeyTranslate with cmdKey) -> separate map
                                     // no worker-side TIS/TSM query

Synthetic shortcut with Command (⌘V)
  ├── Command layer has the character -> that keycode // "Dvorak – QWERTY ⌘"
  └── otherwise -> plain snapshot as above
Shortcuts without Command and recorded hotkeys -> plain snapshot only

Headless lookup without snapshot -> shared native lock -> current layout query
  -> current layout misses ASCII character -> ASCII-capable fallback
Concurrent snapshot construction -> same lock          // serialized, not a separate race
```
Sources: [keyboard.rs](../../src/keyboard.rs),
[macos_rune.rs](../../src/macos_rune.rs).
The lock alone is not GUI thread safety: successful main-thread prewarming and
pure cached misses prevent later GUI workers from querying native layout APIs.
The GUI snapshot remains fixed until restart. CLI lookups without a snapshot
still query the current layout; this fix does not remove their live resolution.
Non-Latin active layouts (Russian, Hebrew, …) produce no ASCII letters with an
empty modifier state, so a snapshot built from them alone would miss paste
shortcuts like ⌘V. macOS resolves such shortcuts against the ASCII-capable
layout; both the GUI snapshot and live lookups now fill missing ASCII letters
from `TISCopyCurrentASCIICapableKeyboardLayoutInputSource` the same way.
Only missing ASCII letters are added, even when some letters already exist;
active mappings and non-letter misses remain unchanged. Duplicate fallback
characters keep the lowest key code, just like the current-layout snapshot.
`non_latin_snapshot_gets_shortcut_letters_without_replacing_active_keys` covers
the merge policy with Cyrillic/Hebrew fixtures; native layout and physical paste
verification remain separate.

Layouts can also switch arrangement while Command is held. "Dvorak – QWERTY ⌘"
types `v` from keycode 47, but ⌘+47 is ⌘. there; ⌘V is keycode 9. Resolving ⌘V
from the plain layer posted ⌘. and the target beeped (kitlangton/Hex#162). The
snapshot and headless lookups now resolve Command shortcuts from the layout's
Command layer first. Ordinary layouts map letters identically on both layers.
`command_shortcuts_resolve_against_the_command_layer` covers the selection and
fallback with Dvorak-QWERTY⌘, QWERTY, and non-Latin fixtures; it does not query
a real Dvorak layout or post a paste.

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

**Contributor-reported September 9, 2026 (PR #81, ASCII-capable fallback):** with the Russian
layout active on macOS 26.6.2 (25G83), Apple M3, a locally built
`/Applications/Hex.app` resolved `paste_key_code=9` at startup and held
Option-dictations pasted into the focused app; the retained
`keyboard_layout` regression still passed all twelve child runs. The
fallback mirrors AppKit's own resolution of shortcuts such as ⌘V on
non-Latin layouts, so a Latin-active snapshot is unchanged.

**Local startup check, September 1, 2026:** Developer ID-signed version 2.1.11,
local build `20111.1`, was installed and launched from `/Applications/Hex.app`.
Bundle identity/signature and the installed executable's match to the signed
build were validated. The new process reported resolved keyboard layout,
loaded transcription model, and Listening, with no startup errors or panic.
The settings file's SHA-256 was unchanged after restart. This local build was
not notarized or published; the previous notarized app was retained for rollback.

**Release verification for 2.1.12:** release commit `f76c115`, build `20112`,
passed 435 Rust tests (nine ignored), all twelve layout-regression child runs in
both debug and release profiles, 46 command-SDK tests, Clippy, formatting, and
identity guards. Linux and Nix CI passed for source commit `f556f9b`; the release
commit changes only the version and release notes.

The app and DMG were Developer ID signed, notarized, and stapled; Gatekeeper
accepted the DMG and its app. The DMG and Sparkle ZIP matched across 207 payload
entries, including file bytes, permissions, and symlink targets. After
artifact-first/feed-last publication, the public DMG, ZIP, and latest-DMG link
matched the prepared SHA-256 checksums. The ZIP's Ed25519 signature verified
against the app's public key, and the public appcast led with build `20112`.
See [release notes](../releases/2.1.12.md) and
[release checks](../../scripts/release-app.sh).

Proof limit: these are native layout-query, local app-startup, and distribution
artifact checks, not physical shortcut/paste acceptance or an exercised Sparkle
installation. The signed release preview created a window, but screenshot
capture failed with Screen Recording permission unavailable; no screenshot
verification is claimed. The installed local app and its settings were not changed.
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

### Runaway Script

```ts
handler or transformation runs past 1 s -> interrupted -> that call fails
  -> transformation chain -> its input is kept           // ordered processing goes on
  -> later call on the same config -> succeeds
config load past 2 s or past 64 MiB -> rejected -> previous config stays active
```
Sources: [script.rs](../../src/personal_commands/script.rs),
[personal_commands.rs](../../src/personal_commands.rs);
check `runaway_scripts_stop_at_the_time_limit_and_the_config_keeps_working`.
Handlers return actions instead of awaiting native results, so the old late
native tool reply ([#20](https://github.com/anomalyco/hex/issues/20)) cannot
occur.

### Slow Paste Consumer

```ts
Chromium 155/156 frontmost? type instead (#212); else publish clipboard -> post Paste
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
