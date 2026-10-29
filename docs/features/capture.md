# Annotate (macOS and Linux)

Annotate is Scrawl's capture engine hosted inside Hex (code and settings keep
the name `capture`), steps 3 to 5 of
[the plan](../plans/capture-in-hex.md). It runs the real
`scrawl-capture` crate, imported into `crates/capture`: it records the active
display with ScreenCaptureKit, or on Linux the screen-cast portal and
PipeWire (`capture.linux`), (or a cabled iPhone through `scrawl-helper`'s
mirror) and narration from Hex's microphone, takes ink and clicks (Shift on
macOS, draw mode on Linux), transcribes the narration with word timestamps on
Hex's loaded model, writes a Scrawl bundle into Scrawl's captures folder,
copies it to the clipboard, and sends it to an OpenCode session with Scrawl's
router (`crates/router`), run in Hex's process. Settings › Annotate (under
Labs in the sidebar) holds its settings. Scrawl's capsule (switch
chips, the on-screen picker, route chips) and its ink and laser overlay are
Rune overlays.

```ts
Turn it on                               // capture.gate
├── On by default in every macOS and Linux build (capture.enabled); the
│   `annotate-on-by-default` migration turns it on once for older settings
│   └── macOS bundles sign scrawl-helper beside `hex`; validate-app.sh requires it
├── Windows -> not compiled; no pane, no `hex annotate`
├── Nothing is asked at launch: Screen Recording (macOS) at the first capture,
│   the portal's dialog (Linux) at the first Screen capture
└── Off -> no capture host, engine, sink, thread, or folder; `hex annotate` -> feature-off

Draw                                     // capture.draw
├── macOS -> hold Shift and move: ink; Shift-click: a numbered pin; ⌘Z: the
│   last stroke back; the laser follows the pointer while it records
├── Linux -> hold Ctrl+Alt+D while it records, as Shift on the Mac (the
│   portal's Activated/Deactivated, or the X11 grab's press and release;
│   linux_input DrawStart/DrawEnd -> capture_host::set_drawing): the ink
│   overlay takes the pointer (crosshair); moving draws ink with no click, a
│   click drops a pin, right-click (or Ctrl+Z, when the compositor gives the
│   overlay the keyboard) takes the last stroke back; letting go gives clicks
│   back to the apps. While held, the capsule takes no clicks.
│   Held with nothing recording -> nothing. `hex annotate draw` toggles it
│   instead, for desktops without the portal (it starts a recording drawing)
│   Shift alone can't be a desktop shortcut; Ctrl+Alt+D sits beside
│   Ctrl+Alt+S and no other default uses it. Check: none physical (needs a
│   real hold on a portal desktop); the hold is logged as "Annotate drawing"
├── Ink holds 650 ms, then fades over 650 ms on screen; every stroke stays in
│   the bundle (keyframes, contact sheet, scrawl.json)
└── Linux recordings show the overlay itself (Wayland can't leave it out)

On Linux                                 // capture.linux
├── Source -> the ScreenCast portal (zbus): Screen asks the desktop once and
│   keeps its restore token (<support>/screencast-token), so later captures
│   start without a dialog; Window shows the desktop's picker every time (the
│   capsule's Window chip goes straight to it); the cursor is embedded
├── Frames -> the portal's PipeWire stream, libpipewire-0.3 loaded at run
│   time; 32-bit RGB as a linear DMA-BUF (niri, GNOME, KDE) or shared memory
├── Encoding -> VA-API (libva loaded at run time): each DMA-BUF imported, scaled
│   to a 2560 px long edge (1920 for a window) and converted on the video
│   engine, H.264 High at a constant QP with Hex-written headers, 30 fps;
│   `ffmpeg -c copy` muxes recording.mp4. No VA-API -> frames copied and
│   encoded with x264 through ffmpeg (slow on DMA-BUF-only compositors)
├── Candidates -> a small copy from the video engine (at most 4× smaller per
│   pass: radeonsi crashes past it)
├── Reading movies back (keyframes, probe, audio) -> `ffmpeg`/`ffprobe`
├── Missing PipeWire, ffmpeg, or a picker answer -> that capture fails with why
├── Narration -> the listener's microphone through ListenerTimeline (each
│   stream anchored to the monotonic clock); released-while-idle microphones
│   open for the capture
├── Words -> one foreground job on the listener's engine (its loaded model)
├── Copy -> the summary text through Hex's Linux clipboard (wl-copy or the
│   X11 clipboard), as the Mac copies it
├── Paste -> as the Mac: the contact sheet (image/png), then the summary,
│   each with Hex's own Ctrl+V (linux_paste). An app that takes no image (a
│   terminal) gets only the summary, which names the contact sheet; the Mac
│   pastes a terminal the sheet's path instead
├── Finished capsule -> "Copied" with Paste (accent), Inspect, and Send to…;
│   unlike the Mac, a guess doesn't open its suggestions by itself
│   (Flow::suggesting_on_route(false)): Send to… is a click away
├── Shortcuts -> Ctrl+Alt+S (start, stop) and Ctrl+Alt+D (hold to draw)
│   through the GlobalShortcuts portal ("Annotate", "Annotate: hold to draw")
│   or X11 grabs. There is no send-to list shortcut on any OS
├── While it records, dictation and Voice Action start nothing (the portal
│   or grab already took their keys, so nothing reaches the app)
├── Pane -> no Screen Recording row, no transcriber choice, no "Show in screen
│   recordings" (the portal asks; Wayland can't keep overlays out)
├── Sending -> OpenCode's sessions from its service registration
│   (service.json, HTTP) and prompts through the `opencode2` CLI Hex's
│   dictation processor finds; the capsule's rows are clicked (no keyboard).
│   See capture.send: everything goes inline, so a remote server works
└── Gaps: no Area (Wayland gives Hex no pointer to aim it), no iPhone, no
    laser or click log outside draw mode, no window log, `hex capture status`
    unsupported, choosing another folder only through settings.json

Settings › Annotate                      // capture.pane
├── Capture with Hex (capture.enabled, default off) -> off: no host, shortcut,
│   input, retention, or folder, as with the experiment off
│   └── On while the shortcut collides -> refused, the shortcut row says why
├── Shortcut (capture.hotkey, default ⌥⇧S) -> same keycap and recorder as dictation;
│   a key is required; refused when it overlaps dictation, Voice Action, or paste
├── How it works (on) -> start, draw, pin, undo, stop, and send, each with
│   its keys: ⇧, ⇧-click, ⌘Z on macOS; held Ctrl+Alt+D, a click while held,
│   right-click or Ctrl+Z on Linux. Check: rune_ui::scrawl tests
├── Screen Recording -> read (preflight, never a prompt) once a second and again
│   when the pane opens or the window becomes key; granted -> Allowed, no button
│   └── Allow -> rechecks, then the onboarding permission guide over System
│       Settings' list (PermissionKind::ScreenRecording), as for Input
│       Monitoring; flips to Allowed on the next read. Never part of onboarding
│       (macOS also asks at the first capture). Check: rune_ui::scrawl tests
├── Show in screen recordings (capture.show_overlays_in_recordings, default
│   off; also the menu bar's Show Hex in Screen Recordings checkmark) -> see
│   capture.own-windows; applies at once, to open windows and a recording capture
├── Transcriber (capture.transcriber) -> Parakeet (default) | Apple
├── Captures folder (capture.folder, default Scrawl's) -> Reveal | choose another
├── Keep (capture.media_days / bundle_days) -> Scrawl's presets, or imported days
├── Scrawl.app running -> "Scrawl is running" card + Use Hex for captures
│   └── Click -> asks Scrawl to quit (nothing else quits it); Hex takes the
│       shortcut within 2 s of Scrawl leaving
└── No iPhone pairing row: ScrawlKit pairing needs the ingest server, which Hex
    doesn't run. A cabled iPhone needs no row (capture.phone)

First launch with the experiment on      // capture.import
├── ~/.config/scrawl/config.json -> transcriber (apple; else Parakeet), capturesDir,
│   retention, read once (migration `scrawl-capture-settings`); token not used
├── Scrawl's router settings -> capture.routing, read once (migration
│   `scrawl-routing-settings`): rules, autoSend, smartRouting and jevRouting;
│   never the Jev key (Jev uses $OPENCODE_API_KEY or OpenCode's credential)
├── Scrawl's Application Support config.json -> its shortcut, unless it collides
└── Scrawl's files are never written

⌥⇧S (capture.shortcut; captures on, Scrawl not running)
├── Press, held as long as you like -> armed; nothing records while ⌥⇧ is down
│   └── One of its modifiers comes up -> start on the display under the pointer,
│       as `hex capture start`                // a fresh press only, no repeats
├── Esc before the modifiers come up -> nothing records
├── Press while recording -> stop, as `hex capture stop` // ignored within 400 ms
├── Press while the capsule's picker is open -> the picker records what it is on
├── Esc while a capture records -> nothing: it reaches the app being recorded,
│   and Hex's tap lets it through (only a dictation hold under it is cancelled)
├── The binding is swallowed, never chord evidence for dictation
└── Hold the dictation shortcut mid-capture -> dictation pastes as usual

While a capture records                  // capture.quiet
├── Start -> a recording-environment session with General's While dictating
│   (read at each start; captures have no setting of their own): mutes output
│   or pauses Music, Spotify, VLC; Do nothing changes nothing. No idle-sleep
│   assertion, and it never counts as an intentional dictation
├── Stop, cancel, discard, failed start, captures off, quit -> released; a
│   switch carries it to the next capture without restoring in between
├── Dictation and Voice Action shortcuts -> start nothing while it records
│   (macOS: recognition.rs swallows them in the tap; Linux: linux_dictation
│   ignores Start and VoiceStart). Nothing leaks to the app in front
└── Dictation that began before the capture -> each effect is held from the first session that
    wants it to the last, so neither unmutes or resumes what the other still
    holds; a While dictating change mid-capture composes too (the capture keeps
    media paused, the dictation mutes). Only what Hex
    changed is restored. The dictation stop tone plays at once while the capture
    keeps output muted. Check: recording_environment tests (overlap both ways),
    capture_host a_capture_stays_quiet_until_it_stops_is_cancelled_or_hex_quits

Switching target                         // capture.switch; the capsule's chips
├── Started by the shortcut -> the snapshot's recording says `target: Pointer`,
│   the only kind that shows the switch chips (Area · Window · Screen)
├── First click, scroll, or ink stroke -> `worked`, so the chips fold
├── Screen -> the capsule folds the chips; the capture goes on
├── CaptureCommand::Switch(target) -> discard and start in one step: no snapshot
│   shows nothing recording in between (Scrawl's switch_recording)
│   ├── No room to start (two writing, Hex stopping) -> refused `busy` before
│   │   the discard; the recording goes on
│   └── The start fails after the discard -> the discarded capture settles
│       `Failed` with the start's error
├── Window or Area -> CaptureCommand::Discard: bundle removed, no outcome, gone
│   from the snapshot at once (and from MAX_FINISHING), so the capsule becomes
│   the picker
├── Picker open (take_keys: Picker) -> Hex's tap hands it ⌥⇧S and Esc; it never
│   takes focus
├── A pick -> CaptureCommand::Start(Window | Area | Display); nothing while one
│   records
├── The picked window closed, or Screen Recording refused -> last outcome
│   `failed` with its reason (`error`)
└── iPhone -> CaptureCommand::Switch(Phone) or Start(Phone), while the snapshot
    names a cabled phone (capture.phone)

A cabled iPhone                          // capture.phone
├── Each capture start, and CaptureCommand::FindPhone (the picker opening) ->
│   a capture-phone thread runs `scrawl-helper devices` (the USB registry, no
│   permission) -> CaptureSnapshot.phone = its name, or None
├── Start(Phone) -> scrawl-helper's mirror window shows the phone and records
│   its native video into recording.mov; `--no-microphone`: the mirror never
│   asks for or opens a microphone
│   ├── Camera access -> macOS asks on Hex's behalf (every bundle carries
│   │   NSCameraUsageDescription; validate-app.sh requires it)
│   └── Pointer and pen input -> normalized to the mirror's picture
├── Narration -> Hex's timeline, as for the screen, moved onto the movie's clock
│   (its first frame, measured from the capture's start) and transcribed by Hex's
│   transcriber (capture.transcriber), not Scrawl's
├── Stop -> as any capture; keyframes decoded from the phone's own video
└── The mirror ends on its own (unplugged, no phone in 12 s, camera denied) ->
    the listener finishes the capture: written if it recorded, else `failed`
    with the helper's reason

The capsule                              // capture.capsule (rune_ui/capture_capsule)
├── A Rune overlay at the top of the pointer's display that never takes focus
│   and lets clicks through except on its live chips, buttons, and list; a
│   new recording brings it to the pointer's display even while it's up on
│   another;
│   its window id goes to the engine (`Host::own_windows`, one id per role, so a
│   recreated window replaces the old one), so recordings leave it out
├── Follows `capture_host::watch()`: Recording from the instant `recording` is
│   Some (the bundle's t0; timer, meter, ✓ finish, ✕ cancel) -> Processing ->
│   done (the lead alone, no word), Failed, or Cancelled, then leaves on its
│   own; a written capture whose transcript has no words reads "no speech"
├── Tones (`feedback::Tone::Capture*`, at the dictation volume; zero is
│   silent): start, stop, a rule's send going, an Undo, a failure
├── Failed -> "Couldn't start" until the engine reports it
│   recording (`screen`), then "Couldn't save"; a failed send ->
│   "Couldn't send to X". Each keeps its `error`, or "Screen Recording is off"
│   when it isn't granted; a start that fails before any snapshot showed it
│   recording still shows, but never over a newer recording, and a capture
│   already under way that fails in the background never takes it over
├── A shown recording that leaves with no outcome (a discard, from a chip or
│   `hex capture discard`) -> the capsule leaves; the picker a chip opened
│   stays. A switch that can't start settles the recording it discarded as
│   failed, which the capsule shows as "Couldn't start"
├── A shortcut start (`target: Pointer`) -> Area · Window · Screen chips, plus
│   iPhone while the snapshot names a cabled phone, for 3.5 s
│   ├── `worked` -> they fold once seen for 0.9 s; never while the pointer is on
│   │   the capsule, and 1.2 s after it leaves
│   ├── A chip restarts rather than retargets (Scrawl's `switch_recording`)
│   ├── Screen -> it's what's recording: the chips fold, no command
│   ├── iPhone -> CaptureCommand::Switch(Phone): the recording restarts on the
│   │   phone
│   └── Window or Area -> Discard; the capsule becomes the picker, and
│       FindPhone looks for a phone cabled since the recording began
├── Picker: ⌥⇧S records what the on-screen picker shows; Esc closes it
├── ✓ -> `hex capture stop`
├── ✕ while recording -> asks first: a destructive "Discard" and "Keep
│   recording", the recording running on; Discard -> `hex capture cancel`.
│   Unanswered, it goes back to recording after 4 s (never while hovered, and
│   1.5 s after the pointer leaves). Esc never discards
├── Routing (Scrawl's routing.md) -> `last.route` once its answer arrives; the
│   capsule shows a rule's and a pick's send as sent itself, and follows
│   only the send going (its tone), Queued (then Sent), how an Undo ended,
│   and a failed send
│   ├── A rule Kit saved -> the only send that goes by itself, at once, with
│   │   no countdown: "Sent to X" and the same quiet Undo (click only, no key)
│   │   for 5 s that a
│   │   pick gets, no ring or drain; ✕ closes. The Sent tone plays when the
│   │   router says it went. No runner-up chip, since the router names none
│   │   beside a saved rule; when the Undo window ends the capsule leaves by
│   │   itself (with the list open it stays; the pointer on it holds it, and a
│   │   folded list gives it a brief hold). Picking the rule's own session there (without
│   │   "Always") only closes the list; nothing is sent again
│   ├── The Undo chip counts nothing down: the capsule reads the same for the
│   │   whole window and wakes once, when it ends
│   ├── Anything else with somewhere to go (a guess, nothing fitting, no
│   │   answer) -> macOS: the lead alone with the suggestions open under it
│   │   and Paste beside it; nothing sends until Kit picks. Linux: closed,
│   │   with Paste, Inspect, and Send to…, which opens them on a click
│   │   ├── Up to five rows, suggested then recent, numbered 1-5; the first
│   │   │   carries its reason in the accent. A reason is a real one in plain
│   │   │   words ("you were just in it", "same page", "mentioned", "recent")
│   │   │   or nothing; never "guess"
│   │   ├── Shift down (a guess or rule with a site or app) -> the footer reads
│   │   │   "Always send [site] to [row]", the site and the highlighted row's
│   │   │   name each in a rounded-rect badge with its kind's icon (a globe for
│   │   │   a site, a window for an app, a bubble for the session), and that
│   │   │   row reads "always"; up, the
│   │   │   footer is "Send 1–5  Always ⇧" again. Shift+1-5 or Shift+click
│   │   │   sends and saves the rule: "Sent to X · saved rule", and its Undo
│   │   │   takes back the send and the rule (what the key held before, or
│   │   │   nothing, comes back)
│   │   ├── 1-5 or a click -> sends at once: "Sent to X", the Sent tone, and a
│   │   │   quiet Undo for the same 5 s, with no ring or drain; ✕ closes;
│   │   │   the capsule then leaves by itself with its exit
│   │   ├── Send to… -> the full list with its search field
│   │   ├── Esc -> not Hex's here: it goes to the app in front
│   │   └── Unpicked -> they leave with the capsule after the done hold
│   ├── Taken back (a rule's send or a pick) -> never
│   │   ends the capture: the list opens again with the same sessions, the one
│   │   it went to (by session id) once, last of all ("taken back"), with 1-9,
│   │   typing, and ⇧⏎ as before. Open, it stays, sent nowhere; a click away
│   │   folds it to "Taken back" with "Send to…", and it leaves after the done
│   │   hold; nothing here takes Esc. ✕ (or Esc in the typed list) dismisses:
│   │   "Kept", the bundle untouched. Either way it then
│   │   waits for Send to…, even after the router said the pick went.
│   │   News of the send being taken back (it went, queued, or failed) leaves
│   │   that list alone, and a pick made before the router answers the Undo
│   │   stands: the Undo's answer and the earlier send's news never take it back
│   ├── Too late to take back -> the same list, the session marked "has it",
│   │   folded "Sent to X · too late"; a pick sends a copy there,
│   │   and ✕ just closes (it leaves by itself after the brief hold). Closed, it doesn't wait for a pick.
│   │   An Undo OpenCode refuses (an error) reads as too late
│   ├── Sends -> CaptureCommand::Send{to} and Undo, naming the
│   │   session by id (`RouteDest.session`), so sessions sharing a title never
│   │   collide; the capsule shows titles
│   ├── Keys (capture_host::take_keys), never taking focus:
│   │   ├── Picker -> ⌥⇧S and Esc through the listener
│   │   ├── Routed or closed -> no keys: Send to… is click-only, and never
│   │   │   Esc, so a send is taken back only by clicking Undo
│   │   ├── A send from the list still undoable -> no keys; Esc goes to the
│   │   │   app, and the Undo button is the only way back
│   │   ├── Suggestions -> at the session tap only 1 to the number of rows, with no
│   │   │   modifier or Shift (`capture_host::suggesting`; Shift's digits read
│   │   │   by key position), and a mouse press, which folds them (the click
│   │   │   still reaches the app); every other key, Esc included, goes on to
│   │   │   the app. Shift's presses and releases (flagsChanged) reach the
│   │   │   capsule too (`CapsuleKey::Shift`) and still go on to the app
│   │   └── List open -> the session tap takes every key press without ⌘ or ⌃
│   │       that isn't a Hex shortcut: text in the active layout, ⌫, ↑↓, ⏎,
│   │       ⇧⏎ (always), 1-9, Esc; their releases too. A send, Esc, or a
│   │       press off the capsule (the tap reports mouse-downs while the list
│   │       is open, and still delivers them) closes the list and gives the
│   │       keys back; hiding drops the grab
│   ├── `offer` (a rule offer) -> not drawn; Scrawl's capsule doesn't draw it
│   ├── The top line is calm: the label, its note, one action, and ✕. Undo
│   │   while a send can be taken back, else Paste and Inspect as two quiet
│   │   icons (tooltips "Paste the capture", "Open the capture") for a written
│   │   capture, with the click-only Send to… chip beside them while there is
│   │   somewhere to send it. Paste and Inspect have no key on either OS.
│   │   The list hangs
│   │   under the capsule, which is never narrower than LIST_W; the capsule
│   │   widens with it. The capsule is a rounded rect with the list's 14 pt
│   │   corners (`SHELL_RADIUS`), not a pill, and its chips and icon buttons
│   │   round concentrically inside it. Two looks while Kit picks (tuning `top_line`):
│   │   TopLine::Quiet (default) says the label in quiet MEDIUM ink;
│   │   TopLine::Icons says it in STRONG. A capture that only copied has no
│   │   label, the lead alone
│   ├── The lead is the capture's first frame (`frames/frame-01.png`, else the
│   │   contact sheet), cropped to a small rounded card once the bundle is
│   │   written and decoded off the UI thread; until then, or without one, a
│   │   check. A warning keeps its glyph
│   │   ├── Paste -> CaptureCommand::Paste: the contact sheet, then the summary,
│   │   │   into the app in front; a send still in its Undo window is taken back
│   │   └── Inspect -> CaptureCommand::Inspect: opens the contact sheet (Hex
│   │       has no review window, and opening the bundle could launch
│   │       Scrawl.app)
│   ├── Undo is a click only, with no shortcut: Escape meant for another app
│   │   was taking sends back
│   ├── Shortcuts read as quiet text after what they do, never a keycap per
│   │   key (those are Settings'): a hint's keys sit in one small rounded rect,
│   │   on a quiet fill. The list's
│   │   footer reads "Send 1–5  Always ⇧", "Send ⏎", "Keep esc", "Always ⇧⏎"; the
│   │   cursor's and the notch's cards end in "Send 1–5" under their guesses.
│   │   The footer sits apart from the rows in one of
│   │   two looks while Kit picks (tuning `list_footer`): ListFooter::Band
│   │   (default), a full-width band on its own fill under a rule, or
│   │   ListFooter::Well, a rounded well inset like the rows
│   └── Long names give way together so the widest row stays inside the panel
├── Every change moves at once, as Dynamic Island's: the walls and what stays
│   travel on `island` (0.30 s, bounce 0.12), while everything leaving and
│   everything arriving blurs (`content_blur` 3 pt), scales toward the
│   capsule's middle (`content_scale` 0.92), and fades over that same move
│   (`content_in` 0.30 s, `content_out` 0.24 s). Nothing staggers, and nothing
│   waits for a wall or for other content; only an entry takes two frames
│   (the origin pose, then rest). The list under the capsule comes and goes
│   on the same springs. The playground's Transition card tunes all of it
├── Reduce Motion -> travel jumps, nothing blurs or scales; opacity still fades
└── After a capture, one of five flows (`capture.capsule_flow`, the
    playground's "Flow after a capture"; prototypes to compare, clips in
    r22-flows). Only the guesses' look changes; rules, Undo, ✕ and the
    keys the capsule takes stay the same. Each capsule story's variants
    pick one, so a link opens a state in a flow:
    `hex://story/prototypes/capture-capsule/suggestions?variant=toast`
    ├── list (default) -> today's list under the capsule, as above
    ├── toast -> one line "Send to X ⏎" with "Next ⇥" for more than one
    │   guess; ⇥/⇧⇥ cycle (CapsuleKeys::Suggest { cycle }), ⏎ or a click on
    │   the line sends the one shown
    ├── menu_bar -> the frame plus "NAME 1/2/3" chips for MENU_HINT_S (4 s),
    │   then the capsule folds into the menu-bar item
    ├── cursor -> recording stays under the menu bar; the "Send to" card
    │   (number, name, reason) opens at the pointer and its Sent stays there
    └── notch -> a black shape grows out of the notch (185 pt wide; on a
        flat display a pretend one the menu bar's height) into the pill and
        the same card, its content under the notch, and shrinks back into it
        (capture_capsule/notch.rs). The playground's "Prototypes/Capture capsule/notch"
        story plays it on a 14-inch MacBook Pro and a 27-inch display, side
        by side or each whole, at their real sizes
    The playground's "Prototypes/Capture capsule/family" story (playground/family.rs)
    plays a dictation and then a capture in one shape on the same springs:
    from the notch on a 14-inch MacBook Pro, and on a flat 27-inch display
    inside the menu bar (hud_placement's 3 pt inset) or floating 6 pt under
    it. Its variants draw the voice as Dot scope's braid (its real
    simulation), bars, or a line; only the capture adds a red dot or a small
    screen, the cue alternating each loop
    Gap: only the shapes are prototyped; the menu-bar item's capture list
    is not, the app doesn't read the screen's notch (the overlay still puts
    the pill's content under it), and no flow but list has run live

The on-screen picker                     // capture.picker (capture_capsule/picker.rs)
├── A full-screen Rune overlay on the pointer's display, under the capsule,
│   left out of recordings (`OwnWindow::Picker`); it takes the pointer only while
│   picking and never takes focus; it follows the pointer to another display
├── Everything but the target dims; the dim moves with the target as one piece
├── Window -> the window under the pointer: tinted, ringed, its title in a badge;
│   the cursor becomes Scrawl's camera; no window under it -> the display
├── Area -> drag at least 36 pt; the badge shows its size; a click alone -> the
│   display
├── Screen -> the display, ringed
│   └── An area's ring is white with a dark hairline outside it, so it reads
│       over white content too (Scrawl's vanishes there)
├── Click or ⌥⇧S -> Action::Start(target) -> CaptureCommand::Start; the picker
│   fades; a pick that starts no capture within 2 s brings it back
└── Windows load off the UI thread (`list_selectable_windows`); nothing picks
    until they arrive

Hex's own windows                        // capture.own-windows
├── Chrome (the capsule, ink, picker, HUDs, meeting capsules: Hex's named
│   windows and any of its windows above the normal level) -> never recorded,
│   never in the window log, never the target, clicks on it not logged
│   ├── Other recorders -> the capsule, picker, and ink are left out
│   │   (sharingType none); the dictation HUD and meeting capsules show
│   ├── Show in screen recordings on -> every recorder, Hex's captures
│   │   included, shows all of it (sharingType readOnly, no SCK exclusions);
│   │   still never in the window log or the target. Rune's
│   │   OverlayHandle::set_exclude_from_capture flips open windows, and the
│   │   sampler rebuilds its filter mid-recording (OwnWindows::recorded) and
│   │   the recording's (capture-core's Recording::set_exclusions)
│   └── Not proven natively end to end yet
│   └── Click-through chrome (the full-screen ink overlay, the capsule's
│       640×424 panel outside its pill) -> clicks there count for the app
│       beneath; only the pill's (capture_host::set_capsule_pill) are Hex's
└── Normal windows (Settings) -> like any app's: recorded, logged, pickable,
    and Hex is the front app when its window is the topmost normal one, so a
    capture of Settings routes by Hex's name. A capsule Paste still goes to
    the last other app, never into Hex

Ink and laser                            // capture.ink
├── A click-through overlay over the recorded display, excluded from the recording
├── Shift strokes draw as ink, hold 650 ms, then fade over 650 ms
└── The laser's tail lasts 420 ms; nothing lags the pointer

While a capture records                  // capture.input
├── The existing session tap adds mouse down/up/drag/scroll and Shift (no second tap)
├── -> Capture::input() through a bounded queue; full -> events dropped and counted
├── Keys pressed in the app -> logged; keys Hex consumes (its shortcuts, Esc) and
│   anything while a shortcut is being recorded -> not logged
├── Scrawl's rules in the engine (hosted_input): Shift drag inks, a click pins,
│   clicks on Hex's capture chrome ignored
└── Shift still held from ⌥⇧S -> must come up once before it can ink; Shift up
    at the start -> the first Shift drag inks

hex capture start                        // capture.start; needs a listening Hex
├── Creates <captures folder>/<unix ms>.scrawl (owner-only) -> reply: recording + bundle path
├── Two stopped captures still writing -> refused `busy`
├── Copies the dictation timeline from now into a bounded sink (no second stream)
├── In the background: Scrawl's engine starts on the active display
│   ├── Needs Screen Recording for Hex (or the terminal running it)
│   └── Fails to start -> sink closed, bundle removed, last outcome `failed`;
│       Esc and the next ⌥⇧S see nothing recording
├── Release microphone while idle -> the capture keeps the microphone open
└── Already recording -> unchanged, same bundle

While it records                         // capture.audio
├── Dictation, Voice Action, and meetings work as usual on the same audio
├── Sink full (collector stalled) -> capture audio dropped as a gap, dictation untouched
└── Microphone gap or recovery -> the gap is recorded, positions stay on the clock

Never on Hex's path                      // capture.isolation
├── No microphone, event tap, global shortcut, or model of its own
├── No ingest server (47777) or recovery workers; delivery retries only while
│   neither Scrawl.app runs nor anything serves the ingest port (capture.route)
├── No ingest token used; Scrawl's two config files read once by the import, never
│   written; nothing else of Scrawl's touched (its log goes to Hex's tracing)
└── Settings from Hex (capture.*), not Scrawl's config: auto-crop on (display
    captures), the agentContext summary format off

Retention                                // capture.retention
└── Hex's own thread: Scrawl's retention pass with capture.*'s policy, 90 s after
    captures turn on, then hourly; skipped while a capture records or writes,
    or while Scrawl.app runs (it cleans the shared folder then)

hex capture stop                         // capture.stop
├── Closes the sink (≤ 100 ms wait) -> reply: idle + the bundle path, at once
├── The screen recording, window log, and input stop at once (Capture::stop)
├── recording.mp4 finishes after that on capture-core's own thread: detached
│   before its stream stops, its final frame held to the stop, capture.json
│   written beside it; only the steps that read the movie wait for it, and it
│   starts where its first frame was shown (video_offset_ms)
└── In the background, on one capture-finish thread, one capture at a time:
    ├── 16 kHz audio with its start offset and gaps -> SealedCapture::finish
    ├── Words: 30 s chunks, each a foreground job on the dictation engine's model,
    │   after queued dictation, before meeting chunks; waits while dictation records
    │   or transcribes; never interrupts a live stream
    ├── Word times moved earlier by the model's measured lag (Parakeet Unified
    │   English: starts −280 ms, −520 ms after a ≥ 250 ms pause; ends −195 ms);
    │   other models and Apple keep their own times
    ├── Past 20 min -> audio kept to 20 min, the rest one gap
    ├── recording.mp4, capture.json, audio.wav, frames/, scrawl.json,
    │   summary.md; then every file 0600 and folder 0700
    ├── Keyframes decoded from recording.mp4 by scrawl-helper (beside `hex`: in
    │   Hex.app's Contents/MacOS or target/<profile>; or $SCRAWL_HELPER); helper
    │   missing -> the 480 px sampled frames
    ├── Transcription fails -> logged; the bundle has no words, and the outcome says why
    ├── Written -> image and text copied to the clipboard (Scrawl's handoff)
    └── Sent -> the capture-route thread (capture.route)

What a send carries                      // capture.send (router opencode::send_bundle)
├── One prompt: the summary (transcript, marks, notes) as text, then the
│   contact sheet and up to 8 keyframes as inline `data:image/png;base64`
│   file parts (12 MiB of encoded images at most; past it, later frames are
│   left out). Nothing in it depends on the bundle's folder, so an OpenCode
│   server on another machine reads all of it; the text only names where it
│   was recorded
├── No video: a capture's movie is too large for a prompt
└── Check: router opencode tests; a throwaway session on OpenCode V2 stored
    the parts as inline image/png (2026-10-29)

Where it goes                            // capture.route; capture.routing's autoSend on
├── At the stop, beside the writing -> a capture-look thread reads the tab,
│   terminal, and sessions in front (Scrawl's router, in Hex's process, with
│   Scrawl's own router folders, so `scrawl router undo` and its evals see Hex's
│   sends as Scrawl's). The whole look has 15 s: steps not begun by then are
│   skipped, and routing waits 1 s more for the one under way, then goes on
│   without it. An app rule's folder is read with the sessions, and a failed
│   page of sessions keeps what was read and still reads the folders and
│   sessions that rules name
├── Written -> the one capture-route thread (queue 16) decides; the snapshot's
│   `last.route` follows: Looking -> Rule | Guess | Nothing | NoAnswer |
│   Unreachable
│   ├── Rule -> only a rule the person saved (`routing.sites`, `routing.apps`,
│   │   a Shift pick's): sent at once, Sending -> Sent, or Queued
│   │   when OpenCode didn't take it; Undo takes it back as it does a pick. Whatever
│   │   autoSend, smartRouting, or jevRouting say, a saved rule sends and
│   │   nothing else sends on its own. smartRouting off decides with the port
│   │   ladder, after rules and addresses, on the same look
│   ├── Guess -> everything else the router would send (Jev's pick, HOME when
│   │   Jev couldn't tell, a dev server's port, a page's data-session or
│   │   publisher, a name that was said, the captured app's checkout (the one
│   │   open session folder named like the app, such as `…/hex` for Hex's own
│   │   Settings; smartRouting only), the port ladder) is suggestion #1, its
│   │   reason as the row's word ("dev server", "same page", "mentioned"; none
│   │   for Jev's pick or fallback), then
│   │   the runners-up and recent sessions; nothing sent until one is picked.
│   │   `site` names what a Shift pick (Shift+digit or click, ⇧⏎ in the list,
│   │   or `send --always`) would save, after which that site's captures are Rules;
│   │   ⇧⏎ in a Rule's list saves the rule for the session picked instead
│   ├── Each Rule or Guess -> routing.json's `shown`: `as` rule or guess, and
│   │   `why`, the router's leading reason; `hex capture route` prints it
│   ├── Nothing / NoAnswer -> recent sessions only
│   └── Unreachable -> the look read no sessions (OpenCode slow past the look's
│       deadline, down, or refusing); a warning in process.log with the error, and
│       the capsule reads "Couldn't reach OpenCode" with why ("didn't answer in
│       time"), keeping Paste
├── An answer for a capsule already closed (dismissed, or past its hold)
│   leaves it closed; `hex capture list` and `send` still offer the capture
├── The capsule's commands act only on `last`, the newest routed capture:
│   ├── Send{to} -> the Rule's own session, already sent, does nothing; any
│   │   other pick sends there (taking back a send already elsewhere first) and logs
│   │   the correction; `always` also saves a rule for its site or app
│   └── Undo -> a sent one is withdrawn unless the
│       session acted on it (TooLate); an undo the router fails is TooLate too
├── Two sends to one place agree -> `offer` names the site or app a rule could
│   send straight there
├── Queued sends -> retried every 15 s, unless Scrawl.app runs or something
│   serves the ingest port (that process retries); a retried send can't be
│   undone, so its "Sent to X" has no Undo. A queued or too-late-to-undo send
│   leaves after the brief hold (4 s) rather than the 25 s done hold; a taken
│   back capture with nothing to list ('Taken back') does too
└── autoSend off -> no look, no route; the capture is only copied

hex capture list|show|send|route         // capture.cli; docs/cli.md
├── list [--limit N] -> the newest bundles in the captures folder: mode, words,
│   where it went
├── show <capture> -> one capture's words, files, and send
├── send <capture> [--to <session>] [--always] -> needs a listening Hex
│   ├── The last routed capture -> as the capsule's pick (no --to: the Rule's
│   │   session, a Guess's first suggestion, or the recorded destination), so
│   │   the capsule's Undo follows it; --always also saves the site or app rule
│   │   for the session it goes to. Also when a newer capture was written first
│   └── Older -> --to must be a session ID; without it, its recorded
│       destination; --always is refused
├── route <capture> [--ask-jev] -> what the router sees and decides; sends nothing
└── <capture> = bundle name, path, or `latest`; unknown -> `not-found`

hex capture cancel -> engine cancelled, sink closed, bundle removed
hex capture status -> recording (bundle, elapsed), bundles still writing, last outcome
                      (words, audio gaps, or why it has no transcript)
Hex quits mid-capture -> cancelled, bundle removed; captures still writing stop
transcribing between chunks and keep their audio (shutdown waits ≤ 2 s)
```

`scrawl.json` is Scrawl's bundle manifest. Its `transcript.words` are
`{t0, t1, text}` in milliseconds from the capture's start (`t0_boot_ns`, the
boot-time clock audio and shortcuts share), and `audio.wav` starts at that
start too, padded with silence when the microphone began late or dropped
audio.

`hex dev capture-replay <wav>…` (debug, with the feature) runs the same
capture host on a fresh transcription engine with the selected model, feeding
WAV files instead of the microphone, and prints the bundle path. It needs the
experiment on too, and records the screen meanwhile.

Sources: [capture_host.rs](../../src/capture_host.rs), the tap in
[suppression.rs](../../src/suppression.rs), the shortcut and Escape in
[recognition.rs](../../src/recognition.rs), `CaptureSettings` and the import in
[app_settings.rs](../../src/app_settings.rs), the pane in
[rune_ui/scrawl.rs](../../src/rune_ui/scrawl.rs), the input rules in
[hosted_input.rs](../../crates/capture/src/hosted_input.rs), the sink in
[dictation_audio.rs](../../src/dictation_audio.rs),
`transcribe_with_words` in [parakeet.rs](../../src/parakeet.rs), routing in
[capture_host/routing.rs](../../src/capture_host/routing.rs), the CLI's reads in
[capture_host/library.rs](../../src/capture_host/library.rs), the phone finish
in [phone.rs](../../crates/capture/src/phone.rs), and the `scrawl-capture`
control commands in [control.rs](../../src/control.rs).

Checks:

- Gate: `capture_exists_only_where_it_has_a_host` in
  [experiments.rs](../../src/experiments.rs);
  `the_host_is_absent_while_the_experiment_is_off` in capture_host.rs;
  [test-app-identity.sh](../../scripts/test-app-identity.sh) for the
  release-path refusals.
- Sink, in dictation_audio.rs: `capture_sink_shares_the_timeline_with_dictation`,
  `a_full_capture_sink_drops_audio_as_a_gap_and_never_stalls_dictation`,
  `capture_sink_records_microphone_gaps`,
  `capture_sink_holds_a_released_microphone_open_until_it_closes`,
  `capture_sink_keeps_every_frame_through_timestamp_jitter`, and
  `a_dropped_capture_receiver_closes_the_sink`.
- Words, in parakeet.rs: `chunked_word_transcription_keeps_word_times_across_chunks`
  and `word_transcription_is_a_foreground_request_on_the_shared_engine`;
  `foreground_work_waits_for_an_open_stream_instead_of_interrupting_it` in
  [transcription_engine.rs](../../src/transcription_engine.rs).
- Host, in capture_host.rs (a fake engine that writes through Scrawl's real
  `finalize_capture_bundle_with_flags`, without a screen):
  `controls_are_idempotent_and_stop_writes_a_scrawl_bundle`,
  `an_engine_that_cannot_start_leaves_nothing_behind`,
  `scrawls_engine_claims_nothing_machine_wide`,
  `the_timeline_resamples_pads_gaps_and_ends_at_the_stop`,
  `a_pending_gap_and_a_stalled_tail_come_back_from_the_closed_sink`,
  `padding_past_the_length_limit_stays_bounded`,
  `captures_finish_one_at_a_time_and_starts_wait_for_room`,
  `a_capture_queued_behind_a_slow_finish_has_already_stopped`,
  `a_failed_start_raced_by_stop_never_wedges_later_starts`,
  `the_engine_config_comes_from_hex`, and
  `shutdown_cancels_the_recording_capture_and_waits_for_its_cleanup`.
- Shortcut and input, in capture_host.rs: `the_host_is_absent_while_captures_are_off`,
  `the_shortcut_starts_and_a_later_press_stops`,
  `input_reaches_only_the_capture_that_is_recording`, `tap_flags_become_modifiers`,
  `the_shortcut_records_once_its_keys_come_up`,
  `a_failed_start_ends_before_escape_or_the_shortcut_sees_it`;
  `the_capture_shortcut_is_suppressed_and_never_chord_evidence` and
  `a_capture_logs_only_keys_that_reach_the_app` in suppression.rs;
  `only_the_capsules_pill_and_not_its_panel_takes_clicks` in capture_host.rs;
  `a_paste_goes_to_an_external_app_never_the_host` in capture/macos.rs;
  `the_hosts_normal_windows_count_and_its_chrome_does_not` in
  [window_log.rs](../../crates/capture/src/window_log.rs) and
  `helper_windows_are_scrawl_s_except_the_capture_source` in
  [capture.rs](../../crates/capture/src/capture.rs) for Hex's own windows;
  the ink and click rules in hosted_input.rs, including
  `a_capture_started_by_its_shortcut_waits_for_shift_to_come_up`,
  `a_capture_started_without_shift_held_inks_from_the_first_stroke`, and
  `a_click_with_the_chords_shift_still_held_is_no_pin`.
- iPhone: `an_iphone_capture_narrates_with_hex_audio_and_finishes_when_its_mirror_ends`
  and `a_mirror_ending_never_stops_a_screen_capture` in capture_host.rs;
  `since_moves_audio_and_gaps_onto_a_later_clock` in
  [audio.rs](../../crates/capture/src/audio.rs). The mirror, its
  `--no-microphone`, and the camera prompt have not run in Hex.
- Routing, in [routing.rs](../../src/capture_host/routing.rs):
  `only_a_saved_rule_sends_on_its_own`,
  `a_rule_sends_and_lists_the_rest`,
  `a_guess_sends_nothing_and_a_failure_still_lists_recent_sessions`, and
  its `looked` tests, which write a bundle and route it through `look` and
  `Worker::written` with fake sessions and tab:
  `a_saved_site_or_app_rule_sends_whatever_the_routing_flags`,
  `the_port_ladder_is_a_guess`, `the_captured_apps_checkout_is_a_guess`,
  `a_look_past_its_deadline_is_routed_without`,
  `an_always_rule_names_the_folder_of_the_session_it_sends_to`, and
  `a_capture_routed_after_a_newer_one_settled_is_still_sendable_by_name`;
  `an_always_rule_makes_its_sites_next_capture_a_saved_rule`,
  `a_failing_page_keeps_what_was_read_and_still_reads_what_rules_name`,
  `an_app_rules_folder_is_read_with_the_sessions`, and
  `a_look_past_its_deadline_skips_what_it_hadnt_begun`
  in [look.rs](../../crates/router/src/routing/look.rs); the CLI's
  `a_capture_on_disk_summarizes_its_mode_words_and_send` in
  [library.rs](../../src/capture_host/library.rs), the protocol round trips in
  control.rs, and `control_commands_parse` in main.rs.
- Switching, in capture_host.rs:
  `a_discarded_capture_leaves_no_trace_and_a_pick_takes_its_place`,
  `a_switch_never_shows_nothing_recording`,
  `a_switch_with_no_room_to_start_keeps_the_recording`,
  `a_switch_that_cannot_start_after_its_discard_settles_failed`,
  `discarded_captures_never_count_against_the_writers`,
  `a_pick_that_cannot_start_says_why`,
  `the_first_click_or_scroll_means_work_started`, and
  `the_newest_grab_takes_only_its_keys_until_it_drops`. Paste and Inspect
  need a written capture
  (`controls_are_idempotent_and_stop_writes_a_scrawl_bundle`); neither has
  pasted into or opened anything in a test.
- Capsule, in rune_ui/capture_capsule (feature builds only): the flow tests
  (`chips_fold_once_seen_when_work_starts_but_never_under_the_pointer`,
  `the_shortcut_records_the_pickers_screen_and_a_chip_discards_the_recording`,
  …), feed tests
  (`the_shortcuts_recording_shows_its_chips_until_work_starts`,
  `a_rule_shows_once_and_only_news_the_capsule_cant_know_moves_it`,
  `a_queued_send_says_so_and_says_sent_when_it_goes`,
  `a_cabled_phone_shows_its_chip`, …),
  `paste_takes_back_a_send_still_undoable_and_the_capsule_stays_gone`,
  `a_rule_reads_sent_at_once_and_sounds_when_it_went`,
  `undoing_a_rule_send_reopens_the_list_until_a_pick`,
  `undoing_a_pick_reopens_the_list_it_came_from`,
  `a_pick_during_an_undo_stands_when_the_undo_answers`,
  `a_send_undone_while_under_way_keeps_the_list_however_it_ends`,
  `the_undo_window_wakes_the_capsule_once_at_its_end`,
  `every_rule_send_sounds_sent_once`,
  `picking_the_rules_own_session_just_closes_the_list`,
  `too_late_to_take_back_offers_the_list_for_a_copy`,
  `shift_return_in_a_rules_list_saves_a_rule_elsewhere`,
  `shift_shows_the_rule_and_a_shift_pick_saves_it`,
  `an_answer_after_the_capsule_closed_leaves_it_closed`,
  `the_capsule_takes_only_the_keys_its_state_needs`,
  `shortcuts_read_as_one_quiet_word`, `the_capsule_draws_no_keycaps`,
  `a_written_copy_offers_paste_and_inspect_as_icons`,
  `every_change_moves_at_once`,
  `entry_lands_the_pose_instantly_then_springs_to_rest_with_its_content`,
  `the_toast_cycles_its_guesses_and_return_sends_the_one_shown`,
  `the_menu_bar_hint_is_brief_and_its_chips_send`,
  `the_cursor_card_lists_its_guesses_at_the_pointer`,
  `the_notch_hangs_from_the_top_edge`,
  `the_menu_bar_flow_folds_up_toward_the_menu_bar`, goldens
  `tests/golden/capture-capsule-*.png`, and
  `the_capsule_moves_inside_its_panel_and_comes_to_rest`, which traces every
  scripted state change per frame, Calm and Reduce Motion, for no pops,
  jumps, or jiggle. `HEX_CAPSULE_MOTION` also writes rune-motion traces for
  motion-lint. `cues_mark_start_stop_sent_and_taking_back`,
  `nothing_heard_says_so`, and `a_capture_with_no_words_says_nothing_was_heard`
  cover the tones and the empty transcript. Observed (Oct 27, worker, r19 motion): all 28
  capsule runs, Calm and Reduce Motion, pass lint.
- Playground: `hex capsule-playground` (feature builds; hidden), or
  `scripts/build-capsule-playground.sh` for an ad-hoc signed
  `Hex Playground.app`, runs the real capsule in a window of its own against a
  fake host, with state buttons, the whole flow, slow motion, Reduce Motion,
  the tones, and live tuning ([tuning.rs](../../src/rune_ui/capture_capsule/tuning.rs);
  "Copy values" copies its `DEFAULT`). It reads and writes no Hex data and
  records or sends nothing. The whole flow picks the first suggestion;
  its "Saved rule" toggle plays a rule's send, sent at once, instead.
  `--snap --scene suggestions|shift|sent-rule|search|picked|rule|recording`
  renders it. The tones were checked by waveform and level only, never on
  speakers.
- Picker, in capture_capsule/picker.rs and tests.rs: geometry, label, and
  target tests, `the_picker_records_what_it_shows_and_comes_to_rest`, and
  `picker_stills_and_motion`, which scripts window hover, an area drag, and
  mode switches per frame (Calm and Reduce Motion). Observed (Oct 26, pro):
  all 6 runs pass motion-lint with two excuses: the target growing from a
  zero-size point, and the camera stopping where the scripted pointer stops.
  Not proven natively end to end: the overlays have been driven only by the
  scripted scenes, not a live capture.
- Pane: `captures_default_off_and_refuse_a_shortcut_in_use` and
  `retention_names_presets_and_imported_days` in rune_ui/scrawl.rs; scenes
  `mac-scrawl-off`, `mac-scrawl`, `mac-scrawl-running`, `mac-scrawl-permission`
  (feature builds only, so no goldens: those run on Linux).
- Settings: every `capture.*` key in `hex settings`
  (`every_persisted_setting_has_a_cli_key`).

Observed 2026-10-25 on pro (M2 Max), before the real engine, with the
stand-in: `HEX_EXPERIMENTS=capture hex dev capture-replay` over three
vocabulary fixtures (6.5 s) loaded Parakeet unified English on Metal and
wrote 18 words with plausible times. The real engine has not run end to end:
the unit tests use a fake engine over Scrawl's real bundle writer, and the
mini (the test Mac) has no Screen Recording grant for a dev build, so neither
`hex capture` against a live app nor `capture-replay` has recorded a screen.
The shortcut, tap input, Escape, switching, routing, the iPhone path, Scrawl
coexistence, and import are covered by unit tests only; none has run against a
live Hex.
