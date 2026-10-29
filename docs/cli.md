# `hex` control CLI

The control commands drive the one running Hex: start or stop a dictation or a
meeting, change any setting, switch models, open the window, quit, or read its
status. They are meant for agents and scripts as much as people, so every
command takes `--json`, is idempotent, and exits with a stable code.

macOS is the only supported host today. On Linux and Windows `hex settings`
reports `unsupported` ("not supported on this OS yet; use the Hex window"); the
Linux CLI keeps its own `dictation`, `voice-action`, `paste-last`, `status`,
`quit`, and `restart` over the Linux service socket (see `docs/linux.md`).

The read-only `hex history` and `hex meetings list|show|chapters|search` commands, and
`hex mcp`, are separate and unchanged; see `docs/transcript-cli.md`.

## Commands

```ts
hex
├── status                         what Hex is doing (see Status below)
├── start                          open Hex.app if it isn't running; wait until it answers
├── quit [--force]                 refused while a meeting or dictation is active, or unconfirmed
├── restart [--force]              quit and reopen (opens Hex when it isn't running)
├── dictation start|stop|cancel|toggle
├── voice-action start|stop|cancel|toggle   needs voice_action.enabled
├── paste-last                     paste the last dictation (Option-Shift-V); refused while recording
├── meetings
│   ├── record [--title T]         as the menu bar's Record Meeting; needs meetings.enabled
│   ├── stop                       Stop & Transcribe
│   ├── copy-last                  copy the last meeting transcript to the clipboard
│   └── list | show | search       read-only (docs/transcript-cli.md)
├── settings
│   ├── list                       every setting: key, value, kind, choices, writable, about
│   ├── get <key|prefix>           one setting, or every key under a prefix (`meetings`)
│   └── set <key> <value> [--yes]
├── model
│   ├── list                       every model, its languages and size, installed or not
│   ├── use <id> [--language L]    installed models only; never downloads or repairs
│   ├── install <id> [--language L] download, then switch to it
│   └── cancel                     stop a download or load in progress
├── commands
│   ├── enable [--yes]             = settings set commands_enabled on
│   ├── disable                    = settings set commands_enabled off
│   ├── retry                      load the command model again after it failed
│   └── init                       create ~/.config/hex (local, no running Hex needed)
├── window
│   ├── show [settings|models|modes|voice-action|history|commands|meetings]
│   ├── hide                       close the window; Hex stays in the menu bar
│   └── story <story> [--variant V] [--play]  the `playground` experiment: open a story
├── history clear --yes            delete every retained dictation
├── vocabulary                     needs the `vocabulary` experiment (marked so in --help)
│   ├── list [--json]              every term and its "sounds like" spellings
│   ├── add <term> [--sounds-like a,b]  add a term, or new spellings to an existing one
│   └── remove <term>              any case
├── annotate (alias capture)       Annotate, macOS and Linux (features/capture.md)
│   ├── start|stop|cancel|status   record a capture, end or discard it, or report
│   ├── list [--limit N]           the newest captures in the captures folder
│   ├── show <capture>             one capture's words, files, and where it went
│   ├── send <capture> [--to <session>] [--always]  send it to an OpenCode session
│   └── route <capture> [--ask-jev]  what routing sees and decides; sends nothing
├── experiments list|enable|disable  edit ~/.config/hex/experiments; no running Hex (features/experiments.md)
├── check-for-updates              check for an update now
├── launch-at-login [on|off]       report or change Launch at Login (installed Hex.app only)
└── logs [--process] [-n N] [-f]   tail live.ndjson (or process.log); no running Hex needed
```

`dictation start` locks the recording, as a double tap would: `dictation stop`
or the next shortcut press finishes it, and `dictation cancel` or Escape
discards it. A CLI start has no press to judge, so its recording counts as
intentional at once: the start tone plays once the microphone is open, and the
recording behavior (mute or pause media, idle-sleep prevention) and the live
stream start as they do after a held shortcut. `voice-action start` behaves
the same way. A `stop` replies `done` only when the clip went to
transcription; a clip with no audio yet (the microphone was still opening) or
a silent one is dropped with `discarded`, and nothing shows as transcribing.

`dictation cancel` with nothing recording cancels the newest unfinished
dictation, like Escape. Escape cancels an active recording before any job, so
while the other capture records (a Voice Action, say), an idle `cancel` is
refused with `busy` rather than reach past it; cancel that recording first.
`voice-action cancel` follows the same rule. `meetings cancel` doesn't exist
because Hex has no way to discard a meeting recording.

`paste-last` needs a listening Hex (`not-ready` during setup) and is refused
with `busy` while anything records or a shortcut gesture is in progress. The
menu bar's Paste Last Dictation does nothing in that case.

`model use` switches only to a downloaded model and checks it on disk; it never
downloads or repairs one (`model install` does). It keeps that model's own
remembered language and recognition hints, as the menu bar does; `--language`
overrides only the language, and must be one the model supports.

`vocabulary` edits the [shared vocabulary](features/vocabulary.md) through the
running Hex, which saves settings and reloads them as a window edit would.
Terms match in any case; `add` of a term already listed only merges new
spellings, and replies `changed: false` when there are none. While the
`vocabulary` experiment is off in the running Hex, every `vocabulary` command
is refused with `feature-off`.

`quit` and `restart` without `--force` are refused with `busy` when the
listener doesn't answer whether it's recording; no listener, or one waiting for
setup, counts as idle.

`start` and `restart` keep the `HEX_*` environment (`HEX_RETAIN_DICTATION_AUDIO`,
`HEX_ENABLE_LLM`, and so on): macOS opens apps without the caller's
environment, so Hex passes each `HEX_*` variable to `open --env`. `start`
takes them from the shell that runs it, `restart` from the running Hex. The
values are never logged.

`annotate` (or its old name `capture`) drives [Annotate](features/capture.md),
in every macOS and Linux build; with `capture.enabled` off it refuses with
`feature-off`. On Linux, `list`, `show`, `route`, and `send` run in the
`hex` process itself and `start`, `stop`, and `cancel` are handed to the
running Hex's listener; `status` is macOS-only for now. `capture stop` replies at once with the bundle
path while the words and files are written in the background; `capture status`
lists bundles still writing and the last outcome (`last`: `bundle`, `stage`
of `done`, `failed`, or `cancelled`, and for a written bundle `words`, `gaps`,
and `missing_transcript`, why it has no words). A start while two captures
are still writing is refused with `busy`. Every action is idempotent.

`<capture>` is a bundle name (`1792933491953.scrawl`, the extension
optional), a path, or `latest`. `list`, `show`, and `route` read the captures
folder and need only the experiment; `send` needs captures on and a listening
Hex. `send` with `--to` takes a session ID; for the newest capture it also
takes a name its route lists, and it acts as the capsule's pick, so the
capsule's Undo follows it. Without `--to` it goes where the capture's route
(its rule, or its first guess) or its last send says. `--always`, for the
newest capture only, also saves a rule sending its site or app there, as the
capsule's ⇧⏎ does. Its reply says what became of it (`Sent to …`,
`Queued for …` when OpenCode didn't answer, which Hex retries). `route` is
Scrawl's `route --explain` (`--explain` is accepted); `--ask-jev` asks Jev
again, one network call. An unknown capture is refused with `not-found`.

A CLI-started meeting is the same recording the menu bar starts: the menu-bar
state, the recording capsule, and Stop & Transcribe all apply to it.
`meetings record` replies once the recording has started (or after 8 s);
`meetings stop` replies once transcription has begun.

## Exit codes

```ts
0  the command ran, or there was nothing to change (`changed: false`)
1  Hex refused or failed it (an error reply), or the transport failed
2  usage error (clap)
3  Hex isn't running (code `not-running`); control commands never launch it
```

## Transport

The CLI reads the owner-only discovery file
`~/Library/Application Support/hex/local-api.json` (`HEX_APPLICATION_SUPPORT_DIR`
overrides the folder), checks that its pid is alive, and sends one
`POST /control` to `127.0.0.1:<port>` with `Authorization: Bearer <token>`.
The route is always on in the desktop app; the headless `hex service` doesn't
offer it.

- Request bodies are JSON up to 64 KiB; replies are at most 1 MiB.
- The app answers within 10 s (`504 control-timeout` otherwise); the CLI waits
  15 s.
- Every answer the app gives is `200` with a `ControlResponse`, refusals
  included. Transport-level failures use HTTP status codes:
  `400 invalid-control-command` (malformed or unknown command),
  `404 control-unavailable` (no control host), `413 control-too-large`,
  `503 control-busy` (too many in flight) or `control-unavailable`, and
  `504 control-timeout`. Bad or missing tokens get `401` like every other
  route.

## Wire format (version 1)

Requests are flat objects tagged by `command` (kebab-case):

```json
{"version": 2, "command": "meeting-record", "title": "Design sync"}
{"version": 2, "command": "settings-set", "key": "sound_effects", "value": "off", "confirm": false}
{"version": 2, "command": "dictation", "action": "start"}
```

```ts
status | quit {force} | restart {force}
dictation {action} | voice-action {action}       action: start | stop | cancel | toggle
paste-last
meeting-record {title?} | meeting-stop | meeting-copy-last
settings-get {key?} | settings-set {key, value, confirm}
commands-retry
model-list | model-use {model, language?} | model-install {model, language?} | model-cancel
window-show {pane?} | window-hide | window-story {story, variant?, play}
history-clear | check-for-updates | launch-at-login {enabled?}
vocabulary-list | vocabulary-add {term, sounds_like} | vocabulary-remove {term}
scrawl-capture {action}                          action: start | stop | cancel | status; `capture` experiment
scrawl-capture-list {limit?} | scrawl-capture-show {capture}
scrawl-capture-send {capture, to?, always?} | scrawl-capture-route {capture, ask_jev}
```

Optional fields may be left out. `hex start` and `hex logs` are local and have
no wire form. `history-clear` has no confirmation field: the CLI asks for
`--yes` before it sends it.

Replies are tagged by `status`:

```json
{"version": 2, "status": "done", "message": "Recording “Design sync”; …", "changed": true}
{"version": 2, "status": "error", "code": "feature-off", "message": "Meetings are off; …"}
{"version": 2, "status": "settings", "settings": [{"key": "sound_effects", "value": true,
  "kind": "bool", "writable": true, "about": "Play recording, cancellation, and error tones"}]}
{"version": 2, "status": "models", "models": [{"id": "parakeet_v3", "name": "…",
  "languages": ["en", "…"], "size": "…", "installed": true, "selected": true,
  "streams": true, "supports_recognition_hints": false}]}
{"version": 2, "status": "vocabulary", "entries": [{"term": "OpenCode",
  "sounds_like": ["open code"]}, {"term": "Rune"}]}
{"version": 2, "status": "scrawl-capture", "changed": true, "state": "idle",
  "bundle": "…/captures/1792933491953.scrawl", "elapsed_ms": null,
  "finishing": ["…/captures/1792933491953.scrawl"], "last": null}
{"version": 2, "status": "scrawl-captures", "captures": [{"id": "1792933491953.scrawl",
  "bundle": "…", "created_at_ms": 1792933491953, "duration_ms": 4000, "mode": "window",
  "app": "Brave", "words": 12, "sent_to": {"session": "ses_…", "title": "Hex"},
  "queued": false}]}
{"version": 2, "status": "scrawl-capture-detail", "id": "…", "…": "the list's fields",
  "window_title": "…", "transcript": "…", "note": null, "missing_transcript": null,
  "marks": 2, "keyframes": 3, "video": "…/video.mp4", "audio": null,
  "contact_sheet": "…/contact-sheet.jpg"}
{"version": 2, "status": "scrawl-capture-route", "explanation": "Capture   …"}
```

`done.changed` is `false` when the command found things already so ("Already
recording", "The window isn't open"). Setting `kind` is one of `bool`,
`number`, `integer`, `choice` (with `choices`), `text`, `shortcut`, `path`, or
`json`; `value` is `null` for an unset optional setting.

### Status

```json
{
  "version": 2, "status": "status",
  "app_version": "2.3.1", "pid": 31708, "memory_footprint_bytes": 412000000,
  "listener": {"running": true, "status": "Listening"},
  "capture": {"state": "idle", "recording": null, "pending_jobs": 0},
  "meeting": {"state": "recording", "title": "Design sync", "elapsed_seconds": 723},
  "model": {"id": "parakeet_v3", "name": "…", "language": "en", "installed": true,
            "preparing": null, "downloaded_bytes": 0, "error": null},
  "features": {"commands": false, "command_model": "disabled", "voice_action": true,
               "meetings": true, "release_microphone_while_idle": false,
               "history_retention": "week"},
  "permissions": {"ready": true, "microphone": "ready", "accessibility": "ready",
                  "transcription_model": true},
  "window": {"open": false, "pane": null},
  "update": "current",
  "experiments": []
}
```

```ts
listener.status     Enabled | Waiting for setup | No microphone | Stopped (running false)
capture.state       idle | recording | transcribing | unknown (the listener didn't answer or stopped)
capture.recording   dictation | voice-action | service | voice | null
meeting.state       idle | starting | recording | transcribing
command_model       disabled | loading | ready | failed
permissions.*       ready | needs-request | needs-settings | needs-restart
window.pane         settings | modes | voice-action | history | commands | meetings | …
update              current | checking | available | unavailable
```

`memory_footprint_bytes` is the physical footprint Activity Monitor shows.
`experiments` lists the [experiments](features/experiments.md) Hex read at its
launch; older Hex omits it.

### Error codes

```ts
not-running            Hex isn't running (CLI only; exit 3)
transport              the request couldn't reach Hex or its reply was unreadable (CLI only)
version-mismatch       this hex speaks another control version; use the running app's hex
unknown-setting        no such key (the message suggests a near match)
read-only              the key can't be set here (the message says where)
invalid-value          the value doesn't parse or is out of range
confirmation-required  the change also turns another setting off, or clears history; pass --yes
shortcut-in-use        another Hex shortcut already uses that binding
not-found              no such capture, or it has no scrawl.json yet
unknown-model          no such model id (`hex model list`)
not-installed          `model use` on a model that isn't downloaded (`hex model install`)
feature-off            the command needs an opt-in that is off (Meetings, Voice Action, commands)
                       or an experiment the running Hex didn't read (vocabulary)
not-ready              Hex is waiting for setup (permissions or the dictation model)
meeting-active         quit/restart while a meeting records (pass --force)
busy                   something is in progress (recording, transcribing, a full queue),
                       or the listener couldn't confirm it's idle
discarded              `stop` dropped the clip: no audio yet, or silent; nothing transcribes
unavailable            a part of Hex the command needs isn't available
unsupported            this build or OS can't do it (e.g. restart from a development binary)
failed                 Hex tried and failed (the message has the cause)
internal               reading or saving settings failed
```

## Settings

Keys are dotted paths into `settings.json`. `hex settings list` prints every
key with its kind and description; `hex settings list --json` adds `choices`.
Changes go through the same rules as the window and are saved by the app,
which then reloads them live; an open window adopts the change unless it has
an unsaved edit.

On macOS, `keyboard_model_switching.enabled` opts into switching the shared
speech-model picker on keyboard input-source changes. The JSON setting
`keyboard_model_switching.mappings` maps enabled input-source IDs to a model and
a language it supports, for example
`{"com.apple.keylayout.Spanish":{"model":"parakeet_v3","language":"es"}}`. Omit a
source for "Don't switch"; Apple Speech can't be mapped. Automatic switches load
installed models only; they never download them. Enabling or editing the active
mapping applies it; launching Hex does not reapply it. Other platforms keep the
settings but do not observe input-source changes or show these controls.

- Booleans take `on`/`off`, `true`/`false`, `yes`/`no`, or `1`/`0`.
- Optional settings take `none` to clear them.
- `microphone` takes a device name exactly as Settings lists it, or
  `system-default` (or `none`) to follow the OS default input.
- `avoid_bluetooth_microphone` (macOS, default on) makes System default record
  from the Mac's built-in microphone, else another wired or USB input, when
  the OS default is a Bluetooth headset, so its audio stays out of call
  quality. A saved microphone and `--device` are always used as named.
- Shortcuts take text such as `option`, `right-command`, `option+shift+v`,
  `control+space`, `fn`, or `f5`. On macOS, Windows and X11, `mouse4` and `option+mouse4`
  also work (Mouse 1/2/3 are left/right/middle). Key names resolve on the active keyboard
  layout, the OS's rules for Settings' recorder apply, and a binding another
  Hex shortcut uses is refused with `shortcut-in-use`. `double_tap_only`
  needs a key chord, as in Settings.
- `json` settings (modes, meeting providers, excluded calendars) take the JSON
  value the file stores.
- `commands_enabled on` while `release_microphone_while_idle` is on, and the
  reverse, need `--yes`, because each turns the other off.
- `voice_action.enabled on` needs a free Voice Action shortcut.
- `updates.channel` takes `stable`, `dev`, or `none` (follow the build: the
  channel it was published on), and checks for updates at once. Choosing
  another channel than the build's offers that channel's current release
  ("Switch to Stable 2.3.1" in the window); Hex installs it only once you
  confirm there. Versions are never compared.
- With the `capture` experiment, `capture.*` keys appear:
  `capture.enabled`, `capture.hotkey` (default `option+shift+s`),
  `capture.list_hotkey` (the capsule's send-to list, default
  `option+shift+v`), `capture.transcriber`, `capture.folder`,
  `capture.media_days`, `capture.bundle_days`, `capture.routing`,
  `capture.show_overlays_in_recordings` (default off; on lets screen
  recordings, Hex's captures included, show its overlays), and
  `capture.capsule_flow` (`list`, the default, `toast`, `menu_bar`, `cursor`
  or `notch`: the capsule's flow after a capture, prototypes to compare). Both
  capture shortcuts need a key, and `capture.enabled on` needs both free; on
  a Mac whose Paste Last is still ⌥⇧V, rebind one of the two first.
- `transcription.model` and `transcription.language` are read-only here: use
  `hex model use`. `transcription_recents` and `meetings.calendar.connected`
  are read-only (connecting a calendar asks macOS for access from Settings).

`recording_audio_behavior` also accepts `duck` on macOS.
`recording_duck_volume` sets the fraction of the original output volume (0–1,
default 0.2), applied on the next intentional capture when ducking is selected.

Writable keys: `commands_enabled`, `release_microphone_while_idle`,
`sound_effects`, `sound_effect_volume`, `sound_theme`, `microphone`,
`avoid_bluetooth_microphone`, `recording_audio_behavior`, `recording_duck_volume`, `double_tap_lock`, `double_tap_only`,
`shortcut_mode`, `dictation_hotkey`, `edit_hotkey`, `paste_last_hotkey`,
`show_dock_icon`, `transcription.recognition_hints`, `vocabulary_boost`,
`dictation_processing.default_mode`, `dictation_processing.modes`,
`voice_action.enabled`, `voice_action.model`, `voice_action.variant`,
`voice_action.deadline_seconds`, `history_retention`, `meetings.enabled`,
`meetings.output_folder`, `meetings.summarize_with_apple_intelligence`,
`meetings.show_recording_indicator`, `meetings.recording_indicator_corner`,
`meetings.providers`, `meetings.calendar.excluded_calendars`,
`meetings.calendar.remind`, `meetings.calendar.lead_minutes`,
`experimental.hex_microphone`, `experimental.hex_microphone_mode`,
`experimental.hex_microphone_comfort_noise`,
`experimental.hex_microphone_strength`, `enable_on_launch`,
`paste_with_shift`, `ui_language`, `indicator_position`, `updates.channel`.

A test (`control::settings::tests::every_persisted_setting_has_a_cli_key`)
fails when a new persisted setting has no key here.

## Compatibility

`CONTROL_VERSION` is 2. A change that breaks a request or reply shape
increments it; adding a command, an optional request field, or a reply field
does not. A `hex` from a different version gets `version-mismatch`; use the
`hex` inside the running Hex.app, or restart Hex after updating. Version 2
removed `permissions.input_monitoring` from the status reply.
