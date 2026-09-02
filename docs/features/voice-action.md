# Voice Action

Hold a separate shortcut and speak an instruction, optionally with text selected.
HEX transcribes locally, asks Voice Action's OpenCode model for paste-ready text,
and pastes at the current focus. This is not
[Voice Commands](commands.md) or ordinary [Dictation](dictation.md).

This map covers macOS. Linked checks are locators, not passing runs; no tests,
previews, providers, or native actions were run for this split.

## How To Get To It

```ts
Hex > Voice Action                                   // voice-action; off by default
├── Enable Voice Action -> Activate saved shortcut
├── Shortcut -> Option-Command by default             // independent of Dictation
└── Processing -> OpenCode model / available Thinking variant

Foreground text target -> Optional selection -> Hold shortcut -> Speak -> Release
```

Requires ordinary setup permissions, a local transcription model, and OpenCode
with an available text model. Commands and its Moonshine model are not required.
`render_voice_action` in [app_window.rs](../../src/app_window.rs) keeps the toggle
reachable when OpenCode is unavailable; the pane offers setup/retry actions.
HEX discovers `opencode2` and its managed service through
[dictation_processor.rs](../../src/dictation_processor.rs), not a separate
HEX-owned provider service.

## Sub-features

### Independent Opt-In

```ts
New settings OR existing settings without enabled=true // voice-action.opt-in
  -> Off -> Saved binding neither active nor reserved

Enable -> Check shortcut conflicts -> Persist opt-in
Disable -> Cancel active Voice Action capture
        -> Keep accepted jobs, saved shortcut, model, and variant
```

Checks in [app_settings.rs](../../src/app_settings.rs):
`voice_action_requires_explicit_opt_in_for_new_and_existing_settings`,
`voice_action_settings_round_trip`, and
`disabled_voice_action_preserves_its_binding_without_reserving_it`.
Checks in [app_window.rs](../../src/app_window.rs):
`voice_action_opt_in_rejects_active_conflicts_without_rebinding` and
`voice_action_toggle_preserves_configuration_and_can_always_disable`.
These cover settings and conflict logic, not native shortcut registration.
Active-capture cancellation is wired in [recognition.rs](../../src/recognition.rs)
when the runtime edit binding becomes `None`; the settings tests alone do not
prove that runtime transition.

### Hold Or Promote

```ts
Hold saved Voice Action shortcut                      // voice-action.shortcut
├── Intentional hold -> Capture -> Release -> Submit
└── Short tap -> Discard                               // no double-tap locking

Default Option dictation already recording + Command joins
  -> Promote same capture -> Preserve recorded audio   // not a second recording
  -> Voice Action owns release, not ordinary Dictation
```

Source: `handle_edit_hotkey_action`, `voice_action_owns_action`, and
`start_pending_voice_action` in [recognition.rs](../../src/recognition.rs).
Checks there: `slow_chord_release_keeps_voice_action_ownership`,
`accepted_voice_action_preserves_promoted_capture`,
`rejected_voice_action_cancels_promoted_capture`, and
`delayed_voice_action_edges_use_their_source_timestamps`.
These cover synthetic edges and capture admission, not physical onset or
sample-for-sample audio continuity through promotion.

`voice_action_uses_separate_binding_after_dictation_is_rebound` in
[suppression.rs](../../src/suppression.rs) checks independent bindings.
`DictationHotkey::new_without_paste` disables locking for this target; ordinary
Dictation's double-tap settings do not add a Voice Action lock. Cold microphone
modifier order remains a [known gap](#cold-modifier-order).

### Optional Selection

```ts
Intentional Voice Action -> Snapshot optional selected text // voice-action.selection
├── Accessibility exposes bounded nonempty text -> Include as prompt context
└── Empty / inaccessible / oversized -> No selection; generate from instruction

Selection capture -> No clipboard read or copy shortcut
Later paste -> Current focus, not a pinned original selection
```

Sources: `voice_action_context_snapshot` in
[recognition.rs](../../src/recognition.rs), `capture_optional` in
[accessibility.rs](../../src/accessibility.rs), and `voice_action_prompt` in
[dictation_processor.rs](../../src/dictation_processor.rs).
Checks in the processor: `voice_action_prompt_keeps_the_instruction_separate_from_selected_text`
and `voice_action_prompt_supports_generation_without_a_selection`.
These construct prompt strings; they do not prove Accessibility support in an
arbitrary application. Selection is limited to 64 KiB; full window text is not
substituted when selection is absent.

### Local Transcript To Model

```ts
Accepted recording -> Selected local transcription model // voice-action.processing
  -> Instruction + optional selection + app / browser host
  -> Voice Action OpenCode model and deadline
     ├── Nonempty reply -> Paste-ready output
     └── Failure / timeout / empty -> Paste nothing      // no raw-transcript fallback

Modes -> Bypassed                                      // no corrections/rewrite/transformations
```

Sources: the `TranscriptionTarget::VoiceAction` processing branch in
[parakeet.rs](../../src/parakeet.rs), `process_voice_action_cancellable` and
`voice_action_failure` in [dictation_processor.rs](../../src/dictation_processor.rs).
Voice Action uses its own model/variant and persisted deadline (60 seconds by
default); no explicit model selection delegates to OpenCode's default.
Audio transcription stays local, but the prompt text goes to the configured
OpenCode provider.

`generation_http_wait_observes_cancellation_and_deadline` and
`cancelled_generation_process_is_killed_promptly` in the processor cover local
HTTP/process fixtures, not a live provider or the full Voice Action pipeline.
Mode bypass and no-fallback routing are source-backed here, not a claimed
end-to-end pass. See [Recovery](recovery.md#degrade-only-the-failing-dependency)
for the distinction from ordinary Dictation's best-effort processing fallback.

### Output And Cancellation

```ts
Accepted jobs -> Shared bounded queue -> Submission-order output // voice-action.output
  -> Standalone paste at current focus
     ├── Successful output -> History when retention permits
     └── Last Dictation -> Unchanged                    // Paste Last never recalls this reply

Escape                                                // voice-action.cancel
├── Active capture -> Cancel that capture first
└── Otherwise -> Cancel newest cancellable accepted job
Before output commit -> No paste; after commit -> No undo
```

Sources: [recognition.rs](../../src/recognition.rs), `finish_output` in
[parakeet.rs](../../src/parakeet.rs), and [paste.rs](../../src/paste.rs).
Checks in `parakeet.rs`: `parallel_processing_results_are_released_in_submission_order`,
`repeated_cancellation_walks_back_through_pending_jobs`, and
`output_stays_cancellable_through_preparation_but_not_after_mutation`.
The last explicitly includes Voice Action, its standalone paste mode, History,
and unchanged last-dictation text. These use controlled output/paste boundaries
and temporary History storage, not a foreground application's clipboard
consumption. See [Dictation](dictation.md#cancellation-before-commit) and
[Recovery](recovery.md#cancel-without-losing-other-work) for the shared boundary.

## Gaps And Constraints

### Cold Modifier Order

```ts
Release when idle + default shortcut                   // known #23, not fixed here
├── Command-first -> Pending Voice Action -> Intentional threshold -> Open mic
└── Option-first -> Already-started Dictation capture -> Promote

Ordinary cold-Dictation checks -> Both Voice Action orders // NOT established
```

Source: `start_pending_voice_action` and intentional-hold handling in
[recognition.rs](../../src/recognition.rs).
[#23](https://github.com/anomalyco/hex/issues/23) still needs both modifier orders
with a controlled opener, then native audio-onset evidence. See
[Recovery](recovery.md#cold-voice-action); a successful HUD fixture does not
establish equivalent audio onset.

- **Preview limits:** [capture-preview.sh](../../scripts/capture-preview.sh)
  provides `voice-action`, `--voice-action-enabled`, and `--opencode-unavailable`
  fixtures. They render states, not selection-to-provider-to-paste execution.
- **Focus and provider limits:** the prompt asks for paste-ready text; tests do
  not guarantee a provider follows it. Moving focus while work is pending changes
  the eventual paste target. A failed action does not paste its local transcript.
- **Native proof:** permissions, shortcut ordering, microphone mode, selection
  support, provider response, and target paste still need an explicit controlled
  end-to-end check. This map does not authorize operating the installed app.
