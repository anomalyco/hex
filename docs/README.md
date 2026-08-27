# Project Documentation

Start with the document for your task. Plans and archived prototypes do not
override shipped behavior.

## User Guides

- [`../README.md`](../README.md): install and use HEX on macOS.
- [`linux.md`](linux.md): install the supported Linux X11 beta from source.
- [`../ios/README.md`](../ios/README.md): build and test the iOS prototype.

## Engineering Reference

- [`../AGENTS.md`](../AGENTS.md): architecture, invariants, development
  commands, and diagnostics. This is the implementation authority.
- [`../ROADMAP.md`](../ROADMAP.md): prioritized product and engineering work.
- [`opencode-compatibility.md`](opencode-compatibility.md): audited OpenCode V2
  source and CLI baseline, integration contract, and validation limits.
- [`plans/personal-commands.md`](plans/personal-commands.md): shipped personal
  command contract and remaining lifecycle work.
- [`specs/local-transcription-service.md`](specs/local-transcription-service.md):
  implemented internal service contract and private SDK target.

## Active Plans

- [`plans/swift-app-handoff.md`](plans/swift-app-handoff.md): a manual download
  handoff from Swift, without data import or automatic replacement.
- [`plans/shared-desktop-ui.md`](plans/shared-desktop-ui.md): converge the macOS
  and Linux GPUI applications on one capability-driven product shell.
- [`plans/typescript-sdk.md`](plans/typescript-sdk.md): private SDK packaging,
  validation, and first-consumer work.
- [`plans/linux.md`](plans/linux.md): Linux X11 validation and later capability
  slices.

When a plan disagrees with `AGENTS.md` about shipped behavior, update the plan.

## Research And Archives

- [`research/transcription-benchmark.md`](research/transcription-benchmark.md):
  historical benchmark method and measurements.
- [`../prototypes/app-shell/README.md`](../prototypes/app-shell/README.md):
  archived app-shell comparison prototype.

## Releases

`releases/` contains current Markdown inputs embedded in signed Sparkle updates.
`../release-notes/` contains legacy inputs from versions 2.0.0 through 2.0.9.
