# Project Documentation

Start with the document for your task.

## User & Platform Guides

- [`../README.md`](../README.md): install and use Hex on macOS, Windows, and Linux.
- [`windows.md`](windows.md): Windows x64 and ARM64 beta requirements, installation, and source builds.
- [`linux.md`](linux.md): Linux X11 and Wayland beta requirements, portals, and installation.
- [`nix.md`](nix.md): Nix flake packaging, development shell, and Home Manager autostart.
- [`../site/public/docs/transforms.md`](../site/public/docs/transforms.md): TypeScript transformation API (`~/.config/hex/hex.config.ts`).
- [`../site/public/docs/admin.md`](../site/public/docs/admin.md): macOS managed-preference feature locks (`anomaly.hex`).
- [`../ios/README.md`](../ios/README.md) & [`../android/README.md`](../android/README.md): build and test Hex Mobile on iOS and Android.
- [`../sdk/typescript/README.md`](../sdk/typescript/README.md): public `@kitlangton/hex` TypeScript transcription SDK.

## Engineering Reference & Contracts

- [`../AGENTS.md`](../AGENTS.md): workflow, repo layout, cross-cutting invariants, and verification commands.
- [`../ROADMAP.md`](../ROADMAP.md): active validation gates, unshipped work, and deliberate deferrals.
- [`features/README.md`](features/README.md): user-facing capability maps, entry points, checks, and proof limits (`dictation.md`, `capture.md`, `meetings.md`, `labs.md`, `own-models.md`, `vocabulary.md`, `control-cli.md`, `transcript-cli.md`, `accessibility.md`, `recovery.md`).
- [`design-system.md`](design-system.md): Rune + `rune-kit` + OpenCode theme tokens, shared UI pieces, copy rules, and `--snap` scenes.
- [`cli.md`](cli.md): `hex` control CLI and `POST /control` protocol contract.
- [`transcript-cli.md`](transcript-cli.md): read-only `hex history` and `hex meetings` JSON contracts.
- [`streams.md`](streams.md): live `/streams` pushed-audio transcription contract.
- [`hex-annotation.md`](hex-annotation.md): `.hexannotation` v1 package format, legacy `.scrawl` discovery, and router boundaries.
- [`opencode-compatibility.md`](opencode-compatibility.md): OpenCode loopback service discovery (including Windows WSL) and HTTP generation contract.
- [`specs/local-transcription-service.md`](specs/local-transcription-service.md): embedded local transcription service protocol and host lifecycle guarantees.
- [`specs/hex-words.md`](specs/hex-words.md): cross-platform `.hexwords` v1 vocabulary and word-replacement file spec.
- [`perf.md`](perf.md): strobe benchmark harness and isolated desktop benchmark cases.
- [`downloads.md`](downloads.md): `src/net` TLS, proxy resolution, resumable downloads, and public download URLs.
- [`releasing.md`](releasing.md): Release and Deploy workflows, security boundary (`.github/release/`), channels, and legacy feeds.
- [`plans/release-service.md`](plans/release-service.md): Anomaly release service target names, `install.json` receipt, and updater client rules.
- [`windows-signing.md`](windows-signing.md): Azure Artifact Signing and three-step Inno Setup engine signing for Smart App Control.
- [`test-builds.md`](test-builds.md): unsigned per-push `Hex Test` artifacts and isolated `hex-test` data root.

## Releases

`releases/` contains versioned release notes (`docs/releases/<X.Y.Z>.md`) that build `/hex/changelog` and gate stable releases. Preserve published release notes as historical records.
