# Dictation

Hold the configured shortcut, speak, and release. Hex transcribes locally and
inserts the result at the current focus. New captures do not cancel accepted
work. Only Toggle mode has a cancel: its cancel shortcut, Escape by default,
discards the recording (#786, #1022); every other intentional dictation
finishes and pastes. The exception is a Labs › Caps Lock dictation capture on
macOS: it is hands-free in every shortcut mode, so its own Escape binding
cancels it even in Hold.

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
standalone function-key bindings. On Windows and Linux/X11 it also accepts mouse buttons, and on
macOS with Labs › Mouse shortcuts on (off by default; issue 209; see labs.md),
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

macOS with Labs › Caps Lock dictation on (`labs.caps_lock_dictation`, off by
default): a physical Caps Lock press toggles a hands-free dictation instead of
typing. From idle it starts a capture that stays recording without a hold; the
next press finishes it through the same transcription, processing, and paste
path as Toggle's lock. A hold capture, a pending double tap, or another
shortcut's recording is left alone, the normal shortcut keeps working when
nothing records and stands down while a Caps Lock capture runs, and Escape
cancels the Caps Lock capture in every shortcut mode (in Toggle mode, Toggle's
other cancel shortcut does not; only Escape does). A Caps Lock remapped to
Control or Escape never changes AlphaShift, so it never toggles dictation. Each
press also clears the system Caps Lock state, so following keys stay
lowercase, and the key's light comes on once the capture's microphone delivers
audio, solid while any capture records (including a normal-shortcut or Annotate
capture), blinks at 350 ms while Hex transcribes or processes, and is off
otherwise. Its Labs › Recording light option can make recording blink slowly at
700 ms instead of staying solid. Turning the lab off or quitting Hex clears the
light and restores normal Caps Lock behaviour. The light needs Input Monitoring
(which Hex already asks for); without it dictation still works and the light
stays under macOS's control. Implemented November 7, 2026, not verified on
hardware; details and checks in [labs.md](labs.md).

Wayland's XDG shortcut syntax has no mouse-button trigger. Use the desktop's
mouse bindings to run `hex dictation start`/`stop` (hold) or `hex dictation toggle`
(toggle), or `hex paste-last`. Existing mouse
bindings from X11 appear ungranted on Wayland; they do not prevent keyboard
shortcuts from registering. Hex does not use raw input devices or XWayland to
bypass the compositor. See the [XDG shortcut specification](https://specifications.freedesktop.org/shortcuts/latest/).

**Fixed November 1, 2026 (issue 846):** the portal path refused navigation and
editing keys ("unsupported shortcut key: home"), though GNOME's custom
shortcuts took them. Both now share one named-key table
(`gnome_shortcuts::named_keysym`): Home, End, Page Up/Down, the arrows,
Insert, Delete, and Print reach the portal as `Home`, `End`, `Page_Up`, … ,
alone or with modifiers. Escape stays refused by the portal, which never binds
it (#786). Check: `navigation_keys_have_portal_triggers` in
`linux/input.rs` (constructed bindings; no desktop portal run). X11 grabs
don't resolve these keys yet.

Linux with Labs › Special keys alone on (`labs.standalone_special_keys`, off
by default; issue 767): Menu, Pause, and Scroll Lock may be any shortcut's
key, alone like a function key or with modifiers, for example Caps Lock
remapped to Menu with GNOME's keyboard options and
`hex settings set dictation_hotkey menu` (also `pause`, `scrolllock`). They
reach X11 grabs as keysyms 0xff67/0xff13/0xff14 and the portal as `Menu`,
`Pause`, `Scroll_Lock`. Off, `hex settings` refuses them, the window's
recorder refuses them, and a saved shortcut on one goes back to its default
when settings load or the toggle turns off. On, the window's recorder takes
them too (#1187). Details and checks in [labs.md](labs.md).

Linux with Labs › Section key alone on (`labs.section_key_alone`, off by
default; issue 1050): the ISO § key, left of 1, may be a shortcut with no
modifier. The window's recorder takes § pressed alone and shows §, and
`hex settings set dictation_hotkey §` (or `section`) saves it. It reaches X11
grabs as keysym 0xa7 and the portal as `section`. Off, the recorder answers
"Add a modifier" as before, `hex settings` refuses it, and a saved § shortcut
without a modifier goes back to its default when settings load or the toggle
turns off. While bound, § is Hex's in every app. Details and checks in
[labs.md](labs.md).

Checks: `mouse_buttons_record_with_or_without_modifiers`,
`mouse_bindings_persist_and_conflict_only_with_the_same_trigger`,
`mouse_hold_and_release_use_the_dictation_state_machine`,
`mouse_double_tap_locks_and_a_fresh_press_finishes`,
`mouse_toggle_uses_the_same_capture_controls`,
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
the side picker (macOS and Windows) and the recorder; with no shortcut the tray reads Off.
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
`function_keys_match_despite_the_fn_flag_macos_adds` in `suppression/tests.rs`
(constructed events; no physical-keyboard run).

**Fixed October 30, 2026 (issue 462):** the Globe key can also send its own key
event (key code 179) beside the Fn modifier change, seen with
Karabiner-Elements running. A Fn shortcut read that event as another key pressed
with it and discarded most presses as a chord 60–130 ms after they started.
For a shortcut that holds Fn, key code 179 is now neither a chord key nor HID
chord evidence; any other key still is. Check:
`globe_key_events_are_not_a_chord_with_the_function_shortcut` in
`suppression/tests.rs` (constructed events; no physical-keyboard run).

**Implemented November 7, 2026 (issue 37), unverified:** remapping Caps Lock to
Control makes macOS's HID keyboard filter rewrite its usage (0x39) to Right
Control (0xE4) before CoreGraphics sees it, so a Right Control shortcut also
started from Caps Lock. Hex now reads the raw HID modifier state below that
filter (`IOHIDManager`, opened without seizing the keyboards), and when Caps
Lock is physically held while a side-specific modifier bit has no matching
physical key, it drops the side and keeps the general modifier flag, which
matches as either side. A real Right Control press (physical usage 0xE4) is
untouched. Hex never requests Input Monitoring, so the tracker runs only when
Hex already has it: turn Hex on under System Settings › Privacy & Security ›
Input Monitoring and restart Hex. `IOHIDCheckAccess` gates the tracker; without
that permission it stays off (logged once), the flags are unchanged, and Right
Control still starts from a remapped Caps Lock. The shortcut recorder applies
the same normalization only while the listener's input monitor is running, so
recording a remapped Caps Lock saves a side-less Control. This is implemented
but not verified on physical hardware; see the missing-evidence note in
[recovery.md](recovery.md#remapped-control).
Check: `normalized_remapped_caps_lock_starts_only_a_side_less_control_binding`
and `a_remapped_caps_lock_does_not_hold_a_real_right_control_capture` in
`suppression/tests.rs`, plus the normalization tests in
`app_settings/hotkeys.rs` (constructed snapshots; no physical remap run in CI).

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
side, so ⌥⌘ works from either hand. On macOS, Labs › Sided modifier chords
(`labs.sided_modifier_chords`, off by default, issue 710; see
[labs.md](labs.md)) makes the recorder keep each modifier's side in a
modifier-only chord instead, so Right ⌘ + Right ⌥ records as `R⌥ R⌘` and the
left keys don't press it; matching was already sided. While recording, every held key shows with
its side (`L⌥ R⌥ ⌘` on the Mac, `Left Alt` elsewhere), so an accidental extra
key is visible before release. `hex settings` takes
`left-option+right-option`. Settings store `{"keys": [...]}`; files with the
old `modifiers`/`key` shape load as the same chord and are written back once.
Windows matches sides through its low-level hook and accepts one modifier
alone (Right Alt or Right Ctrl, [#508](https://github.com/anomalyco/hex/issues/508))
as well as both Alts:

```ts
Windows modifier-only shortcut                           // dictate.windows-lone-modifier
  ├── Press alone -> Down; release -> Up (hold, lock, Toggle as above)
  ├── Any key or click joins while held -> Chord: Cancel if recording, else nothing
  │   (Escape never: it reaches the app and the capture goes on, #786)
  │   (the key reaches the app; locked and toggle captures keep recording)
  ├── Alt or Windows released -> Swallowed, VK 0xE8 tapped, release replayed
  │   -> No menu bar or Start menu; Share shortcut skips the mask
  └── Hex's own injected keys (paste, mask) -> Skipped by the hook (HEX_INJECTED)
```

On layouts where Right Alt is AltGr, Windows adds a Left Ctrl press, so Right
Alt alone never matches there; bind Right Ctrl instead. Checks:
`one_modifier_alone_is_a_shortcut_and_alt_or_windows_mask_their_release`,
`a_key_joining_a_held_modifier_ends_it_quietly_unless_it_records` in
[windows/input.rs](../../src/windows/input.rs), cross-compiled only; the menu
mask and AltGr behavior have not run on a Windows PC.

**Fixed November 4, 2026 (issue 1254):** a Windows key chord holding Alt or
Windows (Alt+0, say) swallows its key, so the app saw Alt pressed and released
alone. Chrome took that as a menu tap and moved focus to its menu button, and
the paste landed there instead of the page. When Hex swallows the key of a
chord (dictation, Dictate without processing, Terminal paste shortcut, Paste
Last, Annotate) that holds
Alt or Windows, that modifier's release is now masked like a modifier-only
shortcut's: swallowed, VK 0xE8 tapped, release replayed (not with Share
shortcut). Check: the `holds_menu_modifier` assertions in
`one_modifier_alone_is_a_shortcut_and_alt_or_windows_mask_their_release`
(cross-compiled only; not run in Chrome on a Windows PC). Linux
binds what the portal and X11 grabs take, a key with modifiers and no side:
releasing modifiers pressed alone in the recorder says Linux needs a key (or
can't bind both sides) and suggests adding one, such as Space.

Checks: the `both_options_*` tests and
`one_sided_options_never_start_with_both_held_but_either_side_does` in
`suppression/tests.rs`, `shortcuts_store_as_chords_and_old_files_convert_once` and
`both_sides_conflict_only_with_either_side_and_match_only_together` in
`app_settings.rs`, `both_options_record_as_one_chord_of_two_keys` in
`rune_ui/macos_host.rs`, `both_sides_must_go_down_together` in
`windows/input.rs`, and
`a_modifier_alone_says_linux_needs_a_key_and_suggests_one` in
`rune_ui/tests.rs`. Constructed events only; no physical keyboard run on any OS.

### Shortcuts Under Secure Input (macOS)

**Added November 9, 2026:** while another app holds Secure Event Input (a
password field, a terminal's Secure Keyboard Entry, an app that never turned it
off), macOS stops handing key presses to Hex's event tap; modifier changes
still arrive. Hex polls `IsSecureEventInputEnabled()` every 0.25 s and, only
while it is on, registers the key shortcuts that start something as Carbon
hotkeys (`suppression/secure_input.rs`). A Carbon hotkey takes its key before
the session tap, so registering only then keeps the tap's chord rules the rest
of the time.

```ts
Secure input turns on  // dictate.secure-input
├── Log: "secure input is on…" with the pid and app name of the app that was
│   in front when it turned on (kCGSSessionSecureInputPID, as `ioreg -l` shows
│   it), which is not necessarily the one holding secure input
├── Registered as Carbon hotkeys: Annotate, Annotate replay, Paste last,
│   Paste meeting, and with Labs › Screenshots its two shortcuts
│   ├── A press -> the same key down the tap would have sent (log: "shortcut
│   │   arrived through secure input"); a release -> its key up
│   ├── Annotate's "starts once a modifier comes up" still works: the tap
│   │   sees the modifier change and records
│   └── Sides can't be told apart: Right ⌥ ⇧ S also fires on Left ⌥
├── Not registered: a binding needing Fn/Globe, mouse buttons, keyless
│   bindings, and nothing while a shortcut is being recorded in Settings
└── Still don't work: a dictation shortcut with a key (modifier-only ones do),
    Dictate without processing with a key, Escape (Toggle cancel, an armed
    Annotate press, stop drawing), and the screenshot picker's Esc and Space
Secure input turns off -> hotkeys unregistered (a held one sends its release),
    log: "secure input is off…"
```

Check: the tests in `suppression/secure_input.rs` (chord selection, press
flags, missing-release bookkeeping, and Annotate's arm-then-record through
`CaptureShortcut`). Live: a synthetic ⌥⇧F20 posted at HID while another app
held secure input fired a Carbon press and release, and a listen-only session
tap saw only the modifier changes. Not run with a physical keyboard press.

**CLI:** `hex listen` uses saved settings; `--device` is authoritative. This is
the CLI listener, not Settings. Do not run it beside an existing listener.

**Separate targets:** Host-owned API capture has its own ownership protocol and
does not prove the physical dictation shortcut; see the [feature index](README.md).

## Sub-features

### Dictate Without Processing

A second, optional dictation shortcut (issue 451, suggested by @ksajan). It
records exactly like the dictation shortcut and pastes the transcript as
heard, skipping the Output pane's steps: replacements, corrections,
transformations, and OpenCode rewriting.

```ts
Output > Without processing > Dictate without processing   // raw.shortcut
├── Unbound by default -> nothing registered, grabbed, or reserved
│   └── Record a shortcut (the same recorder) -> registered on every OS
│       ├── macOS: any binding the dictation shortcut takes; its own
│       │   shortcut machine beside dictation's (recognition::Hotkeys)
│       ├── Windows: a key chord (the hook's raw trigger)
│       └── Linux: an X11 grab, or a portal entry "Dictate without processing",
│           only while bound
├── Hold / double-tap lock / toggle -> as the dictation shortcut
│   ├── The shortcut that started a capture owns it: the other one neither
│   │   finishes it nor counts toward its double tap
│   └── Queueing and ordered output -> shared with dictation
├── Finish -> transcript pasted as heard (trimmed)          // raw.paste
│   ├── History -> kind Raw ("raw" in `hex history --json`)
│   └── Paste Last -> pastes it (it is the last dictation)
├── Clear (trash) -> unbound again
└── hex dictation start|toggle --raw -> the same, without a shortcut
    └── A printing stop (`stop --print`) of it prints the transcript as heard
```

Checks: `raw_dictation_owns_the_capture_it_starts` (Linux reducer),
`every_command_round_trips` and the `check_flags` cases in `control.rs`.
Proof limits: no physical shortcut run on any OS; the macOS machine pairing
was not compiled or run here (Linux check, Windows cross-clippy only).

### Transform Typed Text (Labs, Linux)

With Labs › Transform typed text on (`labs.typed_text`, off by default),
Settings › Output adds one section whose shortcut is unbound until recorded.
The shortcut reads the text already typed in the focused field, runs the
focused app's Output steps on it, and pastes the result over it. `hex
typed-text` runs the same job for desktops without the shortcuts portal, and
refuses clearly while the lab is off or Hex isn't listening. The lab's switch
and binding are saved like any other Labs feature; the listener registers the
shortcut only while the lab is on, and restarts registration when either
changes. The job sends Ctrl+A and Ctrl+C, so it is not meant for terminals.

```ts
Output > Typed text                        // labs.md
├── Labs off -> no section, no recorder, no shortcut registered
├── Labs on -> the section, with the same recorder as Dictate without processing
│   └── typed_text_hotkey unbound by default -> nothing registered or reserved
│       ├── Record one -> saved, and conflicts with the other active shortcuts
│       └── Clear (trash) or `hex settings set typed_text_hotkey none` -> unbound
├── Settings -> saved to settings.json as typed_text_hotkey
└── Shortcut pressed while idle, or `hex typed-text` -> one job on the output worker
    ├── Terminal paste on -> refused before any key: its Ctrl+V would
    │   become Ctrl+Shift+V; the job logs why and sends nothing
    ├── take the clipboard for the job: cancel any paste restore still pending
    │   and keep the clipboard that paste replaced, else snapshot it now
    ├── clear the clipboard, then Ctrl+A and Ctrl+C in the focused app,
    │   pausing 30 ms so the selection settles before the copy
    │   ├── GNOME without data-control clears through `wl-copy --clear`
    │   └── a clear that fails, or leaves text behind, aborts before copying
    │       and restores
    ├── pause 40 ms, then poll the clipboard for text, up to 600 ms, honoring
    │   Cancel; a read that races the copy can take the focus it needs on
    │   GNOME, where every read runs `wl-paste`
    ├── read the selection as text/plain, bounded to 1 MiB and one second
    ├── no text, or only whitespace -> error tone, clipboard restored, nothing pasted
    ├── run the matching mode's Output steps, with transformation context
    │   `source: "typed-text"` (dictation passes `"dictation"`); spoken
    │   formatting, spoken symbols, and filler words are dictation-only
    ├── processed text equals the selection -> clipboard restored, nothing pasted
    ├── clear, re-copy the selection -> different text or an ignored copy ->
    │   abort, clipboard restored, nothing pasted (typing or switching windows
    │   since the first copy)
    ├── otherwise Ctrl+V over the selection; a paste failure restores the
    │   clipboard, and a success restores it 500 ms later unless something newer
    │   replaced the result first
    └── a clipboard Hex could not snapshot still clears and runs, but its result
        stays on the clipboard
```

Checks: `typed_text_is_its_own_bit_and_off_by_default` ([labs.rs](../../src/labs.rs)),
`the_typed_text_shortcut_needs_its_lab_and_its_own_keys`
([app_settings.rs](../../src/app_settings.rs)),
`typed_text_is_registered_only_while_its_lab_is_on` and
`typed_text_fires_only_while_idle_and_changes_no_capture_state`
([input.rs](../../src/linux/input.rs)), `typed_text_skips_the_dictation_only_labs`
and `transformation_context_names_its_source` ([processor.rs](../../src/dictation/processor.rs),
[personal_commands.rs](../../src/personal_commands.rs));
`typed_text_joins_the_commands_only_while_its_lab_is_on`
([gnome_shortcuts.rs](../../src/linux/gnome_shortcuts.rs)); the fake-backed
`an_ignored_copy_pastes_nothing_and_restores_the_clipboard`,
`an_ignored_second_copy_pastes_nothing`, `text_changed_between_copies_pastes_nothing`,
`a_failed_clear_aborts_before_copying_or_pasting`,
`a_clear_that_leaves_text_aborts_before_copying`, `a_failed_paste_restores_the_original`,
`success_pastes_the_result_and_hands_over_the_original`,
`unchanged_processing_pastes_nothing_and_restores`,
`a_job_without_a_snapshot_still_clears_and_pastes`, and
`a_changed_second_copy_aborts_the_replacement`
([typed_text.rs](../../src/linux/typed_text.rs));
`a_new_clipboard_job_adopts_the_pending_restore`,
`a_changed_clipboard_is_not_replaced_by_a_pending_restore`, and
`a_failed_dictation_paste_leaves_the_transcript_on_the_clipboard`
([paste.rs](../../src/linux/paste.rs)); and `the_gnome_shortcut_key_tracks_typed_text`
([app.rs](../../src/linux/app.rs)). Known limits: select-all replaces the
whole field, so a field with mixed formatting loses it; on GNOME the clipboard
text is read through `wl-paste`, which may briefly take focus. Proof limits: no
shortcut press against a real target application was run; the flow is covered
by unit tests only.

### Terminal Paste Shortcut (Labs, Windows and Linux)

A third optional dictation shortcut for typing into a terminal while the main
dictation shortcut keeps the global Terminal paste setting. While Labs ›
Terminal paste shortcut is on (`labs.terminal_paste_shortcut`, off by
default), General shows its recorder under Shortcut:

```ts
Labs > Terminal paste shortcut > recorder   // dictate.terminal-paste-shortcut
├── Unbound by default -> nothing registered, grabbed, or reserved
│   └── Record a shortcut -> the listener registers it while the lab is on
│       ├── Windows: the hook's terminal-paste trigger
│       └── Linux: an X11 grab, or a portal entry "Terminal paste shortcut"
├── Linux has no `hex` command for it (unlike Dictate without processing):
│   without X11 grabs or the desktop's shortcuts portal it stays unregistered
├── Hold / double-tap lock / Toggle -> as the dictation shortcut
│   ├── The shortcut that started a capture owns it: dictation and Dictate
│   │   without processing neither finish it nor count toward its double tap
│   └── Its flavour survives the post-release tail to the paste
├── Finish -> processed like a dictation (modes, History kind Dictation,
│   last dictation updated) and pasted with Ctrl+Shift+V
│   └── Windows javaw/java still take plain Ctrl+V
├── Clear (trash) -> unbound again; the lab off -> inactive and unregistered
└── Paste Last -> keeps the global Terminal paste setting
```

Checks: `terminal_paste_shortcut_owns_the_capture_it_starts` (Linux reducer),
`the_terminal_paste_shortcut_needs_its_lab_and_its_own_keys` (settings), and
the Windows `a_second_dictation_shortcut_owns_the_capture_it_starts`
(cross-compiled only). Proof limits: no physical shortcut or real terminal run
on either OS.

### Second Dictation Shortcut (Labs, every OS)

Another optional dictation shortcut, for a keyboard whose main shortcut key is
out of reach, such as a Globe/Fn key on a laptop and a spare key on an external
keyboard. While Labs › Second dictation shortcut is on
(`labs.second_dictation_shortcut`, off by default; issue 627), General shows
its recorder under Shortcut, unbound until recorded and clearable:

```ts
Labs > Second dictation shortcut > recorder    // dictate.second-dictation
├── Unbound by default -> nothing registered, grabbed, or reserved
│   └── Record one -> registered only while the lab is on
│       ├── macOS: its own shortcut machine beside dictation's (recognition::Hotkeys)
│       ├── Windows: the hook's own trigger, owning its captures
│       └── Linux: an X11 grab, or a portal entry "Second dictation shortcut"
│           └── the desktop may grant a different key; Settings shows it
├── Windows and Linux -> a binding needs a key; a keyless one never runs there
├── Hold / double-tap lock / Toggle -> as the dictation shortcut
│   ├── The shortcut that started a capture owns it: dictation, Dictate
│   │   without processing, and Terminal paste neither finish it nor
│   │   count toward its double tap              // DictationFlavour::Second
│   └── Own flavour -> processed and pasted exactly like dictation
│       (modes, History kind Dictation, last dictation updated)
├── Clear (trash) -> unbound again; the lab off -> inactive and unregistered
└── Paste Last and meeting paste -> only from the main shortcut
```

Checks: `second_dictation_shortcut_owns_the_capture_it_starts` (Linux reducer),
`second_dictation_is_registered_only_while_its_lab_is_on` (Linux listener
options), `the_second_dictation_shortcut_needs_its_lab_and_its_own_keys`
(settings), and the Windows `the_second_dictation_flavour_owns_its_capture`
(cross-compiled only). Proof limits: no physical shortcut press on any OS, no
real Wayland grant or X11 grab run, and no Windows hook run.

### Copy Recent Dictation (Labs, macOS)

Paste Last Dictation reaches only the newest dictation. With Labs › Copy
recent dictations on (`labs.recent_dictations`, off by default, issue 877):

```ts
Menu bar > Copy Recent Dictation (under Paste Last Dictation)  // dictate.copy-recent
├── Menu opens -> History read once: five newest entries within retention,
│   newest first, no printed or empty ones (as Paste Last skips them)
│   └── One line each: whitespace collapsed, cut to 48 characters with "…"
├── Click one -> Its full final text on the clipboard; nothing is pasted
└── Toggle off, History Off, or nothing to list -> Item hidden, History unread
```

Details and checks in [labs.md](labs.md).

### Return to app (Labs, macOS)

A dictation pastes into whichever app is frontmost when its transcript is
ready. With Labs › Return to app on (`labs.return_to_app`, off by default):

```ts
Dictation shortcut, Caps Lock, or `hex dictation start` starts
  -> The frontmost app's pid is recorded (never Hex's own)
Paste job -> Carries it through inference and processing        // dictate.return-to-app
├── Same app in front -> Paste as usual
├── Another app in front -> Activate it, poll 10 ms up to 500 ms,
│   settle 50 ms, then paste
└── Gone or not confirmed -> Nothing pasted; the job fails and its text
    becomes Paste last dictation's unless the job was cancelled
```

Details and checks in [labs.md](labs.md).

### Skip Long Pauses (Labs, Linux and Windows)

A dictation transcribed as a whole clip (no stream on this model or device,
or the fallback after a failed or late stream) sends every second of silence
through the model. With Labs › Skip long pauses on (`labs.skip_pauses`, off
by default, issue 899):

```ts
Whole-clip run starts -> labs::skip_pauses()        // dictate.skip-pauses
├── Off -> The clip, untouched (as before)
├── On -> Each pause at the noise floor keeps its first 2.5 s and last 0.3 s,
│   a trailing pause its first 2.5 s; shorter pauses pass whole (the stream's PauseCap)
├── Nothing heard above the floor -> The clip, untouched (#841)
└── Job deadline, History duration, silence discard -> The full clip
```

Details and checks in [labs.md](labs.md).

**Fixed November 3, 2026 (issue 1194):** a clip longer than the model's
window (Cohere's 35 s) is transcribed in chunks, and a chunk that was only
silence (a locked dictation left waiting before the words) went to the model
anyway; Cohere invented long repeating text for it, pasted before the real
sentence. Each chunk of such a clip now goes through the speech detector
first and is skipped when it hears no speech; without the detector every
chunk still runs. Clips that fit the window are unchanged. Check:
`a_long_clip_skips_only_chunks_the_detector_hears_no_speech_in` in
[gguf_session.rs](../../src/transcription/gguf_session.rs) (detector verdict
overridden; not run with Cohere on real audio).

### Vocabulary Quality Checks

`hex benchmark-transcription` accepts a scoring vocabulary and expected terms,
and reports recall, excess vocabulary occurrences and WER by corpus group, plus
overall latency. Its companion scorer also reports per-group latency.
The [fixture guide](../../tests/fixtures/vocabulary/README.md) describes
the synthetic set, public guard download, and ignored private manifest path.
This is offline evaluation coverage, not a shipped vocabulary feature or proof
of recognition quality on a microphone. Metric checks live in
`transcription::benchmark::tests`; inference results belong in the research report.

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
Checks in [suppression/tests.rs](../../src/suppression/tests.rs):
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
[linux/input.rs](../../src/linux/input.rs) (constructed inputs; not run on
GNOME).

**Fixed October 31, 2026 (issue 733):** an X11 paste waited, with no bound,
until the X server reported no modifiers held. XWayland's modifier state goes
stale while a Wayland window has focus and only a later key event clears it,
so a finished dictation sat in Transcribing for about a minute. The paste now
waits for held modifiers only while one of Hex's shortcuts is down (a newer
capture owns them); otherwise it waits at most 2 s, logs that modifiers still
read as held, and pastes. Checks:
`stale_modifiers_without_a_held_shortcut_stop_holding_the_paste` and
`accepted_output_waits_through_another_capture` in
[linux/paste.rs](../../src/linux/paste.rs) (constructed state; not run on a
desktop).

**Added November 3, 2026 (issue 1111):** on GNOME Wayland the previous
clipboard never came back after a paste, because Mutter offers neither
ext-data-control nor wlr-data-control. Labs › Restore clipboard on GNOME
(`gnome-clipboard-restore`, off by default, Linux) now snapshots, checks, and
restores it through XWayland's CLIPBOARD selection, which Mutter bridges to
Wayland apps both ways. It applies only on a Wayland session without
data-control and only while this user's Xwayland process already runs with
`DISPLAY` set, so Hex never starts XWayland; the generation and
still-holds-the-paste rules are unchanged. While it is off there, Settings ›
Output shows a note pointing to it. Checks:
`xwayland_restores_only_by_opt_in_without_data_control`,
`xwayland_is_looked_for_only_when_it_could_be_used`, and
`xwayland_counts_only_this_users_process` in
[linux/clipboard.rs](../../src/linux/clipboard.rs);
`gnome_desktops_point_to_the_clipboard_lab_until_it_is_on` (constructed state;
not run on GNOME).

### Live Typing (Labs, Linux Wayland)

```ts
Labs › Live typing on (labs.live_typing, off by default, Wayland only)  // labs.live-typing
Capture becomes intentional (hold threshold, or the Toggle press)
  -> Takes a dictation::live_text::TypingDemand, then Begin: get_input_method(seat)
  -> The stream opens afterwards, so the engine sees the demand and publishes partials
  -> Each stable + tentative change -> The newest preedit is sent (older ones coalesce), underlined in the focused field
  -> Escape, a short tap, silence, a lost microphone, a failed job, or the listener quitting -> Cancel: clear, destroy
Release -> Process the final text -> Finish
  ├── Active text input and one line within 4000 bytes -> The owner claims the commit first,
  │     then clear preedit, commit_string, destroy -> inserted, clipboard untouched
  ├── A caller that gives up first -> abandoned; owner only clears (up to 250 ms), then LinuxPaster.paste
  ├── No decision in 350 ms and the owner already claimed -> wait up to 750 ms and do not paste
  └── CPU engine or non-streaming model -> No words while speaking; the final text still commits here
```

The input method exists from that intentional start until Finish or Cancel,
which includes post-release processing; a short tap or chord that never
becomes intentional creates none. It does not need a stream: a CPU engine or
a model that cannot stream shows no provisional words, but the finished text
still goes in through the input method instead of the clipboard. Hex's own
first-run practice field on Linux is an ordinary focused field, so it takes
the preedit and the final commit too while Live typing is on. A newer
capture's Begin takes over from an older job, whose Finish then returns false
and pastes. `zwp_input_method_manager_v2` is a wlroots-family protocol (Sway,
Hyprland, and other wlroots compositors); KWin and Mutter do not offer it, so
KDE and GNOME paste through the RemoteDesktop portal or `wtype` instead. A
compositor with another input method (fcitx, ibus) reports `unavailable` and
Live typing stays inert.

History, last dictation, tones, and Output are unchanged: a committed
dictation counts as pasted. Live text off does **not** disable Live typing:
the capture takes its own `TypingDemand` at that same intentional start
before the stream opens, which the engine sees and the plate does not. The
plate itself is decided once when the stream opens, so toggling Labs › Live
text mid-capture changes nothing. On X11 or with the toggle off, nothing
starts and the capture behaves exactly as before.

The commit claim is one shared atomic state: the owner changes Pending →
Committing before it queues any commit request, and a waiting caller changes
Pending → Abandoned when it gives up. Only one can win. If the caller wins,
it waits up to 250 ms for the owner's clear; after that it pastes anyway, so
a wedged compositor can leave a preedit visible until it tears the input
method down. After an abandoned Finish, Live typing starts no new captures
and pastes instead until Hex restarts; a session that had already begun
still finishes and cancels. A residual protocol race remains: a `done` event can arrive
after `dispatch_pending` and before `commit(serial)`, so a commit can use a
serial one behind; the affected preedit or commit is then ignored by the
compositor. A commit ignored for a stale serial has already returned
“inserted”, so that dictation is lost rather than pasted (wlroots does not
enforce the serial today). Checks (unit tests only):
`provisional_words_become_one_preedit_line`,
`a_preedit_stops_at_a_character_boundary`,
`only_one_line_text_within_the_limit_commits_through_the_input_method`,
`the_cursor_sits_after_the_preedit`,
`done_events_number_the_commit_serial_and_apply_activation`,
`the_owner_wins_the_commit_before_queueing_it`,
`a_waiting_caller_wins_abandonment_before_the_owner_claims`,
`a_started_session_cancels_when_dropped`,
`a_session_that_never_started_sends_nothing`,
`finishing_a_session_never_sends_cancel`,
`only_the_newest_preedit_waits_and_it_wakes_once`,
`a_full_control_queue_stops_new_sessions_but_not_old_ones`, and
`a_stopped_owner_still_finishes_and_cancels_a_session_that_began` in
[linux/live_typing.rs](../../src/linux/live_typing.rs);
`a_typing_demand_feeds_the_engine_without_wanting_the_plate` in
[dictation/live_text.rs](../../src/dictation/live_text.rs); and
`the_plate_follows_the_decision_made_when_the_stream_opened` in
[dictation.rs](../../src/dictation.rs). **Not verified on
a real compositor**: no GTK/Qt field or Sway/Hyprland run; KWin and Mutter
fall back to paste by design.

**Fixed November 3, 2026 (issue 1158):** on Linux with PipeWire, Hex's
microphone stream (held open while the microphone is kept ready) was named
by the ALSA plugin after the binary, with spaces and varying by packaging,
so audio idle inhibitors couldn't be told to ignore it and the screen never
locked. Every ALSA stream Hex opens now carries `application.name` and
`node.name` `hex` (`PIPEWIRE_ALSA`, set at startup beside
`state.restore-target`). Check: `streams_carry_a_stable_name_without_spaces`
in [linux/input_route.rs](../../src/linux/input_route.rs) (property string
only; not run against PipeWire or an inhibitor).

**Fixed November 4, 2026 (issue 1309):** on Linux (PipeWire), about a second
of speech or background audio from before the press was transcribed. The
press was placed at the end of the newest microphone chunk, and ALSA's default
period can be most of a second, so the 450 ms pre-roll counted from there.
Hex now asks for a 20 ms period (falling back to the device default), places
presses and releases on the stream clock (`StreamClock` in
[linux/dictation.rs](../../src/linux/dictation.rs)), and keeps exactly the
pre-roll before the press, dropping earlier audio from chunks still to arrive
(`DictationCapture::start_at_press`). The hold threshold counts from the
press, and a replaced or reopened stream starts its clock over; a press
before its first chunk records from that chunk. Checks:
`a_press_inside_a_large_chunk_keeps_exactly_the_pre_roll` and
`a_press_ahead_of_large_chunks_measures_the_hold_from_the_press` in
[dictation.rs](../../src/dictation.rs),
`a_press_between_large_chunks_is_placed_on_the_stream_clock` and
`a_reopened_stream_carries_no_old_clock` in linux/dictation.rs (constructed
chunks; not run against PipeWire or a physical microphone).

**Changed November 4, 2026 (issue 1309 follow-up):** with "While dictating"
set to Mute or Pause media, the pre-roll came from before the mute took
effect and picked up whatever was playing. On every OS, those hotkey captures
now keep no pre-roll (`dictation::hotkey_pre_roll`); Do nothing keeps 450 ms
(Duck kept it until issue 1468, below). There is no separate setting. Check:
`suppressing_audio_while_dictating_keeps_no_pre_roll` in
[dictation.rs](../../src/dictation.rs) (constructed audio; not run on a
device).

**Changed November 7, 2026 (issue 1468):** with "While dictating" set to
Lower audio, lowering ramped over a full second after the 300 ms hold
threshold, and the 450 ms pre-roll came from before the lower took effect, so
the start of every dictation stayed loud. Lowering now reaches its target in
about 120 ms (`LOWER` in
[audio/output_ducking.rs](../../src/audio/output_ducking.rs)), Duck keeps no
pre-roll like Mute and Pause media, and restoring still eases back over one
second. Checks:
`ducking_preserves_balance_silence_and_restores` and
`lowering_finishes_at_its_own_duration_while_restoring_is_still_ramping` in
audio/output_ducking.rs, and
`suppressing_audio_while_dictating_keeps_no_pre_roll` in
[dictation.rs](../../src/dictation.rs) (constructed audio; not run on a device
or physical output).

**Added November 9, 2026 (Labs › Only on speakers):** with the lab on
(`labs.speakers_only`), "While dictating" applies only while the default
output is speakers. The route is read once per recording or screen capture on
the recording environment's worker thread (macOS) or the audio-control worker
(Linux and Windows), and with headphones the session holds no audio effect at
all (no mute, lower, or media pause); dictation still prevents idle sleep and
screen captures never do. On macOS an overlapping session that shares an
already-held effect reuses the first session's decision. Speakers and anything
unclassified count as speakers, and with the lab off nothing changes and the
output is never read.
Screen-capture sessions go through the same guard: macOS's `begin_capture`
uses the recording environment, and Linux and Windows' capture host uses the
same audio suppressor as dictation. On macOS
[audio/output_route.rs](../../src/audio/output_route.rs) reads the default
output's CoreAudio transport (Bluetooth and Bluetooth LE are headphones), a
built-in device's output-scope data source (`hdpn` when the jack is in use),
and its UID (`BuiltInHeadphoneOutputDevice`). Linux reads `pactl
get-default-sink` and `pactl -f json list sinks`: headphones or a headset in
the default sink's active port (`analog-output-headphones`, `[Out] Headset`)
or its `device.form_factor`, and a Bluetooth sink (`device.api` `bluez5`) is
headphones unless it says speaker. Windows reads the default output's WASAPI
form factor through cpal: headphones, headset, earpiece, and handset are
headphones. Limits: Bluetooth speakers read as headphones on macOS and on
Linux unless PipeWire names them speakers, an external USB headphone DAC
reads as speakers, a Linux session with only `wpctl` and no `pactl` reads
unknown (so "While dictating" applies), and the pre-roll still follows the
setting rather than the route. Checks:
`bluetooth_outputs_are_headphones`,
`a_built_in_output_on_its_headphone_jack_is_headphones`,
`apples_wired_headphone_device_is_headphones`,
`everything_else_is_speakers`,
`an_analog_sink_is_headphones_only_on_a_headphone_port`,
`a_headset_port_is_headphones`,
`bluetooth_sinks_are_headphones_unless_they_say_speakers`,
`a_sink_the_sound_server_does_not_name_is_unknown`,
`a_non_string_property_does_not_hide_the_others`, and
`cpal_headphone_types_are_headphones` in audio/output_route.rs;
`speakers_only_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed on a real Mac or a real Linux
or Windows desktop with headphones.

**Fixed November 2, 2026 (issue 978):** a Windows paste refused to run when
the clipboard held only a format Hex can't snapshot (an app's private
format), failing every dictation with "the current clipboard format cannot be
preserved safely". The transcript now pastes anyway; that clipboard just isn't
restored afterwards, and `process.log` says why. Check:
`an_unreadable_clipboard_still_pastes_and_restores_nothing` in
[windows/paste.rs](../../src/windows/paste.rs) (constructed state; not run on
a Windows PC).

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
General > Shortcut mode (under Shortcut)               // dictate.shortcut-mode
  -> Names the choice; opens the Shortcut mode modal
  Hold or double-tap (default) -> Hold, plus double-tap to lock
  Hold                         -> Hold only
  Tap                          -> Toggle, below
  Double-tap                   -> Double-tap only; needs a key binding, or a
                                  shortcut without a key (one or more
                                  modifiers) with Labs › Double-tap on a lone
                                  modifier (labs.double_tap_modifiers, macOS,
                                  off by default)
Tap
  Press -> Start (pre-roll as usual) -> Release keeps recording
  -> Fresh shortcut press -> Finish (with the release tail)
  -> Cancel shortcut (Escape by default) -> Cancelled, see Toggle Cancel below
  // Shortcut chords during the first 300 ms still discard
  macOS and Linux, Labs › Tap or hold on (labs.tap_or_hold, off by default, #711, #1144):
  ├── Release 300 ms or more after the press -> Finish (with the release tail)
  ├── Sooner -> Keeps recording, as above
  └── Linux: key repeats keep the first press's time; the dictation shortcut
      and Dictate without processing alike; hex dictation commands unchanged
  Linux: release never arrives (Hyprland Lua bind, Alt up before Z)
  ├── Press 1 s or more after the last press -> Finish
  └── Sooner -> Ignored as a key repeat (KWin repeats held shortcuts)
```

The one choice is stored in the three settings it replaced
(`shortcut_mode`, `double_tap_lock`, `double_tap_only`); of their eight
combinations only four behave differently, and `Trigger::of` in
[shortcut_mode.rs](../../src/rune_ui/shortcut_mode.rs) folds the rest onto
them (`redundant_and_invalid_combinations_fold_away`). `hex settings` still
sets each one. Linux with only `hex` commands shows no row: the commands
ignore it.

Tap (`toggle`) reuses the lock state on every platform: each press behaves as the
second tap of a double-tap. Persisted as `shortcut_mode` (`hold`/`toggle`) in
each platform's settings; files without it load as Hold. Checks:
`toggle_mode_starts_on_a_press_and_finishes_on_the_next` and
`toggle_mode_escape_leaves_a_locked_capture_recording` in
[suppression.rs](../../src/suppression.rs) (macOS reducer), the same names in
[windows/input.rs](../../src/windows/input.rs), and
`toggle_mode_starts_on_a_tap_and_finishes_on_the_next`,
`a_capture_that_would_lock_finishes_when_the_desktop_drops_its_release`, and
`a_repeating_held_press_never_finishes_the_capture` in
[linux/input.rs](../../src/linux/input.rs), whose one state machine serves X11
grabs, Wayland portal shortcuts, and the `hex dictation` commands alike. The
dropped release was observed on 2026-10-28 on Hyprland 0.56.2 with
xdg-desktop-portal-hyprland 1.4.1: uinput Alt+Z sent no `Deactivated` when Alt
came up first (the fix itself has not been run there yet). Only
the Windows build was compiled and tested locally (2026-10-19); macOS relies on
CI. The Linux state machine and X11 grab tests ran on 2026-10-22 (the grabs on
an isolated Xwayland display). No physical keyboard check yet.

```ts
General > Shortcut mode                               // dictate.shortcut-explainer
  -> Modal: the four choices, each its name and one line beside the
     shortcut's keycaps (one per key, pressing together) and two lanes (key
     above, recording below) on a chart-recorder tape: a still pen at the
     right, the tape scrolling left without end and fading out at its left
     edge; opening it ends a shortcut recording and closes the model picker
  ├── The current choice has a focus-coloured border
  ├── Double-tap with a shortcut that has no key -> Dimmed, takes no focus
  │   or press, says "Needs a shortcut with a key, like ⌥Space", unless
  │   Labs › Double-tap on a lone modifier is on (macOS)
  ├── Long chord -> The panel widens (up to the window) so each option's
  │   lanes keep at least 64 pt beside the keycaps
  ├── Pick an option (click, Space) -> Saves the choice
  ├── Tab -> each available option, Done; focus stays inside
  ├── Done, Return, Escape, or a press outside -> Closes; focus returns to
  │   the row
  ├── Change pane, menu bar, or `hex window show` -> Closes
  └── Reduce Motion -> One whole take, still, no wakes; turned off while
      open, the tape starts again when the window regains focus
```

The lanes are Rune shapes drawn at their laid-out width and redrawn from the
window's poll clock while the modal is open (`src/rune_ui/explainer.rs`).
Checks: `the_shortcut_mode_row_names_the_choice`,
`shortcut_mode_opens_from_its_row_and_keeps_focus_inside`,
`each_option_picks_its_trigger`,
`the_explainer_loops_on_the_poll_clock`,
`reduce_motion_shows_the_finished_explainer_without_waking`,
`the_explainer_restarts_when_reduce_motion_turns_off`,
`a_long_chord_widens_shortcut_mode` (five keys),
`shortcut_mode_draws_a_keycap_per_key_and_lines_up_the_lanes`, and
`the_explainer_picker_and_shortcut_recording_never_share_the_window` in
[tests/panes.rs](../../src/rune_ui/tests/panes.rs), the tape's own
`nothing_draws_past_the_pen`, `the_tape_scrolls_without_steps`,
`the_still_frame_shows_a_whole_take`, `narrow_lanes_draw_nothing` and
`the_past_leaves_the_tape` in `explainer.rs`, plus the `shortcut-mode`
(Option, Double-tap unavailable), `shortcut-mode-key` (Option-Space,
Double-tap chosen), and `shortcut-mode-chord` (Ctrl+Alt+Space) goldens
(frozen at 1.8 s into the tape). The animation itself has been seen only in
headless frames, not on a real window.

On Linux, `hex dictation start` and `stop` bypass every Shortcut mode: start records unless something already records, stop
finishes, and `toggle` does whichever applies. Bound to a key's press and
release they give hold-to-dictate; a shortcut press still finishes a capture a
command started.

The short-tap threshold belongs to ordinary hold activation. Voice-delimited
and explicit programmatic capture have different intentional-start rules.

### Lock, With Or Without Hold

```ts
Shortcut mode = Hold or double-tap or Double-tap     // dictate.lock
  Hold -> Ordinary hold path remains available
  Tap -> Tap
    ├── Second press AND release <300 ms after first release -> LOCKED
    └── Slow second release -> No lock

Shortcut mode = Double-tap                           // dictate.double-tap-only
  Requires a key binding AND locking enabled, or a shortcut without a key
  with Labs › Double-tap on a lone modifier on (macOS)
  Single press/hold -> No capture                     // disables hold, not a toggle
  Same two qualifying complete taps -> LOCKED         // same lock, not a third mode

LOCKED -> Speak hands-free -> Fresh shortcut press -> Finish
       -> Escape -> Nothing: it reaches the app (#786)
       -> Missed key release -> Neutral keyboard repairs tracking -> Still LOCKED
```

Checks in [suppression/tests.rs](../../src/suppression/tests.rs):
`callback_double_tap_locks_for_modifier_and_key_bindings`,
`callback_hold_and_double_tap_boundaries_are_milliseconds`,
`second_tap_finishes_as_press_and_hold_when_double_tap_cannot_lock`,
`double_tap_only_waits_for_two_complete_key_chord_taps`,
`double_tap_only_with_the_lab_starts_from_a_lone_modifier`,
`double_tap_only_with_the_lab_still_discards_a_lone_modifier_chord`,
`double_tap_only_with_the_lab_does_not_take_a_held_first_press_as_a_tap`,
`double_tap_only_with_the_lab_discards_a_different_modifier_between_taps`,
`double_tap_only_with_the_lab_locks_a_keyless_control_option_chord`,
`double_tap_only_with_the_lab_locks_a_both_options_chord`,
`double_tap_only_with_the_lab_discards_the_other_option_side_between_taps`,
`without_the_lab_double_tap_only_stays_off_for_a_lone_modifier`,
`locked_dictation_finishes_after_a_missing_key_release`.
These cover gesture decisions, not hardware keyboard-layout or remapping equivalence.

### Warm And Cold Capture

```ts
Keep ready -> Press shortcut                          // dictate.boundaries
  -> Include up to 450 ms of available pre-roll
     └── "While dictating" mutes, pauses, or lowers -> No pre-roll (it predates it)
  -> Linux: press and release placed on the stream clock (newest chunk's end
     plus the time since it arrived), so large chunks add no pre-press audio
  -> Delayed handling still uses original event boundaries
  -> Release excludes audio captured after physical release + 200 ms

Release or locked finishing press                     // dictate.release-tail
  -> Hold threshold judged on the physical release alone
  -> Wait for audio through release + 200 ms (POST_RELEASE_TAIL)
     ├── Arrives -> Clip ends exactly at release + 200 ms
     └── Stream stalls/fails -> Finish within tail + 100 ms with what arrived
  -> Not short taps, voice controls, programmatic stop, or meetings
  -> Linux and Windows wait the same tail; Linux ends at the release it observes
     (an X11 key release, a portal Deactivated, or `hex dictation stop`)
  -> A press inside the tail starts after the old clip finishes, with its own pre-roll

Release that will transcribe                          // dictate.stop-tone
  -> Stop tone and mute/pause/sleep restore at the release, while the tail is
     still captured; a muted output (macOS, Linux) unmutes first, then the tone
     ├── Short tap or chord -> No stop tone; the start tone is retracted
     ├── Clip later near-silent or without speech -> Tone already played;
     │   discard quietly, nothing pasted, no error tone or note; the log has
     │   the measured level and peak against the thresholds
     └── Escape after the release -> Nothing; the clip still transcribes
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
     ├── macOS: stream fails after the threshold -> Keep what arrived; copy,
     │   never paste                       // recover.interrupted-dictation
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
([dictation/tail_tests.rs](../../src/dictation/tail_tests.rs); `HEX_TAIL_MODEL`,
`HEX_TAIL_WAVS`) and `evaluate_dictation_corpus`
([dictation/corpus_tests.rs](../../src/dictation/corpus_tests.rs);
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
The owner in [dictation/audio.rs](../../src/dictation/audio.rs) has
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

**Growing tail, November 3, 2026 (issue 1101):** stopping while still saying
the last word cut it partway, and the model guessed another word. The 200 ms
tail is now the minimum: when sustained speech (100 ms of 20 ms frames 12 dB
over the noise floor, syllable gaps up to 40 ms) runs into its end, capture
continues until 160 ms of quiet or 600 ms after the release
(`DictationCapture::release_tail`, `MAX_RELEASE_TAIL`). A key click is too
short to count. The stop tone may be: speech then also has to be heard once
the current set's stop sound (`feedback::stop_tone_length`) plus 60 ms has
passed, so a release followed only by the tone waits until then and keeps
200 ms. The release event stays exact. The macOS audio owner and the Linux
and Windows loops (`ReleaseWait`) wait at most the base tail plus 100 ms, or
700 ms once speech grows it; a lost microphone finishes with what arrived.
Escape, short taps, voice controls, programmatic stops, and meetings still
get no tail. Checks in dictation.rs:
`a_word_said_through_the_release_keeps_the_tail_open_until_it_ends`,
`a_release_after_the_speech_keeps_the_base_tail_without_waiting_longer`,
`speech_that_carries_on_stops_the_tail_at_its_cap`, and
`the_stop_tone_or_a_key_click_never_grows_the_tail` (synthetic tones through
the capture timeline at 16 kHz). Not measured on a microphone or a real
speaker playing the tone.

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
  Release -> Collect the release tail (200 ms, up to 600 ms while speech goes on)
    -> Finish with the complete clip
    -> Feed any unfed remainder
    -> Unified English: feed 400 ms of silence, as the whole clip gets (every OS)
    -> Finalize -> Final text
       ├── Stream failed, produced no text, or was fed > 250 ms past the clip
       │   -> Transcribe the whole clip offline (source=fallback)
       ├── ≥ 300 ms of speech the stream heard ends > 400 ms before its text's start
       │   -> Transcribe the whole clip offline (source=fallback)   // lost opening
       ├── ≥ 300 ms of speech the stream heard lies > 400 ms past its text's end
       │   -> Transcribe the whole clip offline (source=fallback)   // #1101
       ├── The raw decode carries two different <ll-RR> language tags
       │   -> Transcribe the whole clip offline (source=fallback)   // language switch
       ├── No final text within 1.5 s -> Abort the stream -> Whole clip
       └── Model switched during the capture -> Stream closed -> Whole clip
  Short tap, chord discard -> Drop the stream
  Microphone stream failure -> Its audio so far becomes a copy-only dictation  // recovery.md#keep-an-interrupted-dictation
    └── A timeline gap over 250 ms still drops the capture  // recovery.md#reopen-with-fresh-audio
  Voice-delimited and programmatic captures (macOS) -> Whole clip after release
  Nothing is pasted or reaches the focused app until the final text exists
  Labs + labs.live_text -> After each feed the stream publishes its text   // dictate.live-text
  ├── Stable (committed) and tentative text, per capture; a new capture starts empty
  ├── One click-through line just past the HUD, on the HUD's screen
  │   ├── macOS: beside the plate the HUD placed (under the menu bar, over the Dock,
  │   │   in the menu bar, out of the notch), on the pointer's screen
  │   ├── Linux, Windows: past the plate `rune_hud` draws, on the HUD's display
  │   ├── HUD hidden -> In the HUD's place at the top
  │   ├── Appears with the first words, already drawn; prewarmed, so macOS never
  │   │   reveals the last dictation's words for a frame (Linux, Windows: unverified)
  │   ├── Grows with the words to 552 pt; the newest stay in view, "…" cutting the start
  │   └── Tentative words, when the model has any, are dimmed after the stable ones
  ├── Release, short tap, or a lost microphone -> The recording ends -> Hidden
  ├── Provisional: never pasted, copied, or kept (the final text replaces it)
  ├── A failed or interrupted stream clears the text while the capture goes on
  └── process.log: `partials` on "finished live stream"; per change at debug (no words)
  Labs + labs.live_typing (Linux Wayland) -> The same text is the focused field's preedit  // labs.live-typing
  ├── Begin at the intentional start, before the stream opens; destroyed at Finish or Cancel, so another input method keeps the seat
  ├── Non-streaming model -> No words while speaking, but the final text still commits through the input method
  ├── Finish -> The owner wins the commit claim, clears the preedit, commits the processed text, no clipboard
  ├── Caller gives up first (350 ms) -> Owner only clears (up to 250 ms), then Paste
  └── Owner already claimed the commit -> Wait up to 750 ms; never paste over it
  Picker -> "Streams" tag on catalog models whose artifact streams   // setup.model-card
  Picker -> The saved language's Recommended/Fastest/Most accurate cards first  // setup.model-routing
  ├── el: Cohere; tr, ms, fil, fa, mk, ar, ja, fi, sv, da: Whisper turbo first
  ├── auto, yue, ko: no SenseVoice; mt: Parakeet v3 only (Whisper fails Maltese)
  ├── en: Underdog Bark 3.0 Pro and Clean (Parakeet v3 fine-tunes; Clean
  │   drops filler words) follow without a badge
  ├── ar: QwenCleo-ASR 1.7B (Qwen3-ASR 1.7B fine-tune for Egyptian Arabic and
  │   Arabic/English code-switching; Arabic only, 2.2 GB; its 19.85% WER is
  │   the author's own test) follows without a badge
  ├── he: ivrit.ai Whisper large-v3-turbo Recommended (Hebrew only, always with
  │   the `he` hint; 18.45% WER on FLEURS Hebrew test against stock turbo's
  │   29.69%, Hex's run), then Whisper turbo for Vocabulary (#752)
  ├── ru: GigaAM v3 Recommended (ai-sage's e2e RNN-T, Russian only, 274 MB, no
  │   hint; 5.37% WER on FLEURS Russian test against Parakeet v3's 6.52% and
  │   Whisper turbo's 5.36%, Hex's run), then Whisper turbo for Vocabulary (#952)
  ├── no: NB-Whisper Medium Recommended (National Library of Norway, Norwegian
  │   only, always with the `no` hint, 539 MB; 6.68% WER on FLEURS Norwegian
  │   test against Whisper turbo's 8.94%, Hex's run), then Whisper turbo for
  │   Vocabulary (#1150)
  ├── en, fr, de, es, pt, it, ja: Granite Speech 5.0 (IBM's TurboCTC, 506 MB,
  │   no hint, also offered under Auto) follows without a badge: on FLEURS
  │   test it trailed Parakeet Unified/v3 in every European language and tied
  │   Whisper turbo in Japanese (5.04% CER against 5.02%), Hex's run (#1252)
  ├── Orukeet: Oruk AI's Parakeet v3 fine-tune (740 MB, no hint, also offered
  │   under Auto) follows without a badge; 1.86% WER on LibriSpeech test-clean
  │   from transcribe.cpp's published conversion benchmark, with its FLEURS
  │   figures in Details
  └── Every other available model follows without a badge, selectable and
      deletable; never auto-switched or auto-downloaded (setup.language-select)
```

Source: [transcription/engine.rs](../../src/transcription/engine.rs) (one thread
owns the model on every OS; streams, finishes, offline runs, and macOS model
loads and switches share its FIFO because a live stream holds the model's
compute lease), `probe_stream` and `with_stream` in
[transcription/gguf_session.rs](../../src/transcription/gguf_session.rs), the stream entry points in
[transcription/local_transcriber.rs](../../src/transcription/local_transcriber.rs) and
[parakeet.rs](../../src/parakeet.rs), and `begin_stream` in
[dictation.rs](../../src/dictation.rs): the capture owns its stream, feeds it as
audio arrives, cancels it with the recording, and hands it to the finished clip.
macOS streams hotkey captures from the audio owner in
[dictation/audio.rs](../../src/dictation/audio.rs); `DictationWorker::transcribe`
hands the clip to the engine when it is submitted, so its stream finishes before
a later capture opens one, and the job then waits for its text in order. Linux
([linux/dictation.rs](../../src/linux/dictation.rs)) and Windows
([windows/dictation.rs](../../src/windows/dictation.rs)) drive the same engine
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
`hotkey_captures_stream_once_intentional_and_others_do_not` in dictation/audio.rs;
`hold_streams_after_the_threshold_and_toggle_streams_from_the_press`,
`a_release_finishes_its_stream_with_the_complete_clip`,
`escape_and_short_taps_cancel_the_stream_and_submit_nothing`, and
`a_model_that_cannot_stream_transcribes_the_whole_clip_after_release` in
linux/dictation.rs; `source_labels_distinguish_fallbacks`,
`streams_send_their_audio_in_order_and_cancel_when_dropped`, and (macOS, a real
engine thread without a model) `a_finish_without_a_stream_transcribes_the_clip_offline`,
`an_unusable_stream_falls_back_to_the_complete_clip`,
`work_runs_in_order_and_failures_reach_the_caller`, and
`dropping_the_engine_stops_despite_an_open_stream` in transcription/engine.rs.
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

**Observed November 1, 2026, apollo (Radeon 8060S, Vulkan), Nemotron 3.5 ASR
Q8, FLEURS clips replayed at 1× into the production engine with live text:**
the stream qualified and every clip finished from it in 34–40 ms after the
release tail (163 ms for the first, cold), against 168–889 ms whole-clip for
10.6–54.6 s. German (17.9 s and 54.6 s, `de`) and Auto matched the whole clip
word for word; live text showed its first words 2.4–5.0 s into the audio,
updated once per 1.12 s chunk, and never rewrote stable text. A 43.7 s English
clip of five sentences joined by 0.5 s of digital silence lost 13 of 96 words
against the whole clip, which itself lost a sentence; Parakeet v3 lost more of
the same clip, so treat long joined FLEURS clips as a weak test, not a
streaming defect. A clip alternating English and German sentences with Auto
kept each sentence in its language (1 word different from the whole clip),
though both paths dropped one English sentence after a gap. Not a microphone, the HUD, macOS, or Windows.

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
it: `HEX_STREAM_LIVE_TEXT=0` gives the same text. The losses replayed here sit
in the middle of transcribed text, so neither coverage check below sees them;
a loss of at least 300 ms at the stream's very start is checked since
November 7, 2026 (`a_stream_that_lost_the_opening_phrase_falls_back_to_the_whole_clip`).

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
floor (or under −70 dBFS since #841) (`FrameLevel::quiet`); anything louder is
kept. Noisier rooms keep more pause audio rather than lose speech. Check:
`quiet_speech_over_steady_noise_is_never_cut_as_a_pause` (synthetic noise
bursts, a fake stream; no real-model or microphone run).

**Fixed November 1, 2026 (issue 841):** a quiet built-in microphone (Linux,
Wayland) carried whole dictations under −55 dBFS, so every frame counted as
quiet and the stream again kept only the first 2.5 s, which pasted as a
success. The absolute quiet bound is now −70 dBFS, and a stream whose cap
heard no frame above the noise floor yet cut audio as a pause is unusable:
the whole clip is transcribed offline (`source=fallback`). Checks:
`quiet_speech_over_steady_noise_is_never_cut_as_a_pause` (now also −62 dBFS
speech over −72 dBFS noise) and
`a_stream_that_heard_only_quiet_audio_falls_back_to_the_whole_clip`
(synthetic noise, a fake stream; no microphone run).

**Fixed November 1, 2026 (issue 841, follow-up):** on 3.1.2 the same
microphone (a Ryzen built-in digital microphone) still kept exactly 2.5 s in
the stream and relied on the fallback. Its level was measured with the DC
offset included, which the dictation silence gate already removes; a steady
offset louder than the speech pins every frame to the noise floor.
`SpeechFrames` now removes each 20 ms frame's mean before measuring, for the
pause cap and the meeting pause detector alike. Check:
`quiet_speech_over_steady_noise_is_never_cut_as_a_pause` (now also −50 dBFS
speech over −58 dBFS noise on a −26 dBFS DC offset; synthetic, a fake
stream). The DC offset is inferred from the reporter's logs (clips that pass
the −50 dBFS silence gate yet read as quiet), not measured on the device.

**Fixed November 3, 2026 (issue 1101):** a short last phrase after a pause of
about 1 s could be missing from the paste while the clip held it: the stream
finalized without it, and a stream with text was never retranscribed.
`PauseCap` now records where each speech frame (the meeting detector's 12 dB
margin) the stream heard ends, and after finalizing compares that with the
end of the stream's last token. At least 300 ms of speech more than 400 ms
past the text makes the stream unusable, so the whole clip is transcribed
offline (`source=fallback`, reason "the stream's text ends at … ms but … ms
of speech follows"). A key click or stop tone in the 200 ms tail is shorter
than that. Checks:
`a_stream_that_dropped_a_phrase_after_a_pause_falls_back_to_the_whole_clip`
and `a_short_sound_after_the_text_does_not_discard_the_stream` (synthetic
tones, a fake stream). **Observed November 3, 2026, apollo (Radeon 8060S,
Vulkan), Unified English Q8, an ad hoc probe through `PauseCap` and the real
stream:** a FLEURS clip cut to speech, 1 s of digital silence, and a 1.2 s
phrase from the same speaker lost the phrase in the stream (with or without
the macOS 400 ms trailing silence) while the whole clip kept it ("…channels
25 to 30 years"); the check flagged it. Seven ordinary clips (JFK, FLEURS,
product names, 1.7–15 s) finished from the stream; an 84 s speech ending in
applause fell back. The stream's own loss is not fixed upstream.

**Nemotron 3.5 ASR, observed November 3, 2026, apollo (Vulkan, Q8, Auto):**
ten clips through `PauseCap` and the real cache-aware stream (JFK, FLEURS
English and German, product names, and speech, a 1–1.2 s pause, and a short
phrase from the same or another speaker) matched the whole clip word for word
(one clip differed on a single product name), and none lost the late phrase,
so the drop did not reproduce with this model. Its tokens carry real
timings, but it puts the closing full stop on the clip's last frame, which
would have hidden a dropped phrase; the text's end is now the last token with
letters or digits (`spoken_text_end_ms`), and no clip fell back. Check:
`a_closing_full_stop_at_the_clips_end_does_not_cover_a_dropped_phrase`. Not
macOS or a microphone.

**Guard, November 7, 2026 (opening and language):** the coverage check now
also runs at the start. `PauseCap` takes the first token with letters or
digits (`spoken_text_start_ms`, the minimum `t0_ms`) and counts the speech
frames the stream heard that end more than 400 ms before it; at least 300 ms
of such speech means the stream lost its opening, so the whole clip is
transcribed offline (`source=fallback`, reason "the stream's text starts at …
ms but … ms of speech precedes it"). A short click before the speech is
shorter than that. Hesitation fillers, coughs, or clicks that together reach
300 ms before the first word would trip it too, and that false-positive rate
is unmeasured; a false positive costs only the whole clip's latency, since its
text is the fallback's. This check cannot see an opening that was garbled
rather than dropped: a garbled opening still yields word tokens at the speech
onset. The stream's unfiltered raw decode also keeps Nemotron 3.5's `<ll-RR>`
language tags, which the public text drops; two distinct tags in one stream
mean it changed language mid-clip, and the whole clip decides too
(`language_tags`). That catches a garbled opening only when the garbled part
carries a different tag, and neither check catches a stream that stays in one
wrong language for the whole clip. Checks:
`a_stream_that_lost_the_opening_phrase_falls_back_to_the_whole_clip`,
`a_short_click_before_the_text_does_not_discard_the_stream`,
`a_punctuation_token_does_not_start_the_streams_text`, and
`a_stream_that_switched_language_falls_back_to_the_whole_clip` (synthetic
tones and tags, a fake stream). Not measured on a real model, microphone, or
device; the stream's own loss is not fixed upstream.

### Ongoing Jobs And Output

```ts
Completed long recording                  // dictate.model-windows
├── Cohere -> Runs of at most 35 seconds, including the final remainder
└── Other GGUF models -> Existing backend-advertised input limit
    -> Transcribe each chunk -> Join text in original order

Chunking bounds inference, not capture duration. No silence is removed.
```

The shared `max_audio_chunk_samples` policy in
[transcription/models.rs](../../src/transcription/models.rs) is used by
[parakeet.rs](../../src/parakeet.rs) and
[transcription/local_transcriber.rs](../../src/transcription/local_transcriber.rs). Cohere's pinned backend
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

`pad_gguf_samples` in [transcription.rs](../../src/transcription.rs) owns backend
input padding on every OS for ordinary transcription: the minimum duration,
then Unified English's additional trailing silence. The macOS `Transcriber` and
the Linux and Windows `LocalTranscriber::transcribe_clip` both call it; a
non-macOS whole-clip run calls it after its pause cut.
`short_unified_input_is_minimum_padded_before_trailing_context` in
[transcription.rs](../../src/transcription.rs) checks minimum-duration padding
followed by Unified English's additional trailing silence. This is a pure
sample policy check, not native inference-quality proof.

**Fixed October 31, 2026 (issue 657):** with Unified English on macOS,
dictated sentences and questions pasted without their closing full stop or
question mark. Streamed dictations (the Metal path) finalized with no
trailing silence at all, and whole clips got only 200 ms. Both now get
400 ms of silence after the clip (`UNIFIED_ENGLISH_TRAILING_SILENCE_SAMPLES`;
the stream hears it through `PauseCap` just before finalizing). Check:
`a_dictation_stream_hears_trailing_silence_before_it_finalizes` in
transcription/engine.rs (a fake stream). Not measured on the real model or a
microphone; at the time, Linux and Windows streams were unchanged. Padding is
now included in `inference_ms`; `prepare_ms` covers clip conversion and
diagnostic retention.

**Fixed November 10, 2026 (issue 1629):** Windows and Linux had been gated out
of that fix. They now get the same 400 ms for Unified English on stream finish
(`LocalTranscriber::stream_trailing_silence` through `PauseCap`) and on the
whole clip. A whole-clip run cuts long pauses first, then pads, so a trailing
pause cannot trim the silence. Check:
`a_whole_clip_is_padded_after_cutting_its_trailing_pause` in
transcription/engine.rs. Not measured on a real Windows or Linux model or
microphone.

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

Source: `session_options` in [transcription/gguf_session.rs](../../src/transcription/gguf_session.rs) and
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
  │       // Labs › Return to app (macOS): when the app in front now isn't the
  │       // one the capture started in, Hex activates that one and waits up to
  │       // 500 ms for it; if it doesn't arrive, nothing is pasted and Paste
  │       // last dictation keeps the text
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
are deliberately left ungated: they skip both silence checks, and only output
that repeats the vocabulary prompt (Whisper, Qwen3-ASR) is dropped there.
The threshold is conservative, so speech is not dropped; a noisy room, the start
tone, or a key click can lift a capture above it. Those captures then go to the
Silero speech detector ([transcription/speech_detector.rs](../../src/transcription/speech_detector.rs)),
which discards them when it finds no speech. The detector (1.2 MB, pinned
revision and checksum in [transcription/models.rs](../../src/transcription/models.rs))
downloads in the background whenever a transcription model is downloaded or
activated, and at the first dictation, so an installed model still activates at
once offline; a failed download is retried at the next dictation. Model
preparation (macOS, Windows) and the Linux listener load it when present and
never download it. It runs on the CPU with a speech threshold of 0.4 instead of
Silero's 0.5, which misses a whisper at -43 dBFS. While it is missing or fails to
load, captures above the threshold still reach the model, as before.
Output that only repeats the vocabulary prompt (Whisper, Qwen3-ASR) is dropped as well
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
trailing Return, and Paste Last behave as for a paste. Chromium 157 and later, older versions, Electron apps (`Versions/A`),
and every other app keep clipboard paste.
Limits: the framework version is Chromium's for Chrome, Helium, Brave, Edge,
and Chromium; a fork that names that folder after its own version (Opera,
possibly Vivaldi) is classified by that number. Installed web-app shims carry
no framework and keep paste. An active input method may treat typed events
differently from a paste. Remove the typing path once Chrome 157 is the floor.

**Windows paste (issue 477):** Windows puts the transcript on the clipboard,
waits for Shift, Alt, and Win to come up, sends Ctrl+V, and restores the
previous clipboard 500 ms later unless a newer paste or another app changed it.
Labs › Terminal paste (`paste_with_shift`, off by default; on Linux it is a
General › Advanced row) sends Ctrl+Shift+V instead, for apps that ignore
a synthetic Ctrl+V (the herdr desktop app); a Labs › Terminal paste shortcut
capture overrides it per paste, and Paste last dictation does not. Shift goes
down after Ctrl and up
before it; a Ctrl the user still holds is never pressed or released by Hex. A
saved change applies to the next paste without restarting the listener.
Java games (`javaw`/`java`) still take plain Ctrl+V, override or not.
Source: `control_chord` in [windows/paste.rs](../../src/windows/paste.rs).
Check: `paste_chords_press_shift_inside_control_and_leave_a_held_control_alone`
(the synthesized key sequence only; no paste into a real Windows app).

### Toggle Cancel

```ts
Settings › General, Shortcut mode = Tap (Toggle)     // dictate.toggle-cancel
  -> Cancel row: the shortcut (`cancel_hotkey`), Escape by default
     ├── Recorder -> any chord the OS's rules allow (Escape ends the recorder)
     ├── Restore -> back to Escape
     └── Wayland -> no recorder: bind `hex dictation cancel` in the desktop
  Hold modes -> no row, no cancel
Toggled recording (held or locked) -> Cancel shortcut
  -> Recording discarded: nothing transcribed, pasted, or kept in History
  -> Cancel tone; the HUD shows it cancelled
  -> The key is kept from the app (press, repeats, release)
  -> Jobs already transcribing -> finish and paste in order
Idle, or any Hold mode -> Cancel shortcut -> Nothing: it reaches the app
hex dictation cancel -> Discards whatever records, any mode; idle -> unchanged
Annotate drawing -> Escape stops drawing first
Programmatic dictation (local API) -> Its owner cancels through /cancel
```

Until #786, Escape cancelled the active capture in every mode, then the newest
unfinished job, and an accidental Escape wiped a 5-minute dictation. #795
removed cancel; the people who asked for it back use Toggle, so it returned
there only, and never reaches past the recording. Hex takes the key only while
a toggled recording runs: macOS's session tap suppresses it only while the
listener arms it, Windows' hook only while its capture records, and X11
grabs it only for the recording. A shortcuts portal would hold the key from
every app for as long as Hex runs, so Wayland binds nothing for it. Checks:
`the_cancel_shortcut_discards_only_a_toggled_recording` and
`the_cancel_key_is_suppressed_only_while_armed` in
[suppression.rs](../../src/suppression.rs) (macOS, not compiled on Linux),
`cancel_discards_a_toggled_recording`, `cancel_is_ignored_in_hold_mode`, and
`cancel_is_not_taken_while_idle` in [linux/input.rs](../../src/linux/input.rs),
`cancel_is_wanted_only_while_a_toggled_capture_records` in
[windows/input.rs](../../src/windows/input.rs),
`the_cancel_shortcut_shows_in_toggle_and_restores_escape` and the
`toggle-cancel` goldens in [tests/panes.rs](../../src/rune_ui/tests/panes.rs),
and `cancel_parses_and_takes_no_flags` in [control.rs](../../src/control.rs).
The X11 grab and the hooks' suppression have no automated end-to-end check.

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
  Dictation pasted >= 2 s after the stop tone -> Done tone      // dictate.done-tone
     -> Pasted sooner (the usual case) -> Stop tone only; `DONE_MIN_AFTER_STOP`
     -> Stop tone still waiting on Lower audio -> Stop only, never done first
     -> Empty, failed, cancelled, Paste Last, programmatic -> No done tone
  Failure -> Error tone; wake/sleep keep macOS Pop/Tink      // dictate.error-tone
     -> Settings listener card: "Last dictation failed: …"   // macOS, Windows
        -> clears once a later dictation completes
        -> a known microphone cause explains itself; other failures keep their wording
     -> Labs › Failure notices: why beside the HUD for ~6 s  // dictate.failure-notice, off by default
        -> a new capture dismisses it; no old failure is replayed at launch
     -> Classic has none: macOS Basso, Signature's error on Windows and Linux
     -> A lost microphone or input -> A kept clip: no tone; a shortcut or
        `hex dictation start` capture's audio is transcribed and copied
        instead of pasted    // recovery.md#keep-an-interrupted-dictation
        -> No usable audio after the start, or a near-silent clip -> Error tone
  Tone requested -> Playback worker opens the default output, then plays
     -> macOS releases ~10 s after the last tone; Windows 5 min; Linux 1 s
     -> An open macOS output is a live CoreAudio client coreaudiod services
        and holds an idle-sleep assertion for, so it must not idle for long
     -> A fatal stream error (the output became unavailable or invalid)
        drops the output, and the next tone reopens
     -> Released output holds no macOS idle-sleep assertion
  Feedback volume = 0 -> No tones
     -> Playback worker releases output on its next 250 ms observation
     -> Volume enabled again -> Reopen for the next tone, never capture/UI
  Cold audio stack exceeds preload admission -> Warn once; recognition continues
     -> Loader keeps running; later tones play once the output opens
```

A failed dictation also stays visible in Settings' listener card as "Last
dictation failed: …" until a later dictation completes. macOS now reads the
listener's own event log on its runtime thread, as Windows already did. The
listener's own stopped warning and a dictation failure are separate fields, so
`hex status` keeps treating only the former as a stopped listener; the card
shows the stopped warning first. A failure whose timestamp predates this run's
start is ignored. Known
microphone causes say what to do in every interface language — a built-in
microphone switched off by a closed lid, or no microphone available; other
failures keep their logged wording. Sources and checks:
`DesktopActivity::visible_failure_since`, `last_completed_at`, and
`failure_explanation` in [desktop/activity.rs](../../src/desktop/activity.rs),
`newest_failure_shows_until_a_later_transcript_completes`,
`a_startup_reader_ignores_the_previous_sessions_failure`, and
`known_microphone_failures_get_an_actionable_explanation`, and the listener
card in [rune_ui/settings.rs](../../src/rune_ui/settings.rs). Proof limit: the
projection and wording are unit-tested; the macOS event-log wiring and listener
card are not natively verified.

With Labs › Failure notices on (`labs.failure_notices`, off by default), the
same explanation also appears for about six seconds as one click-through line
beside the HUD, reusing the Live text plate and placement. A new capture hides
it at once, and the message travels with the HUD's failure event, so a launch
never replays an old failure. The HUD's coral flash, its 600 ms, tones, and
capture behavior are unchanged. A job failure that arrives while a newer
capture records is skipped. Checks:
`a_failure_notice_lasts_six_seconds`,
`a_new_capture_dismisses_a_failure_notice_at_once`,
`only_a_notices_own_deadline_dismisses_it`,
`a_failure_arriving_after_a_queued_words_end_stays_visible`, and
`a_failure_notice_takes_the_plate_from_the_live_words` in
[rune_ui/live_text.rs](../../src/rune_ui/live_text.rs) cover the timing and
plate; the overlay itself is not natively verified on macOS or Windows. See
[Labs](labs.md) for its per-OS scope.

**Fixed November 1, 2026 (issue 873):** on macOS the HUD remembered its last
placement and skipped sending it to Rune again when it matched, so a HUD Rune
had put in the wrong spot on a second display (offset by the display's origin,
with a different backing scale) stayed there on every later dictation. Each
capture's start and each hide now forget the placement, so the next show sends
the display and rectangle again, as Linux and Windows already do. Not run on a
two-display Mac; the misplacement inside Rune itself is not addressed here.

**Fixed November 10, 2026 (issue 1663):** on Linux Wayland the HUD stopped
appearing after its monitor was removed and added back (a KVM switch, for
example) until Hex restarted. The compositor closes the layer surface with the
output, and Rune hides and drops that overlay on its own while Hex still thinks
the HUD is shown, so no later dictation asked for it again. Each capture's start
now asks for the HUD again, so it reappears on the next dictation, on an output
that exists. Not verified on real Wayland hardware with a monitor
unplugged and re-plugged.

```ts
Settings > Sounds (next to Sound volume)                    // dictate.sound-set
├── Default / older settings -> Classic
├── Row shows the saved set -> Opens the Sounds sheet
│   ├── Stage -> Hex's own HUD, where this OS puts it, does what it does
│   │   │          at each sound of a short dictation
│   │   ├── Where -> Indicator position: top / bottom; Off shows none
│   │   ├── Start (0 s) -> Recording, with a demo voice's level
│   │   ├── Stop (1.4 s) -> Transcribing
│   │   └── Done (2.1 s) -> Done look, then it leaves
│   ├── Lone Error (macOS) -> Failed look 0.6 s (Dot scope coral), then leaves
│   ├── Chips -> Start / Stop / Done / Error (macOS) play one sound
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
replays Hex's HUD on the sheet's clock: `dictation/indicator`'s model, drawn
by the window's compositor with `rune_hud`'s painter and `rune_hud.wgsl`, as
the real overlay draws it. It is a preview, not the overlay: Dot scope's
in-bar and notch placements show below the menu bar, the voice is HUD Lab's
demo, not the microphone, and it ignores Reduce Motion.

Hex signature (2026-10-27) is one kalimba-like FM voice playing one motif,
D-B-A-D (a sixth, since "hex" is six): start D5 to B5 (116 ms), stop B5 to A5
(230 ms), done D6 over A5 (330 ms), error D4
then a minor D4+Bb4 (280 ms), and onboarding success, the whole motif landing
on a D sixth chord (1.1 s), played when the first run reaches Hex is ready. Click is mechanical key clicks, Soft tone warm sine blips in A major,
Wood a soft-mallet marimba with hollow wood blocks for error, and
Warm synth two detuned saws through a closing low-pass filter, and Thock one
mechanical keyboard key, from the team's vote on the start sound (its winner,
an ElevenLabs take kept in `scripts/sounds/thock/`): start is the press, stop
the same key's upstroke, error two keys at once,
twice, and success a run of keys up to a full press; it has no done sound
because the stop already says it. Classic is the
sounds Hex played before the signature set (start clicks, stop swish),
restored from git; it has no done tone, errors play macOS Basso as they
did, and onboarding success plays its stop sound (the other sets have their own).
The done sound of every set plays only when the paste lands at least
`DONE_MIN_AFTER_STOP` (2 s) after the stop sound, as after a slow rewrite; the
Sounds sheet's demo still plays it. Checked by
`done_plays_only_after_a_slow_paste` and `done_never_plays_before_a_delayed_stop_tone`.

Each set is a folder in `resources/audio/sounds/` made by
[`scripts/sounds/synth.py`](../../scripts/sounds/synth.py)
(`uv run scripts/sounds/synth.py` rewrites them; signature regenerates
byte-identical). Synthesized sounds are 48 kHz mono WAVs starting on their
first sample; Classic keeps its 44.1 kHz stereo, trimmed to start at its first
audible sample. Every sound is normalized to one loudness per kind (start
-25.5 LUFS max momentary, stop and done -27, error -26.5, success
-25), so a volume sounds the same in every set. Classic's start and stop come
up 2.5 and 3 dB (a soft limiter touches 0.1% of the start's samples). There
is a cancel sound for Toggle's cancel shortcut. Every OS plays the
error sound for a failed dictation or paste (`fail` in `linux/dictation.rs`
and `windows/dictation.rs`), including an empty transcript, which macOS
treats as nothing pasted. Checks:
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
output for the first tone, keeps it through an idle grace so bursts of
dictation share a warm device, and releases it after that grace or on the next
observation after sounds turn Off. On macOS the grace is ten seconds: an open
output is a live CoreAudio IO client that coreaudiod services and holds an
idle-sleep assertion for. coreaudiod logged client timeouts against Hex's open
output (#1531), and a shorter grace reduces how long such a client exists.
Ten seconds still covers a quick start-tone to stop-tone dictation, though a
dictation held past about ten seconds pays a cold output open for its stop
tone; the tone is delayed, not dropped. Cold-open time on Bluetooth outputs is
unmeasured. Windows keeps five minutes; Linux releases after one second because
an open output is a listed stream in its sound controls (#720). A per-sink flag
records a fatal stream error (the output became unavailable or invalid);
overload glitches (`Xrun`) never set it. A default-output change does not set it
on macOS, where cpal reports `DeviceChanged` only after the default-output unit
has already rerouted the stream. On Windows cpal's WASAPI backend never
rebinds, so a default-device change ends the stream with `StreamInvalidated`
and counts as fatal, dropping that output so the next tone opens the new
default. The stream error callback may run on a CoreAudio thread, so it only
flips the flag; the worker drops a flagged sink on its next 250 ms observation
or next tone, logs one warning, and lets the next tone open a fresh one. macOS idle-sleep timers count from the last user
input, and a tone implies recent input, so the grace never delays sleep by
itself. A device open
already in progress must return before the worker can observe that change.
Output-open failures retry at most once per two seconds, and a released device
reopens without that backoff. Bundled samples remain decoded in memory. A tone
that arrives after the grace pays a cold device open first; the tone is
delayed, not clipped or dropped. Overlapping tones extend the playing window
rather than shortening it.

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
`output_opens_for_a_tone_and_releases_after_the_idle_grace`,
`overlapping_tones_extend_playback_instead_of_truncating_it`, and
`an_errored_sink_is_dropped_and_reopened_on_the_next_tone` check ownership,
grace, drop, and stream-error recovery with a controlled sink;
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

Each time the HUD shows, Rune clears and resets those collection behaviors
before ordering the panel in, and orders it out first if it is still shown on
another Space (#1084). Changing the activation policy (the Dock icon follows
Settings) could leave the panel tied to one Space while AppKit still reported
the old mask, and re-setting an unchanged mask does nothing. Rune repairs that
only as its panel shows, so while the HUD is shown Hex also checks each pump
turn that the panel is on the active Space, and re-homes it when a Space switch
or a full-screen exit left it behind: it orders the panel out, clears and
restores the mask, and orders it back in. That reorders the panel but leaves
Rune's presence state and alpha alone. It is attempted once per show, and once
more for each later Space change after the panel has been seen on the active
Space, so a repair that did not take cannot loop. The live-text plate beside
the HUD is a separate panel and does not get this repair yet. Not yet observed
on hardware; only the macOS compile is checked.

At the top position the HUD sits 6 pt below the menu bar (`rune_hud::hud_placement::TOP_OFFSET`), or below the
notch (`NSScreen.safeAreaInsets`) where a fullscreen Space hides the menu bar
and `visibleFrame` reaches the top of a notched screen. Screens without a notch
and normal Spaces keep their position. Check: `scripts/check
dictation::indicator::tests::top_capsule` covers the math only; placement over a
fullscreen app on a notched Mac is not yet observed.

### Dot scope

The HUD is Dot scope on every OS ([rune_hud/dot_scope.rs](../../src/rune_hud/dot_scope.rs),
`dot_scope_fragment` in [rune_hud.wgsl](../../src/rune_hud.wgsl), drawn by
a Rune overlay). It was the `dot-scope` experiment (#264) until 2026-10-29,
when it replaced the glossy capsule, which Omarchy users on X called a
"static red thingy"; the capsule's shader and HUD Lab styles are deleted. It is the t422 prototype's look A with its values.
Linux and Windows draw it in a 304×80 pt window at the HUD's edge (no
menu-bar or notch placement), on the display under the pointer when the
capture starts, as macOS does (#486; `rune_hud::display`). Windows and X11
read the pointer and pick its monitor; Windows keeps it inside that
monitor's work area and draws it at that monitor's scale. A layer-shell
surface pins no output, so niri, Sway and Hyprland put it on the focused
output. GNOME and KDE on Wayland keep the primary display; on X11 they
follow the pointer like any other X11 desktop (#1200; check
`only_gnome_and_kde_on_wayland_keep_the_primary_display`, not run on a
two-monitor GNOME X11 session). On Windows and X11, while
the HUD shows it checks the pointer's display every 250 ms (`Hud::follow_pointer`,
Rune's `refollow_pointer`), and the HUD and live text move there, without
animating or taking focus, once two checks in a row find the pointer on the
same other display (250–500 ms), so a pointer on a display's edge doesn't
flip it between them (#807). A position change
or the next capture places it again (tested by reading the code only, not on
multi-monitor hardware). On Windows the window is a topmost,
click-through tool window with no redirection bitmap, drawn through
DirectComposition. Rune shows and hides it with Win32 calls alone and
re-asserts its styles on each show; winit never sees its visibility. The
click-through style makes it a layered window, which Windows composes only
once it has layered attributes; Rune gives it opaque ones (5778a3a3). Only
a DX12 DirectComposition swapchain composites alpha on Windows: a Vulkan
surface is a swapchain on the HWND, which this window shows black (#499) or
not at all (#510, a hybrid laptop where the integrated GPU's Vulkan driver
claimed premultiplied alpha). Rune draws transparent Windows surfaces only
through DX12, on the lowest-power adapter that drives a display, and fails
the overlay when none can (93c7a04b; from reading the code and Windows' rules, not yet observed on
Windows). If the window, its GPU surface or the show fails (including a
cloaked window), Rune reports why (`OverlayHandle::failure`). Hex logs the
HUD's surface (backend, adapter, alpha mode) once, and treats a dictation
start whose HUD presents no frame within 1.5 s as a failure too. Each
failure is logged as an error in process.log once per reason, and Settings
shows it as the listener's warning (`HudSender::failure`). Each start that
isn't Hidden opens a fresh first-frame attempt: its own present baseline and
clock, its own one recovery hide, and no inherited no-frame decision, so a
previous attempt's reason can stay in Settings without hiding the new one,
and a second start while the first still runs restarts the attempt. The
attempt hides the overlay at once (the immediate hide, not the exit fade,
which itself needs a frame) when it presents no frame after 1.5 s; Rune's
overlay failure is reported as the reason but never hides on its own, since a
failed overlay presents nothing and the frame-less check covers it. That
clears the HUD's shown state and lets Rune give its window and GPU surface
back (Windows' zero idle parks both), so the next dictation start makes both
again, and a lost GPU device such as a laptop resumed from standby costs one
HUD rather than the rest of the session. Each attempt hides at most once, and
a later frame-less start hides again. Checked by `rune_hud::tests`'
first-frame tests (`a_frame_less_start_hides_once`,
`a_new_start_does_not_hide_on_the_previous_reason`,
`a_second_start_while_pending_restarts_the_attempt`,
`an_overlay_failure_never_hides_on_its_own`, `an_attempt_hides_only_once`,
and `the_next_start_hides_again_after_a_hide`), not on Windows hardware.
Every OS takes dark or light and Reduce Motion from Rune, and loads
`dot-scope.json` at launch.
Windows and Linux drop the HUD's painter, and its buffers, bind group and
pipeline, once Rune's own hold on the device would end: five minutes after
the HUD hides on Windows, ten on Linux (Rune's surface idle plus the five
minutes it then parks the device), and also when the indicator is set to
Hidden while the HUD shows; macOS keeps it. The GPU device itself frees only
once Rune's parked device also expires, and on Wayland layer-shell Rune keeps
its own device regardless. The memory drop is not yet measured on a real
Windows machine (#1431).

Each chunk's meter removes the chunk's mean before measuring RMS and peak, so
a microphone's DC offset (a Ryzen built-in digital microphone, #841) never
reads as loudness; the dictation silence gate and the meeting pause detector
remove it the same way.

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
picture. Placement ([rune_hud/hud_placement.rs](../../src/rune_hud/hud_placement.rs), pure
geometry the capture capsule can reuse) puts it on the screen with the
pointer:

```ts
Placement                                     // rune_hud::hud_placement::place
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
(`rune_hud::dot_scope::lab`).

Hex meters no spectrum, so the prototype's band emphasis is the same for
every component.

Check: `scripts/check rune_hud::dot_scope` (defaults, knobs, frame-rate independence,
Reduce Motion, the stages, auto level, each processing
look settling dark), `scripts/check dictation::indicator` (phases, the
processing minimum, failure and cancel, a hot microphone still moving),
`scripts/check rune_hud::hud_placement` (each placement on flat and notched screens),
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
  lock, Escape passing through, and a second capture during older processing in a controlled
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
