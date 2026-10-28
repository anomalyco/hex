# Project Documentation

Start with the document for your task. Plans and archived prototypes do not
override shipped behavior.

## User Guides

- [`../README.md`](../README.md): install and use Hex on macOS, Windows, and Linux.
- [`windows.md`](windows.md): Windows x64 and ARM64 beta requirements, installation, and source builds.
- [`linux.md`](linux.md): Linux X11 and native Wayland beta requirements and installation.
- [`nix.md`](nix.md): Nix packaging, development, and per-user autostart.
- [`../ios/README.md`](../ios/README.md): build and test the iOS prototype.
- [`../sdk/typescript/README.md`](../sdk/typescript/README.md): use the public
  TypeScript transcription SDK with an explicitly supplied native helper.

## Engineering Reference

- [`../AGENTS.md`](../AGENTS.md): architecture, invariants, development
  commands, and diagnostics. This is the implementation authority.
- [`../ROADMAP.md`](../ROADMAP.md): prioritized product and engineering work.
- [`design-system.md`](design-system.md): Rune + `rune-kit` + OpenCode theme
  components, Hex pieces, copy rules, and `--snap` scene verification.
- [`features/README.md`](features/README.md): existing user-facing capabilities,
  entry points, available verification, and gaps; detailed dictation and recovery maps.
- [`downloads.md`](downloads.md): public download paths, R2 custom domains,
  GitHub release mirrors, and network recovery.
- [`opencode-compatibility.md`](opencode-compatibility.md): audited OpenCode V2
  source and CLI baseline, integration contract, and validation limits.
- [`plans/personal-commands.md`](plans/personal-commands.md): shipped personal
  command contract and remaining lifecycle work.
- [`specs/local-transcription-service.md`](specs/local-transcription-service.md):
  implemented macOS local-service protocol and lifecycle guarantees.

## Architecture Decisions And Active Plans

- [`plans/rune-ui.md`](plans/rune-ui.md): implemented; Rune desktop UI port,
  surfaces, goldens, and remaining real-window checklist.
- [`plans/single-process.md`](plans/single-process.md): implemented; one Hex
  process per user on macOS, Windows, and Linux hosting Rune windows and overlays.
- [`plans/rune-dependency.md`](plans/rune-dependency.md): decision record for
  Rune as a public MIT Git dependency and Nix output-hash updates.
- [`plans/shared-desktop-ui.md`](plans/shared-desktop-ui.md): implemented; how
  macOS, Windows, and Linux share one window and one settings file.
- [`plans/swift-app-handoff.md`](plans/swift-app-handoff.md): manual download
  handoff from Swift, without data import or automatic replacement.
- [`plans/typescript-sdk.md`](plans/typescript-sdk.md): published SDK status,
  remaining signed-helper packaging, and first-consumer validation.
- [`plans/linux.md`](plans/linux.md): Linux X11/Wayland validation and later capability
  slices.
- [`plans/meeting-live-transcription.md`](plans/meeting-live-transcription.md):
  evaluation of Apple SpeechTranscriber vs Moonshine for live meeting drafts.
- [`plans/granola-import.md`](plans/granola-import.md): research on importing
  historical Granola meetings into `hex mcp`.

When a plan disagrees with `AGENTS.md` about shipped behavior, update the plan.

## Research And Archives

- [`research/transcription-benchmark.md`](research/transcription-benchmark.md):
  historical benchmark method and measurements.
- [`../prototypes/app-shell/README.md`](../prototypes/app-shell/README.md):
  archived app-shell comparison prototype.

## Releases

`releases/` contains version-specific release notes and drafts. `../release-notes/` contains legacy inputs from versions
2.0.0 through 2.0.9. Published notes and the SDK changelog describe their release,
not current behavior; preserve them as historical records. A draft note is not
evidence that its release has been published.
