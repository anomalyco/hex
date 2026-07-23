# Project Documentation

This index separates current product truth from active plans and historical
research. Start with the document that answers the question you have; do not
treat every Markdown file as an active specification.

## Current Product Truth

- [`../README.md`](../README.md): installation, everyday use, and
  troubleshooting.
- [`../AGENTS.md`](../AGENTS.md): architecture, invariants, development
  commands, diagnostics, and current engineering gaps. This is the
  authoritative implementation guide.
- [`../ROADMAP.md`](../ROADMAP.md): prioritized product and engineering work.
- [`specs/local-transcription-service.md`](specs/local-transcription-service.md):
  local service and TypeScript SDK contract.

## Active Plans

- [`plans/typescript-sdk.md`](plans/typescript-sdk.md): remaining service
  bootstrap, TypeScript SDK, and first-consumer sequence.
- [`plans/linux.md`](plans/linux.md): supported Linux X11 contract, remaining
  validation, and future capability slices.

Plans explain sequencing. When a plan disagrees with `AGENTS.md` about shipped
behavior, `AGENTS.md` wins and the plan should be corrected.

## Platform Guides

- [`../ios/README.md`](../ios/README.md): build and test the iOS transcription
  prototype and keyboard extension.

## Research And Historical Material

- [`research/transcription-benchmark.md`](research/transcription-benchmark.md):
  benchmark method and provisional runtime measurements.
- [`../prototypes/app-shell/README.md`](../prototypes/app-shell/README.md):
  archived app-shell comparison prototype. Its synthesis and data audit explain
  earlier design inputs, not current production behavior.

## Releases

`../release-notes/` contains the Markdown embedded in signed Sparkle releases.
Files are versioned release inputs rather than general project documentation.
