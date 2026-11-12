# Recovery

Recovery means the next fresh capture succeeds, including its expected output.
A cleared error or a live process alone is not proof.

This page owns interruption and failure transitions. [Dictation](dictation.md)
owns normal capture behavior; the [index](README.md) covers optional processing,
platforms, and dependencies.

## Sub-features

- [Finish without losing other work](#finish-without-losing-other-work).
- [Rearm without a phantom capture](#rearm-without-a-phantom-capture).
- [Reopen with fresh audio](#reopen-with-fresh-audio).
- [Keep an interrupted dictation](#keep-an-interrupted-dictation).
- [Fence late capture notifications](#fence-late-capture-notifications).
- [Degrade only the failing dependency](#degrade-only-the-failing-dependency).
- [Keep Linux listeners managed](#keep-linux-listeners-managed).
- [Load settings with invalid fields](#load-settings-with-invalid-fields).

## How To Get To It

Start with normal dictation in a foreground app. Dictation has no cancel
(#786); after an interruption, try a fresh shortcut hold.
On Linux, Settings also exposes Start/Stop and Retry/Dismiss.

```ts
interruption / dependency failure
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
├── Invalid JSON / not a JSON object -> log why -> defaults in memory
│   └── launch says so; invalid JSON names serde's line and column
├── Unreadable file -> log why -> defaults in memory
│   └── no launch note; a save fails rather than overwrite it
└── Save
    ├── Invalid JSON / not an object on disk -> save refused
    │   └── fix or remove the file; original stays exactly as it was
    ├── This process loaded a file that wasn't an object -> save refused
    │   └── even after the file is fixed; a restart (or the Linux poll) clears it
    └── Invalid fields -> preserve exact original first
        ├── settings.json.unreadable-<sha256>, owner-only; same bytes backed up once
        └── Backup fails / original cannot be read -> save fails, original stays
```

Loading repaired settings does not itself rewrite the file. A later settings
save (including a migration save) takes the backup before replacing invalid
fields. A file that is not valid JSON, or is valid JSON but not an object, is
never backed up or replaced: every save is refused until Hex restarts, and
launch keeps running with defaults in memory while naming the parse error. A
process that loaded such a file keeps refusing saves even after the file is
fixed, because its in-memory settings may be stale defaults. Reading the fixed
file does not clear that refusal; the refusal says to restart. On Linux only,
the two-second poll clears it when it adopts a valid or removed file.
Backups are never automatically removed. On Linux, the running Hex checks
`settings.json` every two seconds and adopts an edit made outside the window
(by hand or by an agent), so its modes reach the listener and the next save
keeps it (#970). The check runs only when the file's mtime changes. A file that
isn't a JSON object is never adopted: the running settings stay, the parse
error appears in the settings error surface, and the message is not logged on
every tick. A later check that reads a valid file, or finds the file removed,
adopts it (even when it equals the running settings), clears that error, and
clears the process's refusal marker, so the window's next save works without a
restart. macOS and Windows still read the file only at launch, through
`hex settings set`, or through their own save-and-reload path. Unchosen
`meetings.retention` is
written as an absent key, so older builds with a non-optional retention still
read the file; existing `null` values also load in the current build.

Every object in the settings file has its keys sorted alphabetically, at every
nesting level, while array order is kept, so a file kept in version control
changes only where a setting does. Loading does not depend on key order.

Source and checks: [settings persistence](../../src/app_settings/persistence.rs)
and its [tests](../../src/app_settings/persistence/tests.rs): old-shape retention,
invalid enums, nested types, list entries, unknown keys, invalid JSON and
non-object documents (defaults, refused saves, no backup), a file fixed while
the process still holds stale settings (refused until the explicit clear),
exactly-once private backups, interrupted saves, backup failure, and sorted keys
at every level (`saved_keys_are_sorted_at_every_level`). These checks use
temporary files; they do not exercise a real app launch or the Linux poll.

### Finish Without Losing Other Work

`recover.finish`: nothing cancels a dictation (#786). Escape reaches the app,
and every accepted job pastes in submission order.

```ts
Escape -> nothing: the capture and every accepted job go on
older job + fresh capture -> both complete, in order
```
Check in [parakeet.rs](../../src/parakeet.rs):
`cancellation_replaces_waiting_output_and_ignores_late_completion` (ordered
output). Proof limit: controlled ordered output; not target-app clipboard
consumption.

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
Checks in [suppression/tests.rs](../../src/suppression/tests.rs):
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
tracked key down in combined-session state but up in HID state  // #612
  -> reads as up for both the press check and the poll   // a latched Tab or Q
     no longer blocks every press until Hex restarts
tracked keyboard key reads down in both states but sent no key event,  // #1008
not even a key repeat, for 5 s (TRACKED_KEY_REPEAT_TIMEOUT)
  -> the press check treats it as released               // logged; mouse buttons excluded
     a Tab really held keeps repeating and still blocks the press
     (with key repeat turned off, a key held past 5 s no longer blocks it)
LOCKED + Escape -> Nothing, stale keys or not (#786)
fenced edges, edges before a disabled tap, suspension -> no neutral evidence
```
Checks in [suppression/tests.rs](../../src/suppression/tests.rs):
`option_soon_after_command_tab_records`,
`shortcut_press_after_command_tab_from_dirty_records`,
`typing_after_command_tab_does_not_block_the_next_press`,
`a_delayed_release_cannot_untrack_a_resynchronized_key_press`,
`a_chord_at_the_resynchronized_press_still_discards_it`,
`fenced_and_pre_reset_modifier_edges_are_not_neutral_evidence`,
`stale_press_waits_for_recovery_while_anything_else_is_held`,
`stale_press_requires_a_neutral_keyboard_after_the_tracked_keys`,
`stale_press_finishes_a_lock_and_leaves_escape_unchanged`,
`toggle_mode_finishes_after_command_tab_while_recording`,
`a_latched_key_without_key_repeats_stops_blocking_the_shortcut`,
`a_key_latched_only_in_session_state_reads_as_released` (the state rule only;
no real latched key has been observed by a test). Unit tests read a keyboard
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
Checks in [suppression/tests.rs](../../src/suppression/tests.rs):
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
Checks in [suppression/tests.rs](../../src/suppression/tests.rs):
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
  -> a dictation held past the hold threshold, or a `hex dictation start`
     control capture, keeps what it heard    // recover.interrupted-dictation
     -> transcribed and copied, never pasted
  -> otherwise cancel the incomplete capture + discard stale audio  // interrupted audio cannot return
  -> bounded backoff -> reopen the resolved input     // warm microphone retries
  -> replacement delivers new audio
  -> fresh capture -> release -> expected output
device refuses a stream in its own default format     // #988, Intel Smart Sound on Windows
  -> log the refused format (channels, Hz, sample format) // audio/input_format
  -> try the other formats it lists (F32, I16, U16, I32), at most 12
  -> none opens -> the default format's error, with its format, goes to recovery
no input device at all (macOS)
  -> devices whose name can't be read are skipped     // audio::device_name, never Display
  -> bounded recovery; `hex status` listener: "No microphone"
no input opens, at startup or after an interruption (Linux) // #937
  -> one "could not open the microphone: …" failure per outage // take_failure
  -> last failure filled in while it keeps retrying; reset once one opens
ALSA's default goes through PipeWire's ALSA plugin (Linux) // main, #1628
  -> DISABLE_RTKIT is set first: PipeWire's realtime module fails to start,
     which is ignored and the stream still opens, so the portal's realtime
     budget never lowers RLIMIT_RTTIME // a zero budget killed Hex
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
  │     └── failed dictation -> Settings listener card explains it   // macOS
  │           "Last dictation failed: Built-in mic is off with the lid closed.
  │            Open the lid or connect a mic."
  │           -> clears once a later dictation completes; the listener's own failure keeps priority
  └── lid opens or closes -> re-resolve like a device-list change // IOKit clamshell notification
        -> switch once capture is idle
Bluetooth default input (macOS, avoid_bluetooth_microphone on) // transport type, read per resolve
  -> built-in mic stands in, else the first other real input
  ├── no other usable input -> the Bluetooth default
  └── setting changes -> re-resolve like a device-list change -> switch once idle
virtual input or iPhone Continuity microphone (macOS: ccwd/ccwl transport) // #1100
  -> never stands in: the phone prompts and can fail to open
  ├── lid closed, nothing else -> "the built-in microphone is off while the lid is closed"
  └── the default, the saved microphone, or --device -> used as named

warm stream keeps running but delivers only exact zeros (macOS and Linux hotkey dictation, #1122)
  -> that dictation is discarded as near-silent, as before
  -> its finish reopens the microphone like a stream failure // logged once
     -> the dead stream is closed first, so the replacement opens cold as at launch
     -> macOS: that open makes the device renegotiate its clock first (#1575)
        -> a throwaway stream at another rate the device lists (44.1 kHz for a
           48 kHz device, else 48 kHz, else another listed rate), stopped at once
        -> then the real stream opens in the format remembered before it, so
           cpal sets the device back; a device on a stale rate delivers only
           zeros, and the change is what clears it
        -> the throwaway step failing only logs; the open still runs
  -> next dictation records from a fresh stream

meeting microphone stream delivers only exact zeros for 3 s (macOS, #1518)
  -> only while the call app records from that same input, by device UID,
     checked once a second; a manual recording never reopens or warns
  -> a gated or noise-suppressed microphone the call app also uses still
     counts as held, so it can trigger the reopens and warning
  -> reopen the microphone source, at most twice in one recording (the call
     app holding the input and 3 s of exact zeros), the budget shared by
     every stream it reopens; audio coming back does not restore it, because
     each reopen changes the device's rate; macOS makes the device
     renegotiate its sample rate first, like dictation's silence recovery
     (#1575), which briefly interrupts every other stream on that mic, the
     call app's and a warm dictation's
  -> a renegotiating open spends that attempt when it runs, so a failed
     open retries plainly instead of flipping the rate again; only a
     deferral for dictation leaves it for a later open
  ├── still silent -> keep recording; the recording capsule says
  │     "Microphone is silent · 12:34" and the menu bar "Microphone is silent"
  │     -> manifest.microphone.warning = digital_silence is written as soon
  │        as it is raised, so a crash while recording keeps it
  ├── still silent after the warning -> only a release runs a renegotiating
  │     retry: the set of processes recording from the opened input shrinks,
  │     Hex's own excluded and the call app included, read once a second from
  │     Core Audio process metadata (device lists only, never samples)
  │     -> at most 3 retries per silent episode; silence alone never retries
  │     -> a release seen before the warning still counts, and one seen
  │        before a reopen and gone after it counts too
  │     -> the release is noted where it is seen, so a stream that fails
  │        before its next chunk cannot lose it; the comparison made by our
  │        own renegotiating open, and by any reopen within a second of it,
  │        ignores a disappearing pid: the driver is restarting, not an app
  │     -> needs macOS 14+ process audio metadata, as call-app input
  │        detection does; older macOS warns without retrying
  ├── a dictation captures or infers -> the quick reopens and retries wait
  │     until it is idle; a release stays pending meanwhile, and an open
  │     already decided under dictation opens without renegotiating: the
  │     still-silent stream reopens once dictation is idle, without spending
  │     another reopen or retry
  ├── a device switch -> the old input's release and pending renegotiation
  │     are forgotten
  ├── nonzero audio returns -> clear the live warning, including in a stream
  │     reopened after a supervisor restart, and start the episode's retries
  │     over, though the recording's two reopens stay spent and a pending
  │     renegotiation is cleared; the persisted warning stays, because the
  │     recording means "silent at some point"
  ├── before a warning, the call app leaves the input -> no further quick
  │     reopens or warnings until it reports the input again
  └── a later episode after audio returns -> warns at once (the two reopens
        are spent), then gets its own retries; the finished meeting's panel
        still says the microphone was silent for part of the call
  // not verified on a live device or call app

open stream stops delivering audio without an error (any OS, #1206)
  -> 4 s with no chunk (STREAM_STALL_TIMEOUT) -> treated as a stream failure
  -> reopened like any interrupted stream, idle or not // logged "microphone stopped delivering audio"
     ├── it delivered for 8 s without stopping (STREAM_PROVEN_UPTIME) -> it was working:
     │     reopen at once with the backoff reset // exactly as before
     └── any stream that fails before that never proved steady delivery (#1681)
           -> the first stream or a reopened one; a stall or any other failure
           -> wait a growing backoff, 250 ms doubling to 60 s
           -> a successful reopen keeps it; only 8 s of continuous delivery resets it
           -> a failed open while it stands also grows to 60 s
           -> the dead stream is released before the wait starts
           -> a route change (the lid opening) or a dictation press reopens at
              once, keeping the backoff // never a fast reopen loop again

programmatic dictation (local API) when the stream interrupts or gaps
  ├── audio delivered before the loss -> kept under its id // recover.programmatic-loss
  │     -> /finish transcribes it; /cancel, heartbeats, and the lease still apply
  │     -> also when /finish arrives before the loss is read (take_loss)
  └── no audio since the start (open failed, or pre-roll only)
        -> /finish answers 409 microphone-lost
```
Checks in [audio.rs](../../src/audio.rs):
`an_unreadable_device_name_is_skipped_instead_of_panicking`,
`a_microphone_that_cannot_open_is_reported_once_per_outage`,
`microphone_recovery_retries_with_bounded_backoff_and_resets`,
`startup_retry_recovers_metadata_and_audio_without_restarting`,
`a_missing_selection_uses_the_default_until_it_returns`,
`a_closed_lid_passes_over_the_built_in_microphone`,
`only_the_internal_microphone_is_switched_off_by_the_lid`,
`a_headphone_jack_microphone_is_used_with_the_lid_closed`,
`an_iphone_microphone_never_stands_in`,
`continuity_capture_transports_are_recognised` (pure resolution and fourcc
values; no iPhone observed with the lid closed),
`where_the_os_reports_new_devices_only_a_failed_open_is_retried`,
`only_an_idle_capture_lets_the_microphone_switch`,
`a_stream_that_stops_delivering_audio_is_reopened`,
`a_stream_that_keeps_stalling_backs_off_to_a_minute`,
`a_reopened_stream_keeps_the_stall_backoff_its_predecessors_grew`,
`a_stream_that_delivers_steadily_reopens_at_once_and_forgets_the_backoff`,
`a_stream_that_stops_before_proving_itself_backs_off`,
`a_route_change_ends_a_stall_wait_without_resetting_its_backoff`,
`a_dictation_press_retries_a_stalled_microphone_at_once`,
`a_dead_stream_is_stopped_before_its_replacement_opens`,
`a_silent_stream_reopens_with_a_rate_renegotiation`;
[audio/recovery.rs](../../src/audio/recovery.rs):
`only_a_silence_recovery_renegotiates_the_next_open`,
`a_reopen_or_a_reset_forgets_a_renegotiation_no_open_took`;
[dictation/audio.rs](../../src/dictation/audio.rs):
`recovered_owner_delivers_audio_and_accepts_a_new_capture`,
`microphone_loss_keeps_a_recording_captures_delivered_audio`,
`microphone_loss_with_only_pre_roll_keeps_nothing`,
`finish_after_an_unread_microphone_loss_takes_the_kept_clip`,
`a_dictation_of_digital_silence_reopens_the_microphone`;
[linux/dictation.rs](../../src/linux/dictation.rs):
`submit_capture_reports_when_a_clip_is_digital_silence`,
`a_warm_silent_stream_reopens_once_unless_released`;
[meeting/warning.rs](../../src/meeting/warning.rs):
`a_silence_budget_is_shared_across_reopened_streams`,
`a_warned_microphone_never_retries_without_a_release`,
`a_release_seen_before_the_warning_retries_once_it_shows`,
`a_release_seen_before_a_reopen_still_retries_after_it`,
`a_release_noted_before_a_failed_source_still_retries_after_its_reopen`,
`a_release_triggers_a_warned_retry`,
`a_silent_episode_gets_three_release_triggered_retries`,
`a_rebuilt_policy_keeps_the_silent_episodes_retries`,
`a_warned_microphone_retries_after_the_call_app_lets_go`,
`audible_audio_resets_the_episodes_retries`,
`a_reopen_waits_while_dictation_is_active`,
`a_spent_recording_warns_even_while_dictation_is_active`,
`a_release_triggered_retry_waits_while_dictation_is_active`,
`a_pending_renegotiation_reopens_once_dictation_is_idle`,
`audible_audio_clears_a_pending_renegotiation`,
`a_renegotiation_is_taken_once_and_dictation_defers_it`,
`audible_audio_clears_a_warning_a_reopened_stream_inherited`,
`a_microphone_the_call_app_does_not_hold_is_never_reopened_or_warned`,
`a_sticky_warning_survives_its_live_one_clearing`,
`a_dropped_warning_update_is_not_committed_and_is_retried`,
`a_warning_is_written_to_the_manifest_as_soon_as_it_is_raised`,
`a_device_switch_restarts_the_zero_count_without_clearing_the_warning`;
[meeting/capture.rs](../../src/meeting/capture.rs):
`a_meeting_follows_the_input_its_call_app_records_from`,
`a_microphone_that_would_not_open_is_tried_a_bounded_number_of_times`,
`a_process_that_stops_recording_between_reopens_is_a_release`,
`a_release_just_after_our_own_rate_change_is_not_another_apps`,
`a_release_belongs_only_to_the_input_that_saw_it`,
`only_an_input_the_call_app_reports_counts_as_held`;
[meeting/tests.rs](../../src/meeting/tests.rs):
`a_meeting_manifest_round_trips_a_microphone_warning_and_old_ones_have_none`
(the exact-zeros watch and reopening are not verified on a live device or a
call app holding the microphone with voice processing);
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
values, not a jack microphone observed with the lid closed. The listener card's
lid-closed wording is a pure mapping and shared projection test
(`known_microphone_failures_get_an_actionable_explanation`,
`a_startup_reader_ignores_the_previous_sessions_failure`); the macOS
card is not natively verified.

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
  └── after the threshold -> keep the audio delivered, transcribed and
        copied (recover.interrupted-dictation), or fail with
        "Microphone capture was interrupted" when none arrived or it is
        near-silent
Microphone overload (Xrun) or refused real-time promotion -> counted; one warning per burst // not a failure
  └── Some hosts report it from the thread that runs the stream: that callback only
      counts, the poll that drains the stream writes the count at most once per 10 s,
      and a stream closed with a count left over writes it as it closes
gap in the microphone timeline (warm or cold)
  ├── macOS: 50–250 ms -> filled with silence, capture continues // #619
  │     └── over 250 ms -> fail as a capture discontinuity
  └── Linux: 50 ms–4 s -> filled with silence // timestamps are wall-clock derived
        └── over 4 s -> restart the capture; audio across it is never stitched
```
Checks in [dictation/audio.rs](../../src/dictation/audio.rs):
`cold_capture_restart_rejects_late_open_results_and_closes_after_finish`,
`enabling_warm_microphone_preserves_pending_capture_and_recovers_after_cancellation`,
`cold_stream_failure_before_hold_threshold_reopens_for_the_same_press`,
`cold_stream_failure_after_hold_threshold_still_fails_the_capture`,
`cold_stream_failure_keeps_the_audio_a_held_capture_already_delivered`,
`a_short_microphone_gap_is_filled_with_silence_instead_of_failing_the_capture`;
in [linux/dictation/gap.rs](../../src/linux/dictation/gap.rs):
`a_short_microphone_gap_is_padded_with_silence`,
`a_gap_past_the_stream_stall_timeout_is_not_stitched`; in
[linux/dictation.rs](../../src/linux/dictation.rs):
`a_short_microphone_gap_is_padded_into_a_finished_clip`; in
[audio.rs](../../src/audio.rs): `overload_glitches_do_not_interrupt_the_stream`,
`cold_open_retries_the_selected_microphone_before_falling_back`; in
[audio/overloads.rs](../../src/audio/overloads.rs):
`a_burst_writes_one_line_and_holds_the_rest`.

**Fixed after 2.1.25 ([#130](https://github.com/anomalyco/hex/issues/130)):** the
first hold after minutes of idle failed with the error tone. The reporter's logs
show the cold stream reporting "A buffer underrun or overrun occurred" 20–76 ms
after opening. cpal 0.18 reports that as `ErrorKind::Xrun` from CoreAudio's
`kAudioDeviceProcessorOverload` listener; the stream keeps running, but Hex
treated every error callback as fatal. In release-when-idle mode the owner then
closed the device without retrying, and recognition suspended the held gesture.
Overloads and refused real-time promotion are now ignored; actual
sample gaps over 50 ms still fail the capture as a discontinuity. An interruption
before the hold threshold re-pends the same press instead of failing it. The same
logs show a cold open falling back from the selected microphone to automatic
selection; whether that device was missing or failed its first open is unknown,
so the cold open retries the selection before the existing fallback.
Not verified: the reporter's hardware, whether 200 ms of retries is long enough
for their device, or a physical cold open after idle.
Proof limit: controlled asynchronous opener and capture ownership, not onset
latency or the macOS microphone indicator.

**Changed after dev (overload logging):** a host can report an overload, or a
refused real-time promotion, on the thread that runs the stream. The old line
formatted itself there and wrote to stderr and `process.log` behind a mutex,
once per overload. Hex now only counts them there, and writes one
`microphone stream glitched; continuing` line with `count` from the poll that
drains the stream, at most once every 10 s. A count that window holds back is
written by that poll later, and a stream that closes with a count left over
writes it as it closes (`Drop for AudioInput` calling `overloads::flush`) rather
than beside the next open's lines hours later. `since_last_stream_glitch` still
answers capture diagnostics from the same counters. On macOS the first chunk of
each open also logs the callback's frames and period, which is the hardware
period cpal does not report for `BufferSize::Default` (skipped under
`HEX_AUDIO_FIXTURE`, whose chunks are the file's); no CoreAudio FFI was added.
Check: `a_burst_writes_one_line_and_holds_the_rest` covers the window;
`overload_glitches_do_not_interrupt_the_stream` covers counting instead of
failing. Not verified: a real overload burst after this change, whether the
coalesced rate suits a loaded machine, or the close-time flush on a device that
glitches.

**Fixed after dev ([#619](https://github.com/anomalyco/hex/issues/619)):** a
Bluetooth headset's stream skipped about 130–145 ms of audio soon after it
started, and every gap over 50 ms failed the held capture with the error tone.
Gaps up to 250 ms are now filled with silence on the timeline, so the capture
(and its pre-roll ring) keeps going; longer gaps still fail it. The Linux
listener pads gaps up to 4 s into its capture, because its cpal timestamps are
wall-clock derived; a longer gap ends the capture and starts a fresh one, so
audio from either side is never stitched together. On both platforms, the
Annotate sink still records the timestamp gap itself.
Not verified on the reporter's headset; the
report that a Bluetooth device was used with `avoid_bluetooth_microphone` on
was not reproduced (a saved Bluetooth microphone, or no other usable input,
keeps it by design).

**Fixed after dev ([#1122](https://github.com/anomalyco/hex/issues/1122)):**
after a WhatsApp desktop call ended, the warm built-in microphone stream kept
running without an error but delivered only exact zeros, so every dictation
was discarded as near-silent (`zero_fraction` 1.0) until Hex restarted. A
finished dictation whose audio through the release is exactly zero now
reopens the stream the way a stream failure does, closing the dead stream
before its replacement opens so the new one starts cold as at launch; a cold
microphone (release-while-idle) already opens fresh for each press. Linux hotkey
dictation does the same for a warm microphone; service and programmatic
captures do not check it, and one released while idle opens fresh. The
first such dictation is still discarded. Not verified with a real WhatsApp
call.

**Fixed after dev ([#1575](https://github.com/anomalyco/hex/issues/1575)):** a
silence reopen opened its stream in the device's current default format. A
device a call app had just let go could already report that format as its own,
so cpal's physical-format change did nothing and the reopened stream delivered
zeros too, until Hex restarted. The one open that follows digital silence now
renegotiates the device's clock first, on macOS: it opens a throwaway stream at
another rate the device lists (44.1 kHz for a 48 kHz device, else 48 kHz, else
another listed rate) and stops it, then opens the real stream in the format it
remembered before the throwaway, so cpal sets the device back. A stream-failure
reopen is unchanged, the renegotiation is used by exactly one open, and a
throwaway that fails only logs. Field logs showed the one same-process reopen
that recovered was followed by cpal reporting a sample-rate change and another
reopen; this makes that change happen on purpose.
Not verified on a device that stayed silent before this change.

**Meeting capture ([#1518](https://github.com/anomalyco/hex/issues/1518)):**
during a meeting, the microphone stream kept delivering chunks of exact 0.0
samples for the whole call while the call app held the built-in microphone,
so the existing stall watchdog never fired and the user's side was missing.
A microphone stream that delivers 3 s of exact zeros now reopens like a
failed source, at most twice in one recording (the call app holding the
input and 3 s of exact zeros), but only while the call app still reports
recording from the same input; a manual recording, or an app that reports no
input, never reopens or warns, and a gated or noise-suppressed microphone the
call app also uses still counts as held so it can trigger the reopens and
warning. The budget is per recording and audio coming back does not restore
it, because every silent reopen renegotiates the whole device's nominal rate
and a gated or noise-suppressed mic can be silent during every listening
stretch of the call. On macOS each of those reopens renegotiates the device's
sample rate first, like dictation's silence recovery (#1575); that changes
the nominal rate of the whole device, so it briefly interrupts every other
stream on the microphone, including the call app's and a warm dictation's.
If it is still silent, Hex keeps recording and warns in the recording
capsule, the menu bar, and the finished meeting's detail. The manifest
records `microphone.warning: digital_silence` as soon as it is raised, so a
crash keeps it, and it means the microphone was silent at some point during
the recording. Once warned, a renegotiating retry runs only when another
process stops recording from the opened input: Hex reads Core Audio's
process metadata (device lists, never samples) once a second, excludes its
own process, and includes the call app, so its release also counts. The
shared last-known set survives a supervisor reopen, so a process seen before
one and gone after it counts as a release, a release is noted where it is
seen so a stream that fails or stalls before its next chunk cannot lose it,
and a release seen before the warning still counts once it shows. The
comparison made by our own renegotiating open, and by any reopen within a
second of it, ignores a disappearing pid: the driver is restarting its
streams, not another app. Release detection needs macOS 14+ process audio
metadata, as call-app input detection does, so older macOS raises the
warning without retrying. Switching inputs forgets the old input's release
and pending renegotiation. A renegotiating open takes the renegotiation when
it is attempted, so a failing open retries plainly rather than flipping the
rate on every supervisor retry, and a successful one spends a release noted
before it. Silence alone never retries, and one silent episode gets at most 3
such retries. A dictation capture or inference defers both the quick reopens
and the retries until it is idle, while the warning is still raised, and a
release stays pending meanwhile; an open already decided under dictation
opens without renegotiating, and the still-silent stream reopens once
dictation is idle without spending another reopen or retry, so a rate change
never cuts a dictation. Nonzero audio clears the live warning, including in
a stream reopened after a supervisor restart, and starts the silent
episode's retry count over, clears a pending renegotiation, and leaves the
recording's two quick reopens spent. A later episode warns at once, then
gets its own retries. Each gated episode is also
logged once at info level with the opened device and the call app's reported
inputs, so field logs can confirm the gate. The 3 s watch, its per-recording
reopen budget, the release-triggered retry cap, and its gating are
unit-checked; none of it was observed on a live device or a call app holding
the microphone with voice processing.

**Fixed after 3.3.0 ([#1206](https://github.com/anomalyco/hex/issues/1206)):**
after a burst of overrun warnings while idle, a warm USB microphone's stream
stopped delivering audio without reporting an error, so every dictation held
only the stale pre-roll and was discarded as no speech until Hex restarted. An
open stream that delivers nothing for 4 s now counts as failed and is reopened
with the usual bounded recovery. Not verified on the reporter's microphone.

### Keep An Interrupted Dictation

`recover.interrupted-dictation`: a shortcut dictation held past the hold
threshold, or a `hex dictation start` control capture, whose microphone fails
keeps what it already heard instead of dropping the whole dictation.

```ts
shortcut held past the hold threshold, or a `hex dictation start` capture
  -> stream fails or stalls                         // a gap over 250 ms still drops it
  -> audio delivered up to the loss is kept        // pre-roll alone keeps nothing
  -> transcribed and processed like a dictation
     └── Dictate without processing -> copied as heard, raw history
  -> final text copied, never pasted              // the disrupting app may hold focus
     -> Paste Last Dictation and History keep it
  -> no error tone or Failed HUD; the HUD transcribes, processes, completes
  -> no audio after the start, or a near-silent clip
     -> the previous failure, "Microphone capture was interrupted"
  -> release or finishing press queued before the listener reads the loss
     -> that finish takes the kept clip, once, with its own capture's target
        and context, before a later press can start
     └── the next press still records; the old loss arrives stale
```

The copied text is processed exactly like a normal dictation, including mode
corrections, OpenCode rewriting, and transformations, unless it came from
Dictate without processing, which is copied as heard and recorded as raw
history. The copy is forced for this job whether or not Labs › Copy dictation
only is on. The shortcut machine is suspended, so the eventual key release does
nothing, and the microphone reopens with the usual bounded recovery. A
microphone gap over 250 ms still fails the capture as a discontinuity, and a
near-silent clip saves its diagnostics but keeps the previous failure.

Checks in [dictation/audio.rs](../../src/dictation/audio.rs):
`microphone_loss_keeps_a_recording_captures_delivered_audio` (a recording
shortcut keeps its clip; a gap still keeps only a programmatic capture's),
`cold_stream_failure_after_hold_threshold_still_fails_the_capture` (nothing to
keep keeps nothing), and
`cold_stream_failure_keeps_the_audio_a_held_capture_already_delivered` (a held
cold capture keeps what it delivered), and
`a_shortcut_finish_takes_its_kept_clip_once_and_the_loss_stays_queued`. In
[recognition.rs](../../src/recognition.rs):
`a_release_queued_behind_a_microphone_loss_copies_the_kept_audio_before_the_next_press`
(with Release microphone while idle, the release and the next press queued
behind the loss: one salvaged submission, and the next press records). In
[parakeet.rs](../../src/parakeet.rs):
`a_salvaged_dictation_is_copied_and_kept_for_paste_last_and_history` (normal
and raw clips are both copied, with normal and raw history). Proof limit:
channel-backed audio and the output worker, not a physical microphone taken by
a call app or a target app's clipboard.

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

`recover.gpu-full`: on Linux and Windows a model, language, or Speech engine
change loads beside the running model, so a small GPU (2 GB) can run out of
memory for the second copy (#1170).

```ts
new model loads beside the running one -> GPU out of memory -> CPU copy loads
  -> the CPU copy is dropped (not swapped in); the GPU isn't remembered as failed
  -> once nothing records or transcribes -> engine drops its model, then loads
     the new one in its place (GPU again, CPU only if it still doesn't fit)
  -> commands sent meanwhile wait for the load; nothing recorded is lost
```
Source: `ModelSwap::place`/`take_reload` in
[transcription/local_transcriber.rs](../../src/transcription/local_transcriber.rs)
and `Command::Reload` in [transcription/engine.rs](../../src/transcription/engine.rs).
Check: `a_model_without_room_on_the_gpu_reloads_in_place_once` (swap state
only). Not observed on a real 2 GB GPU. A second process (another `hex`
command that loads a model itself) still loads its own copy.

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
                             // the log says why: rewrite off for the mode, or the failure reason

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

Wayland listener starts without wl-copy -> process.log warns once; Settings and
`hex status` carry the warning
  -> capture and transcription still run; copy and paste fail naming wl-clipboard

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

**Escape on Linux since #786:** dictation has no cancel, so Escape stays with
applications on X11 and Wayland. Hex grabs it on X11 only while Annotate
draws, where it stops drawing; a grab conflict is logged once and drawing goes
on. The portal never binds Escape or any cancel shortcut, and the shortcut set
is bound once in one session.

```ts
listener starts -> one session, one BindShortcuts: Hex's shortcuts only
  └── KDE -> remove the stale `cancel` and `cancel-dictation` entries first
desktop closes the shortcuts session -> cancel active capture
  -> listener stops with an error -> Retry / hex start binds again
X11, Annotate drawing -> grab Escape (AnyModifier, else bare + lock variants)
  └── BadAccess -> warn once, keep drawing; other grab errors stop the listener
```

`recover.linux-portal`: on Wayland, a missing GlobalShortcuts portal is
looked for again instead of leaving shortcuts off for the session (#875).

```ts
listener starts -> no GlobalShortcuts portal
  -> warn (desktop name, restart-the-portal hint) -> hex commands drive dictation
  -> check again on its own thread: after 15 s, doubling up to every 2 min
     └── portal appears (e.g. xdg-desktop-portal restarted) -> bind shortcuts as usual
```

**Fixed after dev-520** ([#875](https://github.com/anomalyco/hex/issues/875)):
on KDE Plasma Wayland, xdg-desktop-portal started before Plasma exported its
session environment picked the wrong backend and offered no shortcuts
interface; Hex fell back to commands and never checked again. Source:
`Backend::wait_for_portal` in [linux/input.rs](../../src/linux/input.rs);
`commands_alone_drive_a_listener_without_shortcuts` covers the command loop,
not the portal appearing. Not run on a live Plasma session.

**Fixed after dev-249** ([#482](https://github.com/anomalyco/hex/issues/482),
reported by ohlmann on KDE Plasma): Hex bound Escape in a second portal session
when recording started. KDE keys shortcuts by app, so that session's
BindShortcuts removed Hex's other shortcuts, so recording never started. Hex
binds every shortcut once in one session and never binds Escape there.

Checks in [linux/input.rs](../../src/linux/input.rs):
`escape_never_ends_a_dictation`, `escape_grab_conflict_is_not_fatal`,
`non_conflict_grab_errors_stay_fatal`, and the ignored X11 checks
`commands_and_grabs_share_the_x11_listener` (Escape stays free while a capture
records) and `conflicted_escape_fallback_releases_partial_grabs`. The KDE
removal has not run on a live Plasma session.

### Keyboard Layout Resolution

```ts
GUI startup -> require OS main thread -> initialize_layout // recover.keyboard-layout
  -> publish complete snapshot BEFORE settings/workers/Rune
     ├── cached character -> keycode
     ├── missing ASCII character -> filled from ASCII-capable layout
     └── Command layer (UCKeyTranslate with cmdKey) -> separate map
                                     // no worker-side TIS/TSM query

Selected input source changes (Dvorak -> U.S.)       // recover.keyboard-layout-change
  -> observer marks it -> pump on the main thread -> refresh_layout
  -> rebuild with the same fills -> publish whole       // workers see old or new
  -> changed -> resolve runtime shortcuts again from the last applied settings
     ├── letter key -> the key that types the letter now
     ├── other keys -> recorded key code
     └── listener rebinds each shortcut once its machine is idle

Synthetic shortcut with Command (⌘V)
  ├── Command layer has the character -> that keycode // "Dvorak – QWERTY ⌘"
  └── otherwise -> plain snapshot as above
Shortcuts without Command -> plain snapshot only

Headless lookup without snapshot -> shared native lock -> current layout query
  -> current layout misses ASCII character -> ASCII-capable fallback
Concurrent snapshot construction -> same lock          // serialized, not a separate race
```
Sources: [keyboard.rs](../../src/keyboard.rs),
[macos_rune.rs](../../src/macos_rune.rs).
The lock alone is not GUI thread safety: successful main-thread prewarming and
pure cached misses prevent later GUI workers from querying native layout APIs.
Until t1304 the GUI snapshot stayed fixed until restart: launched on Dvorak and
switched to U.S., ⌘V posted keycode 47 (U.S. `.`), so dictations logged
"pasted" but nothing appeared. One input-source observer now runs for the
app's life (keyboard model switching reads the same one while it is on), and
each change rebuilds the snapshot on the main thread.
`a_published_layout_replaces_the_one_before`,
`readers_see_a_whole_layout_while_it_is_replaced`, and
`letter_shortcuts_resolve_again_on_a_new_layout` cover the swap and the
shortcut re-resolution; the TIS notification and a real switch followed by a
paste are unverified on a Mac. CLI lookups without a snapshot still query the
current layout.
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
Caps Lock remapped to Control
  -> IOHIDKeyboardFilter rewrites usage 0x39 to Right Control (0xE4)
  -> Quartz flags carry Right Control; the event keycode cannot tell them apart
  -> raw IOHIDManager below the filter reads Caps Lock 0x39 and E0-E7
  -> side bit set, its physical key not held, Caps Lock held
     -> drop the side bit, keep the general flag -> matches as either side
  -> physical E4 held -> a genuine Right Control press is untouched
  -> Caps Lock's flags event reaches the tap before the raw callback
     marks Caps Lock held -> flags pass unchanged
     -> Right Control still starts; correctness needs the raw callback first,
        which is not guaranteed
  -> raw callback lags a real side key while Caps Lock is held
     -> that one event can lose the side until the next event
  -> IOHIDCheckAccess not granted, open fails, or start times out
     -> tracker inactive, CoreGraphics flags unchanged (today's behavior)
```
Source: [suppression/physical_keys.rs](../../src/suppression/physical_keys.rs)
(IOHIDManager on its own run loop, one `PhysicalKeyMonitor` per input monitor,
started only when `IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)` is granted),
[app_settings/hotkeys.rs](../../src/app_settings/hotkeys.rs) (`PhysicalModifierKeys`
and `normalized_modifier_flags`), [suppression.rs](../../src/suppression.rs)
(flags-changed and key-event flags), and
[rune_ui/macos_host.rs](../../src/rune_ui/macos_host.rs) (the shortcut recorder).
Checks: the normalization tests in `app_settings/hotkeys.rs`,
`normalized_remapped_caps_lock_starts_only_a_side_less_control_binding`, and
`a_remapped_caps_lock_does_not_hold_a_real_right_control_capture` in
`suppression/tests.rs` (constructed snapshots and events).
Issue [#37](https://github.com/anomalyco/hex/issues/37).
Missing evidence: a physical Caps Lock-to-Control remap has not been run in CI
or on hardware, and ordering the raw callback before the Caps Lock flags event
is unproven. Hex never requests Input Monitoring, so the tracker runs only when
Hex already has it (turn Hex on under System Settings › Privacy & Security ›
Input Monitoring and restart Hex); otherwise it stays off and Right Control
still starts from a remapped Caps Lock. The shortcut recorder normalizes only
while the listener's input monitor is running; a start that takes too long is
told to give up rather than delay the listener.

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
transformation runs past its timeout -> interrupted -> that call fails
  // 1 s, or 30 s when the config uses fetch or node:child_process; 60 s cap
  -> its requests abandoned, its process trees killed
  -> transformation chain -> its input is kept           // ordered processing goes on
  -> later call on the same config -> succeeds
fetch or exec before the person allows the config -> throws -> input kept
config load past 2 s or past 64 MiB -> rejected -> previous config stays active
```
Sources: [script.rs](../../src/personal_commands/script.rs),
[personal_commands.rs](../../src/personal_commands.rs);
check `runaway_scripts_stop_at_the_time_limit_and_the_config_keeps_working`,
`the_transformation_timeout_ends_the_whole_process_tree` (Linux), and
`a_throwing_or_slow_transformation_pastes_the_text_unchanged` in
[tests/host_apis.rs](../../src/personal_commands/tests/host_apis.rs).
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
