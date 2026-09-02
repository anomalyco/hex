# Voice Commands

Opt in to spoken commands without changing ordinary hotkey dictation. Commands
uses its own Moonshine model and awake/sleep state; it is not
[Voice Action](voice-action.md).

This map covers macOS. Linked checks are locators, not passing runs; no tests,
previews, providers, or native actions were run for this split.

## How To Get To It

```ts
Hex > Commands                                      // commands; off by default
├── Enable -> Prepare command model                  // needs setup permissions
├── Search / select a catalog row -> Inspect, not execute
└── Create Config / Edit Config / Copy agent prompt  // personal workspace

hex commands init -> Create or refresh ~/.config/hex // does not start listening
hex listen -> Use saved command opt-in               // never beside another listener
```

The catalog and config controls remain reachable while Commands is off. Sources:
`render_commands` in [app_window.rs](../../src/app_window.rs), CLI routes in
[main.rs](../../src/main.rs), and workspace paths in
[app_paths.rs](../../src/app_paths.rs). Debug-only `hex dev commands on|off`
changes the running app's opt-in; it is not a release CLI route.

## Sub-features

### Opt In And Prepare

```ts
Enable Commands                                     // commands.opt-in
├── Release when idle active -> Confirm Keep Microphone Ready & Enable Commands
└── Load separate Moonshine model in background
    ├── Ready -> Recognize commands, initially awake
    └── Failure -> Keep opt-in; show Retry / Turn off commands

Disable -> Unload command recognizer and action executor
Hotkey dictation -> Remains available during preparation and failure
```

Sources: `load_command_recognizer` and live policy handling in
[recognition.rs](../../src/recognition.rs); `render_model_notice` in
[app_window.rs](../../src/app_window.rs). Checks:
`command_preparation_does_not_block_the_listener_or_propagate_failure`,
`command_model_failure_has_recovery_without_blocking_dictation`, and
`missing_fields_receive_defaults` /
`microphone_policy_requires_explicit_combined_transitions` in
[app_settings.rs](../../src/app_settings.rs).
These cover a controlled loader, notice copy, and settings transitions, not
download-to-speech recognition or physical microphone readiness.

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
└── Personal commands -> Explicit TypeScript config

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

Native action sequences resolve against the initial context, not the app opened
by an earlier action. `only_uses_commands_available_in_the_initial_context` and
`interactive_commands_cannot_participate_in_sequences` cover this boundary.
Browser-host capture currently has a Brave adapter in
[context.rs](../../src/context.rs), not universal browser support. A catalog row
is discoverability, not proof that its scope is active or its action succeeded.

### Personal Config

```ts
Create Config / hex commands init                      // commands.personal-config
  -> Managed Bun workspace + @hex/commands SDK
  -> ~/.config/hex/hex.config.ts                         // trusted executable user code

Running watcher sees config edit -> Validate candidate
├── Valid -> Replace active registry and catalog
└── Invalid -> Preserve active registry; show reload error

Same workspace -> Custom transformations for Modes     // Commands need not be enabled
```

Sources: `initialize_workspace`, `run_worker`, `reload`, and `compile_registration`
in [personal_commands.rs](../../src/personal_commands.rs), plus
[host.ts](../../sdk/commands/src/host.ts). Checks in `personal_commands.rs`:
`clean_workspace_provisioning_preserves_user_config_on_refresh`,
`invalid_candidate_does_not_mutate_base_registry`,
`wake_sleep_ids_and_phrases_are_reserved`.
These cover provisioning/registration fixtures, not a live editor-to-reload path.

[Host tests](../../sdk/commands/test/host.test.ts), including
`emits a bounded serializable registration without handlers` and
`waits for vanilla tool calls even when the handler forgets await`, cover the
wire protocol with supplied messages. The
[workspace smoke](../../scripts/smoke-commands-workspace.sh) exercises temporary
Bun installs, type checking, and host registration, not speech-to-native-action
success. See [Modes](README.md#process-text-with-modes) for transformation output.

### Spoken Dictation Controls

```ts
Commands enabled + command model ready + awake         // dictate.voice-controls
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

- **Pre-watcher startup failure:** `run_worker` can return before starting
  `watch_workspace` when config, Bun, or SDK preparation is unavailable. Repairing
  the files alone then has no watcher to trigger a reload. The live-reload
  contract above applies only after the watcher starts; startup recovery remains
  a gap.
- **Timeout and late native result:** [#20](https://github.com/anomalyco/hex/issues/20)
  remains a known gap. Host interruption removes tool correlation;
  a late reply is rejected, and Rust rejects completion with outstanding tools.
  `reports Effect timeouts without losing subsequent invocations` in the host
  tests has no outstanding native tool and does not cover this failure. See
  [Recovery](recovery.md#late-native-tool-reply) for the missing lifecycle evidence.
- **Trust and scope:** personal config and npm dependencies are not sandboxed.
  Ordinary hotkey dictation needs neither Bun nor this opt-in; disabling Commands
  does not stop a personal host still needed by selected custom transformations.
- **Native proof:** grammar, protocol, and rendered catalog checks do not prove
  that speech matched the expected command, captured the right context, and
  completed the native action. This map does not authorize live input or actions.
