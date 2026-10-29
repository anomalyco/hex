# Voice Commands

Opt in to spoken commands without changing ordinary hotkey dictation. Commands
uses its own Moonshine model and awake/sleep state; it is not
[Voice Action](voice-action.md).

This map covers macOS. Linked checks are locators, not passing runs; no tests,
previews, providers, or native actions were run for this split.

## How To Get To It

```ts
Hex > Commands                                      // commands; off by default
├── Top card "Voice commands" switch -> Prepare command model // needs setup permissions
├── Model failed -> card below it: Retry / Turn off commands  // only in this pane
├── Search / select a catalog row -> Inspect, not execute
└── Create config / Edit config / Copy agent prompt  // personal workspace

hex commands init -> Create or refresh ~/.config/hex // does not start listening
hex listen -> Use saved command opt-in               // never beside another listener
```

The catalog and config controls remain reachable while Commands is off. Sources:
[rune_ui/commands.rs](../../src/rune_ui/commands.rs), CLI routes in
[main.rs](../../src/main.rs), and workspace paths in
[app_paths.rs](../../src/app_paths.rs). The release
`hex commands enable|disable|retry` ([control CLI](control-cli.md)) changes the
running app's opt-in like the window; debug `hex dev commands on|off` remains.

## Sub-features

### Opt In And Prepare

```ts
Enable Commands                                     // commands.opt-in
├── Release when idle active -> Confirm Keep microphone ready & enable commands
└── Load separate Moonshine model in background
    ├── Ready -> Recognize commands, initially awake
    └── Failure -> Keep opt-in; show Retry / Turn off commands

Disable -> Unload command recognizer and action executor
Hotkey dictation -> Remains available during preparation and failure
```

Sources: `load_command_recognizer` and live policy handling in
[recognition.rs](../../src/recognition.rs); the enable card and command model
notice in [rune_ui/commands.rs](../../src/rune_ui/commands.rs). Checks:
`command_preparation_does_not_block_the_listener_or_propagate_failure`,
`a_failed_command_model_offers_retry_and_turning_commands_off`,
`commands_turn_on_from_their_card` (scene `mac-commands-unavailable`), and
`missing_fields_receive_defaults` /
`microphone_policy_requires_explicit_combined_transitions` in
[app_settings.rs](../../src/app_settings.rs).
These cover a controlled loader, notice copy, and settings transitions, not
download-to-speech recognition or physical microphone readiness.

`cleanup_frees_inactive_streams_without_stopping_them` in
[moonshine.rs](../../src/moonshine.rs) checks active/inactive stream cleanup and
continued freeing after a failed stop. It runs fixture callbacks, not the native
Moonshine library.

Transfer failures identify `download.moonshine.ai`, suggest restoring connectivity
or asking IT to allow that host, and direct the user to Retry. The error also
explains that hotkey dictation does not need the Commands model. Existing partial
downloads are retained for retry; this is distinct from the app download host.
See [download recovery](README.md#get-ready).

### Awake And Asleep

```ts
Ready + awake                                       // commands.awake-sleep
├── Completed ordinary phrase -> Resolve -> Execute match
├── No unambiguous match -> Ignore and log
└── "go to sleep" / "stop voice control" -> Sleeping

Sleeping -> Standalone "voice control" / "wake up" / "start voice control"
         -> Awake                                   // not wake + command in one utterance
Sleeping -> Hotkey dictation and paste shortcuts still available
```

Sources: `voice_control_for` in [config.rs](../../src/config.rs),
`CommandConfig::resolve` in [commands.rs](../../src/commands.rs), and
`handle_command` in [recognition.rs](../../src/recognition.rs).
Checks in `commands.rs`: `sleeping_only_accepts_standalone_wake_phrase`,
`sleep_phrase_must_be_the_whole_utterance`, `listening_resolves_configured_target`.
These supply text directly; they do not prove Moonshine recognition or a native
action. Wake/sleep use quiet tones; successful actions provide their own feedback,
and execution failures use an error tone.

Sleeping is not microphone release. Commands keeps input open to hear a wake
phrase. Hotkey dictation/paste resets command recognition so shortcut audio
cannot become a later command; backlog pressure is covered by
[Recovery](recovery.md#degrade-only-the-failing-dependency).

### Catalog And Context

```ts
Compiled registry -> Catalog phrases / aliases / scopes // commands.catalog
├── Protected commands + typed patterns -> Native Rust
└── Personal commands -> Explicit config script (embedded QuickJS)

Completed speech + current context                     // commands.context
├── Matching application / browser host -> Include contextual candidate
├── Unmatched predicate -> Exclude candidate
└── Unique supported parse -> Bounded asynchronous action execution
```

Sources: [command_grammar.rs](../../src/command_grammar.rs), `catalog`, `resolve`,
and `ActionExecutor` in [commands.rs](../../src/commands.rs).
Checks there: `catalog_is_derived_from_resolvable_commands`,
`typed_command_catalog_is_derived_from_its_patterns`,
`overlapping_typed_patterns_are_rejected_at_configuration_time`,
`application_command_requires_matching_foreground_application`, and
`contextual_command_requires_matching_foreground_browser`.
They cover registry/text decisions, not live context capture or target-app effects.

`erased_action_factories_are_lazy_and_preserve_alias_order` in
[command_grammar.rs](../../src/command_grammar.rs) checks the shared pattern-to-action
conversion through compiled, personal-handler, and literal command constructors.
Construction and unmatched input do not invoke factories; matching aliases keep
their catalog order. This builds actions without executing native effects.

Native action sequences resolve against the initial context, not the app opened
by an earlier action. `only_uses_commands_available_in_the_initial_context` and
`interactive_commands_cannot_participate_in_sequences` cover this boundary.
Browser-host capture currently has a Brave adapter in
[context.rs](../../src/context.rs), not universal browser support. A catalog row
is discoverability, not proof that its scope is active or its action succeeded.

### Personal Config

```ts
Create config / hex commands init                      // commands.personal-config
  -> ~/.config/hex: hex.d.ts typings + AGENTS.md (Hex-owned, refreshed)
  -> hex.config.ts starter, published last, atomically // user code, run by Hex
     // TypeScript or JavaScript; imports only @hex/commands and folder files

Hex starts or the folder changes (polled, 350 ms debounce)
  -> Erase TypeScript types in place -> Evaluate in embedded QuickJS
     // one worker thread; no filesystem, network, process, or timers
├── Registration compiles -> Replace active registry and catalog
├── Load, syntax, or overlap error -> Keep active registry; show file:line:column
└── Config deleted -> Native registry; Not configured

Handler phrase -> Call handler (1 s limit) -> Returned + queued actions
  -> Validate all -> Run in order on the native executor // first failure fails it

Same config -> Custom transformations for Modes        // Commands need not be enabled
hex commands check -> Load as Hex would -> Summary or error  // macOS CLI
```

Sources: `initialize_workspace`, `Worker::reload`, `Worker::invoke`,
`load_config`, and `compile_registration` in
[personal_commands.rs](../../src/personal_commands.rs), the engine in
[script.rs](../../src/personal_commands/script.rs), the module and registration
glue in [commands.js](../../src/personal_commands/commands.js) and
[host.js](../../src/personal_commands/host.js), and type erasure in
[typescript.rs](../../src/personal_commands/typescript.rs). Checks in
`personal_commands.rs`:
`the_starter_config_registers_and_its_handlers_return_actions`,
`handlers_queue_and_return_actions_in_order_and_fail_on_bad_ones`,
`load_errors_name_the_file_line_and_column_and_keep_the_last_good_config`,
`runaway_scripts_stop_at_the_time_limit_and_the_config_keeps_working`,
`stale_generations_never_reach_a_newer_config`,
`scripts_import_only_hex_commands_and_files_in_the_folder`,
`initialization_writes_typings_and_a_starter_config_but_keeps_the_users`,
`invalid_candidate_does_not_mutate_base_registry`, and
`wake_sleep_ids_and_phrases_are_reserved`; `typescript.rs` checks erasure and
rejected syntax. They drive the worker directly with real scripts, resolve
spoken text through the compiled registry, and stop before native actions run;
they do not cover the polling watcher thread or a live editor save.

Capture descriptors are plain data; Rust alone validates and normalizes them
when it compiles the registration. `normalizes_spoken_numbers_and_punctuation`
in [spoken_text.rs](../../src/spoken_text.rs) also preserves distinct `Ä`/`ä`
aliases. See [Modes](README.md#process-text-with-modes) for transformation
output.

### Spoken Dictation Controls

```ts
Commands enabled + command model ready + awake         // dictate.hexs
  -> "dictate start" -> Voice-delimited capture
     // wake first if sleeping; up to one second of available pre-roll

Active voice capture -> Its captured protocol
├── "dictate stop"   -> Finish -> Paste
├── "dictate send"   -> Finish -> Paste -> Enter
└── "dictate cancel" -> Discard -> Insert nothing

User config -> Replace protocol phrases -> Strip phrases from result
```

Stable start/control phrases deliberately allow streaming recognition, unlike
ordinary commands, which wait for completed speech. Actual audio capture,
ordered processing, and output commitment belong to
[Dictation](dictation.md#ongoing-jobs-and-output); Send is not an atomic
target-app transaction.

Source: `handle_voice_dictation_control` in
[recognition.rs](../../src/recognition.rs), with checks
`stable_standalone_start_phrase_activates_voice_dictation` and
`stable_start_prefix_with_dictated_speech_activates`.
Checks in [dictation.rs](../../src/dictation.rs):
`configured_protocol_is_exact_and_rejects_streaming_ambiguity`,
`strips_voice_protocols_from_transcripts`, and
`voice_start_recovers_audio_consumed_while_recognizing_the_trigger`.
`configured_dictation_protocol_replaces_defaults_and_omission_falls_back` in
[personal_commands.rs](../../src/personal_commands.rs) covers config replacement.
These use text, recognition-state, and sample-buffer fixtures.
`dictation_protocol_audio` in [parakeet.rs](../../src/parakeet.rs) is ignored by
default and requires audio fixtures and a model; none establishes a current
native voice-to-paste pass.

## Gaps And Constraints

- **Actions run after the handler:** a handler cannot observe an action's
  result; `hex.*` calls resolve at once and Hex runs every action once the
  handler returns. Scripts have no I/O, so there is nothing else to wait on.
- **Trust and scope:** the script engine exposes no filesystem, network, or
  process API, but the actions a config asks for run with the user's
  permissions. Ordinary hotkey dictation needs no config and no opt-in;
  disabling Commands does not unload a config still needed by selected custom
  transformations.
- **Native proof:** grammar, protocol, and rendered catalog checks do not prove
  that speech matched the expected command, captured the right context, and
  completed the native action. This map does not authorize live input or actions.
