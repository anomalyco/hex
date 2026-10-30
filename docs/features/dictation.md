# Dictation

Hold the configured shortcut, speak, and release. Hex transcribes locally and
inserts the result at the current focus. New captures do not cancel accepted
work; cancellation prevents insertion only before output commits.

This map covers the macOS release path. Linux shares the basic journey, not
every timing, input, or recovery guarantee; see the
[platform map](README.md#other-platforms-and-consumers). This is a map, not a
verified end-to-end driver. Linked checks are locators; no tests or previews
were run for this rewrite.

## How To Get To It

Settings shortcut capture persists the candidate before showing Saved. If the
write fails, capture remains active with an error; retry or cancel it explicitly.
The GPUI regression that injected this failure went with GPUI; no Rune test
injects a failed shortcut save yet.

**Shortcut:** Option by default, after macOS permissions and the selected local
model are ready. Commands and OpenCode are not required. General > Dictation >
Shortcut accepts modifier-only, modifier-plus-key, standalone Fn/Globe, and
standalone function-key bindings. On macOS, Windows and Linux/X11 it also accepts mouse buttons,
alone or with modifiers: click Record, then press the desired mouse button.
Mouse 1/2/3 are left/right/middle; Mouse 4 and higher are auxiliary buttons.
Mouse 1 (left click) needs a modifier: alone it would swallow every click,
Settings' own included, so the recorder answers "Add a modifier", the CLI
refuses `mouse1`, and a saved bare left-click shortcut goes back to its
default when settings load (issue 411; checks
`a_saved_bare_left_click_shortcut_returns_to_the_default_on_load` and
`a_bare_left_click_is_refused_but_other_mouse_buttons_are_not`, constructed
settings only, no physical mouse run).
Escape cancels recording a binding. Mouse bindings use the same hold, toggle,
and double-tap rules and conflict checks as keyboard bindings. A consumed
mouse press, drag and release do not reach the foreground app; ordinary clicks
retain their existing behavior. Capturing a replacement suspends global matching.
The CLI accepts the same bindings, for example `mouse4` or `option+mouse4`.
Windows supports the five mouse buttons exposed by its low-level mouse hook.
Linux/X11 maps Mouse 4/5 to native buttons 8/9, skipping wheel events; the
recorder temporarily grabs the pointer while its window has focus and releases
it on save, cancellation, focus loss or window close. Windows waits for the
click that opened the recorder to be released before accepting a mouse button.

Wayland's XDG shortcut syntax has no mouse-button trigger. Use the desktop's
mouse bindings to run `hex dictation start`/`stop` (hold) or `hex dictation toggle`
(toggle), or `hex paste-last`. Existing mouse
bindings from X11 appear ungranted on Wayland; they do not prevent keyboard
shortcuts from registering. Hex does not use raw input devices or XWayland to
bypass the compositor. See the [XDG shortcut specification](https://specifications.freedesktop.org/shortcuts/latest/).

Checks: `mouse_buttons_record_with_or_without_modifiers`,
`mouse_bindings_persist_and_conflict_only_with_the_same_trigger`,
`mouse_hold_and_release_use_the_dictation_state_machine`,
`mouse_double_tap_locks_and_a_fresh_press_finishes`,
`mouse_toggle_and_escape_use_the_same_capture_controls`,
`mouse_held_during_stale_keyboard_recovery_is_not_forgotten`,
`mouse_release_stays_balanced_after_its_binding_is_removed`, and
`mouse_suppression_balances_release_and_drag_even_after_modifiers_change`.
These reducer/recorder checks passed on October 25, 2026, with the full macOS
`scripts/check` suite. The locally signed macOS build was installed and the user
confirmed it worked. Linux input-module tests and Xvfb button injection checks
(`x11_mouse_grab_delivers_hold_and_release` and
`x11_mouse_recorder_captures_auxiliary_button_and_releases_its_grab`) passed in
an isolated Linux container. The full Linux app passed `cargo check` with the
transcription Vulkan backend disabled for the container's older shader compiler.
Windows input code and tests were cross-checked for `x86_64-pc-windows-gnu` in
an isolated input-module harness; no full Windows app build or physical Windows
mouse test was run. Xvfb does not establish physical Linux desktop behavior.

The shortcut's keys sit in a recessed tray as tall as the recorder, between
the macOS side picker and the recorder; with no shortcut the tray reads Off.
On macOS and Windows the recorder shows the modifiers held so far as pressed
keys (`⌥ ⌘ …`); a key records them at once, and releasing them all records the
most held. Saved shows for a second, then the recorder returns to Change
shortcut. The recorder keeps one width in every state, so it, the tray, and the
side picker never move while it records; with the picker showing, keycaps leave
the side to it, and otherwise they name it (`Left ⌥`).
Checks: `a_recorder_shows_the_modifiers_held_so_far` in `rune_ui/tests.rs`,
`the_shortcut_row_holds_still_through_every_face_and_side`,
`a_new_shortcut_s_keys_stay_inside_their_tray`, and
`esc_stays_on_the_recorder_however_many_keys_are_held` in
`rune_ui/tests/panes.rs`, the `shortcut-row*` scenes, and the `ChordCapture`
tests in `rune_ui/macos_host.rs` (no native end-to-end run).

**Fixed October 29, 2026 (issue 400):** macOS marks every function, arrow, and
navigation key press with the Fn flag, and the recorder leaves it out of the
binding, so a recorded F14 (or ⌘F14 for Voice Action) never matched and never
started. Matching now ignores that flag only for a binding on one of those keys
that does not ask for Fn; Fn stays significant everywhere else. Check:
`function_keys_match_despite_the_fn_flag_macos_adds` in `suppression.rs`
(constructed events; no physical-keyboard run).

**Fixed October 30, 2026 (issue 462):** the Globe key can also send its own key
event (key code 179) beside the Fn modifier change, seen with
Karabiner-Elements running. A Fn shortcut read that event as another key pressed
with it and discarded most presses as a chord 60–130 ms after they started.
For a shortcut that holds Fn, key code 179 is now neither a chord key nor HID
chord evidence; any other key still is. Check:
`globe_key_events_are_not_a_chord_with_the_function_shortcut` in
`suppression.rs` (constructed events; no physical-keyboard run).

### Chord Shortcuts

**Added October 29, 2026 (issue 394):** every shortcut (dictation, Voice
Action, Paste Last, Annotate) is a chord: the set of keys held together, in any
order and nothing else. A modifier key names its side (left, right, or any);
at most one other key or mouse button joins them. Both Options together is
`[⌥ left, ⌥ right]`, so it no longer reads as plain Option.

```ts
Press the chord's keys -> Down on the edge that completes the set  // dictate.chord
  ├── One key (⌥, Right ⌥, F5, Mouse 4) -> Down at once, no added wait
  ├── Both sides of one modifier (⌥⌥)
  │   ├── Second side within 80 ms of the first, either order -> Down
  │   └── One held, the other added later -> Nothing until both lift
  ├── A side alone (Right ⌥) with the other side also down -> Nothing
  ├── Option (any side) -> One Option or both, as before chords
  └── Modifiers then a key (⌥ Space) -> Down on the key, as before
Lift any of the chord's keys -> Up
Hold, double-tap lock, Toggle, the 300 ms hold threshold, the provisional
start tone, extra-key discards, stale recovery, and the release tail run on
Down and Up unchanged, on the same event timestamps.
```

Karabiner-Elements' simultaneous keys default to 50 ms against typing rollover;
two thumbs on two modifiers do not roll during typing, so the window is 80 ms
(`BOTH_SIDES_WINDOW`). The recorder keeps a lone modifier's side (the Left ·
Either · Right picker widens it, and shows only for one-modifier chords),
keeps both sides of a modifier, and records every other modifier as either
side, so ⌥⌘ works from either hand. While recording, every held key shows with
its side (`L⌥ R⌥ ⌘` on the Mac, `Left Alt` elsewhere), so an accidental extra
key is visible before release. `hex settings` takes
`left-option+right-option`. Settings store `{"keys": [...]}`; files with the
old `modifiers`/`key` shape load as the same chord and are written back once.
Windows matches sides through its low-level hook and accepts both Alts (two
keys) as a modifier-only chord; one modifier alone stays refused there. Linux
binds what the portal and X11 grabs take, a key with modifiers and no side:
releasing modifiers pressed alone in the recorder says Linux needs a key (or
can't bind both sides) and suggests adding one, such as Space.

Checks: the `both_options_*` tests and
`one_sided_options_never_start_with_both_held_but_either_side_does` in
`suppression.rs`, `shortcuts_store_as_chords_and_old_files_convert_once` and
`both_sides_conflict_only_with_either_side_and_match_only_together` in
`app_settings.rs`, `both_options_record_as_one_chord_of_two_keys` in
`rune_ui/macos_host.rs`, `both_sides_must_go_down_together` in
`windows_input.rs`, and
`a_modifier_alone_says_linux_needs_a_key_and_suggests_one` in
`rune_ui/tests.rs`. Constructed events only; no physical keyboard run on any OS.

**CLI:** `hex listen` uses saved settings; `--device` is authoritative. This is
the CLI listener, not Settings. Do not run it beside an existing listener.

**Separate targets:** Host-owned API capture has its own ownership protocol and
does not prove the physical dictation shortcut; see the [feature index](README.md).

## Sub-features

### Vocabulary Quality Checks

`hex benchmark-transcription` accepts a scoring vocabulary and expected terms,
and reports recall, excess vocabulary occurrences and WER by corpus group, plus
overall latency. Its companion scorer also reports per-group latency.
The [fixture guide](../../tests/fixtures/vocabulary/README.md) describes
the synthetic set, public guard download, and ignored private manifest path.
This is offline evaluation coverage, not a shipped vocabulary feature or proof
of recognition quality on a microphone. Metric checks live in
`transcription_benchmark::tests`; inference results belong in the research report.

### Hold And Short Taps

```ts
Ordinary hold                            // lock activation is mapped below
  ├── Hold -> Speak -> Release -> Finish          // dictate.hold
  ├── Release before 300 ms -> Discard            // dictate.short-tap
  ├── Early unrelated shortcut/click -> Discard   // not intentional dictation
  │   └── Includes another app's global shortcut  // dictate.hidden-chord
  │       (Spaceman's Option-1) -> Seen at HID -> Discard, no double-tap lock
  ├── Modifier-only hold + any key, at any time -> Discard  // dictate.held-chord
  │   (key reaches the app; locked and toggle captures keep recording)
  └── Modifier-only hold past 300 ms -> One unassigned key press  // dictate.chord-breaker
      -> Other apps' modifier-tap hotkeys see a chord, not a tap

Ordinary shortcut -> No recording mute/pause or idle-sleep prevention
Intentional recording -> No automatic duration limit
```

Source: `handle_hotkey_action` in [recognition.rs](../../src/recognition.rs).
Checks in [suppression.rs](../../src/suppression.rs):
`callback_timestamps_preserve_nanoseconds_and_short_tap_discard`,
`callback_press_reaches_intentional_hold_on_the_audio_clock`.
These cover constructed callback/reducer timing, not physical event delivery.

A chord discard logs its cause to `process.log` (`dictation press discarded as
a chord`, or `dictation press dropped` when HID evidence overtook the press)
with the key code, the milliseconds after the press, and for HID evidence the
raw timestamp, how it was read, and its arrival delay. The `discarded` event in
`live.ndjson` carries the same cause as `reason`: `key`, `hid-key`, `mouse`, or
`extra-modifier`. Short taps have no `reason`.

**Fixed October 22, 2026:** a global shortcut registered with Carbon (Spaceman's
Option-1 to Option-0 workspace keys, for example) consumes its key before the
session taps, so Hex saw only the Option press and release. Holding Option past
300 ms while switching workspaces recorded and pasted whatever the microphone
heard, and two quick switches could double-tap lock. A listen-only HID tap now
reports key presses as chord evidence only; session events still own key state,
and Escape and Hex's own shortcuts are excluded. Evidence that overtakes its
press still prevents the capture. Command-Tab likewise swallows Tab's release,
which caused the routine `resynchronized stale input tracking` warnings, now
logged with `key_codes`. Until October 26 an Option press within 100 ms of
Command-Tab was silently ignored; the press now resynchronizes itself
(`recover.shortcut-press` in [recovery](recovery.md)). Checks:
`hid_evidence_discards_a_chord_hidden_by_a_global_shortcut`,
`hidden_chords_in_quick_succession_never_double_tap_lock`,
`hid_evidence_that_overtakes_its_press_prevents_the_capture`. Observed with
listen-only session and HID taps on Kit's Studio: Spaceman's Option-1/2/3
reached HID but never the session. The installed fix has no physical-keyboard
run yet.

**Fixed October 30, 2026 (issue 485):** on GNOME Wayland, releasing Ctrl
before Space (or both at once) can leave the GlobalShortcuts portal without a
`Deactivated`, so a Hold capture kept recording until the shortcut was pressed
and released again. Linux now treats a press that arrives while the shortcut
is still held as a key repeat only within 1 s of the previous press; a later
press is new and finishes the Hold capture whose release was lost, and that
press's own repeats and release start nothing. Hex never infers a release from
silence or a timeout. Checks:
`a_hold_whose_release_never_arrives_finishes_on_the_next_new_press` and
`a_balanced_release_finishes_a_repeating_hold` in
[linux_input.rs](../../src/linux_input.rs) (constructed inputs; not run on
GNOME).

**Changed October 22, 2026:** Kit holds Option a little past 300 ms before
pressing the digit, which 2.3.0 and the Swift app both ignored after the
threshold. Any non-modifier key pressed while a modifier-only shortcut is still
held now discards the capture, with no transcription or paste. Keyed shortcuts
keep the 300 ms rule. Check:
`held_modifier_only_shortcut_plus_any_key_discards_after_the_hold_threshold`.

**Changed October 27, 2026 (chord breaker):** a bare-modifier shortcut is not
consumed, so other apps saw an Option dictation as an Option tap on release. A
Raycast hotkey on an Option tap activated Slack on every dictation, switching
Kit out of a fullscreen Space before the paste. Tap detectors (Raycast,
Karabiner's `to_if_alone`, AutoHotkey's Start-menu mask) cancel a tap when any
key goes down while the modifier is held. Once Hex owns a modifier-only press
it now posts one press of key code 0x44 under the modifiers held at that
moment, once per press and only while exactly the shortcut's modifiers are
still physically down (any other modifier already breaks the tap):

```ts
Hold past 300 ms (Hold mode, or a held second tap) -> Chord breaker
Toggle start press, 40 ms after the press          -> Chord breaker
  (the start tone's chord grace: an extra modifier pressed with it,
   as for ⌥⇧S, discards the press first; a toggle tap is too short
   to wait out the 300 ms hold)
Press that finishes a lock                         -> Chord breaker at once
Short tap, chord discard, double-tap lock taps     -> Nothing (still a tap)
Press during voice-delimited dictation             -> Nothing (starts nothing)
Keyed shortcuts (consumed), Globe/Fn (system tap)  -> Nothing
```

The breaker never reaches the focused app:

```ts
Posted at HID with Hex's marker
  ├── HID taps, session taps            -> See a key: tap cancelled
  ├── NSEvent global monitors           -> See a key: tap cancelled
  ├── Hex's HID tap and its key tap     -> Marker: not a key, chord, or HID evidence
  └── Hex's annotated-session key tap   -> Dropped
      -> The focused app never gets it, so it never beeps (a non-text
         view, or any text view with ⌘ held, beeps at an unhandled key)
```

0x44 is a hole in the virtual key codes (between keypad `*` and `+`): no
keyboard sends it, it has no system action, and of the 264 layouts macOS ships
only Unicode Hex Input maps it (to U+0000, never with Option alone). Source:
`DictationHotkey::break_chord` and `event_callback` in
[suppression.rs](../../src/suppression.rs), `post_chord_breaker` in
[keyboard.rs](../../src/keyboard.rs), called from the listener loop once no
input is pending. Checks:
`a_held_modifier_only_shortcut_breaks_the_chord_once_after_the_hold_threshold`,
`short_taps_chords_and_other_bindings_never_break_the_chord`,
`locked_and_toggle_captures_break_the_chord_only_for_presses_hex_owns`,
`marked_chord_breaker_never_reaches_the_shortcut_machines_or_the_app`.

Observed October 27, 2026 on a macOS 27.0.1 VM (`macvm`), with a stand-in for
Hex (the same marked HID post, dropped by a head-inserted annotated-session
default tap) rather than Hex itself: an Option hold reached a test app as an
unhandled `keyDown` that beeped in a plain view when the breaker passed
through; when dropped, the app received nothing, while an Option-tap detector
on a HID tap, a session tap, and an NSEvent global monitor each saw the key and
cancelled its tap. Without the breaker all three fired. Unproven: Raycast
itself (its binary uses both event taps and a global monitor), a detector on
an annotated-session tap placed after Hex's (it never sees the key), and a
physical keyboard. If Hex's key tap is briefly disabled by a timeout, the
breaker reaches the app.

### Shortcut Mode

```ts
General > Shortcut mode = Hold (default)             // dictate.shortcut-mode
  -> Hold and lock behave as described above
General > Shortcut mode = Toggle
  Press -> Start (pre-roll as usual) -> Release keeps recording
  -> Fresh shortcut press -> Finish (with the release tail)
  -> Escape -> Cancel
  // Double-tap to lock and Double-tap only are hidden and ignored
  // Shortcut chords during the first 300 ms still discard
  Linux: release never arrives (Hyprland Lua bind, Alt up before Z)
  ├── Press 1 s or more after the last press -> Finish
  └── Sooner -> Ignored as a key repeat (KWin repeats held shortcuts)
```

Toggle reuses the lock state on every platform: each press behaves as the
second tap of a double-tap. Persisted as `shortcut_mode` (`hold`/`toggle`) in
each platform's settings; files without it load as Hold. Checks:
`toggle_mode_starts_on_a_press_and_finishes_on_the_next` and
`toggle_mode_escape_cancels_a_locked_capture` in
[suppression.rs](../../src/suppression.rs) (macOS reducer), the same names in
[windows_input.rs](../../src/windows_input.rs), and
`toggle_mode_starts_on_a_tap_and_finishes_on_the_next`,
`a_capture_that_would_lock_finishes_when_the_desktop_drops_its_release`, and
`a_repeating_held_press_never_finishes_the_capture` in
[linux_input.rs](../../src/linux_input.rs), whose one state machine serves X11
grabs, Wayland portal shortcuts, and the `hex dictation` commands alike. The
dropped release was observed on 2026-10-28 on Hyprland 0.56.2 with
xdg-desktop-portal-hyprland 1.4.1: uinput Alt+Z sent no `Deactivated` when Alt
came up first (the fix itself has not been run there yet). Only
the Windows build was compiled and tested locally (2026-10-19); macOS relies on
CI. The Linux state machine and X11 grab tests ran on 2026-10-22 (the grabs on
an isolated Xwayland display). No physical keyboard check yet.

```ts
General > Shortcut mode > How it works                // dictate.shortcut-explainer
  -> Modal: Hold and Toggle cards, each the shortcut's keycaps (one per key,
     pressing together) beside two lanes (key above, recording below) on a
     chart-recorder tape: a still pen at the right, the tape scrolling left
     without end and fading out at its left edge; opening it ends a shortcut
     recording and closes the model picker
  ├── Opens centred at its Hold height in either mode; a window resize
  │   centres it again
  ├── Long chord -> The panel widens (up to the window) so each card's lanes
  │   keep at least 64 pt beside the keycaps
  ├── Pick a card (click, Enter, Space) -> Saves Shortcut mode
  ├── Hold -> Double-tap to lock row: switch + striped hands-free lane
  ├── Toggle -> No lock row; it folds inside its own slot and the panel's
  │   bottom edge rises with it (the top stays put)
  ├── Tab -> Hold, Toggle, lock switch, Close; focus stays inside
  ├── Close, Escape, or a press outside -> Closes; focus returns to the link
  ├── Change pane, menu bar, or `hex window show` -> Closes
  └── Reduce Motion -> One whole take, still, no wakes; turned off while
      open, the tape starts again when the window regains focus
```

The lanes are Rune shapes drawn at their laid-out width and redrawn from the
window's poll clock while the modal is open (`src/rune_ui/explainer.rs`).
Checks: `how_it_works_opens_the_shortcut_explainer_and_keeps_focus_inside`,
`explainer_cards_pick_the_shortcut_mode`,
`the_explainer_loops_on_the_poll_clock`,
`reduce_motion_shows_the_finished_explainer_without_waking`,
`the_explainer_restarts_when_reduce_motion_turns_off`,
`the_explainer_centres_at_its_hold_height_in_either_mode`,
`a_long_chord_widens_the_explainer` (five keys),
the tape's own `nothing_draws_past_the_pen`, `the_tape_scrolls_without_steps`,
`the_still_frame_shows_a_whole_take`, `narrow_lanes_draw_nothing` and
`the_past_leaves_the_tape` in `explainer.rs`,
`the_explainer_draws_a_keycap_per_key_and_lines_up_the_lanes`, and
`the_explainer_picker_and_shortcut_recording_never_share_the_window` in
[tests/panes.rs](../../src/rune_ui/tests/panes.rs), plus the
`shortcut-explainer`, `shortcut-explainer-toggle`, and
`shortcut-explainer-chord` (Ctrl+Alt+Space) goldens (frozen at 1.8 s into the
tape). The animation itself has been seen only in headless
frames, not on a real window; a headless trace of Hold, Toggle and rapid
switching on 2026-10-25 kept every element inside the panel and clear of
Close on every frame (no automated check yet).

On Linux, `hex dictation start` and `stop` bypass Hold, Toggle, and the
double-tap rules: start records unless something already records, stop
finishes, and `toggle` does whichever applies. Bound to a key's press and
release they give hold-to-dictate; a shortcut press still finishes a capture a
command started.

The short-tap threshold belongs to ordinary hold activation. Voice-delimited
and explicit programmatic capture have different intentional-start rules.

### Lock, With Or Without Hold

```ts
General > Double-tap to lock                          // dictate.lock; Hold only
  Hold -> Ordinary hold path remains available
  Tap -> Tap
    ├── Second press AND release <300 ms after first release -> LOCKED
    └── Slow second release -> No lock

General > Advanced > Double-tap only                  // dictate.double-tap-only
  Requires a key binding AND locking enabled
  Single press/hold -> No capture                     // disables hold, not a toggle
  Same two qualifying complete taps -> LOCKED         // same lock, not a third mode

LOCKED -> Speak hands-free -> Fresh shortcut press -> Finish
       -> Escape -> Cancel
       -> Missed key release -> Neutral keyboard repairs tracking -> Still LOCKED
```

Checks in [suppression.rs](../../src/suppression.rs):
`callback_double_tap_locks_for_modifier_and_key_bindings`,
`callback_hold_and_double_tap_boundaries_are_milliseconds`,
`second_tap_finishes_as_press_and_hold_when_double_tap_cannot_lock`,
`double_tap_only_waits_for_two_complete_key_chord_taps`,
`locked_dictation_finishes_after_a_missing_key_release`.
These cover gesture decisions, not hardware keyboard-layout or remapping equivalence.

### Warm And Cold Capture

```ts
Keep ready -> Press shortcut                          // dictate.boundaries
  -> Include up to 450 ms of available pre-roll
  -> Delayed handling still uses original event boundaries
  -> Release excludes audio captured after physical release + 200 ms

Release or locked finishing press                     // dictate.release-tail
  -> Hold threshold judged on the physical release alone
  -> Wait for audio through release + 200 ms (POST_RELEASE_TAIL)
     ├── Arrives -> Clip ends exactly at release + 200 ms
     └── Stream stalls/fails -> Finish within tail + 100 ms with what arrived
  -> Not Escape, short taps, voice controls, programmatic stop, or meetings
  -> Linux and Windows wait the same tail; Linux ends at the release it observes
     (an X11 key release, a portal Deactivated, or `hex dictation stop`)
  -> A press inside the tail starts after the old clip finishes, with its own pre-roll

Release that will transcribe                          // dictate.stop-tone
  -> Stop tone and mute/pause/sleep restore at the release, while the tail is
     still captured; a muted output (macOS, Linux) unmutes first, then the tone
     ├── Short tap or chord -> No stop tone; the start tone is retracted
     ├── Clip later near-silent or without speech -> Tone already played;
     │   discard, nothing pasted
     └── Escape after the release -> Not bound; the clip still transcribes
  -> macOS: the audio owner announces it before draining the tail; HUD
     Transcribing and submission still follow the tail
  -> Paused media resumes without delaying the tone
  -> Voice controls (no tail) -> Tone when the clip is submitted, as before
  -> The silence gate ignores the tail, so a tone the microphone hears there
     never makes a silent clip transcribe

Release when idle -> Press shortcut                   // dictate.cold-microphone
  -> Open microphone asynchronously, no pre-roll
     ├── Release before readiness -> Discard
     ├── Stream fails before the hold threshold -> Reopen for the same press
     └── Ready while held or locked -> Capture
  -> Capture idle -> Microphone can close             // accepted jobs do not keep it open
```

Source: `DictationCapture` in [dictation.rs](../../src/dictation.rs); checks:
`delayed_press_reconstructs_audio_from_the_original_boundary`,
`delayed_release_removes_audio_captured_after_the_physical_release`,
`capture_continues_past_sixty_seconds_until_explicitly_finished`,
`release_tail_keeps_audio_through_the_tail_and_no_further`,
`short_tap_discards_on_its_physical_release_despite_the_tail`,
`resampled_clips_keep_the_last_word_through_a_delayed_release` (44.1/48 kHz,
streamed and not, with the tail). These cover sample-buffer/timestamp logic, not
a microphone recording. The physical release stays exact; the 200 ms tail is an
explicit extension of the trim boundary. The opt-in `release_boundary_harness`
([dictation_tail_tests.rs](../../src/dictation_tail_tests.rs); `HEX_TAIL_MODEL`,
`HEX_TAIL_WAVS`) and `evaluate_dictation_corpus`
([dictation_corpus_tests.rs](../../src/dictation_corpus_tests.rs);
[`scripts/dictation-corpus.sh`](../../scripts/dictation-corpus.sh),
`HEX_DICTATION_CORPUS_DIR`) replay recordings through the capture timeline at
release offsets (`-300 ms` to `+200 ms`) and onset offsets (`-300 ms` to
`+100 ms`). On 2026-10-15, before the tail, across 102 clips (70 real voice
recordings, 20 macOS `say` clips, and 12 ElevenLabs `eleven_v4` clips) with
Unified English, clips matched the release to the sample; releasing at
`0 ms`..`+200 ms` kept the final word on 10/10 real clips (`-50 ms`: 9/10;
`-100 ms`: 7/10; `-150 ms`: 4/10; `-200 ms`: 2/10). Warm 450 ms pre-roll kept the
initial word even when speech began `-300 ms` before press, whereas cold 0 ms
pre-roll lost initial syllables when speech preceded readiness by `150–300 ms`.
The since-removed pause chunking on or off, and 1 versus default decoder
threads, gave identical tail results. The harnesses now apply the tail; they have not been re-run with it. The
tail also keeps any key-up or click sound made at release; no filtering is applied.
The owner in [dictation_audio.rs](../../src/dictation_audio.rs) has
`cold_capture_restart_rejects_late_open_results_and_closes_after_finish` and
`pending_open_uses_physical_press_for_intentional_threshold`: controlled opening
and ownership transitions, not physical device startup latency. Tail checks there
drive the owner with a fake stream: `release_waits_for_its_tail_and_keeps_no_later_audio`,
`a_press_inside_the_tail_starts_its_own_capture_after_the_old_one_finishes`,
`a_stalled_or_failed_stream_bounds_the_tail_wait`, and
`escape_and_short_taps_finish_without_waiting_for_a_tail`. The macOS coordinator
blocks on the tail, so HUD Transcribing and submission follow the release by up
to 200 ms (300 ms on a stalled stream). The stop tone no longer does: the audio
owner's `Finish` ends the recording environment through
`end_recording_environment_then` and plays the tone from the environment worker
once a muted output is restored (at once with paused media), then drains the
tail; `finish_dictation` plays the tone itself only for tail-free finishes. This
macOS change compiles and passes Clippy on hailmary (2026-10-22); physical tone
timing and the unmute-then-tone order on a Mac audio device remain unverified.
Linux and Windows play the stop tone at the release too. On Linux this used
to follow the tail too: on 2026-10-22, an isolated service on apollo (ALSA
Loopback microphone and speaker, `hex dictation stop`) measured the stop tone
requested 203 ms after the release was handled before the fix and within the same
loop pass after it (about 40 ms after the CLI mark, mostly process start), with
the tail still collected for 203 ms afterwards; the output was already open, so
no device reopen was involved. Checks: `a_release_finishes_its_stream_with_the_complete_clip`
(a release that transcribes is announced before its tail),
`escape_and_short_taps_cancel_the_stream_and_submit_nothing` (a short tap is
silent), `restoring_without_a_mute_runs_its_callback_first`, and
`the_stop_tone_in_the_tail_does_not_make_silence_transcribe`. The mute ordering
(unmute, then tone) is checked by reading the code, not on a muted device, and a
physical F12 release has not been recorded.

### Streaming Finish

```ts
Model advertises streaming, GPU device             // dictate.streaming
  // Metal on macOS, Vulkan on Linux and x64 Windows
  Load or switch model -> Probe a one-second silent stream (also prewarms the stream graph)
       ├── Succeeds -> Stream shortcut captures
       └── Fails, CPU device, Apple Speech, or HEX_STREAMING=0 -> Whole-clip path only
  Shortcut capture passes the hold threshold (Toggle on Windows and Linux: on press)
    -> Open one live stream; feed the 16 kHz backlog, then every 160 ms
       // same audio as the final clip, pre-roll included; never blocks capture
  Release -> Collect the 200 ms tail -> Finish with the complete clip
    -> Feed any unfed remainder -> Finalize -> Final text
       ├── Stream failed, produced no text, or was fed > 250 ms past the clip
       │   -> Transcribe the whole clip offline (source=fallback)
       ├── No final text within 1.5 s -> Abort the stream -> Whole clip
       └── Model switched during the capture -> Stream closed -> Whole clip
  Short tap, Escape, chord discard, microphone failure -> Drop the stream
  Voice-delimited and programmatic captures (macOS) -> Whole clip after release
  Nothing is pasted or reaches the focused app until the final text exists
  live-text experiment -> After each feed the stream publishes its text   // dictate.live-text
  ├── Stable (committed) and tentative text, per capture; a new capture starts empty
  ├── One click-through line just past the HUD, on the HUD's screen
  │   ├── macOS: beside the plate the HUD placed (under the menu bar, over the Dock,
  │   │   in the menu bar, out of the notch), on the pointer's screen
  │   ├── Linux, Windows: past the plate `rune_hud` draws, on the primary display
  │   ├── HUD hidden -> In the HUD's place at the top
  │   ├── Appears with the first words, already drawn; prewarmed, so macOS never
  │   │   reveals the last dictation's words for a frame (Linux, Windows: unverified)
  │   ├── Grows with the words to 552 pt; the newest stay in view, "…" cutting the start
  │   └── Tentative words, when the model has any, are dimmed after the stable ones
  ├── Release, Escape, short tap, or a lost microphone -> The recording ends -> Hidden
  ├── Provisional: never pasted, copied, or kept (the final text replaces it)
  ├── A failed or interrupted stream clears the text while the capture goes on
  └── process.log: `partials` on "finished live stream"; per change at debug (no words)
  Picker -> "Streams" tag on catalog models whose artifact streams   // setup.model-card
  Picker -> The saved language's Recommended/Fastest/Most accurate cards first  // setup.model-routing
  ├── el: Cohere; tr, ms, fil, fa, mk, ar, ja, fi, sv, da: Whisper turbo first
  ├── auto, yue, ko: no SenseVoice; mt: Parakeet v3 only (Whisper fails Maltese)
  ├── en: Underdog Bark 3.0 Pro and Clean (Parakeet v3 fine-tunes; Clean
  │   drops filler words) follow without a badge
  └── Every other available model follows without a badge, selectable and
      deletable; never auto-switched or auto-downloaded (setup.language-select)
```

Source: [transcription_engine.rs](../../src/transcription_engine.rs) (one thread
owns the model on every OS; streams, finishes, offline runs, and macOS model
loads and switches share its FIFO because a live stream holds the model's
compute lease), `probe_stream` and `with_stream` in
[gguf_session.rs](../../src/gguf_session.rs), the stream entry points in
[local_transcriber.rs](../../src/local_transcriber.rs) and
[parakeet.rs](../../src/parakeet.rs), and `begin_stream` in
[dictation.rs](../../src/dictation.rs): the capture owns its stream, feeds it as
audio arrives, cancels it with the recording, and hands it to the finished clip.
macOS streams hotkey captures from the audio owner in
[dictation_audio.rs](../../src/dictation_audio.rs); `DictationWorker::transcribe`
hands the clip to the engine when it is submitted, so its stream finishes before
a later capture opens one, and the job then waits for its text in order. Linux
([linux_dictation.rs](../../src/linux_dictation.rs)) and Windows
([windows_dictation.rs](../../src/windows_dictation.rs)) drive the same engine
from their capture loops. Parakeet buffers with its largest context menu
(Unified English: 5.6 s left, 1.04 s chunk, 1.04 s right), its most accurate and
least compute-intensive setting; `HEX_STREAM_CHUNK_MS` overrides the chunk for
benchmarks and `HEX_STREAMING=1` streams on any device, including a CPU
(diagnostics and the Linux CI fixture). Each dictation logs
`source=stream|offline|fallback` and `finish_ms`. The macOS "Transcribe during
pauses" setting no longer affects dictation.

Checks: `recorded_audio_is_a_prefix_of_the_finished_clip` (16, 44.1, and
48 kHz), `resampled_clips_keep_the_last_word_through_a_delayed_release` (streamed
and not), `a_stream_starts_with_the_backlog_and_feeds_in_blocks`,
`live_text_follows_the_recording_and_never_outlives_it`, and
`short_taps_cancellation_and_unstreamable_models_end_the_stream` in dictation.rs;
`hotkey_captures_stream_once_intentional_and_others_do_not` in dictation_audio.rs;
`hold_streams_after_the_threshold_and_toggle_streams_from_the_press`,
`a_release_finishes_its_stream_with_the_complete_clip`,
`escape_and_short_taps_cancel_the_stream_and_submit_nothing`, and
`a_model_that_cannot_stream_transcribes_the_whole_clip_after_release` in
linux_dictation.rs; `source_labels_distinguish_fallbacks`,
`streams_send_their_audio_in_order_and_cancel_when_dropped`, and (macOS, a real
engine thread without a model) `a_finish_without_a_stream_transcribes_the_clip_offline`,
`an_unusable_stream_falls_back_to_the_complete_clip`,
`work_runs_in_order_and_failures_reach_the_caller`, and
`dropping_the_engine_stops_despite_an_open_stream` in transcription_engine.rs.
These use synthetic audio and recorded engine commands. The Linux CI fixture
[`scripts/test-linux-dictation.sh`](../../scripts/test-linux-dictation.sh)
replays the same speech through a virtual microphone twice with the real model:
once on the CPU (whole clip, `source=offline`) and once with `HEX_STREAMING=1`
(`source=stream`, or `fallback` when the CPU stream misses its deadline). The
hidden Windows `model stream-bench <wav> [--speed N]` command and the opt-in
macOS `metal_stream_tracks_offline_transcription` (parakeet.rs;
`HEX_STREAM_MODEL`, `HEX_STREAM_WAVS`, `HEX_STREAM_SPEED`) replay a WAV in real
time and compare both paths.

**Observed October 19, 2026, RTX 5070 Ti, Vulkan, Unified English Q8, Windows
TTS clips replayed at 4× (131 s also at 1×):** release-to-text 55/55/93 ms
(10/27/131 s) streaming versus 82/185/873 ms whole-clip; GPU busy 3.1–3.6% of
the dictation; process peak working set 214 MB and about 3% of one core during
a real-time 131 s stream. Against the script, both paths transcribed the 10 s
and 27 s clips perfectly; on 131 s the stream added one homophone error ("cache"
→ "cash") to formatting differences both paths share. Smaller chunks (560 or
160 ms) added more errors and up to 20% GPU without a faster release. After the
engine became shared (same machine and clips at 4×): 56/91 ms streaming versus
92/865 ms whole-clip for 10/131 s, with 0/28 and 1/408 word differences, matching
the earlier build.

**Observed October 19, 2026, MacBook Air M4, Metal, Unified English Q8, the same
clips at 4× through `metal_stream_tracks_offline_transcription`:** Metal
qualified for streaming and both clips finished from the stream: 140/253 ms
release-to-text (10/131 s) versus 156/2,838 ms whole-clip, with 0/28 and 1/408
word differences. This drives the production engine and Parakeet adapter, not
the shortcut, microphone, or paste. Not measured: physical microphone corpora,
noisy rooms, or Linux on a real GPU.

**Observed October 28, 2026, Apple Silicon Mac (macOS 27), Metal, Unified
English Q8, a 13.1 s `say` clip (40 words) at 1× through
`metal_stream_tracks_offline_transcription` with live text:** the text changed
12 times, once per 1.04 s chunk; the first words arrived 2.24 s into the audio,
and at 12.6 s 35 of 40 words showed (about 1–2 s behind speech). The stable
text was never rewritten and the tentative text was always empty, so with
Parakeet's buffered settings live text grows in whole-chunk steps. The final
text came from the stream 154 ms after release, with 0/40 word differences
from the whole clip. Synthetic speech; not a physical microphone or the HUD.

**Live text observed October 28, 2026, Apple M3 Pro, Metal, Unified English Q8,
`Hex Test.app` at `84f594a` with `live-text`, a physical microphone:** the line
appeared under the HUD and grew with the words; three dictations in a row
reappeared with no trace of the previous text; a dictation released before its
first words never showed the line and one released just after hid it; with the
HUD at the Bottom the line sat above it, and with the HUD hidden it took the
HUD's place at the top. Not observed: a second display, Dot scope placements,
Linux, Windows.

**Known defect, observed October 28, 2026 (same Mac and model):** the stream
can drop the first words spoken after a pause, and the paste carries the loss
because a stream with text is never retranscribed. In real dictation the
words that resumed after 6 s and 13 s pauses were lost or garbled. Replaying
`say` speech around 3, 8, and 13 s gaps (digital silence and a −50 dBFS noise
floor) through `metal_stream_tracks_offline_transcription` lost "I" after the
8 s gap of digital silence only, and the whole clip kept it: the loss depends
on where speech resumes against the stream's chunks. Live text does not cause
it: `HEX_STREAM_LIVE_TEXT=0` gives the same text.

**Mitigation, October 29, 2026 (#338's diagnosis, by ElliottH):** the engine's
`PauseCap` now feeds a dictation stream only the first 2.5 s and last 0.3 s of
each pause (20 ms frames the meeting pause detector calls not speech); the
clip and its offline fallback keep every sample, and "finished live stream"
logs `pauses_dropped_ms`. `/streams` is unchanged. Check:
`a_dictation_stream_hears_only_the_ends_of_long_pauses` (synthetic audio, a
fake stream). Not re-measured on a real model here; ElliottH's Metal replays
went from 18/36 to 0/36 lost words for 6–14 s gaps of silence or steady noise.
Limit: noise that rises mid-dictation counts as speech for up to about 10 s,
so that pause is not cut.

**Fixed October 30, 2026 (issue 458):** over steady background noise, speech
only about 8 dB louder than the room missed the meeting detector's 12 dB
speech margin, so the whole dictation counted as one pause and the stream
heard only its first 2.5 s ("This in a test on the" for a 17 s clip over
−35 dBFS noise). `PauseCap` now cuts only frames within 4 dB of the noise
floor (or under −55 dBFS) (`SpeechFrames::is_quiet`); anything louder is
kept. Noisier rooms keep more pause audio rather than lose speech. Check:
`quiet_speech_over_steady_noise_is_never_cut_as_a_pause` (synthetic noise
bursts, a fake stream; no real-model or microphone run).

### Ongoing Jobs And Output

```ts
Completed long recording                  // dictate.model-windows
├── Cohere -> Runs of at most 35 seconds, including the final remainder
└── Other GGUF models -> Existing backend-advertised input limit
    -> Transcribe each chunk -> Join text in original order

Chunking bounds inference, not capture duration. No silence is removed.
```

The shared `max_audio_chunk_samples` policy in
[transcription_models.rs](../../src/transcription_models.rs) is used by
[parakeet.rs](../../src/parakeet.rs) and
[local_transcriber.rs](../../src/local_transcriber.rs). Cohere's pinned backend
reports a 400-second encoder capacity, while its reference processor uses a
35-second transcription window. Treating the encoder capacity as a useful
inference window caused [#73](https://github.com/anomalyco/hex/issues/73)'s lost
passages. A stricter backend bound still wins; other model policies are unchanged.

`cohere_chunks_at_reference_window_not_encoder_capacity` checks the policy.
The opt-in `cohere_long_form_keeps_all_sections_with_and_without_pauses` runs the
real pinned model on synthetic speech. Reproduce it on macOS with
[`scripts/test-cohere-long-form.sh`](../../scripts/test-cohere-long-form.sh) and an
explicit model path. The script uses Samantha speech synthesis, never microphone
capture, and retains fixtures on failure (`HEX_KEEP_COHERE_FIXTURES=1` keeps successes).

**Observed September 4, 2026:** the same 57-second fixture went from 44 word errors
to zero after chunking; the 73-second paused fixture went from 71 errors to one.
Short references transcribed correctly before and after. The retained native
regression also passed a roughly 235-second repeated fixture, retaining all three
sections in each repetition. This is Metal inference evidence on an M2 Max, not
physical capture/paste or Linux runtime proof. Chunk boundaries can still affect
individual words; this is not a claim of perfect recognition or silence handling.

The macOS `Transcriber` owns backend input padding for ordinary
transcription. `short_unified_input_is_minimum_padded_before_trailing_context`
in [transcription.rs](../../src/transcription.rs) checks minimum-duration padding
followed by Unified English's additional trailing silence. This is a pure sample
policy check, not native inference-quality proof. Padding is now included in
`inference_ms`; `prepare_ms` covers clip conversion and diagnostic retention.

```ts
Listening idle                            // dictate.model-memory
├── Keep the selected model weights warm // avoids loading on each dictation
└── Completed offline inference
    -> Release input-sized GGML/Metal compute scratch
    -> Keep only reusable model/session state
```

The selected GGUF remains resident by design; Parakeet v3's pinned Q8 artifact is
739,508,576 bytes before runtime metadata and decoder state. Input-sized scheduler
scratch must not remain at its high-water mark after a dictation. The pinned
`transcribe-cpp` 0.1.3 runtime owns that scratch for the session lifetime, so Hex
rotates the lightweight session after every offline run while retaining the loaded
model. A later dictation recreates its session without reloading the weights.

**Observed September 18, 2026, isolated Metal probe on M2 Max:** with the similarly
sized Parakeet Unified English model, `transcribe-cpp` 0.1.3 grew from a 792 MB loaded
footprint to 1,195 MB after a 45-second input and retained 1,195 MB after another
short run. The same 0.1.3 runtime with Hex's session rotation measured 792 MB loaded,
801 MB after the long input, and 802 MB after the following short run. The probe used
generated silence and no live microphone, UI, or paste. This
establishes the backend scratch cause and reclamation on that model and machine, not
Parakeet v3's exact whole-app footprint on the reporter's Mac. Activity Monitor can
include additional app memory.

```ts
Accepted job on a GPU device             // dictate.inference-threads
├── Inference thread -> macOS user-interactive QoS
└── GGUF session -> One CPU decoder thread, no ggml worker pool
    // CPU-only devices keep the backend's default thread count
```

Source: `session_options` in [gguf_session.rs](../../src/gguf_session.rs) and
`prioritize_inference_thread` in [parakeet.rs](../../src/parakeet.rs); check
`gpu_sessions_bound_decoder_threads_and_cpu_sessions_keep_library_default`.
Parakeet's per-token TDT decoder runs on CPU even with Metal encoding. ggml's
default pool (up to 8 spin-waiting threads) is created with plain `pthread_create`,
so its workers start at default QoS rather than inheriting the inference thread's.

**Observed October 14, 2026, standalone `transcribe-cpp` 0.2.4 bench on a Mac
Studio (18 cores) already busy with other work, Unified English Q8, Metal:** under
36 extra `yes` processes, user-interactive QoS medians for 3/30/90-second clips were
170/703/1,819 ms with one thread, 136/1,907/4,940 ms with two, and 178/4,304/13,519 ms
with four. Without the extra load, one thread measured 83/364/1,631 ms and two
64/705/2,889 ms. This is backend timing, not physical capture/paste on the
reporter's MacBook Pro, and not Linux runtime proof.

```ts
Finish
  ├── Near-silent capture -> Discard, no inference    // dictate.silence-gate
  │       // loudest 30 ms window (DC removed) below -50 dBFS; no paste,
  │       // History, or last-result update; process.log records the level
  ├── Louder capture, no detected speech -> Discard, no inference
  │       // the Silero speech detector, once installed, finds no speech of
  │       // at least 160 ms before the release; process.log records
  │       // audio_ms and vad_ms (at debug level when speech passes)
  ├── Capacity available -> Accepted job -> Local transcription -> Mode processing
  │    -> Paste successful nonempty output once, at current focus
  │       // the clipboard write carries nspasteboard.org Transient and
  │       // AutoGenerated markers, so conforming clipboard managers skip it
  │       // a Chromium 155/156 app in front (read from its bundle's
  │       // framework version) gets the text typed instead (#212)
  │       // synthetic ⌘V resolves on any active layout: it uses the layout's
  │       // Command layer ("Dvorak – QWERTY ⌘"), and a non-Latin layout
  │       // falls back to the ASCII-capable layout like AppKit itself
  └── Queue full -> Explicit rejection                // bounded admission

A pending -> Start capture B                          // dictate.overlap
  -> B records independently; A is not cancelled
  -> Accepted outputs retain submission order
  -> Completing/pasting A must not end B
```

Source: `finish_dictation` in [recognition.rs](../../src/recognition.rs).
The silence gate stops models from hallucinating closing phrases such as "thank
you" on an empty hold ([#108](https://github.com/anomalyco/hex/issues/108)). It
covers hotkey and voice-delimited captures on macOS, and the Linux
hotkey loop. The local API's programmatic dictation and host-audio transcription
are deliberately left ungated: they skip both silence checks, and only Whisper
output that repeats the recognition hints is dropped there.
The threshold is conservative, so speech is not dropped; a noisy room, the start
tone, or a key click can lift a capture above it. Those captures then go to the
Silero speech detector ([speech_detector.rs](../../src/speech_detector.rs)),
which discards them when it finds no speech. The detector (1.2 MB, pinned
revision and checksum in [transcription_models.rs](../../src/transcription_models.rs))
downloads in the background whenever a transcription model is downloaded or
activated, and at the first dictation, so an installed model still activates at
once offline; a failed download is retried at the next dictation. Model
preparation (macOS, Windows) and the Linux listener load it when present and
never download it. It runs on the CPU with a speech threshold of 0.4 instead of
Silero's 0.5, which misses a whisper at -43 dBFS. While it is missing or fails to
load, captures above the threshold still reach the model, as before.
Whisper output that only repeats the recognition hints is dropped as well
([models](README.md#get-ready)). Other hallucinations after real
speech are not addressed.
`silence_gate_discards_only_near_silent_captures` and
`speech_detector_discards_loud_captures_without_speech` in
[dictation.rs](../../src/dictation.rs) check the policy on synthetic samples
with the detector's verdict fixed; `real_detector_rejects_silence_and_noise`,
`real_detector_hears_speech`, and `real_detector_hears_quiet_whispers` (ignored;
`HEX_VAD_MODEL`, `HEX_VAD_SPEECH_WAV`, `HEX_VAD_WHISPER_WAV`) run the real
detector, and `an_installed_model_does_not_wait_for_the_speech_detector` checks
the background download.
Key resolution: [keyboard.rs](../../src/keyboard.rs); non-Latin layout
fallback details live in [shortcut recovery](recovery.md#keyboard-layout-resolution).
Checks in [parakeet.rs](../../src/parakeet.rs):
`parallel_processing_results_are_released_in_submission_order`,
`ordered_waiting_outputs_remain_bounded_after_channel_drain`.
These cover output bookkeeping/admission, not sustained end-to-end inference/paste.
`transcript_write_is_marked_for_clipboard_managers` in [paste.rs](../../src/paste.rs)
checks the markers on a private pasteboard; whether a given clipboard manager
(Maccy, Raycast) honors them is not verified here. The restored previous clipboard
is written back unmarked, exactly as captured.
`detects_the_chromium_major_version_from_the_bundle_framework` and
`chromium_versions_need_four_numeric_components` in [paste.rs](../../src/paste.rs)
check version detection on fake bundles;
`typing_splits_text_into_twenty_unit_events_without_splitting_surrogate_pairs` and
`typing_sends_each_line_break_as_one_shift_return` in
[keyboard.rs](../../src/keyboard.rs) check the typed events. Neither proves
insertion into a real Chrome page.

**Chromium 155 and 156:** these versions on macOS can lose a ⌘V that races
Chromium's own pasteboard change notification
([#136](https://github.com/anomalyco/hex/issues/136),
[#212](https://github.com/anomalyco/hex/issues/212); regression Chromium
[CL 8217390](https://chromium-review.googlesource.com/c/chromium/src/+/8217390),
fixed by [CL 8503629](https://chromium-review.googlesource.com/c/chromium/src/+/8503629)
in 157). The race depends on how busy Chrome's main thread is, so the earlier
fixed 150 ms pre-paste delay still missed about 1 in 4 dictations and was
removed; no app waits before ⌘V. Instead, when the frontmost app's bundle
contains `Contents/Frameworks/*Framework.framework/Versions/<a.b.c.d>/` with
major version 155 or 156 (any version present counts, so a staged update is
covered), Hex types the text as synthesized Unicode key events: at most 20
UTF-16 units per event, never splitting a surrogate pair, and every line break
as Shift-Return so chat boxes don't submit. Typing leaves the clipboard
untouched, so there is nothing to restore; ordering, continuation joins, Send's
trailing Return, Paste Last, and cancellation before commitment behave as for a
paste. Chromium 157 and later, older versions, Electron apps (`Versions/A`),
and every other app keep clipboard paste.
Limits: the framework version is Chromium's for Chrome, Helium, Brave, Edge,
and Chromium; a fork that names that folder after its own version (Opera,
possibly Vivaldi) is classified by that number. Installed web-app shims carry
no framework and keep paste. An active input method may treat typed events
differently from a paste. Remove the typing path once Chrome 157 is the floor.

**Windows paste (issue 477):** Windows puts the transcript on the clipboard,
waits for Shift, Alt, and Win to come up, sends Ctrl+V, and restores the
previous clipboard 500 ms later unless a newer paste or another app changed it.
General › Advanced › Output › Terminal paste (`paste_with_shift`, off by
default; shared with Linux) sends Ctrl+Shift+V instead, for apps that ignore
a synthetic Ctrl+V (the herdr desktop app). Shift goes down after Ctrl and up
before it; a Ctrl the user still holds is never pressed or released by Hex. A
saved change applies to the next paste without restarting the listener.
Source: `control_chord` in [windows_paste.rs](../../src/windows_paste.rs).
Check: `paste_chords_press_shift_inside_control_and_leave_a_held_control_alone`
(the synthesized key sequence only; no paste into a real Windows app).

### Cancellation Before Commit

```ts
Escape                                               // dictate.cancel
  ├── Active capture -> Cancel that capture first
  └── No active capture -> Cancel newest cancellable accepted job
     -> Repeat Escape -> Walk back through unfinished jobs
  └── Programmatic dictation (local API) -> Not claimed; the key reaches the app
     -> Even with other jobs queued; its owner cancels through /cancel
     -> The lease ends an abandoned one

Cancelled before output commit -> No insertion
Already committed clipboard insertion -> Not undone   // not target-app undo
```

Checks in [parakeet.rs](../../src/parakeet.rs):
`output_stays_cancellable_through_preparation_but_not_after_mutation`,
`repeated_cancellation_walks_back_through_pending_jobs`,
`escape_skips_programmatic_dictation_jobs`; [recognition.rs](../../src/recognition.rs):
`a_programmatic_dictation_leaves_escape_with_its_owner`.
These use a controlled paste boundary and temporary history store;
no foreground application consumes the clipboard.

### Feedback

```ts
Record -> Release -> Transcribing/processing/paste -> Finished // dictate.feedback
  HUD + tones distinguish capture from pending work          // never take focus
  Pending processing/paste -> Still unfinished, not recording
  Shortcut press -> Start tone ~50 ms later                  // dictate.start-tone
     -> Chord within 40 ms (e.g. Option-Command-I) -> Silent
     -> Chord or short tap before 300 ms -> Tone fades out over 20 ms
     -> Hold reaches 300 ms -> Tone already played; mute/pause/sleep begin now
     -> Release microphone while idle -> Output warms on press; tone once
        the microphone is open and the hold counts
  Dictation pasted -> Done tone                                // dictate.done-tone
     -> Empty, failed, cancelled, Paste Last, programmatic -> No done tone
  Failure (macOS) -> Error tone; wake/sleep keep macOS Pop/Tink
  Tone requested -> Playback worker opens the default output, then plays
     -> Output stays open five minutes past the last tone, then releases
     -> Released output holds no macOS idle-sleep assertion
  Feedback volume = 0 -> No tones
     -> Playback worker releases output on its next 250 ms observation
     -> Volume enabled again -> Reopen for the next tone, never capture/UI
  Cold audio stack exceeds preload admission -> Warn once; recognition continues
     -> Loader keeps running; later tones play once the output opens
```

```ts
Settings > Sounds (next to Sound volume)                    // dictate.sound-set
├── Default / older settings -> Hex signature
├── Row shows the saved set -> Opens the Sounds sheet
│   ├── Stage -> Hex's own HUD, where this OS puts it, does what it does
│   │   │          at each sound of a short dictation
│   │   ├── Where -> Indicator position: top / bottom; Off shows none
│   │   ├── Start (0 s) -> Recording, with a demo voice's level
│   │   ├── Stop (1.4 s) -> Transcribing
│   │   └── Done (2.1 s) -> Done look, then it leaves
│   ├── Lone Cancel -> The HUD just leaves: no cancel look on any OS
│   ├── Lone Error (macOS) -> Failed look 0.6 s (Dot scope coral), then leaves
│   ├── Chips -> Start / Stop / Done / Cancel / Error (macOS) play one sound
│   │   └── Set has no such sound (Classic's or Thock's done) -> Chip faint and inert
│   ├── Each set -> Name, what it sounds like, play button
│   │   ├── Play -> Plays its dictation without choosing it; again stops it
│   │   └── Press the set -> Save and apply live; play its dictation
│   ├── Any playback -> Stops the one before it (20 ms fade)
│   ├── Sound volume -> Plays at it; Off plays nothing, the HUD still shows
│   └── Close / Escape / scrim / leaving the pane -> Stops playback
├── hex settings set sound_theme <signature|classic|click|soft|wood|analog|thock>
└── Every set -> Same loudness per sound at a given volume
```

The sheet is a modal like the shortcut explainer, opened from a
`sheet_button` because its choices need room to be heard, not a menu. Checks:
`auditions_play_each_sound_at_its_moment`,
`stopping_an_audition_fades_all_of_it`,
`each_moment_shows_from_its_sound_to_the_next`,
`the_hud_shows_a_dictation_and_then_leaves`,
`a_lone_sound_shows_its_moment_and_leaves`,
`dot_scope_turns_coral_on_failure`,
`the_sound_sets_sheet_plays_and_chooses_sets` (Linux), and the `sound-sets`,
and `sound-sets-playing` goldens. The stage
replays Hex's HUD on the sheet's clock: `dictation_indicator`'s model, drawn
by the window's compositor with `rune_hud`'s painter and `rune_hud.wgsl`, as
the real overlay draws it. It is a preview, not the overlay: Dot scope's
in-bar and notch placements show below the menu bar, the voice is HUD Lab's
demo, not the microphone, and it ignores Reduce Motion.

Hex signature (2026-10-27) is one kalimba-like FM voice playing one motif,
D-B-A-D (a sixth, since "hex" is six): start D5 to B5 (116 ms), stop B5 to A5
(230 ms), done D6 over A5 (330 ms), cancel B5 down to D5 (225 ms), error D4
then a minor D4+Bb4 (280 ms), and onboarding success, the whole motif landing
on a D sixth chord (1.1 s), played when the first-run tour reaches its last
step. Click is mechanical key clicks, Soft tone warm sine blips in A major,
Wood a soft-mallet marimba with hollow wood blocks for cancel and error, and
Warm synth two detuned saws through a closing low-pass filter, and Thock one
mechanical keyboard key, from the team's vote on the start sound (its winner,
an ElevenLabs take kept in `scripts/sounds/thock/`): start is the press, stop
the same key's upstroke, cancel a lower, duller key, error two keys at once,
twice, and success a run of keys up to a full press; it has no done sound
because the stop already says it. Classic is the
sounds Hex played before the signature set (start clicks, stop swish, cancel
thud), restored from git; it has no done tone, errors play macOS Basso as they
did, and onboarding success plays signature's.

Each set is a folder in `resources/audio/sounds/` made by
[`scripts/sounds/synth.py`](../../scripts/sounds/synth.py)
(`uv run scripts/sounds/synth.py` rewrites them; signature regenerates
byte-identical). Synthesized sounds are 48 kHz mono WAVs starting on their
first sample; Classic keeps its 44.1 kHz stereo, trimmed to start at its first
audible sample. Every sound is normalized to one loudness per kind (start
-25.5 LUFS max momentary, stop and done -27, cancel and error -26.5, success
-25), so a volume sounds the same in every set. Classic's start and stop come
up 2.5 and 3 dB (a soft limiter touches 0.1% of the start's samples) and its
cancel, once 15 dB louder than the rest, comes down. Windows and Linux play
start, stop, done, and cancel; their failures stay silent as before. Checks:
`bundled_sounds_are_short_and_sound_immediately` (each sound under its length
budget; every start, and every synthesized sound, audible within 1 ms),
`every_set_but_classic_has_every_sound`, `sound_set_offers_every_set`. The
sounds were checked by waveform, spectrum and loudness only, never played
aloud on a Mac.

Feedback admission is advisory on every platform: a cold audio stack can exceed
the two-second preload timeout, so the recognition worker logs a warning and
continues rather than stopping. The loader thread keeps running and registers
the player when the default output finally opens, restoring tones for the rest
of the session. Linux already logged and continued; the macOS worker now
matches that behavior instead of stopping desktop recognition.

The playback worker never opens the output at startup. It opens the default
output for the first tone, keeps it through a five-minute idle grace so bursts
of dictation share a warm device, and releases it after that grace or on the
next observation after sounds turn Off. macOS idle-sleep timers count from the
last user input, and a tone implies recent input, so the grace never delays
sleep by itself. A device open already in progress must return before the
worker can observe that change. Output-open failures retry at most once per
two seconds, and a released device reopens without that backoff. Bundled
samples remain decoded in memory. A tone that arrives after the grace pays a
cold device open first; the tone is delayed, not clipped or dropped.
Overlapping tones extend the playing window rather than shortening it.

**Fixed after 2.3.0:** the start tone used to wait for the 300 ms intentional
hold, then pay rodio's default 50 ms output buffer, so it sounded about 345 ms
after the press (about 375 ms from a cold device) where the Swift app rang on
key-down. The press now requests a provisional tone. The worker opens the
output at once, waits out a 40 ms chord grace (on Kit's Mac about 40% of
discarded presses were modifier chords landing within ~25 ms), then plays
through a 512-frame (~10 ms) buffer: about 50 ms warm or cold on the built-in
speakers. A chord or short tap retracts the tone, dropping it inside the grace
or fading it over 20 ms. Recording mute, media pause, and idle-sleep prevention
still begin only at the 300 ms threshold. Checks:
`provisional_start_sounds_after_the_chord_grace_unless_retracted`,
`retracted_tone_fades_out_without_a_click_or_channel_skew`, and the ignored
`native_start_tone_path_latency` (open-to-presentation timing, with
`HEX_TONE_PATH=legacy` for the old path; route output to a silent device or set
`HEX_TONE_MUTED`). The chord-to-retraction path is checked by reading the code,
not by a physical keyboard run.

**Fixed in 2.1.21:** through 2.1.20 the worker opened the output at startup
and kept it open for as long as sounds were On, so `coreaudiod` held a
`PreventUserIdleSystemSleep` assertion for Hex's PID and an idle Mac never
slept ([#95](https://github.com/anomalyco/hex/issues/95)). 2.1.18 released the
stream only when sounds were Off. Startup admission therefore no longer
reports `could not open the audio output`; a device failure surfaces as a
warning when the first tone plays.
`output_opens_for_a_tone_and_releases_after_the_idle_grace` and
`overlapping_tones_extend_playback_instead_of_truncating_it` check ownership,
grace, and drop with a controlled sink;
`failed_output_initialization_retries_with_a_bounded_backoff` checks
failure-to-success recovery without an audio device. The opt-in
`native_output_release_clears_the_idle_sleep_assertion` opens the real default
output and reads `pmset -g assertions`; on September 22, 2026 it observed the
assertion appear for the test PID and clear within a second of release on the
built-in speakers. That is not an installed-app or USB/Bluetooth measurement.
`timed_out_admission_still_publishes_a_usable_player` checks the production
publication path after the admission receiver has gone away. These checks do
not measure the reporter's CoreAudio CPU usage or establish audible playback.

**Contributor-reported September 10, 2026 (PR #84):** a locally built `/Applications/Hex.app`
(approximating 2.1.16 plus this change) was started right after a restart where
the stock build had stopped its desktop recognition worker with
`timed out preloading feedback audio`. The patched build started on the first
attempt: the worker reached `dictation listener started` and
`resolved paste key … paste_key_code=9`, and held-Option dictations pasted into
the focused app. The late-registration path (loader still running when the
admission wait times out) is implemented, but its end-to-end tone recovery has
not yet been observed on hardware;
it is expected behavior, not executed evidence.

Linux now uses the same recording sounds with its own persisted Sound volume
control; see [Linux feedback](README.md#other-platforms-and-consumers) for its
entry points, timing, defaults, and verification limits.

Check: in a debug app, `hex dev hud recording|transcribing|processing|reset`
or the HUD Lab pane drives the real HUD with deterministic state, so
fullscreen and Space transitions can be checked. (The `dictation-hud` preview
that looped the sequence went with GPUI.) This does not prove that actual
capture caused that state.

The macOS HUD is a click-through, nonactivating `NSPanel` with
`CanJoinAllSpaces`, `CanJoinAllApplications`, and `FullScreenAuxiliary` collection
behaviors. The cross-application flag explicitly allows the overlay to join
another application's fullscreen Space (#75); panel style flags on an ordinary
`NSWindow` were not sufficient.

At the top position the HUD sits 6 pt below the menu bar (`hud_placement::TOP_OFFSET`), or below the
notch (`NSScreen.safeAreaInsets`) where a fullscreen Space hides the menu bar
and `visibleFrame` reaches the top of a notched screen. Screens without a notch
and normal Spaces keep their position. Check: `scripts/check
dictation_indicator::tests::top_capsule` covers the math only; placement over a
fullscreen app on a notched Mac is not yet observed.

### Dot scope

The HUD is Dot scope on every OS ([dot_scope.rs](../../src/dot_scope.rs),
`dot_scope_fragment` in [rune_hud.wgsl](../../src/rune_hud.wgsl), drawn by
a Rune overlay). It was the `dot-scope` experiment (#264) until 2026-10-29,
when it replaced the glossy capsule, which Omarchy users on X called a
"static red thingy"; the capsule's shader and HUD Lab styles are deleted. It is the t422 prototype's look A with its values.
Linux and Windows draw it in a 304×80 pt window at the HUD's edge (no
menu-bar or notch placement). On Windows the window is a topmost,
click-through tool window with no redirection bitmap, drawn through
DirectComposition. Rune shows and hides it with Win32 calls alone and
re-asserts its styles on each show; winit never sees its visibility. A
black rectangle that stayed after the first dictation (#499) and then no HUD
at all (#510) followed from mixing winit and Win32 (fixed by reading the
code, not yet observed on Windows). Every OS takes dark or light and Reduce Motion
from Rune, and loads `dot-scope.json` at launch.

```ts
Dot scope                                    // dot_scope
├── Recording -> A scanner unfolds a resting braid; the voice's level widens it
│   ├── Auto level (default) -> 24 dB below the recent peak (3 s release),
│   │   at least 16 dB above the room and the −60 dB noise floor   // AutoLevel
│   ├── Auto level off -> Fixed RMS window, −40 to −10 dB
│   └── Warm pearl; history travels out from the centre; phosphor tail 0.6 s
├── Locked / hands-free -> Same as recording; the HUD draws no lock
├── Transcribing / Processing -> Processing look, at least 200 ms so a fast
│   transcription still shows it (PROCESSING_MIN_DURATION):
│   ├── Flow (default) -> The braid keeps turning; bands of light travel right
│   ├── Pulse -> Rings swell in at the middle and travel out
│   └── Sweep -> Braid irons flat; a cool beam sweeps 76 pt (round 1)
├── Completed -> Every look clears from the middle out (0.3 s; Sweep's
│   beam fades where it was and its dark field shows only the two fronts);
│   100 ms in, the HUD (or island) collapses, so the two end together
├── Failed -> Coral for 0.6 s, then fades
├── Cancelled / discarded -> Fades at once
├── Reduce Motion -> A still braid; opacity fades only
├── Dark / light -> Read from AppleInterfaceStyle at each capture start
└── Queued jobs -> Not drawn                // gap
```

The simulation steps at a fixed 240 Hz, so any frame rate draws the same
picture. Placement ([hud_placement.rs](../../src/hud_placement.rs), pure
geometry the capture capsule can reuse) puts it on the screen with the
pointer:

```ts
Placement                                     // hud_placement::place
├── Below the menu bar (default) -> 6 pt under the menu bar, or above the Dock
├── In the menu bar -> A plate fitted to the bar's height, centred on a flat
│   screen, just right of the notch on a notched one
├── From the notch -> A black island grows out of the notch, 28 pt wider
│   each side (Island past the notch, 0–60 pt), and shrinks back into it;
│   the field fills it at the same dot spacing (more columns), inset 7 pt
│   all round with corners that follow the island's, snapped to whole
│   columns so the dots sit as far from the sides as from the top and
│   bottom; In the menu bar on a flat screen
├── Pretend notch -> Draws the island from a 185 pt notch on a flat screen
└── Over the menu bar -> Window level NSStatusWindowLevel + 1, click-through
```

The window is 320×128 pt so the widest plate, its shadow, and the widest
island fit. Notch metrics come from `safeAreaInsets.top` and
`auxiliaryTopLeftArea`/`auxiliaryTopRightArea`; the bar's height from
`NSStatusBar.systemStatusBar.thickness`. HUD Lab tunes it live in debug
builds, and in release builds while the `playground` experiment is on
(Settings › Labs › HUD Lab):

```ts
HUD Lab
├── Open -> The HUD shows a demo: Recording follows a synthetic voice
├── Move a slider -> Applies live to whatever the HUD shows, a real
│   dictation included; the demo doesn't restart
├── Recording / Transcribing / Processing -> Restarts the demo in that state
├── Processing, Placement, Pretend notch, Auto level, Island past the notch
│   -> Apply live
├── Save -> Writes dot-scope.json in Application Support; the HUD loads it at launch
├── Copy values -> The same JSON; Reset -> the prototype's values
└── Leave the pane or close the window -> The demo hides; unsaved values
    stay on the HUD until Hex restarts
```

The playground (⇧⌘D, experiment `playground`) lists HUD stories
([hud_stories.rs](../../src/rune_ui/hud_stories.rs)),
so every combination plays without HUD Lab:

```ts
Prototypes › HUD                             // R plays, P next look, A auto level, S stop
├── Notch, Pretend notch, Menu bar, Below the menu bar -> A whole dictation
│   from that placement; variants <look>-<ending>: flow, pulse, or sweep
│   with done, cancel (while recording), or fail (while processing)
├── Processing -> Records, then holds processing 12 s
├── Your voice -> Waits for your shortcut and meters your microphone
│   └── Both: variants hud-lab, notch, pretend-notch, menu-bar, below
└── Links name the story and variant:
    hex://story/prototypes/hud/notch?variant=pulse-fail&play=1
```

The HUD is its own Rune overlay, not a view in the window, so a story drives
the real HUD at the top of the screen and its stage only says what it shows. Playing a story
puts its placement and look into the one tuning it shares with HUD Lab
(`dot_scope::lab`).

Hex meters no spectrum, so the prototype's band emphasis is the same for
every component.

Check: `scripts/check dot_scope` (defaults, knobs, frame-rate independence,
Reduce Motion, the stages, auto level, each processing
look settling dark), `scripts/check dictation_indicator` (phases, the
processing minimum, failure and cancel, a hot microphone still moving),
`scripts/check hud_placement` (each placement on flat and notched screens),
and `rune_hud::tests::rune_hud_frames` (ignored; renders a fixed
dictation through the real shaders offscreen into PNG frames, with
`HEX_HUD_TUNING` for the look and placement over a pretend 14-inch MacBook
Pro's menu bar). On 2026-10-28 its Dot scope and notch frames matched the
retired Metal HUD's render of the same inputs on hailmary to within 3/255
per channel; on 2026-10-29 a headless capture on apollo showed recording,
processing, and done after Dot scope became the default.
Not yet observed on a real notched display, over a fullscreen app, or with
a hidden menu bar.

Hex itself never activates on paste. Another app's modifier-tap hotkey once
switched Spaces before the paste; the [chord breaker](#hold-and-short-taps)
keeps a held bare-modifier dictation from reading as a tap.

**Observed September 14, 2026:** the unmodified HUD was not found on screen over
a native fullscreen TextEdit fixture. Changing only the window class to
`NSPanel` did not produce a visible HUD in the initial check. The candidate with
`CanJoinAllApplications` and the repeating fixture visibly showed the red HUD
over fullscreen TextEdit, captured through Cua Driver. This was a debug preview
with no microphone capture, not a signed installed-app or multi-display test.
Kit independently confirmed the repeating candidate was visible in fullscreen.

**Combined verification September 14, 2026:** 468 Rust tests passed with ten
native/opt-in checks ignored, plus all twelve keyboard-layout child scenarios
in debug and release. Strict all-target/all-feature Clippy, 46 command-SDK tests,
and the command-SDK build passed. The native preview also built successfully.
These checks preceded the app release.

**2.1.18 candidate verification September 14, 2026:** the optimized suite passed
all 468 Rust tests and twelve keyboard-layout child scenarios; ten opt-in checks
were skipped. Strict release all-target/all-feature Clippy and app-identity
fixtures passed. The signed, notarized bundle's isolated HUD preview was visibly
present over native fullscreen TextEdit. This exercises the packaged renderer,
not microphone capture or an installed-app update. The preview and disposable
TextEdit process were stopped afterward.

## Gaps And Constraints

- **Physical input:** Fn/Globe, assistive input, remapped modifier sides, and
  cross-tap delivery ordering need their own native evidence. A passing Option
  fixture does not cover every binding. See [shortcut recovery](recovery.md).
- **Full user path:** source connections and individual checks do not prove the
  complete path on a user's Mac. Retain action-to-result evidence for hold,
  lock, Escape, and a second capture during older processing in a controlled
  target app. Observe insertion and input responsiveness, not just the HUD.
- **Microphone modes:** verify onset and idle closure separately for Keep ready
  and Release when idle on an identified device. The macOS microphone indicator
  while Keep ready is idle is expected, not proof of recording.
- **Streaming:** `dictate.streaming` has benchmark, synthetic, and Linux CI
  (CPU-forced) evidence only. Real microphones, room noise, Bluetooth inputs,
  and a physical shortcut on a Mac or a Linux GPU have not been observed with a
  live stream.
- **Paste consumption:** current paste policy waits a fixed interval rather than
  acknowledging target consumption. Internal ordering does not establish
  correctness for an arbitrarily slow target application.
- **Native checks require deliberate setup:** permissions, model, keyboard,
  microphone, output device, and a disposable text target must be recorded. This
  map does not authorize operating the installed app or injecting global input.
